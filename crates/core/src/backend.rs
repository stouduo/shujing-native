use crate::model::{es, Backend, ConnInfo, DbType, PgPool};
use std::sync::atomic::Ordering;
use std::time::Duration;

fn req<'a>(v: &'a Option<String>, msg: &str) -> Result<&'a str, String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| msg.to_string())
}

/// postgres 连接串里的值用单引号包裹,转义反斜杠和单引号
pub(crate) fn pg_quote(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

pub async fn connect(
    info: &ConnInfo,
    pg_pid: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>,
) -> Result<Backend, String> {
    match info.db_type {
        DbType::Sqlite => {
            let path = req(&info.file_path, "缺少 SQLite 数据库文件路径")?;
            let conn = rusqlite::Connection::open(path)
                .map_err(|e| format!("打开 SQLite 文件失败: {e}"))?;
            let _ = conn.busy_timeout(Duration::from_secs(5));
            // WAL 失败(比如内存库/网络盘)不影响使用
            let _ = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0));
            let _ = conn.execute_batch("PRAGMA foreign_keys=ON");
            conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
                .map_err(|e| format!("该文件不是有效的 SQLite 数据库: {e}"))?;
            Ok(Backend::Sqlite(conn))
        }
        DbType::MySql => {
            let host = req(&info.host, "缺少主机地址")?;
            let port = info.port.unwrap_or(3306);
            let user = req(&info.user, "缺少用户名")?;
            let pass = info.password.as_deref().unwrap_or("");
            let db = info.database.as_deref().map(str::trim).filter(|s| !s.is_empty());
            // 连接池:多标签页并发查询不再互斥排队;保持 2 条热连接
            let opts = mysql_async::OptsBuilder::default()
                .ip_or_hostname(host)
                .tcp_port(port)
                .user(Some(user))
                .pass(Some(pass))
                .db_name(db)
                .pool_opts(
                    mysql_async::PoolOpts::default().with_constraints(
                        mysql_async::PoolConstraints::new(2, 8).expect("min<=max"),
                    ),
                );
            let pool = mysql_async::Pool::new(opts);
            Ok(Backend::MySql(crate::model::MySqlPool {
                pool,
                session_db: None,
                running_id: std::sync::atomic::AtomicU64::new(0),
            }))
        }
        DbType::Redis => {
            let host = req(&info.host, "缺少主机地址")?;
            let port = info.port.unwrap_or(6379);
            let pass = info.password.as_deref();
            let db_idx: i64 = info
                .database
                .as_deref()
                .and_then(|d| d.trim().parse().ok())
                .unwrap_or(0);
            let auth = pass.map(|p| format!(":{p}@")).unwrap_or_default();
            let url = format!("redis://{auth}{host}:{port}/{db_idx}");
            let client = redis::Client::open(url).map_err(|e| format!("Redis 地址无效: {e}"))?;
            // 试连 + PING
            let mut con = client
                .get_async_connection()
                .await
                .map_err(|e| format!("连接 Redis 失败: {e}"))?;
            redis::cmd("PING")
                .query_async::<String>(&mut con)
                .await
                .map_err(|e| format!("Redis PING 失败: {e}"))?;
            Ok(Backend::Redis(client))
        }
        DbType::Postgres => {
            let host = req(&info.host, "缺少主机地址")?;
            let port = info.port.unwrap_or(5432);
            let user = req(&info.user, "缺少用户名")?;
            let pass = info.password.as_deref().unwrap_or("");
            // 数据库可选:不填默认连 postgres 库
            let db = info.database.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("postgres");
            let cfg = format!(
                "host={} port={} user={} password={} dbname={} connect_timeout=8 application_name=数镜",
                pg_quote(host),
                port,
                pg_quote(user),
                pg_quote(pass),
                pg_quote(db)
            );
            // 多会话池:PG 单会话内查询串行,3 条会话轮发实现真并发
            let mut clients = Vec::new();
            let mut last_err: Option<tokio_postgres::Error> = None;
            for _ in 0..3 {
                match tokio_postgres::connect(&cfg, tokio_postgres::NoTls).await {
                    Ok((c, conn)) => {
                        tokio::spawn(async move {
                            if let Err(e) = conn.await {
                                eprintln!("[postgres] 连接中断: {e}");
                            }
                        });
                        clients.push(std::sync::Arc::new(c));
                    }
                    Err(e) => {
                        last_err = Some(e);
                        break;
                    }
                }
            }
            if clients.is_empty() {
                return Err(format!(
                    "连接 PostgreSQL 失败: {}",
                    last_err.map(|e| e.to_string()).unwrap_or_default()
                ));
            }
            Ok(Backend::Pg(PgPool {
                clients,
                next: std::sync::atomic::AtomicUsize::new(0),
                session_db: None,
                applied: vec![None; 3],
                running_pid: pg_pid.unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0))),
            }))
        }    }
}

