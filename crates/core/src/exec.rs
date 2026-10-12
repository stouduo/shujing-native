use crate::model::{es, Backend, ExecResult};
use std::time::Instant;

pub const DEFAULT_MAX_ROWS: usize = 1000;
pub const HARD_MAX_ROWS: usize = 50_000;

fn fmt_f64(v: f64) -> String {
    if v.is_finite() { format!("{v:?}") } else { v.to_string() }
}

pub fn fmt_mysql_value(value: &mysql_async::Value) -> Option<String> {
    use mysql_async::Value;
    Some(match value {
        Value::NULL => return None,
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            Err(_) => format!("<BLOB {} B>", b.len()),
        },
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => fmt_f64(*v as f64),
        Value::Double(v) => fmt_f64(*v),
        Value::Date(y, mo, d, h, mi, s, us) => {
            let mut out = format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}");
            if *us > 0 { out.push_str(&format!(".{us:06}")); }
            out
        }
        Value::Time(neg, d, h, mi, s, us) => {
            let mut out = if *neg { "-".to_string() } else { String::new() };
            if *d > 0 { out.push_str(&format!("{d}d ")); }
            out.push_str(&format!("{h:02}:{mi:02}:{s:02}"));
            if *us > 0 { out.push_str(&format!(".{us:06}")); }
            out
        }
    })
}

fn one(columns: Vec<String>, rows: Vec<Vec<Option<String>>>, truncated: bool, affected: u64, started: Instant) -> Vec<ExecResult> {
    vec![ExecResult {
        columns,
        rows,
        affected,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }]
}

