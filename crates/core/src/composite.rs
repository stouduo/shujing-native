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

/// 导入:按 col_map 拼 INSERT,单事务批量执行。返回写入行数。
pub async fn import_csv(
    service: &Arc<crate::Service>,
    info: ConnInfo,
    imp: CsvImport,
) -> Result<usize, String> {
    let (headers, rows) = parse_csv(&imp.path)?;
    if imp.has_header && rows.len() == 1 {
        return Err("除表头外没有数据行".into());
    }
    let data_rows = if imp.has_header { &rows[1..] } else { &rows[..] };

    let cols: Vec<String> = imp.col_map.iter().map(|(_, c)| c.clone()).collect();
    let total = data_rows.len();
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
                                        format!("'{}'", v.replace('\'', "''"))
                                    }
                                })
                                .unwrap_or_else(|| "NULL".into())
                        })
                        .collect();
                    format!(
                        "INSERT INTO {} ({}) VALUES ({});",
                        crate::edits::quote_ident_pub(&imp.database, &imp.table),
                        cols.iter()
                            .map(|c| crate::edits::quote_ident_pub(&imp.database, c))
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
    let n = service.count_rows(&info.id, table).await?;
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
