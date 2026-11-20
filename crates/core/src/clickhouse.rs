//! ClickHouse 连接器(HTTP 接口 8123):查询 JSONCompact / 结构 system.columns /
//! 计数 / 建库表清单。reqwest 复用(与 ai.rs 同依赖),无专用驱动。
//! 事务语义缺失:回写按顺序逐条执行(CH 无 BEGIN;ALTER 变更是异步 mutation)。

use crate::model::{Backend, ColumnDef, TableStructure};
use serde_json::Value;

fn state_of(b: &mut Backend) -> Result<&mut crate::model::ClickHouseState, String> {
    match b {
        Backend::ClickHouse(st) => Ok(st),
        _ => Err("非 ClickHouse 连接".into()),
    }
}

/// 字符串字面量转义(' 与 \)
pub fn ch_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// 标识符反引号引注(防关键字/特殊字符)
pub fn ch_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "\\`"))
}

/// 首个 SQL 关键字(大写);-- 与 /* */ 注释剥除后取词
fn first_keyword(sql: &str) -> String {
    let mut s = sql.trim().to_string();
    loop {
        if let Some(rest) = s.strip_prefix("--") {
            s = match rest.find('\n') {
                Some(i) => rest[i + 1..].to_string(),
                None => String::new(),
            };
            continue;
        }
        if let Some(rest) = s.strip_prefix("/*") {
            s = match rest.find("*/") {
                Some(i) => rest[i + 2..].to_string(),
                None => String::new(),
            };
            continue;
        }
        break;
    }
    s.split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase()
}

fn is_select_kw(kw: &str) -> bool {
    matches!(kw, "SELECT" | "WITH" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN")
}

/// JSONCompact 单元格值 → 网格文本(NULL→None;嵌套/数组走紧凑 JSON)
pub fn cell_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => serde_json::to_string(other).ok(),
    }
}

/// JSONCompact 响应 → (列名, 行);meta.name / data[[...]]
pub fn parse_json_compact(body: &str, max: usize) -> Result<(Vec<String>, Vec<Vec<Option<String>>>, bool), String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("ClickHouse 响应解析失败: {e}"))?;
    let columns: Vec<String> = v["meta"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| m["name"].as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    let all = v["data"].as_array().cloned().unwrap_or_default();
    let truncated = all.len() > max;
    let rows = all
        .into_iter()
        .take(max)
        .map(|r| {
            r.as_array()
                .map(|cells| cells.iter().map(cell_text).collect())
                .unwrap_or_default()
        })
        .collect();
    Ok((columns, rows, truncated))
}

