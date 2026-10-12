use crate::model::ConnInfo;
use std::fs;
use std::path::Path;

fn conns_file(config_dir: &Path) -> Result<std::path::PathBuf, String> {
    Ok(config_dir.join("connections.json"))
}

pub fn load_conns(config_dir: &Path) -> Vec<ConnInfo> {
    let Ok(path) = conns_file(config_dir) else { return vec![] };
    let Ok(text) = fs::read_to_string(path) else { return vec![] };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_conns(config_dir: &Path, conns: &[ConnInfo]) -> Result<(), String> {
    let path = conns_file(config_dir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(conns).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| format!("写入配置失败: {e}"))
}
