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

fn one(
    columns: Vec<String>,
    column_types: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
    truncated: bool,
    affected: u64,
    started: Instant,
) -> Vec<ExecResult> {
    vec![ExecResult {
        columns,
        rows,
        affected,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
        column_types,
    }]
}

/// SQLite 表达式列没有声明类型,按实际值推断:位次 NULL < INTEGER < REAL < TEXT < BLOB,
/// 同列出现多种值类型时取最宽的(整数列里混了小数 → REAL)。
fn widen_sqlite(ty: &mut Option<&'static str>, v: rusqlite::types::ValueRef<'_>) {
    use rusqlite::types::ValueRef;
    let (rank, name) = match v {
        ValueRef::Null => return,
        ValueRef::Integer(_) => (1, "INTEGER"),
        ValueRef::Real(_) => (2, "REAL"),
        ValueRef::Text(_) => (3, "TEXT"),
        ValueRef::Blob(_) => (4, "BLOB"),
    };
    if ty.map_or(0, |t| sqlite_rank(t)) < rank {
        *ty = Some(name);
    }
}

fn sqlite_rank(ty: &str) -> u8 {
    match ty {
        "INTEGER" => 1,
        "REAL" => 2,
        "TEXT" => 3,
        "BLOB" => 4,
        _ => 0,
    }
}

