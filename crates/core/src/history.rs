//! 查询执行历史:JSONL 持久化(~/.shujing-native/query_history.jsonl)。
//! 追加 + 滚动上限;条目含连接/库/SQL/成败/行数/耗时,供查询页「历史」弹窗回填。

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    /// epoch 毫秒
    pub ts: i64,
    /// 连接名(展示用)
    pub conn: String,
    /// 库名(可空)
    pub db: String,
    /// SQL 文本
    pub sql: String,
    pub ok: bool,
    pub rows: u64,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub err: String,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            ts: 0,
            conn: String::new(),
            db: String::new(),
            sql: String::new(),
            ok: true,
            rows: 0,
            elapsed_ms: 0,
            err: String::new(),
        }
    }
}

/// epoch ms → 本地 "YYYY-MM-dd HH:MM:SS"(展示用)
pub fn format_ts_local(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return "—".into();
    }
    chrono::DateTime::from_timestamp_millis(ts_ms)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

/// 追加一条并滚动截断(最多保留 `max` 条新条目)。文件损坏按空文件处理。
/// 追加一条(O(1) 追加,高频查询路径不再全量重写);
/// 文件超过 512KB 触发一次 compaction(读→保留 max 条→重写)
pub fn append(path: &Path, entry: &HistoryEntry, max: usize) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("历史写入失败: {e}"))?;
        writeln!(f, "{line}").map_err(|e| format!("历史写入失败: {e}"))?;
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 512 * 1024 {
        let items = load(path, usize::MAX);
        let start = items.len().saturating_sub(max);
        let mut out = String::new();
        for it in &items[start..] {
            let l = serde_json::to_string(it).map_err(|e| e.to_string())?;
            out.push_str(&l);
            out.push('\n');
        }
        std::fs::write(path, out).map_err(|e| format!("历史写入失败: {e}"))?;
    }
    Ok(())
}

/// 读取全部(新的在后);损坏行跳过
pub fn load(path: &Path, limit: usize) -> Vec<HistoryEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<HistoryEntry>(line) {
            out.push(e);
        }
    }
    if out.len() > limit {
        out = out.split_off(out.len() - limit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("shujing_history_test_{tag}.jsonl"));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn append_and_load_roundtrip() {
        let p = tmp("rt");
        let e1 = HistoryEntry {
            ts: 1,
            conn: "本地库".into(),
            db: "shop".into(),
            sql: "SELECT * FROM users WHERE name='张三'".into(),
            ok: true,
            rows: 4,
            elapsed_ms: 12,
            ..Default::default()
        };
        let e2 = HistoryEntry {
            ts: 2,
            conn: "c2".into(),
            sql: "DELETE FROM logs".into(),
            ok: false,
            err: "权限不足".into(),
            ..Default::default()
        };
        append(&p, &e1, 100).unwrap();
        append(&p, &e2, 100).unwrap();
        let all = load(&p, usize::MAX);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].sql, e1.sql);
        assert_eq!(all[1].ok, false);
        assert_eq!(all[1].err, "权限不足");
        assert!(all[0].sql.contains("张三"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn scroll_limit_keeps_latest() {
        let p = tmp("scroll");
        for i in 0..10u64 {
            append(&p, &HistoryEntry { ts: i as i64, sql: format!("SELECT {i}"), ..Default::default() }, 5).unwrap();
        }
        // 追加不即时截断(max 表示 compaction 保留数);读取侧 load(limit) 限流
        let all = load(&p, usize::MAX);
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].sql, "SELECT 0");
        let limited = load(&p, 5);
        assert_eq!(limited.len(), 5);
        assert_eq!(limited[0].sql, "SELECT 5");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn compaction_triggers_on_size() {
        let p = tmp("compact");
        // 直接构造 >512KB 的历史文件(~6000 条)
        let big: Vec<String> = (0..12000u32)
            .map(|i| {
                format!(
                    "{{\"ts\":{i},\"conn\":\"c\",\"sql\":\"SELECT {i} padding padding padding padding\"}}"
                )
            })
            .collect();
        std::fs::write(&p, big.join("\n") + "\n").unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 512 * 1024);
        append(&p, &HistoryEntry { ts: 9999, sql: "after".into(), ..Default::default() }, 500).unwrap();
        let all = load(&p, usize::MAX);
        assert_eq!(all.len(), 500, "compaction 保留 500 条");
        assert_eq!(all[499].sql, "after", "最新条在尾");
        assert_eq!(std::fs::metadata(&p).unwrap().len() <= 512 * 1024, true, "体积回落");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupted_lines_skipped() {
        let p = tmp("bad");
        std::fs::write(&p, "not-json\n{\"ts\":1,\"sql\":\"SELECT 1\"}\n{}\n").unwrap();
        let all = load(&p, usize::MAX);
        // 非 JSON 行跳过;合法 JSON(哪怕字段不全)按 serde default 宽容解析
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].sql, "SELECT 1");
        let _ = std::fs::remove_file(&p);
    }
}