async fn exec_text(
    st: &crate::model::ClickHouseState,
    sql: &str,
    database: Option<&str>,
) -> Result<String, String> {
    let mut url = st.base.clone();
    if let Some(d) = database.map(str::trim).filter(|s| !s.is_empty()) {
        url = with_database_param(&url, d);
    }
    let mut req = st.http.post(url);
    if let Some((u, p)) = &st.auth {
        req = req.header("X-ClickHouse-User", u).header("X-ClickHouse-Key", p);
    }
    let resp = req
        .body(sql.to_string())
        .send()
        .await
        .map_err(|e| format!("ClickHouse: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("ClickHouse: {e}"))?;
    if !status.is_success() {
        return Err(format!("ClickHouse: {}", text.trim()));
    }
    Ok(text)
}

/// base URL 上追加 database 参数(兼容 base 已带 query 的情况)
pub fn with_database_param(base: &str, db: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}database={}", urlencode(db))
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 查询/执行:SELECT 族追加 FORMAT JSONCompact 解析;其余语句按文本执行
pub async fn query(
    st: &crate::model::ClickHouseState,
    sql: &str,
    database: Option<&str>,
    max: usize,
) -> Result<crate::model::ExecResult, String> {
    let started = std::time::Instant::now();
    let kw = first_keyword(sql);
    if is_select_kw(&kw) {
        let body = exec_text(st, &format!("{sql}\nFORMAT JSONCompact"), database).await?;
        let (columns, rows, truncated) = parse_json_compact(&body, max)?;
        Ok(crate::model::ExecResult {
            columns,
            rows,
            affected: 0,
            truncated,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    } else {
        exec_text(st, sql, database).await?;
        Ok(crate::model::ExecResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            truncated: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

pub async fn list_databases(
    st: &crate::model::ClickHouseState,
) -> Result<Vec<String>, String> {
    let body = exec_text(
        st,
        "SELECT name FROM system.databases WHERE name NOT IN ('system','information_schema','INFORMATION_SCHEMA') ORDER BY name FORMAT JSONCompact",
        None,
    )
    .await?;
    let (_, rows, _) = parse_json_compact(&body, 10_000)?;
    Ok(rows.into_iter().flatten().flatten().collect())
}

pub async fn list_tables(
    st: &crate::model::ClickHouseState,
    database: &str,
) -> Result<Vec<crate::model::TableMeta>, String> {
    let body = exec_text(
        st,
        &format!(
            "SELECT name, engine FROM system.tables WHERE database = '{}' ORDER BY name FORMAT JSONCompact",
            ch_quote(database)
        ),
        None,
    )
    .await?;
    let (_, rows, _) = parse_json_compact(&body, 10_000)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let name = r.first().cloned().flatten().unwrap_or_default();
            let is_view = r
                .get(1)
                .and_then(|c| c.as_ref())
                .map(|e| e.to_lowercase().contains("view"))
                .unwrap_or(false);
            crate::model::TableMeta {
                name,
                kind: if is_view { "view" } else { "table" }.into(),
                owner: None,
            }
        })
        .collect())
}

pub async fn structure(
    st: &crate::model::ClickHouseState,
    database: Option<&str>,
    table: &str,
) -> Result<TableStructure, String> {
    let db = database
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未指定数据库".to_string())?;
    let sql = format!(
        "SELECT name, type, is_in_primary_key, comment, default_kind, default_expression \
         FROM system.columns WHERE database = '{}' AND table = '{}' ORDER BY position FORMAT JSONCompact",
        ch_quote(db),
        ch_quote(table)
    );
    let body = exec_text(st, &sql, None).await?;
    let (_, rows, _) = parse_json_compact(&body, 5_000)?;
    let mut columns = Vec::with_capacity(rows.len());
    for r in rows {
        let get = |i: usize| -> Option<String> { r.get(i).cloned().flatten() };
        let name = get(0).unwrap_or_default();
        let data_type = get(1).unwrap_or_default();
        // Nullable(…) 类型 + 默认表达式 DEFAULT/MATERIALIZED/ALIAS 语义收敛
        let nullable = data_type.starts_with("Nullable(")
            || get(4).as_deref().map(|k| k == "ALIAS").unwrap_or(false);
        let default = get(5).filter(|e| !e.is_empty()).map(|e| {
            format!("{} {}", get(4).unwrap_or_default(), e)
        });
        columns.push(ColumnDef {
            name,
            data_type,
            nullable,
            key: if get(2).as_deref() == Some("1") {
                "PRI".into()
            } else {
                String::new()
            },
            default,
            extra: String::new(),
            comment: get(3).unwrap_or_default(),
        });
    }
    Ok(TableStructure {
        columns,
        indexes: vec![],
        ddl: String::new(),
    })
}

pub async fn count(
    st: &crate::model::ClickHouseState,
    database: Option<&str>,
    table: &str,
) -> Result<u64, String> {
    // 无显式库时依赖连接 URL 上的默认库参数
    let full = match database.map(str::trim).filter(|s| !s.is_empty()) {
        Some(db) => format!("{}.{}", ch_ident(db), ch_ident(table)),
        None => ch_ident(table),
    };
    let body = exec_text(
        st,
        &format!("SELECT count() FROM {full} FORMAT JSONCompact"),
        None,
    )
    .await?;
    let (_, rows, _) = parse_json_compact(&body, 10)?;
    Ok(rows
        .first()
        .and_then(|r| r.first())
        .and_then(|c| c.as_ref())
        .and_then(|s| s.replace(',', "").parse().ok())
        .unwrap_or(0))
}

#[cfg(test)]
mod ch_tests {
    use super::*;

    #[test]
    fn keyword_detection() {
        assert_eq!(first_keyword("  select 1"), "SELECT");
        assert_eq!(first_keyword("-- c\nINSERT INTO t"), "INSERT");
        assert_eq!(first_keyword("/* x */ SHOW TABLES"), "SHOW");
        assert_eq!(first_keyword("WITH t AS (SELECT 1) SELECT * FROM t"), "WITH");
        assert!(is_select_kw("DESCRIBE"));
        assert!(!is_select_kw("ALTER"));
    }

    #[test]
    fn quote_and_ident() {
        assert_eq!(ch_quote("a'b\\c"), "a\\'b\\\\c");
        assert_eq!(ch_ident("or`der"), "`or\\`der`");
    }

    #[test]
    fn cell_mapping() {
        assert_eq!(cell_text(&Value::Null), None);
        assert_eq!(cell_text(&serde_json::json!("张三")), Some("张三".into()));
        assert_eq!(cell_text(&serde_json::json!(42)), Some("42".into()));
        assert_eq!(cell_text(&serde_json::json!(1.5)), Some("1.5".into()));
        assert_eq!(
            cell_text(&serde_json::json!([1, "a"])),
            Some("[1,\"a\"]".into())
        );
    }

    #[test]
    fn parse_compact_response() {
        let body = r#"{"meta":[{"name":"id","type":"UInt32"},{"name":"name","type":"String"}],
            "data":[[1,"张三"],[2,null],[3,"李四"],[4,"王五"]],"rows":4,"statistics":{}}"#;
        let (cols, rows, truncated) = parse_json_compact(body, 3).unwrap();
        assert_eq!(cols, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(rows.len(), 3);
        assert!(truncated);
        assert_eq!(rows[1][1], None);
        assert_eq!(rows[0][1].as_deref(), Some("张三"));
    }

    #[test]
    fn parse_compact_bad_json() {
        assert!(parse_json_compact("boom", 10).is_err());
    }

    #[test]
    fn urlencode_basic() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
        assert_eq!(urlencode("数据库"), "%E6%95%B0%E6%8D%AE%E5%BA%93");
    }
}