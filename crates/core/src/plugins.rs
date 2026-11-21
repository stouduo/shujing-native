//! 插件系统 v1(wasmi 沙箱解释器):
//! 插件 = `~/.shujing-native/plugins/<名>/` 内 plugin.toml(清单)+ plugin.wasm。
//!
//! ABI 约定(纯函数、零宿主导入 = 插件没有任何 IO 面,天然沙箱):
//! - 插件导出:`memory` 线性内存、`alloc(size: i32) -> i32`(申请输入区)、
//!   `transform(input_ptr: i32, input_len: i32) -> i32`(返回输出区偏移)
//! - 输出格式:输出区前 4 字节 = u32 LE 输出长度,其后为 UTF-8 JSON 文本
//! - 输入:UTF-8 JSON(行集 = {"columns":[..],"rows":[[..]..]} 或任意 JSON)
//!
//! 安全:fuel 限额防死循环(超限报错);无导入函数 → 无法触网/读盘;
//! manifest 可选 sha256 与 wasm 实际哈希不符时拒绝加载(完整性校验)。

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// fuel 上限:约 2e8 wasmi 燃料单位(解释器毫秒~秒级),防插件死循环卡 UI
const FUEL_LIMIT: u64 = 200_000_000;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    /// 加载成功与否;false 时 err 给出原因(清单缺失/哈希不符/wasm 编译失败)
    pub ok: bool,
    pub err: Option<String>,
}

pub struct PluginManager {
    pub plugins: Vec<PluginInfo>,
    modules: Vec<Option<(wasmi::Engine, wasmi::Module)>>,
}

fn parse_manifest(text: &str) -> (String, String, String, Option<String>) {
    // 极简 key = "value" 解析(四键;不引 toml 依赖)
    let mut name = String::new();
    let mut version = String::new();
    let mut description = String::new();
    let mut sha256: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim();
        let v = v.trim().trim_matches('"');
        match k {
            "name" => name = v.to_string(),
            "version" => version = v.to_string(),
            "description" => description = v.to_string(),
            "sha256" => sha256 = Some(v.to_string()),
            _ => {}
        }
    }
    (name, version, description, sha256)
}

fn wasm_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

impl PluginManager {
    /// 扫描插件目录(每个子目录 = 一个插件);目录不存在返回空管理器
    pub fn load_all(dir: &Path) -> Self {
        let mut plugins = Vec::new();
        let mut modules: Vec<Option<(wasmi::Engine, wasmi::Module)>> = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => {
                return Self {
                    plugins,
                    modules,
                }
            }
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for d in dirs {
            let mf_path = d.join("plugin.toml");
            let wasm_path = d.join("plugin.wasm");
            let Ok(mf_text) = std::fs::read_to_string(&mf_path) else {
                plugins.push(PluginInfo {
                    name: d
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    version: String::new(),
                    description: String::new(),
                    dir: d.clone(),
                    ok: false,
                    err: Some("缺少 plugin.toml".into()),
                });
                modules.push(None);
                continue;
            };
            let (name, version, description, sha256) = parse_manifest(&mf_text);
            let name = if name.is_empty() {
                d.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                name
            };
            let bad = |err: String| PluginInfo {
                name: name.clone(),
                version: version.clone(),
                description: description.clone(),
                dir: d.clone(),
                ok: false,
                err: Some(err),
            };
            let Ok(bytes) = std::fs::read(&wasm_path) else {
                plugins.push(bad("缺少 plugin.wasm".into()));
                modules.push(None);
                continue;
            };
            if let Some(expected) = &sha256 {
                if !expected.is_empty() && expected.to_lowercase() != wasm_sha256(&bytes) {
                    plugins.push(bad("sha256 校验失败(文件与清单不符)".into()));
                    modules.push(None);
                    continue;
                }
            }
            let mut cfg = wasmi::Config::default();
            cfg.consume_fuel(true);
            let engine = wasmi::Engine::new(&cfg);
            match wasmi::Module::new(&engine, &bytes[..]) {
                Ok(module) => {
                    plugins.push(PluginInfo {
                        name,
                        version,
                        description,
                        dir: d.clone(),
                        ok: true,
                        err: None,
                    });
                    modules.push(Some((engine, module)));
                }
                Err(e) => {
                    plugins.push(bad(format!("wasm 编译失败: {e}")));
                    modules.push(None);
                }
            }
        }
        Self { plugins, modules }
    }