/// 按分号拆分多语句,正确跳过字符串/标识符引号、注释,以及
/// BEGIN...END 块内的分号(触发器/存储过程体)。
fn split_statements(sql: &str) -> Vec<String> {
    let cs: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = '\0';
    let mut depth: i32 = 0;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if quote != '\0' {
            cur.push(c);
            if c == quote {
                if cs.get(i + 1) == Some(&quote) {
                    cur.push(quote);
                    i += 1;
                } else {
                    quote = '\0';
                }
            }
            i += 1;
            continue;
        }
        if c.is_alphabetic() {
            let mut w = String::new();
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                w.push(cs[i]);
                i += 1;
            }
            match w.to_ascii_uppercase().as_str() {
                "BEGIN" => depth += 1,
                "END" => depth = (depth - 1).max(0),
                _ => {}
            }
            cur.push_str(&w);
            continue;
        }
        match c {
            '\'' | '"' | '`' => {
                quote = c;
                cur.push(c);
            }
            '-' if cs.get(i + 1) == Some(&'-') => {
                while i < cs.len() && cs[i] != '\n' {
                    cur.push(cs[i]);
                    i += 1;
                }
                continue;
            }
            '/' if cs.get(i + 1) == Some(&'*') => {
                cur.push('/');
                cur.push('*');
                i += 2;
                while i + 1 < cs.len() && !(cs[i] == '*' && cs[i + 1] == '/') {
                    cur.push(cs[i]);
                    i += 1;
                }
                cur.push('*');
                cur.push('/');
                i += 2;
                continue;
            }
            ';' if depth == 0 => {
                let t = cur.trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    out
}

fn run_sqlite(conn: &rusqlite::Connection, sql: &str, max: usize) -> Result<Vec<ExecResult>, String> {
    // 多语句:逐条执行并聚合结果集
    let stmts = split_statements(sql);
    if stmts.len() > 1 {
        let started = Instant::now();
        let mut results = Vec::new();
        let mut budget = max;
        for s in &stmts {
            let mut r = run_sqlite(conn, s, budget)?.remove(0);
            if !r.columns.is_empty() {
                budget = budget.saturating_sub(r.rows.len());
            }
            r.elapsed_ms = 0;
            results.push(r);
        }
        let elapsed = started.elapsed().as_millis() as u64;
        for r in &mut results {
            r.elapsed_ms = elapsed;
        }
        return Ok(results);
    }
    let sql = stmts.into_iter().next().unwrap_or_default();
    let sql = sql.as_str();
    let started = Instant::now();
    match conn.prepare(sql) {
        Ok(mut stmt) => {
            if stmt.column_count() > 0 {
                let n = stmt.column_count();
                let columns: Vec<String> = (0..n)
                    .map(|i| stmt.column_name(i).map(str::to_string).unwrap_or_else(|_| "?".into()))
                    .collect();
                let mut out: Vec<Vec<Option<String>>> = Vec::new();
                let mut truncated = false;
                let mut rows = stmt.query([]).map_err(es)?;
                while let Some(row) = rows.next().map_err(es)? {
                    if out.len() >= max { truncated = true; break; }
                    let mut vals = Vec::with_capacity(n);
                    for i in 0..n {
                        vals.push(match row.get_ref(i).map_err(es)? {
                            rusqlite::types::ValueRef::Null => None,
                            rusqlite::types::ValueRef::Integer(v) => Some(v.to_string()),
                            rusqlite::types::ValueRef::Real(v) => Some(fmt_f64(v)),
                            rusqlite::types::ValueRef::Text(t) => {
                                Some(String::from_utf8_lossy(t).into_owned())
                            }
                            rusqlite::types::ValueRef::Blob(b) => Some(format!("<BLOB {} B>", b.len())),
                        });
                    }
                    out.push(vals);
                }
                let affected = out.len() as u64;
                Ok(one(columns, out, truncated, affected, started))
            } else {
                let affected = stmt.execute([]).map_err(es)? as u64;
                Ok(one(vec![], vec![], false, affected, started))
            }
        }
        Err(e) => {
            if sql.contains(';') {
                conn.execute_batch(sql).map_err(|e2| format!("{e2}"))?;
                Ok(one(vec![], vec![], false, 0, started))
            } else {
                Err(es(e))
            }
        }
    }
}

async fn run_mysql_single(conn: &mut mysql_async::Conn, sql: &str, max: usize) -> Result<ExecResult, String> {
    use mysql_async::prelude::*;
    let started = Instant::now();
    let mut result = conn.query_iter(sql).await.map_err(es)?;
    let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().to_string()).collect();
    if columns.is_empty() {
        let affected = result.affected_rows();
        result.drop_result().await.map_err(es)?;
        Ok(ExecResult {
            columns,
            rows: vec![],
            affected,
            truncated: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    } else {
        let mut out: Vec<Vec<Option<String>>> = Vec::new();
        let mut truncated = false;
        result
            .for_each(|row| {
                if out.len() < max {
                    out.push(row.unwrap().iter().map(fmt_mysql_value).collect());
                } else {
                    truncated = true;
                }
            })
            .await
            .map_err(es)?;
        let affected = out.len() as u64;
        Ok(ExecResult {
            columns,
            rows: out,
            affected,
            truncated,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

async fn run_mysql(conn: &mut mysql_async::Conn, sql: &str, max: usize) -> Result<Vec<ExecResult>, String> {
    // 多语句:拆分逐条执行聚合结果集(query_iter 只返回第一个结果集)
    let stmts = split_statements(sql);
    if stmts.len() > 1 {
        let started = Instant::now();
        let mut results = Vec::new();
        let mut budget = max;
        for s in &stmts {
            let mut r = run_mysql_single(conn, s, budget).await?;
            if !r.columns.is_empty() {
                budget = budget.saturating_sub(r.rows.len());
            }
            r.elapsed_ms = 0;
            results.push(r);
        }
        let elapsed = started.elapsed().as_millis() as u64;
        for r in &mut results {
            r.elapsed_ms = elapsed;
        }
        return Ok(results);
    }
    let r = run_mysql_single(conn, sql, max).await?;
    Ok(vec![r])
}

/// PostgreSQL simple_query 天然返回全部消息流,按 CommandComplete 分组为多个结果集
async fn run_pg(client: &tokio_postgres::Client, sql: &str, max: usize) -> Result<Vec<ExecResult>, String> {
    use tokio_postgres::SimpleQueryMessage;
    let started = Instant::now();
    let messages = client.simple_query(sql).await.map_err(es)?;
    let mut results: Vec<ExecResult> = Vec::new();
    let mut columns: Vec<String> = vec![];
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut truncated = false;

    for msg in messages {
        match msg {
            SimpleQueryMessage::Row(row) => {
                if columns.is_empty() {
                    columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                }
                if rows.len() >= max { truncated = true; continue; }
                let vals: Vec<Option<String>> =
                    (0..row.len()).map(|i| row.get(i).map(str::to_string)).collect();
                rows.push(vals);
            }
            SimpleQueryMessage::CommandComplete(n) => {
                let affected = if columns.is_empty() { n } else { rows.len() as u64 };
                results.push(ExecResult {
                    columns: std::mem::take(&mut columns),
                    rows: std::mem::take(&mut rows),
                    affected,
                    truncated,
                    elapsed_ms: 0,
                });
                truncated = false;
            }
            _ => {}
        }
    }
    let elapsed = started.elapsed().as_millis() as u64;
    // 末尾未闭合的结果组(防御性)
    if !columns.is_empty() || !rows.is_empty() {
        let affected = rows.len() as u64;
        results.push(ExecResult {
            columns,
            rows,
            affected,
            truncated,
            elapsed_ms: elapsed,
        });
    }
    for r in &mut results {
        r.elapsed_ms = elapsed;
    }
    if results.is_empty() {
        results.push(ExecResult {
            columns: vec![],
            rows: vec![],
            affected: 0,
            truncated: false,
            elapsed_ms: elapsed,
        });
    }
    Ok(results)
}

impl Backend {
    pub async fn run_sql(
        &mut self,
        sql: &str,
        max: usize,
        running_id: Option<&std::sync::atomic::AtomicU64>,
    ) -> Result<Vec<ExecResult>, String> {
        let max = max.clamp(1, HARD_MAX_ROWS);
        match self {
            // sqlite 驱动是同步的;本地文件查询通常很快,占用一个 runtime worker 可接受
            Backend::Sqlite(conn) => run_sqlite(conn, sql, max),
            Backend::MySql(mp) => {
                // 统一走 mysql_conn:自动恢复会话库上下文
                let mut conn = Backend::mysql_conn(mp).await?;
                // 记录会话 ID 供取消(查询结束时清零)
                if let Some(rid) = running_id {
                    rid.store(conn.id() as u64, std::sync::atomic::Ordering::Relaxed);
                }
                let r = run_mysql(&mut conn, sql, max).await;
                if let Some(rid) = running_id {
                    rid.store(0, std::sync::atomic::Ordering::Relaxed);
                }
                if r.is_ok() {
                    if let Some(db) = crate::model::extract_use_db(sql) {
                        mp.session_db = Some(db);
                    }
                }
                r
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                let r = run_pg(&client, sql, max).await;
                if r.is_ok() {
                    // SET search_path 语句更新会话上下文,后续取连接恢复
                    if let Some(db) = crate::model::extract_use_db(sql) {
                        mp.session_db = Some(db);
                        mp.applied.iter_mut().for_each(|a| *a = None);
                    }
                }
                r
            }
            Backend::Redis(_) => Err("Redis 请使用专门的命令通道".into()),
        }
    }

    /// 单结果便捷取用(编辑回写等只关心 affected)
    pub async fn run_one(&mut self, sql: &str, max: usize) -> Result<ExecResult, String> {
        let mut results = self.run_sql(sql, max, None).await?;
        if results.is_empty() {
            return Ok(ExecResult {
                columns: vec![],
                rows: vec![],
                affected: 0,
                truncated: false,
                elapsed_ms: 0,
            });
        }
        Ok(results.remove(0))
    }
}
