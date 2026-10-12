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

/// Redis 只读连接的写命令黑名单
pub fn is_readonly_redis(cmd: &str) -> bool {
    const WRITE_PREFIXES: &[&str] = &[
        "SET", "DEL", "UNLINK", "EXPIRE", "PERSIST", "RENAME", "APPEND", "INCR", "DECR",
        "LPUSH", "RPUSH", "LPOP", "RPOP", "LSET", "LTRIM", "SADD", "SREM", "HSET", "HDEL",
        "ZADD", "ZREM", "FLUSHDB", "FLUSHALL", "MSET", "GETSET", "SETEX", "SELECT",
    ];
    let head = cmd.trim_start().split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
    !WRITE_PREFIXES.contains(&head.as_str())
}

/// 只读连接允许执行的语句前缀白名单
pub fn is_readonly_sql(sql: &str) -> bool {
    let head = sql.trim_start().get(..8).unwrap_or_default().to_ascii_uppercase();
    head.starts_with("SELECT")
        || head.starts_with("WITH")
        || head.starts_with("EXPLAIN")
        || head.starts_with("SHOW")
        || head.starts_with("DESCRIBE")
        || head.starts_with("DESC")
        || head.starts_with("PRAGMA")
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
    Redis(redis::Client),
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
        if let (Some(a), Some(b)) = (rest.find('`'), rest[1..].find('`').map(|i| i + 1)) {
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
        if let Some(i) = lower.find("to") {
            let rest = s[i + 2..].trim();
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

    pub fn get(&self, id: &str) -> Option<std::sync::Arc<tokio::sync::Mutex<LiveConn>>> {
        self.conns.lock().unwrap().get(id).map(|e| e.live.clone())
    }
}

pub fn es<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
