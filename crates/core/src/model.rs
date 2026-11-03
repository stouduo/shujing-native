use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DbType {
    Sqlite,
    MySql,
    Postgres,
    Redis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnInfo {
    pub id: String,
    pub name: String,
    pub db_type: DbType,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub file_path: Option<String>,
    /// 侧栏标识色(连接着色)
    #[serde(default)]
    pub color: Option<String>,
    /// 只读模式:后端强制只放行查询语句
    #[serde(default)]
    pub read_only: bool,
}

/// Redis 只读连接的读命令白名单(此前黑名单漏 EVAL/Lua、GETDEL、*STORE 等大量写命令)
pub fn is_readonly_redis(cmd: &str) -> bool {
    const READ_CMDS: &[&str] = &[
        "GET", "MGET", "STRLEN", "EXISTS", "TYPE", "TTL", "PTTL", "KEYS", "SCAN", "RANDOMKEY",
        "HGET", "HMGET", "HGETALL", "HKEYS", "HVALS", "HLEN", "HEXISTS", "HSCAN",
        "LRANGE", "LLEN", "LINDEX", "SISMEMBER", "SMEMBERS", "SCARD", "SRANDMEMBER", "SSCAN",
        "ZRANGE", "ZREVRANGE", "ZRANGEBYSCORE", "ZSCORE", "ZCARD", "ZCOUNT", "ZRANK", "ZREVRANK", "ZSCAN",
        "PFCOUNT", "GEOPOS", "GEODIST", "GEORADIUS_RO", "GEORADIUSBYMEMBER_RO",
        "XRANGE", "XREVRANGE", "XLEN", "XINFO",
        "DBSIZE", "INFO", "PING", "ECHO", "TIME", "LASTSAVE", "OBJECT",
        "LPOS", "GETRANGE", "SUBSTR", "BITCOUNT", "BITPOS",
    ];
    let mut it = cmd.trim_start().split_whitespace();
    let head = it.next().unwrap_or_default().to_ascii_uppercase();
    // CLIENT/MEMORY 只放行只读子命令(KILL/NO-EVICT/PURGE 是管理/写操作);
    // XREAD/XREADGROUP 移除(BLOCK 0 会无限期占住串行锁)
    if head == "CLIENT" {
        return matches!(
            it.next().unwrap_or_default().to_ascii_uppercase().as_str(),
            "LIST" | "INFO" | "GETNAME" | "ID"
        );
    }
    if head == "MEMORY" {
        return matches!(
            it.next().unwrap_or_default().to_ascii_uppercase().as_str(),
            "USAGE" | "DOCTOR" | "STATS"
        );
    }
    READ_CMDS.contains(&head.as_str())
}

/// 只读连接允许执行的语句白名单。
/// 注意:WITH 可携带写 CTE(PG `WITH d AS (DELETE..) SELECT..`),
/// EXPLAIN ANALYZE 会实际执行被分析语句,PRAGMA 带 `=` 是写——逐项排除。
pub fn is_readonly_sql(sql: &str) -> bool {
    let s = sql.trim_start();
    let upper = s.to_ascii_uppercase();
    // WITH:顶层必须是 SELECT(粗扫首个顶层关键字;CTE 体内的写关键字出现在括号内,
    // 用"去括号后的首词"判定:逐层剥掉 WITH name AS ( ... ) 前缀)
    if upper.starts_with("WITH") {
        return with_top_is_select(s);
    }
    // EXPLAIN:带 ANALYZE(含括号选项 EXPLAIN (ANALYZE) DELETE)会真实执行内层语句。
    // 剥掉选项 token(平衡括号块/ANALYZE/VERBOSE/COSTS/BUFFERS/TIMING/SUMMARY/
    // FORMAT xxx)后对内层语句递归校验
    if upper.starts_with("EXPLAIN") {
        let mut inner = s["EXPLAIN".len()..].trim_start();
        loop {
            let up = inner.to_ascii_uppercase();
            let up = up.trim_start();
            let up_trim = inner.trim_start();
            if up_trim.starts_with('(') {
                // 剥平衡括号块(选项列表)
                let mut d = 0usize;
                let mut closed = None;
                for (i, b) in up_trim.bytes().enumerate() {
                    match b {
                        b'(' => d += 1,
                        b')' => {
                            d -= 1;
                            if d == 0 {
                                closed = Some(i + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                match closed {
                    Some(c) => inner = up_trim[c..].trim_start(),
                    None => return false, // 括号不平衡,保守拒绝
                }
                continue;
            }
            let mut hit = false;
            // QUERY PLAN 是 SQLite 的 EXPLAIN 语法(只列计划不执行)
            if up.starts_with("QUERY PLAN") {
                inner = up_trim[10..].trim_start();
                continue;
            }
            for opt in ["ANALYZE", "ANALYSE", "VERBOSE", "COSTS", "BUFFERS", "TIMING", "SUMMARY"] {
                if up.starts_with(opt) {
                    let nxt = up_trim[opt.len()..].chars().next();
                    if nxt.map(|c| c.is_whitespace()).unwrap_or(true) {
                        inner = up_trim[opt.len()..].trim_start();
                        hit = true;
                        break;
                    }
                }
            }
            if hit {
                continue;
            }
            if up.starts_with("FORMAT") {
                // FORMAT xxx(剥词)
                let mut it = up_trim.split_whitespace();
                it.next();
                match it.next() {
                    Some(_) => {
                        let consumed: usize = up_trim
                            .split_whitespace()
                            .take(2)
                            .map(|w| w.len() + 1)
                            .sum::<usize>()
                            .saturating_sub(1);
                        inner = up_trim[consumed..].trim_start();
                        continue;
                    }
                    None => return false,
                }
            }
            break;
        }
        return is_readonly_sql(inner);
    }
    // 词边界:showcase/selection/description 之类表名不能被当关键字前缀
    let kw = |k: &str| {
        upper == k
            || (upper.starts_with(k)
                && upper[k.len()..]
                    .chars()
                    .next()
                    .map(|c| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(false))
    };
    if kw("SELECT") {
        // SELECT ... INTO 是写:MySQL INTO OUTFILE/DUMPFILE 写文件、PG INTO 建表。
        // 仅放行 INTO @var(会话变量赋值,不落库)
        let uw = upper;
        let mut idx = 6usize;
        while let Some(p) = uw[idx..].find(" INTO ") {
            let after = uw[idx + p + 6..]
                .trim_start()
                .chars()
                .next()
                .unwrap_or(' ');
            if after != '@' {
                return false;
            }
            idx += p + 6;
        }
        return true;
    }
    if kw("SHOW") || kw("DESC") {
        return true;
    }
    // PRAGMA:仅放行查询型。`PRAGMA x = v` 与函数形式 `PRAGMA x(v)` 都是写;
    // 查询型函数(table_info(x) 等)走白名单
    if upper.starts_with("PRAGMA") {
        if upper.contains('=') {
            return false;
        }
        const QUERY_PRAGMAS: &[&str] = &[
            "TABLE_INFO", "TABLE_LIST", "INDEX_LIST", "INDEX_INFO", "INDEX_XINFO",
            "FOREIGN_KEY_LIST", "DATABASE_LIST", "COMPILE_OPTIONS", "FUNCTION_LIST",
            "MODULE_LIST", "PRAGMA_LIST", "COLLATION_LIST",
        ];
        if let Some(open) = upper.find('(') {
            let name = upper[6..open].trim().trim_end_matches('.');
            let name = name.rsplit('.').next().unwrap_or(name).trim();
            return QUERY_PRAGMAS.contains(&name);
        }
        return true;
    }
    false
}

/// 剥掉 `WITH name [(cols)] AS ( ... ), ...` 前缀后,顶层语句是否为 SELECT
fn with_top_is_select(s: &str) -> bool {
    let mut rest = s.trim_start();
    loop {
        let mut r = rest.strip_prefix("WITH").unwrap_or(rest).trim_start();
        // 修饰词:RECURSIVE / [NOT] MATERIALIZED 在名字前
        loop {
            let up = r.to_ascii_uppercase();
            if up.starts_with("RECURSIVE") && r[9..].starts_with(char::is_whitespace) {
                r = r[9..].trim_start();
            } else if up.starts_with("NOT MATERIALIZED") {
                r = r[16..].trim_start();
            } else if up.starts_with("MATERIALIZED") && r[12..].starts_with(char::is_whitespace) {
                r = r[12..].trim_start();
            } else {
                break;
            }
        }
        // 名字(可带列清单)
        let after_name = match r.find(|c: char| c == '(' || c.is_whitespace()) {
            Some(0) => return false, // WITH( 直接触发键?非标准,保守拒绝
            Some(i) => &r[i..],
            None => return false,
        };
        let after_name = after_name.trim_start();
        // 可选列清单 name(a,b) AS
        let after_name = if after_name.starts_with('(') {
            match skip_balanced(after_name) {
                Some((i, _)) => after_name[i..].trim_start(),
                None => return false,
            }
        } else {
            after_name
        };
        let after_as = match after_name.strip_prefix("AS") {
            Some(a) => a.trim_start(),
            // 没到 AS:可能还是名字的一部分,无法推进,保守拒绝
            None => return false,
        };
        // PG:AS 与括号之间可带 [NOT] MATERIALIZED
        let au = after_as.to_ascii_uppercase();
        let after_as = if au.starts_with("NOT MATERIALIZED") {
            after_as[16..].trim_start()
        } else if au.starts_with("MATERIALIZED") && after_as[12..].starts_with(char::is_whitespace) {
            after_as[12..].trim_start()
        } else {
            after_as
        };
        if !after_as.starts_with('(') {
            return false;
        }
        // 跳过匹配的括号对(引号/注释感知:字符串里的 ')' 不能配平)
        let Some((end, _)) = skip_balanced(after_as) else { return false };
        // PG 数据修改 CTE:AS ( INSERT/UPDATE/DELETE/MERGE ... ) 顶层是 SELECT 也会真实写入;
        // 嵌套 WITH x AS (WITH d AS (DELETE..) SELECT..) SELECT 同理——递归判定
        let body_head = after_as[1..end - 1].trim_start();
        let first_word: String = body_head
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        if first_word == "WITH" {
            if !with_top_is_select(body_head) {
                return false;
            }
        } else if matches!(
            first_word.as_str(),
            "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" | "CREATE" | "DROP" | "ALTER"
        ) {
            return false;
        }
        let tail = after_as[end..].trim_start();
        if tail.starts_with(',') {
            rest = tail[1..].trim_start();
            // 下一段还是 name AS ( ... ),继续剥
            continue;
        }
        return tail.to_ascii_uppercase().starts_with("SELECT");
    }
}

/// 从 `(...` 开头的串里跳过首个平衡括号对(跳过字符串字面量与注释)。
/// 返回 (闭合括号后的字节偏移, 闭合括号内不含的区间),配不平返回 None
fn skip_balanced(s: &str) -> Option<(usize, ())> {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' && (q == b'\'' || q == b'"') {
                i += 2; // MySQL 反斜杠转义
                continue;
            }
            if c == q {
                if b.get(i + 1) == Some(&q) {
                    i += 2; // 双写引号转义
                    continue;
                }
                quote = None;
            }
        } else {
            match c {
                b'\'' | b'"' | b'`' => quote = Some(c),
                b'-' if b.get(i + 1) == Some(&b'-') => {
                    // 行注释
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    match s[i + 2..].find("*/") {
                        Some(p) => {
                            i += p + 4;
                            continue;
                        }
                        None => return None,
                    }
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((i + 1, ()));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMeta {
    pub name: String,
    /// table | view
    pub kind: String,
    /// 触发器所属表(PG 的 DROP TRIGGER 必须带 ON <表>;其余为 None)
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDef {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// PRI / UNI / MUL / ""
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub comment: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDef {
    pub name: String,
    pub columns: String,
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    pub columns: Vec<ColumnDef>,
    pub indexes: Vec<IndexDef>,
    pub ddl: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    /// 为空表示非查询语句(INSERT/UPDATE/...)
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub affected: u64,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    pub version: String,
}

pub enum Backend {
    Sqlite(rusqlite::Connection),
    MySql(MySqlPool),
    Pg(PgPool),
    Redis(RedisState),
}

/// Redis 连接状态:ConnectionManager 自动重连(MultiplexedConnection 的驱动任务出错即终止,不重连——
/// 此前每条命令新建一次 TCP 连接)
pub struct RedisState {
    pub(crate) client: redis::Client,
    pub(crate) con: tokio::sync::Mutex<Option<redis::aio::ConnectionManager>>,
    /// 操作串行锁:共享连接上 SELECT+命令必须成对执行,
    /// 否则两个 db 的标签页并发时 SELECT 会交错到错误的 db
    pub(crate) op_lock: tokio::sync::Mutex<()>,
}

/// PostgreSQL 连接池:多条会话轮发,单会话内的串行不再阻塞其他标签页
pub struct PgPool {
    pub(crate) clients: Vec<std::sync::Arc<tokio_postgres::Client>>,
    pub(crate) next: std::sync::atomic::AtomicUsize,
    pub(crate) session_db: Option<String>,
    /// 每个槽位已应用的 search_path(与 clients 一一对应)
    pub(crate) applied: Vec<Option<String>>,
    /// 最近 checkout 会话的 backend pid(取消用)
    pub(crate) running_pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

/// MySQL 连接池 + 会话库上下文(每次取连接自动 USE 恢复)
pub struct MySqlPool {
    pub(crate) pool: mysql_async::Pool,
    pub(crate) session_db: Option<String>,
    /// 最近一次取用连接的会话 ID(KILL QUERY 用)
    pub(crate) running_id: std::sync::atomic::AtomicU64,
}

/// 从 USE / SET search_path 语句提取库名
pub fn extract_use_db(sql: &str) -> Option<String> {
    // 扫全部语句取最后一次 USE/SET search_path(此前只看整串前缀,
    // `SELECT 1; USE other` 的库切换不进会话记忆 → 连接池各连接库漂移)
    let mut last = None;
    for stmt in crate::exec::split_statements(sql) {
        if let Some(db) = extract_use_db_single(&stmt) {
            last = Some(db);
        }
    }
    last
}

fn extract_use_db_single(sql: &str) -> Option<String> {
    let s = sql.trim();
    // ascii 小写化保证与原文等长:Unicode to_lowercase('İ'→3字节)会让后续
    // find 的字节偏移错位,拿去切原串可落在多字节字符中间 → panic
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("use ") {
        let rest = s[4..].trim();
        if let (Some(a), Some(b)) = (rest.find('`'), rest.get(1..).and_then(|r| r.find('`').map(|i| i + 1))) {
            if a < b {
                return Some(rest[a + 1..b].to_string());
            }
        }
        return rest
            .split_whitespace()
            .next()
            .map(|w| w.trim_matches(';').to_string());
    }
    if lower.starts_with("set search_path") {
        // 按词边界找 TO 或 =(此前 find("to") 会命中 "todos" 之类的子串)
        let sep = lower
            .find(" to ")
            .map(|i| (i, 4))
            .or_else(|| lower.find('=').map(|i| (i, 1)));
        if let Some((i, w)) = sep {
            let rest = s[i + w..].trim();
            let end = rest.find(',').unwrap_or(rest.len());
            return Some(
                rest[..end]
                    .trim()
                    .trim_matches('"')
                    .trim_matches(';')
                    .to_string(),
            );
        }
    }
    None
}

pub struct LiveConn {
    pub info: ConnInfo,
    pub backend: Backend,
}

/// 取消查询所需的目标信息(独立于执行锁:正在跑的查询不会阻塞取消)
pub struct CancelState {
    pub db_type: DbType,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    /// MySQL 当前会话 ID(KILL QUERY 用)
    pub mysql_id: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// PG 当前会话的 backend pid
    pub pg_pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl CancelState {
    pub fn new(info: &ConnInfo, pg_pid: std::sync::Arc<std::sync::atomic::AtomicU32>) -> Self {
        Self {
            db_type: info.db_type,
            host: info.host.clone(),
            port: info.port,
            user: info.user.clone(),
            password: info.password.clone(),
            database: info.database.clone(),
            mysql_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            pg_pid,
        }
    }
}

/// 单个连接条目:执行与取消通道分离(运行中的查询只持有 live 锁)
pub struct ConnEntry {
    pub live: std::sync::Arc<tokio::sync::Mutex<LiveConn>>,
    pub cancel: std::sync::Arc<CancelState>,
}

#[derive(Default)]
pub struct AppState {
    conns: Mutex<HashMap<String, Arc<ConnEntry>>>,
}

impl AppState {
    pub fn insert(&self, live: LiveConn, cancel: std::sync::Arc<CancelState>) {
        let id = live.info.id.clone();
        self.conns.lock().unwrap().insert(
            id,
            Arc::new(ConnEntry {
                live: std::sync::Arc::new(tokio::sync::Mutex::new(live)),
                cancel,
            }),
        );
    }

    pub fn remove(&self, id: &str) -> Option<Arc<ConnEntry>> {
        self.conns.lock().unwrap().remove(id)
    }

    /// 执行用的连接锁
    pub fn live(&self, id: &str) -> Option<Arc<tokio::sync::Mutex<LiveConn>>> {
        self.conns.lock().unwrap().get(id).map(|e| e.live.clone())
    }

    /// 取消通道(轻锁,运行中查询也不阻塞读取)
    pub fn cancel(&self, id: &str) -> Option<Arc<CancelState>> {
        self.conns.lock().unwrap().get(id).map(|e| e.cancel.clone())
    }

}

pub fn es<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[cfg(test)]
mod readonly_tests {
    use super::*;

    #[test]
    fn readonly_allows_reads() {
        assert!(is_readonly_sql("SELECT * FROM t"));
        assert!(is_readonly_sql("  select 1"));
        assert!(is_readonly_sql("SHOW TABLES"));
        assert!(is_readonly_sql("DESC t"));
        assert!(is_readonly_sql("EXPLAIN SELECT * FROM t"));
        assert!(is_readonly_sql("PRAGMA table_info(t)"));
        // 纯读 CTE
        assert!(is_readonly_sql("WITH x AS (SELECT 1) SELECT * FROM x"));
    }

    #[test]
    fn readonly_blocks_writes() {
        assert!(!is_readonly_sql("DELETE FROM t"));
        assert!(!is_readonly_sql("UPDATE t SET a=1"));
        // EXPLAIN ANALYZE 会真实执行
        assert!(!is_readonly_sql("EXPLAIN ANALYZE DELETE FROM t"));
        // 数据修改 CTE:顶层 SELECT 也写入
        assert!(!is_readonly_sql("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"));
        assert!(!is_readonly_sql("WITH x AS (SELECT 1) DELETE FROM t"));
        // PRAGMA 赋值是写
        assert!(!is_readonly_sql("PRAGMA user_version = 5"));
        // 词边界:showcase/withdraw 不是关键字
        assert!(!is_readonly_sql("showcase"));
        assert!(!is_readonly_sql("withdraw from danger"));
    }

    #[test]
    fn extract_use_variants() {
        assert_eq!(extract_use_db("USE `shop`;"), Some("shop".into()));
        assert_eq!(extract_use_db("use shop"), Some("shop".into()));
        // "to" 子串不再误命中(search_path = todos)
        assert_eq!(
            extract_use_db("SET search_path = todos"),
            Some("todos".into())
        );
        assert_eq!(
            extract_use_db("SET search_path TO \"pub\""),
            Some("pub".into())
        );
        assert_eq!(extract_use_db("SELECT 1"), None);
    }
}

#[cfg(test)]
mod readonly_bypass_tests {
    use super::*;

    #[test]
    fn explain_option_forms_blocked() {
        assert!(!is_readonly_sql("EXPLAIN (ANALYZE) DELETE FROM t"));
        assert!(!is_readonly_sql("EXPLAIN VERBOSE ANALYZE DELETE FROM t"));
        assert!(!is_readonly_sql("EXPLAIN ANALYZE DELETE FROM t"));
        assert!(is_readonly_sql("EXPLAIN SELECT 1"));
        assert!(is_readonly_sql("EXPLAIN (COSTS) SELECT 1"));
    }

    #[test]
    fn cte_string_literal_not_confused() {
        // 字符串里的 ')' 不能让括号提前配平
        assert!(!is_readonly_sql(
            "WITH x AS (SELECT ') SELECT 1' AS c) DELETE FROM t"
        ));
        assert!(is_readonly_sql("WITH x AS (SELECT 1 AS c) SELECT * FROM x"));
    }

    #[test]
    fn nested_write_cte_blocked() {
        assert!(!is_readonly_sql(
            "WITH x AS (WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d) SELECT 1"
        ));
    }

    #[test]
    fn recursive_and_materialized_allowed() {
        assert!(is_readonly_sql(
            "WITH RECURSIVE c AS (SELECT 1) SELECT * FROM c"
        ));
        assert!(is_readonly_sql("WITH x AS MATERIALIZED (SELECT 1) SELECT * FROM x"));
        assert!(is_readonly_sql("WITH x(a, b) AS (SELECT 1, 2) SELECT * FROM x"));
    }

    #[test]
    fn select_into_blocked() {
        assert!(!is_readonly_sql("SELECT * FROM t INTO OUTFILE '/tmp/x'"));
        assert!(!is_readonly_sql("SELECT * INTO newt FROM t"));
        assert!(is_readonly_sql("SELECT c INTO @v FROM t"));
        assert!(is_readonly_sql("SELECT 1"));
    }

    #[test]
    fn pragma_function_form_blocked() {
        assert!(!is_readonly_sql("PRAGMA journal_mode(WAL)"));
        assert!(!is_readonly_sql("PRAGMA user_version(5)"));
        assert!(is_readonly_sql("PRAGMA table_info(t)"));
        assert!(is_readonly_sql("PRAGMA user_version"));
    }
}

#[cfg(test)]
mod use_db_tests {
    use super::extract_use_db;

    /// USE 出现在任意语句位置都要进会话记忆(取最后一次)
    #[test]
    fn use_anywhere_tracked() {
        assert_eq!(
            extract_use_db("SELECT database(); USE other_db;"),
            Some("other_db".into())
        );
        assert_eq!(extract_use_db("USE a; USE b;"), Some("b".into()));
        assert_eq!(extract_use_db("SELECT 1"), None);
    }
}
