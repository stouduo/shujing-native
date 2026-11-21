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

// ================================================================
// 驱动插件(v2):sidecar 子进程插件,让使用者接入"无 Rust 客户端"的数据库。
//
// plugin.toml:
//   kind = "driver"
//   name = "oracle-driver"
//   version = "0.1.0"
//   description = "Oracle 驱动(使用者自备驱动与实现)"
//   db_types = ["oracle"]      # 该插件提供的自定义连接类型名
//   exec = "./agent"           # 可执行文件(相对插件目录);也可含参数占位
//
// 通信:宿主 spawn exec,stdio 上每行一个 JSON-RPC 2.0(与数镜 MCP 同构):
//   connect  {host,port,user,password,database} -> {version}
//   databases {}                                 -> {databases:[".."]}
//   tables    {database}                         -> {tables:[{name,kind}]}
//   structure {database,table}                   -> {columns:[{name,type,nullable,
//                                                   default,comment}], pk:[".."]}
//   query    {database,sql,max}                  -> {columns:[".."],rows:[["v"..]..],
//                                                   affected,truncated}
//   count    {database,table}                    -> {count}
//   ping     {}                                  -> {ok:true}
// 数据库语义完全由 agent 解释:数镜零方言知识。

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone)]
pub struct DriverPlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    pub exec: String,
    pub db_types: Vec<String>,
}

/// 插件目录(SHUJING_CONFIG_DIR 优先,默认 ~/.shujing-native)
pub fn plugin_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let base = std::env::var("SHUJING_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{home}/.shujing-native"));
    PathBuf::from(base).join("plugins")
}

/// 扫描插件目录,提取 kind="driver" 的插件(与 transform 型 WASM 同目录共存)
pub fn scan_drivers(dir: &Path) -> Vec<DriverPlugin> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let Ok(mf) = std::fs::read_to_string(d.join("plugin.toml")) else {
            continue;
        };
        let (name, version, description, _sha) = parse_manifest(&mf);
        // 逐行抓 driver 专属键(kind/exec/db_types;parse_manifest 不含这些)
        let mut kind = String::new();
        let mut exec = String::new();
        let mut db_types = Vec::new();
        for line in mf.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            match k {
                "kind" => kind = v.to_string(),
                "exec" => exec = v.to_string(),
                "db_types" => {
                    // ["a","b"] 或 "a" 宽容解析
                    let inner = v.trim_start_matches('[').trim_end_matches(']');
                    db_types = inner
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
        if kind != "driver" || exec.is_empty() {
            continue;
        }
        out.push(DriverPlugin {
            name: if name.is_empty() {
                d.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            } else {
                name
            },
            version,
            description,
            dir: d.clone(),
            exec,
            db_types,
        });
    }
    out
}

/// 一个驱动插件 agent 的运行实例:子进程 + 串行请求/响应
pub struct DriverAgent {
    pub plugin_name: String,
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: AtomicU64,
}

impl DriverAgent {
    /// 启动 agent 并做协议握手(ping)
    pub async fn spawn(plugin: &DriverPlugin) -> Result<Self, String> {
        let exec_path = {
            let p = PathBuf::from(&plugin.exec);
            if p.is_absolute() {
                p
            } else {
                plugin.dir.join(p)
            }
        };
        #[cfg(target_os = "macos")]
        {
            // 允许执行位缺失(脚本场景)——由使用者保证可执行
        }
        let mut cmd = tokio::process::Command::new(&exec_path);
        cmd.current_dir(&plugin.dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            // 数镜退出/断开连接(LiveConn drop)时必须带走 agent 子进程:
            // tokio Child 默认 drop 不杀 → 孤儿进程残留
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(md) = std::fs::metadata(&exec_path) {
                if md.permissions().mode() & 0o111 == 0 {
                    // 无执行位:尝试 chmod(使用者自备文件常见疏漏)
                    let _ = std::fs::set_permissions(
                        &exec_path,
                        std::fs::Permissions::from_mode(md.permissions().mode() | 0o111),
                    );
                }
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("驱动插件启动失败({}): {e}", exec_path.display()))?;
        let stdin = child.stdin.take().ok_or("agent stdin 缺失")?;
        let stdout = child.stdout.take().ok_or("agent stdout 缺失")?;
        let mut agent = Self {
            plugin_name: plugin.name.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
        };
        // 协议握手
        let pong: serde_json::Value = agent
            .call("ping", &serde_json::json!({}))
            .await
            .map_err(|e| format!("驱动插件握手失败: {e}"))?;
        if pong.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            return Err("驱动插件握手失败(ping 未回 ok)".into());
        }
        Ok(agent)
    }