/// 按分号拆分多语句,正确跳过字符串/标识符引号、注释,以及
/// BEGIN...END 块内的分号(触发器/存储过程体)。
/// 多语句切分(字符串/注释内的分号保护);pub 供只读拦截逐条校验
pub fn split_statements(sql: &str) -> Vec<String> {
    let cs: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = '\0';
    let mut depth: i32 = 0;
    // 过程体上下文:CREATE TRIGGER/PROCEDURE/FUNCTION 之后的 BEGIN..END 块
    let mut in_routine = false;
    let mut pending_routine = false;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if quote != '\0' {
            cur.push(c);
            // MySQL 转义:'a\'; b' 里的 \' 不闭合字符串(PG 标准串是字面反斜杠,
            // 误吞闭合只会让只读校验偏保守,安全方向)
            if c == '\\' && (quote == '\'' || quote == '"') && cs.get(i + 1).is_some_and(|&n| n == quote || n == '\\') {
                cur.push(cs[i + 1]);
                i += 2;
                continue;
            }
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
        // mysqldump 的 DELIMITER 行:不是语句,原样吞掉(否则当 SQL 发给服务器必报错)
        if depth == 0 && (c == 'D' || c == 'd') {
            let head: String = cs[i..(i + 9).min(cs.len())].iter().collect::<String>()
                .to_ascii_lowercase();
            if head == "delimiter" && cs.get(i + 9).is_some_and(|&n| n.is_whitespace()) {
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                }
                continue;
            }
        }
        if c.is_alphabetic() {
            let mut w = String::new();
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                w.push(cs[i]);
                i += 1;
            }
            let upper = w.to_ascii_uppercase();
            if in_routine {
                // 存储过程/触发器体内:BEGIN..END / CASE..END / IF..END IF / LOOP..END LOOP
                // 每个开场对应一个 END(表达式里的 IF() 函数没有 END,不受影响)
                match upper.as_str() {
                    "BEGIN" | "CASE" | "IF" | "LOOP" | "WHILE" | "REPEAT" => depth += 1,
                    "END" => {
                        depth = (depth - 1).max(0);
                        // 过程体闭合:回到普通语句模式(同脚本后续 BEGIN 是事务)
                        if depth == 0 {
                            in_routine = false;
                        }
                    }
                    _ => {}
                }
            } else {
                match upper.as_str() {
                    // CREATE TRIGGER/PROCEDURE/FUNCTION 进入"过程体"上下文,
                    // 之后第一个 BEGIN 开始按块计数
                    "CREATE" => pending_routine = true,
                    // 事务 BEGIN/COMMIT 是普通语句边界,不再粘连后续语句
                    _ => {}
                }
                if pending_routine && upper == "BEGIN" {
                    depth += 1;
                    in_routine = true;
                    pending_routine = false;
                }
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
                pending_routine = false;
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
                // 类型:表列用声明类型;表达式列声明为空,读行时按值推断
                let decl: Vec<Option<String>> = stmt
                    .columns()
                    .iter()
                    .map(|c| c.decl_type().map(str::to_owned).filter(|t| !t.is_empty()))
                    .collect();
                let mut inferred = vec![None::<&'static str>; n];
                let mut out: Vec<Vec<Option<String>>> = Vec::new();
                let mut truncated = false;
                let mut rows = stmt.query([]).map_err(es)?;
                while let Some(row) = rows.next().map_err(es)? {
                    if out.len() >= max { truncated = true; break; }
                    let mut vals = Vec::with_capacity(n);
                    for i in 0..n {
                        let vr = row.get_ref(i).map_err(es)?;
                        widen_sqlite(&mut inferred[i], vr);
                        vals.push(match vr {
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
                let types = (0..n)
                    .map(|i| {
                        decl[i]
                            .clone()
                            .or_else(|| inferred[i].map(str::to_owned))
                            .unwrap_or_default()
                    })
                    .collect();
                Ok(one(columns, types, out, truncated, affected, started))
            } else {
                let affected = stmt.execute([]).map_err(es)? as u64;
                Ok(one(vec![], vec![], vec![], false, affected, started))
            }
        }
        Err(e) => {
            if sql.contains(';') {
                conn.execute_batch(sql).map_err(|e2| format!("{e2}"))?;
                Ok(one(vec![], vec![], vec![], false, 0, started))
            } else {
                Err(es(e))
            }
        }
    }
}

/// MySQL 列类型码 → 类型名(小写,写法同 information_schema.COLUMNS.DATA_TYPE)。
///
/// 文本协议回包的类型码比 information_schema 粗,三处得靠别的字段区分
/// (规则取自 MySQL 8.4 实测,见 live_mysql_reports_server_column_types):
/// - 是不是二进制数据看字符集编号 63(binary),不看 BINARY_FLAG——时间、JSON、
///   utf8mb4_bin 排序规则的 VARCHAR 都带 BINARY_FLAG;
/// - ENUM / SET 与 CHAR 共用 MYSQL_TYPE_STRING,靠 ENUM_FLAG / SET_FLAG;
/// - TEXT / BLOB 各四档,表列多数只回裸 MYSQL_TYPE_BLOB,靠 column_length 分档。
/// 变体逐条列全:驱动新增类型时编译报错,而不是静默变空串。
fn mysql_type_name(c: &mysql_async::Column) -> String {
    use mysql_async::consts::ColumnFlags as F;
    use mysql_async::consts::ColumnType as CT;
    /// binary 字符集的编号
    const BINARY_CHARSET: u16 = 63;
    let ct = c.column_type();
    let flags = c.flags();
    let binary = c.character_set() == BINARY_CHARSET;
    let len = u64::from(c.column_length());
    // 大对象四档:类型码自带档位的以码为准(GROUP_CONCAT 回 LONG_BLOB);
    // 文本档的上限按服务器给的显示宽度(已含 utf8mb4 的 4 倍)
    let (lobs, limits): ([&str; 4], [u64; 3]) = if binary {
        (["tinyblob", "blob", "mediumblob", "longblob"], [255, 65_535, 16_777_215])
    } else {
        (["tinytext", "text", "mediumtext", "longtext"], [1020, 262_140, 67_108_860])
    };
    let lobs_by_len = || lobs[limits.iter().position(|l| len <= *l).unwrap_or(3)];
    let name: &str = match ct {
        CT::MYSQL_TYPE_DECIMAL | CT::MYSQL_TYPE_NEWDECIMAL => "decimal",
        CT::MYSQL_TYPE_TINY => "tinyint",
        CT::MYSQL_TYPE_SHORT => "smallint",
        CT::MYSQL_TYPE_LONG => "int",
        CT::MYSQL_TYPE_INT24 => "mediumint",
        CT::MYSQL_TYPE_LONGLONG => "bigint",
        CT::MYSQL_TYPE_FLOAT => "float",
        CT::MYSQL_TYPE_DOUBLE => "double",
        CT::MYSQL_TYPE_BIT => "bit",
        CT::MYSQL_TYPE_YEAR => "year",
        CT::MYSQL_TYPE_TIMESTAMP | CT::MYSQL_TYPE_TIMESTAMP2 => "timestamp",
        CT::MYSQL_TYPE_DATETIME | CT::MYSQL_TYPE_DATETIME2 => "datetime",
        CT::MYSQL_TYPE_DATE | CT::MYSQL_TYPE_NEWDATE => "date",
        CT::MYSQL_TYPE_TIME | CT::MYSQL_TYPE_TIME2 => "time",
        CT::MYSQL_TYPE_JSON => "json",
        CT::MYSQL_TYPE_ENUM => "enum",
        CT::MYSQL_TYPE_SET => "set",
        CT::MYSQL_TYPE_GEOMETRY => "geometry",
        CT::MYSQL_TYPE_VECTOR => "vector",
        CT::MYSQL_TYPE_STRING => {
            if flags.contains(F::ENUM_FLAG) {
                "enum"
            } else if flags.contains(F::SET_FLAG) {
                "set"
            } else if binary {
                "binary"
            } else {
                "char"
            }
        }
        CT::MYSQL_TYPE_VARCHAR | CT::MYSQL_TYPE_VAR_STRING => {
            if binary { "varbinary" } else { "varchar" }
        }
        CT::MYSQL_TYPE_TINY_BLOB => lobs[0],
        CT::MYSQL_TYPE_MEDIUM_BLOB => lobs[2],
        CT::MYSQL_TYPE_LONG_BLOB => lobs[3],
        CT::MYSQL_TYPE_BLOB => lobs_by_len(),
        // 字面 NULL / 复制专用 / 未知:没有可报的类型
        CT::MYSQL_TYPE_NULL | CT::MYSQL_TYPE_TYPED_ARRAY | CT::MYSQL_TYPE_UNKNOWN => "",
    };
    let signed = matches!(
        ct,
        CT::MYSQL_TYPE_TINY
            | CT::MYSQL_TYPE_SHORT
            | CT::MYSQL_TYPE_INT24
            | CT::MYSQL_TYPE_LONG
            | CT::MYSQL_TYPE_LONGLONG
            | CT::MYSQL_TYPE_DECIMAL
            | CT::MYSQL_TYPE_NEWDECIMAL
            | CT::MYSQL_TYPE_FLOAT
            | CT::MYSQL_TYPE_DOUBLE
    );
    if name.is_empty() {
        String::new()
    } else if signed && flags.contains(F::UNSIGNED_FLAG) {
        format!("{name} unsigned")
    } else {
        name.to_owned()
    }
}

async fn run_mysql_single(
    conn: &mut mysql_async::Conn,
    sql: &str,
    max: usize,
    cancel: Option<&crate::model::CancelState>,
) -> Result<ExecResult, String> {
    use mysql_async::prelude::*;
    let started = Instant::now();
    let mut result = conn.query_iter(sql).await.map_err(es)?;
    let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().to_string()).collect();
    let column_types: Vec<String> = result.columns_ref().iter().map(mysql_type_name).collect();
    if columns.is_empty() {
        let affected = result.affected_rows();
        result.drop_result().await.map_err(es)?;
        Ok(ExecResult {
            columns,
            rows: vec![],
            affected,
            truncated: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
            column_types,
        })
    } else {
        let mut out: Vec<Vec<Option<String>>> = Vec::new();
        let mut truncated = false;
        // 截断即停:首次超限 KILL QUERY 自己的连接(与 PG 超限带外取消同语义),
        // 服务端立即停发;连接作废由 mysql_async 池重建
        loop {
            match result.next().await {
                Ok(Some(row)) => {
                    if out.len() < max {
                        out.push(
                            (0..row.len())
                                .map(|i| row.as_ref(i).and_then(|v| fmt_mysql_value(v)))
                                .collect(),
                        );
                    } else {
                        truncated = true;
                        if let Some(cs) = cancel {
                            let cid = cs.mysql_id.load(std::sync::atomic::Ordering::Relaxed);
                            if cid != 0 {
                                let cs2 = cs.clone();
                                let sid = cid.to_string();
                                tokio::spawn(async move {
                                    let _ = Backend::kill_session_standalone(&cs2, &sid, true).await;
                                });
                            }
                        }
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if truncated {
                        // KILL 生效:连接报错视为自然终止
                        break;
                    }
                    return Err(es(e));
                }
            }
        }
        let affected = out.len() as u64;
        Ok(ExecResult {
            columns,
            rows: out,
            affected,
            truncated,
            elapsed_ms: started.elapsed().as_millis() as u64,
            column_types,
        })
    }
}

async fn run_mysql(
    conn: &mut mysql_async::Conn,
    sql: &str,
    max: usize,
    cancel: Option<&crate::model::CancelState>,
) -> Result<Vec<ExecResult>, String> {
    // 多语句:拆分逐条执行聚合结果集(query_iter 只返回第一个结果集)
    let stmts = split_statements(sql);
    if stmts.len() > 1 {
        let started = Instant::now();
        let mut results = Vec::new();
        let mut budget = max;
        for s in &stmts {
            let mut r = run_mysql_single(conn, s, budget, cancel).await?;
            if !r.columns.is_empty() {
                budget = budget.saturating_sub(r.rows.len());
            }
            results.push(r);
        }
        let elapsed = started.elapsed().as_millis() as u64;
        for r in &mut results {
            r.elapsed_ms = elapsed;
        }
        return Ok(results);
    }
    let r = run_mysql_single(conn, sql, max, cancel).await?;
    Ok(vec![r])
}

/// PostgreSQL:single_query_raw 流式逐行消费(文本协议,列值天然为 str)。
/// 达到行数上限即取消服务端执行(cancel_token 走带外连接),再把流排空到
/// ReadyForQuery——残留消息会毒化连接的下一个查询,必须读干净。
/// 大表浏览不再整体物化(此前 simple_query 一次性收全量,千万行 OOM)。
/// 多语句仍走 simple_query 一次性执行:组间原子语义;cancel 会中断后续语句,不能用。
async fn run_pg(client: &tokio_postgres::Client, sql: &str, max: usize) -> Result<Vec<ExecResult>, String> {
    use tokio_postgres::SimpleQueryMessage;
    use futures_util::StreamExt;
    let started = Instant::now();

    // ── 多语句路径(原物化逻辑,语义不变) ──
    if split_statements(sql).len() > 1 {
        let messages = client.simple_query(sql).await.map_err(crate::backend::pg_err)?;
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
                    if rows.len() >= max {
                        truncated = true;
                        continue;
                    }
                    rows.push((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect());
                }
                SimpleQueryMessage::CommandComplete(n) => {
                    let affected = if columns.is_empty() { n } else { rows.len() as u64 };
                    results.push(ExecResult {
                        columns: std::mem::take(&mut columns),
                        rows: std::mem::take(&mut rows),
                        affected,
                        truncated,
                        elapsed_ms: 0,
                        // PG 驱动的简单查询协议只保留列名(类型 OID 被丢弃),类型留空
                        column_types: vec![],
                    });
                    truncated = false;
                }
                _ => {}
            }
        }
        if !columns.is_empty() || !rows.is_empty() {
            let affected = rows.len() as u64;
            results.push(ExecResult {
                columns,
                rows,
                affected,
                truncated,
                elapsed_ms: 0,
                column_types: vec![],
            });
        }
        let elapsed = started.elapsed().as_millis() as u64;
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
                column_types: vec![],
            });
        }
        return Ok(results);
    }

    // ── 单条语句:流式逐行,超限即取消 ──
    let stream = client
        .simple_query_raw(sql)
        .await
        .map_err(crate::backend::pg_err)?;
    futures_util::pin_mut!(stream);
    let mut results: Vec<ExecResult> = Vec::new();
    let mut columns: Vec<String> = vec![];
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut truncated = false;

    loop {
        match stream.next().await {
            Some(Ok(SimpleQueryMessage::RowDescription(cols))) => {
                columns = cols.iter().map(|c| c.name().to_string()).collect();
            }
            Some(Ok(SimpleQueryMessage::Row(row))) => {
                if rows.len() >= max {
                    truncated = true;
                    break;
                }
                rows.push((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect());
            }
            Some(Ok(SimpleQueryMessage::CommandComplete(n))) => {
                let affected = if columns.is_empty() { n } else { rows.len() as u64 };
                results.push(ExecResult {
                    columns: std::mem::take(&mut columns),
                    rows: std::mem::take(&mut rows),
                    affected,
                    truncated: false,
                    elapsed_ms: 0,
                    column_types: vec![],
                });
            }
            Some(Err(e)) => return Err(crate::backend::pg_err(e)),
            Some(Ok(_)) => {} // 其他消息类型:忽略(协议完备性兜底)
            None => break,
        }
    }
    if truncated {
        // 带外取消服务端查询(独立连接发 CancelRequest);失败也无妨——
        // 排空循环会把剩余行读完即弃,内存有界,连接最终回到 ReadyForQuery
        let _ = client
            .cancel_token()
            .cancel_query(tokio_postgres::NoTls)
            .await;
        while let Some(m) = stream.next().await {
            let _ = m;
        }
    }
    if !columns.is_empty() || !rows.is_empty() {
        let affected = rows.len() as u64;
        results.push(ExecResult {
            columns,
            rows,
            affected,
            truncated,
            elapsed_ms: 0,
            column_types: vec![],
        });
    }
    let elapsed = started.elapsed().as_millis() as u64;
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
            column_types: vec![],
        });
    }
    Ok(results)
}

