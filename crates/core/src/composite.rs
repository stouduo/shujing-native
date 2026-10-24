//! 组合服务:CSV 导入、表统计(原生版 UI 用,不经过 Tauri)。

use crate::model::ConnInfo;
use std::sync::Arc;

/// CSV 导入请求
pub struct CsvImport {
    pub path: String,
    pub database: Option<String>,
    pub table: String,
    /// 源列索引 -> 目标表列名
    pub col_map: Vec<(usize, String)>,
    /// 跳过首行表头
    pub has_header: bool,
}

/// 解析 CSV 文件:返回 (表头, 全部行)。支持引号包裹与转义。
pub fn parse_csv(path: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("读文件失败: {e}"))?;
    parse_csv_str(&raw)
}

pub fn parse_csv_str(raw: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    // UTF-8 BOM:不剥会让首列表头带 \u{FEFF},按名映射列时静默失配
    let raw = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => in_quotes = true,
                ',' => {
                    cur.push(std::mem::take(&mut field));
                }
                '\r' => {}
                '\n' => {
                    // 完全空的行(含文件末尾多余空行)跳过,不产出 [""] 行
                    if cur.is_empty() && field.is_empty() {
                        continue;
                    }
                    cur.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut cur));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !cur.is_empty() {
        cur.push(field);
        rows.push(cur);
    }
    if rows.is_empty() {
        return Err("文件为空".into());
    }
    let header = rows.remove(0).into_iter().map(|h| h.trim().to_string()).collect();
    Ok((header, rows))
}

/// 字符串字面量:单引号双写;MySQL 默认把反斜杠当转义符,须同时双写
fn literal_by(v: &str, mysql: bool) -> String {
    let mut out = v.replace('\'', "''");
    if mysql {
        out = out.replace('\\', "\\\\");
    }
    format!("'{out}'")
}

/// 导入:按 col_map 拼 INSERT,单事务批量执行。返回写入行数。
pub async fn import_csv(
    service: &Arc<crate::Service>,
    info: ConnInfo,
    imp: CsvImport,
) -> Result<usize, String> {
    // parse_csv 已把首行剥为表头;此前这里又切了一次 rows[1..],
    // 首条数据行永远丢失、且"恰好 1 行数据"被误判为没有数据行
    let (_, rows) = parse_csv(&imp.path)?;
    import_rows(service, info, imp, rows).await
}

/// 已解析的数据行 → 批量 INSERT(CSV/XLSX 共用)
pub async fn import_rows(
    service: &Arc<crate::Service>,
    info: ConnInfo,
    imp: CsvImport,
    rows: Vec<Vec<String>>,
) -> Result<usize, String> {
    let data_rows = &rows[..];
    if data_rows.is_empty() {
        return Err("没有数据行".into());
    }

    let mysql = info.db_type == crate::model::DbType::MySql;
    let cols: Vec<String> = imp.col_map.iter().map(|(_, c)| c.clone()).collect();
    const BATCH: usize = 200;
    let mut done = 0usize;
    for chunk in data_rows.chunks(BATCH) {
        let sql = format!(
            "BEGIN;\n{}\nCOMMIT;",
            chunk
                .iter()
                .map(|row| {
                    let list: Vec<String> = imp
                        .col_map
                        .iter()
                        .map(|(src, _)| {
                            row.get(*src)
                                .map(|v| {
                                    if v.is_empty() {
                                        "NULL".into()
                                    } else {
                                        literal_by(v, mysql)
                                    }
                                })
                                .unwrap_or_else(|| "NULL".into())
                        })
                        .collect();
                    format!(
                        "INSERT INTO {} ({}) VALUES ({});",
                        crate::edits::quote_ident_pub(&imp.database, &imp.table, mysql),
                        cols.iter()
                            .map(|c| crate::edits::quote_ident_pub(&imp.database, c, mysql))
                            .collect::<Vec<_>>()
                            .join(", "),
                        list.join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        );
        crate::Service::run_sql_standalone(service, &info, &sql)
            .await
            .map_err(|e| format!("第 {}~{} 行失败: {e}", done + 1, done + chunk.len()))?;
        done += chunk.len();
    }
    Ok(done)
}

/// 表统计:行数 + (可选)数据/索引长度(MySQL)
pub async fn table_stats(
    service: &Arc<crate::Service>,
    info: &ConnInfo,
    database: Option<String>,
    table: &str,
) -> Result<(u64, Option<u64>, Option<u64>), String> {
    // 行数按目标库限定(count_rows 不带库,多库连接下数错表)
    let mysql = info.db_type == crate::model::DbType::MySql;
    let full = match database.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(db) => crate::edits::qualified_table(table, Some(db), mysql),
        None => match info.db_type {
            crate::model::DbType::Postgres => format!("public.{}", crate::edits::qualified_table(table, None, false)),
            _ => crate::edits::qualified_table(table, None, mysql),
        },
    };
    let rs = crate::Service::run_sql_standalone(
        service,
        info,
        &format!("SELECT COUNT(*) FROM {full}"),
    )
    .await?;
    let n = rs
        .first()
        .and_then(|r| r.rows.first())
        .and_then(|row| row.first())
        .and_then(|v| v.as_deref())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if info.db_type == crate::model::DbType::MySql {
        let sql = format!(
            "SELECT data_length, index_length FROM information_schema.tables WHERE table_schema = '{}' AND table_name = '{}'",
            database.clone().unwrap_or_default().replace('\'', "''"),
            table.replace('\'', "''")
        );
        let rs = crate::Service::run_sql_standalone(service, info, &sql).await?;
        if let Some(r) = rs.first() {
            let get = |name: &str| -> Option<u64> {
                r.columns
                    .iter()
                    .position(|c| c == name)
                    .and_then(|i| r.rows.first().and_then(|row| row.get(i)))
                    .and_then(|v| v.as_deref())
                    .and_then(|s| s.parse().ok())
            };
            return Ok((n, get("data_length"), get("index_length")));
        }
    }
    Ok((n, None, None))
}

#[cfg(test)]
mod csv_tests {
    use super::*;

    #[test]
    fn parse_strips_bom_and_blank_lines() {
        let (header, rows) = parse_csv_str("\u{FEFF}id,name\n1,a\n\n2,b\n").unwrap();
        assert_eq!(header, vec!["id", "name"]);
        // 空行不产出 [""] 行;数据行完整(表头只剥一次)
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0], vec!["1", "a"]);
        assert_eq!(rows[1], vec!["2", "b"]);
    }

    #[test]
    fn quoted_newline_and_comma() {
        let (_, rows) = parse_csv_str("a,b\n\"x,y\",\"line1\nline2\"\n").unwrap();
        assert_eq!(rows[0][0], "x,y");
        assert_eq!(rows[0][1], "line1\nline2");
    }
}
