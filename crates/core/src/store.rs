use crate::model::ConnInfo;
use std::fs;
use std::path::Path;

fn conns_file(config_dir: &Path) -> Result<std::path::PathBuf, String> {
    Ok(config_dir.join("connections.json"))
}

pub fn load_conns(config_dir: &Path) -> Vec<ConnInfo> {
    let Ok(path) = conns_file(config_dir) else { return vec![] };
    let Ok(text) = fs::read_to_string(&path) else { return vec![] };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            // 解析失败不当空表:把坏文件备份留存,否则下次保存会把仅剩 1 条的列表
            // 写回去,其余连接(含密码)永久丢失
            let bak = path.with_extension("json.corrupt");
            let _ = fs::copy(&path, &bak);
            eprintln!(
                "[store] connections.json 解析失败({e}),已备份到 {},按空表处理",
                bak.display()
            );
            vec![]
        }
    }
}

pub fn save_conns(config_dir: &Path, conns: &[ConnInfo]) -> Result<(), String> {
    let path = conns_file(config_dir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(conns).map_err(|e| e.to_string())?;
    // 临时文件 + rename 原子替换:直写中途崩溃会留下半截 JSON
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &text).map_err(|e| format!("写入配置失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("替换配置失败: {e}"))
}