impl Backend {
    /// MySQL:从池取连接并恢复会话库上下文(库已删除时清除记忆,不阻断取连接)
    pub(crate) async fn mysql_conn(
        mp: &mut crate::model::MySqlPool,
    ) -> Result<mysql_async::Conn, String> {
        use mysql_async::prelude::*;
        let mut conn = mp.pool.get_conn().await.map_err(es)?;
        // 记录会话 ID(免往返,来自握手),供 KILL QUERY 取消使用
        mp.running_id.store(conn.id() as u64, Ordering::Relaxed);
        if let Some(db) = mp.session_db.clone() {
            if conn
                .query_drop(format!("USE `{}`", db.replace('`', "``")))
                .await
                .is_err()
            {
                mp.session_db = None;
            }
        }
        Ok(conn)
    }

    /// PostgreSQL:轮发取一个会话,按需恢复 search_path
    pub(crate) async fn pg_client(
        mp: &mut crate::model::PgPool,
    ) -> Result<std::sync::Arc<tokio_postgres::Client>, String> {
        if mp.clients.is_empty() {
            return Err("PostgreSQL 连接池为空".into());
        }
        let n = mp.next.fetch_add(1, Ordering::Relaxed) % mp.clients.len();
        let client = mp.clients[n].clone();
        let want = mp.session_db.clone();
        if want.is_some() && mp.applied.get(n).map(|a| a.as_deref()) != Some(want.as_deref()) {
            let stmt = format!(
                "SET search_path TO \"{}\"",
                want.clone().unwrap_or_default().replace('"', "\"\"")
            );
            client
                .simple_query(&stmt)
                .await
                .map_err(|e| format!("恢复 search_path 失败: {e}"))?;
            if let Some(a) = mp.applied.get_mut(n) {
                *a = want;
            }
        }
        Ok(client)
    }

    /// 会话库上下文读取(MySQL/PG)
    pub fn session_db(&self) -> Option<&str> {
        match self {
            Backend::MySql(mp) => mp.session_db.as_deref(),
            Backend::Pg(p) => p.session_db.as_deref(),
            _ => None,
        }
    }

    pub fn set_session_db(&mut self, db: Option<&str>) {
        match self {
            Backend::MySql(mp) => mp.session_db = db.map(str::to_string),
            Backend::Pg(p) => {
                p.session_db = db.map(str::to_string);
                // 上下文变化:所有槽位重新应用
                p.applied.iter_mut().for_each(|a| *a = None);
            }
            _ => {}
        }
    }

    pub async fn server_info(&mut self) -> Result<String, String> {
        match self {
            Backend::Sqlite(_) => Ok(format!("SQLite {}", rusqlite::version())),
            Backend::MySql(mp) => {
                use mysql_async::prelude::*;
                let mut conn = mp.pool.get_conn().await.map_err(es)?;
                let mut result = conn.query_iter("SELECT VERSION()").await.map_err(es)?;
                let mut version = String::new();
                result
                    .for_each(|row| {
                        if version.is_empty() {
                            if let Some(v) = row.unwrap().first() {
                                if let Some(s) = crate::exec::fmt_mysql_value(v) {
                                    version = s;
                                }
                            }
                        }
                    })
                    .await
                    .map_err(es)?;
                Ok(if version.is_empty() { "MySQL".into() } else { version })
            }
            Backend::Redis(client) => {
                let mut con = client
                    .get_async_connection()
                    .await
                    .map_err(es)?;
                let info_txt: String = redis::cmd("INFO")
                    .arg("server")
                    .query_async(&mut con)
                    .await
                    .map_err(es)?;
                let ver = info_txt
                    .lines()
                    .find(|l| l.starts_with("redis_version:"))
                    .map(|l| l.to_string())
                    .unwrap_or_else(|| "Redis".into());
                Ok(format!("Redis {ver}"))
            }
            Backend::Pg(mp) => {
                let client = mp.clients.first().cloned().ok_or("PostgreSQL 连接池为空")?;
                let rows = client.simple_query("SELECT version()").await.map_err(es)?;
                for msg in rows {
                    if let tokio_postgres::SimpleQueryMessage::Row(row) = msg {
                        if let Some(v) = row.get(0) {
                            return Ok(v.to_string());
                        }
                    }
                }
                Ok("PostgreSQL".into())
            }
        }
    }
}