/// 插件响应里的字符串列(如 `columnTypes`):补齐/截断到 `n` 个,缺的为空串。
/// JSON 输出按列数对齐,界面按下标取值不必再判长短。非字符串项原位留空——
/// 跳过它会让后面的类型整体错位一格。
pub fn json_str_column(v: &serde_json::Value, key: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = v
        .get(key)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect())
        .unwrap_or_default();
    out.resize(n, String::new());
    out
}

impl Backend {
    pub async fn run_sql(
        &mut self,
        sql: &str,
        max: usize,
        running_id: Option<&std::sync::atomic::AtomicU64>,
        cancel: Option<&crate::model::CancelState>,
    ) -> Result<Vec<ExecResult>, String> {
        let max = max.clamp(1, HARD_MAX_ROWS);
        match self {
            // sqlite 驱动是同步的;本地文件查询通常很快,占用一个 runtime worker 可接受
            Backend::Sqlite(conn) => run_sqlite(conn, sql, max),
            Backend::Plugin(s) => {
                let started2 = Instant::now();
                let params = serde_json::json!({
                    "database": s.database,
                    "sql": sql,
                    "max": max,
                });
                let r = s.call("query", &params).await?;
                let columns: Vec<String> = r
                    .get("columns")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                let rows: Vec<Vec<Option<String>>> = r
                    .get("rows")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|row| {
                                row.as_array()
                                    .map(|cells| {
                                        cells
                                            .iter()
                                            .map(|c| c.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let affected = r.get("affected").and_then(|v| v.as_u64()).unwrap_or(0);
                let truncated = r.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
                // 可选字段:插件给不出类型的列留空(契约 v1.2 起)
                let column_types = json_str_column(&r, "columnTypes", columns.len());
                return Ok(vec![ExecResult {
                    columns,
                    rows,
                    affected,
                    truncated,
                    elapsed_ms: started2.elapsed().as_millis() as u64,
                    column_types,
                }]);
            }
            Backend::MySql(mp) => {
                // 统一走 mysql_conn:自动恢复会话库上下文
                let mut conn = Backend::mysql_conn(mp).await?;
                // 记录会话 ID 供取消(查询结束时清零);代数戳同步递增,
                // 取消侧开好控制连接后核对代数,旧 KILL 不误伤新查询
                mp.gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(rid) = running_id {
                    rid.store(conn.id() as u64, std::sync::atomic::Ordering::Relaxed);
                }
                let r = run_mysql(&mut conn, sql, max, cancel).await;
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
            #[cfg(feature = "heavy-mw")]
            Backend::Mongo(_) => {
                Err("MongoDB 不支持 SQL 查询(浏览集合用数据页;过滤输入 JSON 对象)".into())
            }
            Backend::Mssql(st) => {
                let r = crate::mssql::query(st, sql, max).await?;
                Ok(vec![r])
            }
            Backend::ClickHouse(st) => {
                let r = crate::clickhouse::query(st, sql, None, max).await?;
                Ok(vec![r])
            }
            // 中间件族(Kafka/Rabbit/Mqtt/后续新增)统一无 SQL 执行面
            Backend::Rabbit(_)
            | Backend::Redis(_)
            | Backend::Kv(_) => Err("该数据源无 SQL 执行面(浏览请用专属控制台)".into()),
            #[cfg(feature = "heavy-mw")]
            Backend::Kafka(_) | Backend::Mqtt(_) => {
                Err("该数据源无 SQL 执行面(浏览请用专属控制台)".into())
            }
        }
    }

    /// 单结果便捷取用(编辑回写等只关心 affected)
    pub async fn run_one(&mut self, sql: &str, max: usize) -> Result<ExecResult, String> {
        let mut results = self.run_sql(sql, max, None, None).await?;
        if results.is_empty() {
            return Ok(ExecResult {
                columns: vec![],
                rows: vec![],
                affected: 0,
                truncated: false,
                elapsed_ms: 0,
                column_types: vec![],
            });
        }
        Ok(results.remove(0))
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn transaction_begin_does_not_glue() {
        // 事务 BEGIN 此前被当块开头,整段粘成一条导致结果集丢失
        let ss = split_statements("BEGIN; UPDATE t SET a=1; COMMIT; SELECT 2;");
        assert_eq!(ss.len(), 4, "{ss:?}");
        assert_eq!(ss[3], "SELECT 2");
    }

    #[test]
    fn routine_body_kept_intact() {
        let sql = "CREATE TRIGGER trg AFTER INSERT ON t FOR EACH ROW BEGIN INSERT INTO log VALUES (1); END; SELECT 1;";
        let ss = split_statements(sql);
        assert_eq!(ss.len(), 2, "{ss:?}");
        assert!(ss[0].starts_with("CREATE TRIGGER"), "{ss:?}");
        assert!(ss[0].contains("INSERT INTO log"));
        assert_eq!(ss[1], "SELECT 1");
    }

    #[test]
    fn string_semicolon_protected() {
        let ss = split_statements("SELECT 'a;b'; SELECT 2");
        assert_eq!(ss.len(), 2);
        assert_eq!(ss[0], "SELECT 'a;b'");
    }
}

#[cfg(test)]
mod escape_tests {
    use super::split_statements;

    /// MySQL 反斜杠转义:'a\'; b' 不在 \' 处断句
    #[test]
    fn mysql_backslash_quote_not_split() {
        let stmts = split_statements("INSERT INTO t VALUES ('a\\'; b'); SELECT 1;");
        assert_eq!(stmts.len(), 2, "转义引号不该断句: {stmts:?}");
        assert!(stmts[0].contains("'a\\'; b'"), "{}", stmts[0]);
    }

    /// mysqldump 的 DELIMITER 行原样跳过
    #[test]
    fn delimiter_line_skipped() {
        let stmts = split_statements("DELIMITER ;;\nSELECT 1;\nDELIMITER ;\nSELECT 2;");
        assert_eq!(stmts, vec!["SELECT 1", "SELECT 2"]);
    }
}

#[cfg(test)]
mod column_type_tests {
    use super::*;
    use crate::model::Backend;

    async fn sqlite() -> Backend {
        Backend::Sqlite(rusqlite::Connection::open_in_memory().unwrap())
    }

    async fn run(b: &mut Backend, sql: &str) -> Vec<ExecResult> {
        b.run_sql(sql, 100, None, None).await.unwrap()
    }

    /// 列类型与列名逐位对应:拿不到的列是空串,整列缺失时为空表
    #[test]
    fn column_type_accessor_is_bounds_safe() {
        let mut r = ExecResult::default();
        assert_eq!(r.column_type(0), "");
        r.column_types = vec!["int".into()];
        assert_eq!(r.column_type(0), "int");
        assert_eq!(r.column_type(7), "");
    }

    #[tokio::test]
    async fn sqlite_table_columns_report_declared_type() {
        let mut b = sqlite().await;
        run(&mut b, "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, price REAL, photo BLOB, note VARCHAR(50))").await;
        let rs = run(&mut b, "SELECT * FROM t").await;
        // 空表没有值可推断:全靠声明类型
        assert_eq!(rs[0].column_types, ["INTEGER", "TEXT", "REAL", "BLOB", "VARCHAR(50)"]);
        assert_eq!(rs[0].column_types.len(), rs[0].columns.len());
    }

    #[tokio::test]
    async fn sqlite_expression_columns_inferred_from_values() {
        let mut b = sqlite().await;
        let rs = run(&mut b, "SELECT 1 + 1 AS a, 'x' AS b, 2.5 AS c, x'00ff' AS d, NULL AS e, length('ab') AS n").await;
        assert_eq!(
            rs[0].column_types,
            ["INTEGER", "TEXT", "REAL", "BLOB", "", "INTEGER"]
        );
    }

    #[tokio::test]
    async fn sqlite_mixed_values_take_the_widest_type() {
        let mut b = sqlite().await;
        // 无类型列:声明为空串,只能按值推断;首个值是 NULL 不该定调
        run(&mut b, "CREATE TABLE t (v)").await;
        run(&mut b, "INSERT INTO t VALUES (NULL), (1), (2)").await;
        let rs = run(&mut b, "SELECT v AS n FROM t").await;
        assert_eq!(rs[0].column_types, ["INTEGER"]);
        run(&mut b, "INSERT INTO t VALUES (2.5)").await;
        let rs = run(&mut b, "SELECT v AS n FROM t").await;
        assert_eq!(rs[0].column_types, ["REAL"]);
        run(&mut b, "INSERT INTO t VALUES ('text')").await;
        let rs = run(&mut b, "SELECT v AS n FROM t").await;
        assert_eq!(rs[0].column_types, ["TEXT"]);
    }

    #[tokio::test]
    async fn sqlite_truncated_result_still_has_types() {
        let mut b = sqlite().await;
        run(&mut b, "CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1),(2),(3),(4)").await;
        let rs = b.run_sql("SELECT id FROM t", 2, None, None).await.unwrap();
        assert!(rs[0].truncated);
        assert_eq!(rs[0].rows.len(), 2);
        assert_eq!(rs[0].column_types, ["INTEGER"]);
    }

    #[tokio::test]
    async fn sqlite_multi_statement_each_set_carries_its_own_types() {
        let mut b = sqlite().await;
        run(&mut b, "CREATE TABLE t (id INTEGER, name TEXT)").await;
        let rs = run(&mut b, "SELECT id FROM t; SELECT 'a' AS s; INSERT INTO t VALUES (1, 'x'); SELECT name FROM t").await;
        assert_eq!(rs.len(), 4);
        assert_eq!(rs[0].column_types, ["INTEGER"]);
        assert_eq!(rs[1].column_types, ["TEXT"]);
        // 非查询语句:没有列,也没有类型
        assert!(rs[2].columns.is_empty() && rs[2].column_types.is_empty());
        assert_eq!(rs[3].column_types, ["TEXT"]);
    }

    /// 插件给的列类型按列数对齐:非字符串项置空,多了截断,缺的补空
    #[test]
    fn plugin_column_types_are_aligned_to_columns() {
        let v = serde_json::json!({ "columnTypes": ["int", 7, "text", "extra"] });
        assert_eq!(json_str_column(&v, "columnTypes", 3), ["int", "", "text"]);
        assert_eq!(json_str_column(&v, "columnTypes", 5), ["int", "", "text", "extra", ""]);
        assert_eq!(json_str_column(&v, "missing", 2), ["", ""]);
        assert_eq!(json_str_column(&serde_json::json!({ "columnTypes": "int" }), "columnTypes", 1), [""]);
    }

    /// 按 MySQL 回包的四个字段造列定义(类型码 / 标志 / 字符集 / 显示宽度)
    fn mysql_col(
        ct: mysql_async::consts::ColumnType,
        flags: mysql_async::consts::ColumnFlags,
        charset: u16,
        len: u32,
    ) -> String {
        mysql_type_name(
            &mysql_async::Column::new(ct)
                .with_name(b"c")
                .with_flags(flags)
                .with_character_set(charset)
                .with_column_length(len),
        )
    }

    /// 文本协议的三个坑:BINARY_FLAG 不等于二进制数据(看字符集 63)、
    /// ENUM/SET 靠标志位、TEXT/BLOB 四档靠显示宽度
    #[test]
    fn mysql_type_names_read_what_the_server_actually_sends() {
        use mysql_async::consts::ColumnFlags as F;
        use mysql_async::consts::ColumnType as CT;
        const UTF8MB4: u16 = 45;
        const BIN: u16 = 63;
        let col = |ct: CT| mysql_col(ct, F::empty(), UTF8MB4, 20);

        assert_eq!(col(CT::MYSQL_TYPE_TINY), "tinyint");
        assert_eq!(col(CT::MYSQL_TYPE_SHORT), "smallint");
        assert_eq!(col(CT::MYSQL_TYPE_LONG), "int");
        assert_eq!(col(CT::MYSQL_TYPE_INT24), "mediumint");
        assert_eq!(col(CT::MYSQL_TYPE_LONGLONG), "bigint");
        assert_eq!(col(CT::MYSQL_TYPE_NEWDECIMAL), "decimal");
        assert_eq!(col(CT::MYSQL_TYPE_FLOAT), "float");
        assert_eq!(col(CT::MYSQL_TYPE_DOUBLE), "double");
        assert_eq!(col(CT::MYSQL_TYPE_DATETIME), "datetime");
        assert_eq!(col(CT::MYSQL_TYPE_DATETIME2), "datetime");
        assert_eq!(col(CT::MYSQL_TYPE_TIMESTAMP), "timestamp");
        assert_eq!(col(CT::MYSQL_TYPE_DATE), "date");
        assert_eq!(col(CT::MYSQL_TYPE_TIME), "time");
        assert_eq!(col(CT::MYSQL_TYPE_JSON), "json");
        assert_eq!(col(CT::MYSQL_TYPE_GEOMETRY), "geometry");
        assert_eq!(col(CT::MYSQL_TYPE_VAR_STRING), "varchar");
        assert_eq!(col(CT::MYSQL_TYPE_VARCHAR), "varchar");
        assert_eq!(col(CT::MYSQL_TYPE_STRING), "char");
        assert_eq!(col(CT::MYSQL_TYPE_NULL), "");

        // 数值列带 unsigned
        assert_eq!(
            mysql_col(CT::MYSQL_TYPE_LONGLONG, F::UNSIGNED_FLAG, BIN, 20),
            "bigint unsigned"
        );

        // BINARY_FLAG 出现在时间 / utf8mb4_bin 的 VARCHAR 上,都不是二进制数据
        assert_eq!(mysql_col(CT::MYSQL_TYPE_VAR_STRING, F::BINARY_FLAG, UTF8MB4, 40), "varchar");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_TIMESTAMP, F::BINARY_FLAG, BIN, 19), "timestamp");
        // 字符集 63 才是二进制数据
        assert_eq!(mysql_col(CT::MYSQL_TYPE_VAR_STRING, F::BINARY_FLAG, BIN, 16), "varbinary");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_VAR_STRING, F::empty(), BIN, 255), "varbinary");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_STRING, F::BINARY_FLAG, BIN, 8), "binary");

        // ENUM / SET 与 CHAR 同码
        assert_eq!(mysql_col(CT::MYSQL_TYPE_STRING, F::ENUM_FLAG, UTF8MB4, 4), "enum");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_STRING, F::SET_FLAG, UTF8MB4, 12), "set");

        // TEXT 四档:服务器给的显示宽度已含 utf8mb4 的 4 倍
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, UTF8MB4, 1020), "tinytext");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, UTF8MB4, 262_140), "text");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, UTF8MB4, 67_108_860), "mediumtext");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, UTF8MB4, u32::MAX), "longtext");
        // 旧服务器按单字节报宽度也能分对档
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, 8, 255), "tinytext");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, 8, 65_535), "text");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG, 8, 16_777_215), "mediumtext");
        // BLOB 四档
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG | F::BINARY_FLAG, BIN, 255), "tinyblob");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG | F::BINARY_FLAG, BIN, 65_535), "blob");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG | F::BINARY_FLAG, BIN, 16_777_215), "mediumblob");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_BLOB, F::BLOB_FLAG | F::BINARY_FLAG, BIN, u32::MAX), "longblob");
        // 类型码自带档位时以码为准:GROUP_CONCAT 回 LONG_BLOB 而宽度只有 65536
        assert_eq!(mysql_col(CT::MYSQL_TYPE_LONG_BLOB, F::empty(), UTF8MB4, 65_536), "longtext");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_TINY_BLOB, F::empty(), UTF8MB4, 65_536), "tinytext");
        assert_eq!(mysql_col(CT::MYSQL_TYPE_MEDIUM_BLOB, F::BINARY_FLAG, BIN, 100), "mediumblob");
    }

    /// 以下两条要本机测试容器(shujing-mysql:3306 空密码 / shujing-pg:5433 trust),
    /// 平时忽略:`cargo test -p shujing-core -- --ignored live_`
    fn live_enabled() -> bool {
        std::env::var("SHUJING_LIVE_DB").is_ok()
    }

    async fn mysql_cols_types(conn: &mut mysql_async::Conn, sql: &str) -> (Vec<String>, Vec<String>) {
        let r = run_mysql(conn, sql, 10, None).await.unwrap().remove(0);
        (r.columns, r.column_types)
    }

    /// MySQL 文本协议真实回包:各列类型的类型码由服务器给出,映射对不对只能实连验证
    #[tokio::test]
    #[ignore]
    async fn live_mysql_reports_server_column_types() {
        if !live_enabled() {
            return;
        }
        use mysql_async::prelude::*;
        let mut conn = mysql_async::Conn::new(
            mysql_async::OptsBuilder::default()
                .ip_or_hostname("127.0.0.1")
                .tcp_port(3306)
                .user(Some("root"))
                .pass(Some("")),
        )
        .await
        .expect("连接本机 MySQL 测试容器");
        // 临时表只在本连接内存在,连接一断自动消失,不在测试库里留东西
        run_mysql(&mut conn, "CREATE TEMPORARY TABLE shop.sj_ct (\
            id INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY, tiny TINYINT, big BIGINT UNSIGNED, \
            deci DECIMAL(10,2), f FLOAT, d DOUBLE, bit_c BIT(8), year_c YEAR, \
            ts TIMESTAMP NULL, dt DATETIME(3), dte DATE, tme TIME(3), js JSON, \
            en ENUM('a','b'), st SET('x','y'), vc VARCHAR(20), vcb VARCHAR(20) CHARACTER SET binary, \
            ch CHAR(5), chb BINARY(5), ttx TINYTEXT CHARACTER SET latin1, tx TEXT, mtx MEDIUMTEXT, \
            ltx LONGTEXT, tbb TINYBLOB, blb BLOB, mbb MEDIUMBLOB, lblb LONGBLOB, vb VARBINARY(16), \
            vcutf8 VARCHAR(10) COLLATE utf8mb4_bin)", 10, None)
            .await
            .unwrap();
        let (cols, types) = mysql_cols_types(&mut conn, "SELECT * FROM shop.sj_ct").await;
        assert_eq!(cols.len(), types.len(), "类型须与列名逐位对应");
        assert_eq!(
            types,
            [
                "int unsigned", "tinyint", "bigint unsigned", "decimal", "float", "double", "bit", "year",
                "timestamp", "datetime", "date", "time", "json", "enum", "set", "varchar", "varbinary", "char",
                "binary", "tinytext", "text", "mediumtext", "longtext", "tinyblob", "blob", "mediumblob",
                "longblob", "varbinary", "varchar",
            ]
        );
        // 表达式列:类型码来自服务器对表达式的推导
        let (_, types) = mysql_cols_types(&mut conn, "SELECT 1 + 1 AS n, 'x' AS s, NOW() AS t, \
            NULL AS z, CAST('a' AS BINARY) AS cb, upper(en) AS up, en AS enalias, tx AS txalias, \
            blb AS blbalias, id * 2 AS doubled, id / 2 AS halved, tiny + 1 AS tinyp, f * 2 AS fp, \
            en = 'a' AS bl FROM shop.sj_ct").await;
        assert_eq!(
            types,
            [
                "bigint", "varchar", "datetime", "", "varbinary", "varchar", "enum", "text", "blob",
                "bigint unsigned", "decimal", "bigint", "double", "bigint",
            ]
        );
        // 聚合:GROUP_CONCAT 回 LONG_BLOB 码,须按码定档而不是按宽度
        let (_, types) = mysql_cols_types(&mut conn, "SELECT GROUP_CONCAT(id) AS gc, SUM(id) AS sm, \
            MAX(en) AS men, MAX(vc) AS mvc, MAX(dt) AS mx, COUNT(*) AS c FROM shop.sj_ct").await;
        assert_eq!(types, ["longtext", "decimal", "char", "varchar", "datetime", "bigint"]);
    }

    /// PG 简单查询协议拿不到类型:验证降级路径——列照常返回、取类型得空串、不 panic
    #[tokio::test]
    #[ignore]
    async fn live_pg_degrades_to_unknown_types() {
        if !live_enabled() {
            return;
        }
        let (client, conn) = tokio_postgres::connect(
            "host=127.0.0.1 port=5433 user=postgres dbname=postgres",
            tokio_postgres::NoTls,
        )
        .await
        .expect("连接本机 PG 测试容器");
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let rs = run_pg(&client, "SELECT 1 AS a, 'x' AS b, now() AS c", 10).await.unwrap();
        assert_eq!(rs[0].columns, ["a", "b", "c"]);
        assert_eq!(rs[0].rows.len(), 1);
        assert_eq!(rs[0].column_type(0), "");
        assert_eq!(rs[0].column_type(9), "");
        let rs = run_pg(&client, "SELECT 1 AS a; SELECT 'x' AS b", 10).await.unwrap();
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[1].columns, ["b"]);
        assert_eq!(rs[1].column_type(0), "");
    }

    /// SQL Server TDS 类型码 → 类型名
    #[test]
    fn mssql_type_names() {
        use tiberius::ColumnType as CT;
        let f = crate::mssql::mssql_type_name;
        assert_eq!(f(CT::Int1), "tinyint");
        assert_eq!(f(CT::Int2), "smallint");
        assert_eq!(f(CT::Int4), "int");
        assert_eq!(f(CT::Int8), "bigint");
        assert_eq!(f(CT::Intn), "int");
        assert_eq!(f(CT::Datetime2), "datetime2");
        assert_eq!(f(CT::DatetimeOffsetn), "datetimeoffset");
        assert_eq!(f(CT::Float8), "float");
        assert_eq!(f(CT::Decimaln), "decimal");
        assert_eq!(f(CT::Guid), "uniqueidentifier");
        assert_eq!(f(CT::BigVarBin), "varbinary");
        assert_eq!(f(CT::NVarchar), "nvarchar");
        assert_eq!(f(CT::Null), "");
    }
}