    /// 重载(设置弹窗「重新扫描」)
    pub fn reload(&mut self, dir: &Path) {
        *self = Self::load_all(dir);
    }

    pub fn ok_count(&self) -> usize {
        self.plugins.iter().filter(|p| p.ok).count()
    }

    /// 运行插件:input = UTF-8 JSON;返回插件输出的 JSON 文本
    pub fn transform(&self, idx: usize, input: &str) -> Result<String, String> {
        let Some((engine, module)) = self.modules.get(idx).and_then(|m| m.as_ref()) else {
            return Err(format!(
                "插件 {} 不可用: {}",
                self.plugins[idx].name,
                self.plugins[idx]
                    .err
                    .clone()
                    .unwrap_or_else(|| "未加载".into())
            ));
        };
        let mut store = wasmi::Store::new(engine, ());
        store
            .set_fuel(FUEL_LIMIT)
            .map_err(|e| format!("沙箱初始化失败: {e}"))?;
        let linker = wasmi::Linker::new(engine);
        let instance = linker
            .instantiate(&mut store, module)
            .and_then(|pre| pre.start(&mut store))
            .map_err(|e| format!("插件初始化失败: {e}"))?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or("插件缺少导出的 memory")?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "alloc")
            .map_err(|_| "插件缺少导出的 alloc(i32)->i32")?;
        let transform = instance
            .get_typed_func::<(i32, i32), i32>(&store, "transform")
            .map_err(|_| "插件缺少导出的 transform(i32,i32)->i32")?;

        let input = input.as_bytes();
        let in_ptr = alloc
            .call(&mut store, input.len() as i32)
            .map_err(|e| format!("插件 alloc 失败: {e}"))?;
        memory
            .write(&mut store, in_ptr as usize, input)
            .map_err(|e| format!("插件内存写入失败: {e}"))?;
        let out_ptr = transform
            .call(&mut store, (in_ptr, input.len() as i32))
            .map_err(|e| {
                let fuel_left = store.get_fuel().unwrap_or(u64::MAX);
                if fuel_left == 0 {
                    "插件执行超限(燃料耗尽,疑似死循环)".to_string()
                } else {
                    format!("插件执行失败: {e}")
                }
            })?;
        let mut len_buf = [0u8; 4];
        memory
            .read(&store, out_ptr as usize, &mut len_buf)
            .map_err(|e| format!("插件输出读取失败: {e}"))?;
        let out_len =
            u32::from_le_bytes(len_buf) as usize;
        if out_len > 64 * 1024 * 1024 {
            return Err("插件输出超大(>64MB),已拒绝".into());
        }
        let mut out_buf = vec![0u8; out_len];
        memory
            .read(&store, out_ptr as usize + 4, &mut out_buf)
            .map_err(|e| format!("插件输出读取失败: {e}"))?;
        String::from_utf8(out_buf).map_err(|_| "插件输出不是 UTF-8".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parser_extracts_keys() {
        let (name, version, desc, sha) = parse_manifest(
            "# 注释\nname = \"rows-to-md\"\nversion = \"0.1.0\"\ndescription = \"行集转 Markdown\"\nsha256 = \"abc\"\n",
        );
        assert_eq!(name, "rows-to-md");
        assert_eq!(version, "0.1.0");
        assert_eq!(desc, "行集转 Markdown");
        assert_eq!(sha.as_deref(), Some("abc"));
    }

    #[test]
    fn sha256_is_stable_hex() {
        assert_eq!(
            wasm_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// 端到端:真实示例插件(wasm32 编译产物)在 wasmi 沙箱里跑行集转换
    #[test]
    fn transform_sample_plugin_end_to_end() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/plugins-sample");
        let m = PluginManager::load_all(&fixture);
        assert_eq!(m.ok_count(), 1, "示例插件应加载成功: {:?}", m.plugins);
        let input = r#"{"columns":["name","score"],"rows":[["张三","91.5"],["李四",null]]}"#;
        let out = m.transform(0, input).unwrap();
        assert!(out.contains("text"), "输出 JSON 含 text: {out}");
        assert!(out.contains("张三"), "中文进表格: {out}");
        assert!(out.contains("NULL"), "null → NULL 列: {out}");
        assert!(out.contains("|---|"), "Markdown 表头分隔: {out}");
    }

    #[test]
    fn load_all_missing_dir_is_empty() {
        let m = PluginManager::load_all(Path::new("/tmp/definitely-missing-plugins-dir"));
        assert!(m.plugins.is_empty());
        assert_eq!(m.ok_count(), 0);
    }
}