    /// 单次请求(串行;调用方负责持锁)。子进程已死时返回明确错误。
    pub async fn call(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        /// 单请求上限:agent 卡死/死循环时兜底,不让 UI 永挂
        const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
        let call = self.call_inner(method, params);
        match tokio::time::timeout(CALL_TIMEOUT, call).await {
            Ok(r) => r,
            Err(_) => Err(format!(
                "驱动插件 {} 请求超时(60s):方法 {method}",
                self.plugin_name
            )),
        }
    }

    async fn call_inner(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err(format!("驱动插件 {} 进程已退出", self.plugin_name));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let line = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("驱动插件写失败: {e}"))?;
        self.stdin.write_all(b"\n").await.map_err(|e| format!("驱动插件写失败: {e}"))?;
        self.stdin.flush().await.map_err(|e| format!("驱动插件写失败: {e}"))?;
        loop {
            let mut buf = String::new();
            let n = self
                .stdout
                .read_line(&mut buf)
                .await
                .map_err(|e| format!("驱动插件读失败: {e}"))?;
            if n == 0 {
                return Err(format!("驱动插件 {} 意外退出", self.plugin_name));
            }
            let buf = buf.trim();
            if buf.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(buf)
                .map_err(|e| format!("驱动插件回包不是 JSON: {e}"))?;
            let rid = v.get("id").and_then(|x| x.as_u64());
            if rid != Some(id) {
                continue; // 非本请求的回包(如通知)跳过
            }
            if let Some(err) = v.get("error") {
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("未知错误");
                return Err(format!("驱动插件错误: {msg}"));
            }
            return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
        }
    }

    /// 关闭子进程(尽力)
    pub async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;

    #[test]
    fn scan_drivers_finds_kind_driver() {
        let dir = std::env::temp_dir().join(format!("shujing_drv_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = dir.join("sample-driver");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("plugin.toml"),
            "name = \"sample\"\nkind = \"driver\"\nversion = \"0.1.0\"\ndescription = \"测试\"\ndb_types = [\"sampledb\", \"other\"]\nexec = \"./agent.sh\"\n",
        )
        .unwrap();
        // transform 型插件不应混入
        let d2 = dir.join("wasm-thing");
        std::fs::create_dir_all(&d2).unwrap();
        std::fs::write(
            d2.join("plugin.toml"),
            "name = \"wasm1\"\nversion = \"0.1.0\"\nkind = \"transform\"\n",
        )
        .unwrap();
        let drivers = scan_drivers(&dir);
        assert_eq!(drivers.len(), 1);
        assert_eq!(drivers[0].name, "sample");
        assert_eq!(drivers[0].db_types, vec!["sampledb", "other"]);
        assert_eq!(drivers[0].exec, "./agent.sh");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_drivers_missing_dir_empty() {
        assert!(scan_drivers(Path::new("/tmp/definitely-missing-driver-dir")).is_empty());
    }
}

/// 驱动插件会话:agent 进程 + 连接参数缓存 + 会话库记忆
pub struct DriverSession {
    pub agent: tokio::sync::Mutex<DriverAgent>,
    /// 连接参数(connect 用;Redis 式重连时重建 agent 用)
    pub cfg: serde_json::Value,
    /// agent connect 返回的版本串
    pub version: String,
    /// 会话库记忆(USE 类语义由 agent 侧维护;数镜侧只透传)
    pub database: Option<String>,
}

impl DriverSession {
    pub async fn start(
        plugin: &DriverPlugin,
        params: &serde_json::Value,
    ) -> Result<Self, String> {
        let mut agent = DriverAgent::spawn(plugin).await?;
        let result = agent
            .call("connect", params)
            .await
            .map_err(|e| format!("驱动插件连接失败: {e}"))?;
        let version = result
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("插件驱动")
            .to_string();
        Ok(Self {
            agent: tokio::sync::Mutex::new(agent),
            cfg: params.clone(),
            version,
            database: None,
        })
    }
}
