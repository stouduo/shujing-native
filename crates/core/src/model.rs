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
        "XRANGE", "XREVRANGE", "XLEN", "XREAD", "XINFO",
        "DBSIZE", "INFO", "PING", "ECHO", "TIME", "LASTSAVE", "MEMORY", "OBJECT",
        "CLIENT", "LPOS", "GETRANGE", "SUBSTR", "BITCOUNT", "BITPOS", "HMGETALL",
    ];
    let head = cmd
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
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
    // EXPLAIN:带 ANALYZE 会真实执行被分析的写语句
    if upper.starts_with("EXPLAIN") {
        let rest = upper["EXPLAIN".len()..].trim_start();
        if rest.starts_with("ANALYZE") {
            let inner = &s["EXPLAIN".len()..].trim_start()["ANALYZE".len()..].trim_start();
            return is_readonly_sql(inner);
        }
        return true;
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
    if kw("SELECT") || kw("SHOW") || kw("DESC") {
        return true;
    }
    // PRAGMA:仅放行查询型(不带 = 赋值;写入形式如 PRAGMA user_version = 5)
    if upper.starts_with("PRAGMA") {
        return !upper.contains('=');
    }
    false
}

/// 剥掉 `WITH name [(cols)] AS ( ... ), ...` 前缀后,顶层语句是否为 SELECT
fn with_top_is_select(s: &str) -> bool {
    let mut rest = s.trim_start();
    loop {
        let r = rest.strip_prefix("WITH").unwrap_or(rest).trim_start();
        // 名字(可带列清单)
        let after_name = match r.find(|c: char| c == '(' || c.is_whitespace()) {
            Some(0) => return false, // WITH( 直接触发键?非标准,保守拒绝
            Some(i) => &r[i..],
            None => return false,
        };
        let after_name = after_name.trim_start();
        let after_as = match after_name.strip_prefix("AS") {
            Some(a) => a.trim_start(),
            // 没到 AS:可能还是名字的一部分,无法推进,保守拒绝
            None => return false,
        };
        if !after_as.starts_with('(') {
            return false;
        }
        // 跳过匹配的括号对
        let mut depth = 0usize;
        let mut end = None;
        for (i, b) in after_as.bytes().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { return false };
        // PG 数据修改 CTE:AS ( INSERT/UPDATE/DELETE/MERGE ... ) 顶层是 SELECT 也会真实写入
        let body_head = after_as[1..end - 1].trim_start();
        let first_word: String = body_head
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase();
        if matches!(
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMeta {
    pub name: String,
    /// table | view
    pub kind: String,
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

/// Redis 连接状态:缓存多路复用连接(MultiplexedConnection 可 Clone 且驱动任务自动重连,
/// 此前每条命令新建一次 TCP 连接)
pub struct RedisState {
    pub(crate) client: redis::Client,
    pub(crate) con: tokio::sync::Mutex<Option<redis::aio::MultiplexedConnection>>,
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
    let s = sql.trim();
    let lower = s.to_lowercase();
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
