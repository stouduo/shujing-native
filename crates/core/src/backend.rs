use crate::model::{es, Backend, ConnInfo, DbType, PgPool};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// tokio_postgres::Error 的 Display 只有「db error」,细节在 source(DbError)里
pub(crate) fn pg_err(e: tokio_postgres::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(x) = src {
        out.push_str(": ");
        out.push_str(&x.to_string());
        src = x.source();
    }
    out
}

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
    gen: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
) -> Result<Backend, String> {
    match info.db_type {
        DbType::Rabbit => {
            let host = req(&info.host, "缺少 RabbitMQ management 地址")?;
            let port = info.port.unwrap_or(15672);
            // host 含 : 视为完整 management 地址;否则 host:port
            let base = if host.starts_with("http://") || host.starts_with("https://") {
                host.to_string()
            } else if host.contains(':') {
                format!("http://{host}")
            } else {
                format!("http://{host}:{port}")
            };
            let user = info
                .user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("guest")
                .to_string();
            let pass = info.password.clone().unwrap_or_default();
            let auth = if user != "guest" || !pass.is_empty() {
                Some((user, pass))
            } else {
                None
            };
            let st = crate::rabbit::connect(base, auth).await?;
            Ok(Backend::Rabbit(st))
        }
        DbType::Mqtt => {
            let host = req(&info.host, "缺少 MQTT broker 地址")?;
            let port = info.port.unwrap_or(1883);
            let user = info
                .user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let auth = user.map(|u| (u, info.password.clone().unwrap_or_default()));
            let st = crate::mqtt::MqttState {
                host: host.to_string(),
                port,
                auth,
            };
            crate::mqtt::probe(&st).await?;
            Ok(Backend::Mqtt(st))
        }
        DbType::Nacos | DbType::Consul | DbType::Etcd => {
            let host = req(&info.host, "缺少配置中心地址")?;
            let default_port = match info.db_type {
                DbType::Nacos => 8848,
                DbType::Consul => 8500,
                _ => 2379,
            };
            let base = if host.starts_with("http://") || host.starts_with("https://") {
                host.trim_end_matches('/').to_string()
            } else if host.contains(':') {
                format!("http://{host}")
            } else {
                format!("http://{host}:{default_port}")
            };
            let base = if info.db_type == DbType::Nacos && !base.contains("/nacos") {
                format!("{base}/nacos")
            } else {
                base
            };
            let user = info
                .user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let auth = user.map(|u| (u, info.password.clone().unwrap_or_default()));
            let kind = match info.db_type {
                DbType::Nacos => crate::kv::KvKind::Nacos,
                DbType::Consul => crate::kv::KvKind::Consul,
                _ => crate::kv::KvKind::Etcd,
            };
            let st = crate::kv::connect(
                kind,
                base,
                auth,
                info.database.clone().filter(|d| !d.trim().is_empty()),
            )
            .await?;
            Ok(Backend::Kv(st))
        }
        DbType::Kafka => {
            let host = req(&info.host, "缺少 Kafka bootstrap 地址")?;
            let port = info.port.unwrap_or(9092);
            let addr = if host.contains(':') {
                host.to_string()
            } else {
                format!("{host}:{port}")
            };
            let st = crate::kafka::connect(&addr).await?;
            Ok(Backend::Kafka(st))
        }
        DbType::Sqlite => {
            let path = req(&info.file_path, "缺少 SQLite 数据库文件路径")?;
            // 路径必须已存在:Connection::open 默认带 CREATE,打错的路径会静默
            // 新建空库(随后 SELECT 1 校验照样通过,用户以为连上了真库)
            if !std::path::Path::new(path).exists() {
                return Err(format!("文件不存在: {path}(不会自动新建数据库文件)"));
            }
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
            // 池是惰性的:立即探活一次,给不可达主机一个 8s 上限(否则 OS 级 TCP 超时 75s+)
            {
                use mysql_async::prelude::*;
                let mut probe = tokio::time::timeout(
                    std::time::Duration::from_secs(8),
                    pool.get_conn(),
                )
                .await
                .map_err(|_| "连接 MySQL 超时(8 秒): 请检查主机/端口/防火墙".to_string())?
                .map_err(|e| format!("连接 MySQL 失败: {e}"))?;
                probe
                    .query_drop("SELECT 1")
                    .await
                    .map_err(|e| format!("MySQL 探活失败: {e}"))?;
            }
            Ok(Backend::MySql(crate::model::MySqlPool {
                pool,
                session_db: None,
                running_id: std::sync::atomic::AtomicU64::new(0),
                gen: gen.unwrap_or_else(|| {
                    std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0))
                }),
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
            // 密码 percent-encode:驱动会 percent-decode userinfo,含 /#?% 的明文密码会错位
            let enc: String = pass
                .unwrap_or_default()
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"._~-".contains(&b) {
                        format!("{}", b as char)
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect();
            let auth = (!enc.is_empty()).then(|| format!(":{enc}@")).unwrap_or_default();
            let url = format!("redis://{auth}{host}:{port}/{db_idx}");
            let client = redis::Client::open(url).map_err(|e| format!("Redis 地址无效: {e}"))?;
            // 试连 + PING(用 ConnectionManager 验证,与运行期连接类型一致)
            let mut con = redis::aio::ConnectionManager::new(client.clone())
                .await
                .map_err(|e| format!("连接 Redis 失败: {e}"))?;
            redis::cmd("PING")
                .query_async::<String>(&mut con)
                .await
                .map_err(|e| format!("Redis PING 失败: {e}"))?;
            Ok(Backend::Redis(crate::model::RedisState {
                client,
                con: tokio::sync::Mutex::new(None),
                op_lock: tokio::sync::Mutex::new(()),
            }))
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
                gen: gen.unwrap_or_else(|| {
                    std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0))
                }),
            }))
        }
        DbType::Mssql => {
            let host = req(&info.host, "缺少 SQL Server 主机地址")?;
            let port = info.port.unwrap_or(1433);
            let user = req(&info.user, "缺少用户名")?;
            let pass = info.password.as_deref().unwrap_or("");
            let db = info.database.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let mut clients = Vec::new();
            let mut last_err: Option<String> = None;
            for _ in 0..3 {
                let tcp = match tokio::time::timeout(
                    Duration::from_secs(8),
                    tokio::net::TcpStream::connect((host, port)),
                )
                .await
                {
                    Ok(Ok(t)) => t,
                    Ok(Err(e)) => {
                        last_err = Some(format!("TCP 连接失败: {e}"));
                        break;
                    }
                    Err(_) => {
                        last_err = Some("连接 SQL Server 超时(8s)".into());
                        break;
                    }
                };
                tcp.set_nodelay(true).ok();
                let mut config = tiberius::Config::new();
                config.host(host);
                config.port(port);
                config.authentication(tiberius::AuthMethod::sql_server(user, pass));
                if let Some(d) = db {
                    config.database(d);
                }
                config.encryption(tiberius::EncryptionLevel::NotSupported);
                use tokio_util::compat::TokioAsyncWriteCompatExt;
                match tiberius::Client::connect(config, tcp.compat_write()).await {
                    Ok(c) => clients.push(tokio::sync::Mutex::new(c)),
                    Err(e) => {
                        last_err = Some(format!("SQL Server 认证/握手失败: {e}"));
                        break;
                    }
                }
            }
            if clients.is_empty() {
                return Err(format!(
                    "连接 SQL Server 失败: {}",
                    last_err.unwrap_or_default()
                ));
            }
            Ok(Backend::Mssql(crate::model::MssqlState {
                clients,
                next: std::sync::atomic::AtomicUsize::new(0),
            }))
        }
        DbType::ClickHouse => {
            let host = req(&info.host, "缺少 ClickHouse 主机地址")?;
            let port = info.port.unwrap_or(8123);
            // host 栏粘 http(s):// 开头 = 整串当 base(HTTPS/自建代理直填)
            let base = if host.starts_with("http://") || host.starts_with("https://") {
                host.to_string()
            } else {
                format!("http://{host}:{port}")
            };
            let user = info
                .user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("default")
                .to_string();
            let pass = info.password.clone().unwrap_or_default();
            // default 用户可匿名;其余带鉴权头(CH 专用头比 Basic 兼容面更宽)
            let auth = if user != "default" || !pass.is_empty() {
                Some((user, pass))
            } else {
                None
            };
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| format!("HTTP 客户端构建失败: {e}"))?;
            let mut st = crate::model::ClickHouseState { base, auth, http };
            // 探活
            crate::clickhouse::query(&st, "SELECT 1", None, 1)
                .await
                .map_err(|e| format!("连接 ClickHouse 失败: {e}"))?;
            // 会话默认库挂 URL 参数,后续查询不用带库限定
            if let Some(d) = info.database.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                st.base = crate::clickhouse::with_database_param(&st.base, d);
            }
            Ok(Backend::ClickHouse(st))
        }
        DbType::Mongo => {
            let host = req(&info.host, "缺少 MongoDB 主机地址")?;
            // 主机栏直接粘完整连接串(含 Atlas 的 mongodb+srv://)即 URL 模式
            if host.starts_with("mongodb://") || host.starts_with("mongodb+srv://") {
                let client = mongodb::Client::with_uri_str(host)
                    .await
                    .map_err(|e| format!("MongoDB 连接失败: {e}"))?;
                tokio::time::timeout(
                    Duration::from_secs(8),
                    client
                        .database("admin")
                        .run_command(mongodb::bson::doc! { "ping": 1 }),
                )
                .await
                .map_err(|_| "MongoDB 连接超时(8s)".to_string())?
                .map_err(|e| format!("MongoDB 连接失败: {e}"))?;
                return Ok(Backend::Mongo(crate::model::MongoState {
                    client,
                    cur_db: tokio::sync::Mutex::new(
                        info.database
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                    ),
                }));
            }
            let port = info.port.unwrap_or(27017);
            let mut url = match (&info.user, &info.password) {
                (Some(u), Some(p)) => format!(
                    "mongodb://{}:{}@{host}:{port}",
                    urlencode(u),
                    urlencode(p)
                ),
                _ => format!("mongodb://{host}:{port}"),
            };
            let auth_db = info
                .database
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(d) = auth_db {
                url.push('/');
                url.push_str(d);
            }
            url.push_str("?serverSelectionTimeoutMS=8000");
            let client = mongodb::Client::with_uri_str(&url)
                .await
                .map_err(|e| format!("MongoDB 连接失败: {e}"))?;
            // 探活:8s 上限(与其他后端一致,避免 OS 级 TCP 超时)
            tokio::time::timeout(
                Duration::from_secs(8),
                client
                    .database("admin")
                    .run_command(mongodb::bson::doc! { "ping": 1 }),
            )
            .await
            .map_err(|_| "MongoDB 连接超时(8s)".to_string())?
            .map_err(|e| format!("MongoDB 连接失败: {e}"))?;
            Ok(Backend::Mongo(crate::model::MongoState {
                client,
                cur_db: tokio::sync::Mutex::new(auth_db.map(str::to_string)),
            }))
        }
    }
}

/// URL userinfo 的 percent-encode(密码含 @/:/? 等会破坏连接串解析)
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

    /// PostgreSQL:轮发取一个会话,按需恢复 search_path。
    /// 返回的是共享 Arc<Client>:事务类操作(apply_changes 的 BEGIN/COMMIT)之所以安全,
    /// 依赖 Service 所有 DB 方法都持有同一把每连接互斥锁——新增长连接方法时必须沿用该纪律
    /// 会话列表(MySQL SHOW FULL PROCESSLIST / PG pg_stat_activity)
    pub(crate) async fn list_sessions(
        backend: &mut Backend,
    ) -> Result<Vec<crate::model::SessionInfo>, String> {
        match backend {
            Backend::MySql(mp) => {
                use mysql_async::prelude::*;
                let mut conn = Backend::mysql_conn(mp).await?;
                let mine = conn.id() as u64;
                let rows: Vec<(
                    u64,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    Option<u64>,
                    Option<String>,
                    Option<String>,
                )> = conn
                    .query(
                        "SELECT Id, User, Host, COALESCE(Db,''), Command, COALESCE(Time,0), COALESCE(State,''), COALESCE(Info,'') FROM information_schema.processlist ORDER BY Time DESC",
                    )
                    .await
                    .map_err(|e| format!("MySQL: {e}"))?;
                Ok(rows
                    .into_iter()
                    .filter(|(id, _, _, _, _, _, _, _)| *id != mine)
                    .map(
                        |(id, user, host, db, command, time, state, info)| {
                            crate::model::SessionInfo {
                                id: id.to_string(),
                                user: user.unwrap_or_default(),
                                host: host.unwrap_or_default(),
                                db: db.filter(|s| !s.is_empty()),
                                command: command.unwrap_or_default(),
                                time_secs: time.unwrap_or(0) as i64,
                                state: state.unwrap_or_default(),
                                query: info.unwrap_or_default(),
                            }
                        },
                    )
                    .collect())
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                let rows = client
                    .simple_query(
                        "SELECT pid::text, COALESCE(usename,''), COALESCE(application_name,''),                          COALESCE(datname,''), COALESCE(state,''),                          COALESCE(ROUND(EXTRACT(EPOCH FROM (now()-query_start)))::text,'0'),                          COALESCE(wait_event_type,''), COALESCE(query,'')                          FROM pg_stat_activity WHERE pid <> pg_backend_pid() ORDER BY query_start DESC NULLS LAST",
                    )
                    .await
                    .map_err(crate::backend::pg_err)?;
                let mut out = Vec::new();
                for m in rows {
                    let tokio_postgres::SimpleQueryMessage::Row(r) = m else {
                        continue;
                    };
                    let get = |i: usize| -> String {
                        r.get(i).unwrap_or_default().to_string()
                    };
                    out.push(crate::model::SessionInfo {
                        id: get(0),
                        user: get(1),
                        host: get(2),
                        db: Some(get(3)).filter(|s| !s.is_empty()),
                        command: get(4),
                        time_secs: get(5).parse().unwrap_or(0),
                        state: get(6),
                        query: get(7),
                    });
                }
                Ok(out)
            }
            _ => Err("该数据库类型不支持会话面板".into()),
        }
    }

    /// 终止会话(query_only=true 仅中断当前查询)
    pub(crate) async fn kill_session(
        backend: &mut Backend,
        sid: &str,
        query_only: bool,
    ) -> Result<(), String> {
        match backend {
            Backend::MySql(mp) => {
                use mysql_async::prelude::*;
                // processlist 的 Id 是纯数字;校验后裸用(引注反而是语法错误)
                if !sid.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("非法会话 ID".into());
                }
                let mut conn = Backend::mysql_conn(mp).await?;
                let sql = if query_only {
                    format!("KILL QUERY {sid}")
                } else {
                    format!("KILL {sid}")
                };
                conn.query_drop(sql)
                    .await
                    .map_err(|e| format!("终止失败: {e}"))
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                // pid 纯数字校验(引注是标识符语义,PG 函数要的是数字)
                if !sid.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("非法会话 ID".into());
                }
                let sql = if query_only {
                    format!("SELECT pg_cancel_backend({sid})")
                } else {
                    format!("SELECT pg_terminate_backend({sid})")
                };
                client.simple_query(&sql).await.map(|_| ()).map_err(crate::backend::pg_err)
            }
            _ => Err("该数据库类型不支持会话面板".into()),
        }
    }

    pub(crate) async fn pg_client(
        mp: &mut crate::model::PgPool,
    ) -> Result<std::sync::Arc<tokio_postgres::Client>, String> {
        if mp.clients.is_empty() {
            return Err("PostgreSQL 连接池为空".into());
        }
        let n = mp.next.fetch_add(1, Ordering::Relaxed) % mp.clients.len();
        let client = mp.clients[n].clone();
        // 代数戳递增:本次 checkout 视为一次新活动,旧取消作废
        mp.gen.fetch_add(1, Ordering::Relaxed);
        // 回填会话 pid:取消查询(pg_cancel_backend)依赖它,不填则取消永远报"无运行中查询"
        // 函数调用必须有括号:pg_backend_pid 裸名被解析成列;消息序列首个是
        // RowDescription 而非 Row(rows.first() 永远拿错)——逐条找 Row。
        // 此回填曾两处皆错,PG 取消一直报「没有正在运行的查询」(E2E 实测抓到)
        if let Ok(rows) = client.simple_query("SELECT pg_backend_pid()").await {
            let pid_str = rows.iter().find_map(|m| match m {
                tokio_postgres::SimpleQueryMessage::Row(r) => r.get(0),
                _ => None,
            });
            if let Some(p) = pid_str.and_then(|v| v.parse::<u32>().ok()) {
                mp.running_pid.store(p, Ordering::Relaxed);
            }
        }
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

    /// SQL Server:轮发取一条会话
    pub(crate) async fn mssql_client(
        st: &mut crate::model::MssqlState,
    ) -> Result<tokio::sync::MutexGuard<'_, crate::model::MssqlClient>, String> {
        if st.clients.is_empty() {
            return Err("SQL Server 连接池为空".into());
        }
        let n = st.next.fetch_add(1, Ordering::Relaxed) % st.clients.len();
        let idx = n.min(st.clients.len().saturating_sub(1));
        Ok(st.clients[idx].lock().await)
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
            Backend::Redis(rs) => {
                let mut con = rs
                    .client
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
            Backend::Mongo(m) => {
                let info = m
                    .client
                    .database("admin")
                    .run_command(mongodb::bson::doc! { "buildInfo": 1 })
                    .await
                    .map_err(|e| format!("MongoDB: {e}"))?;
                let ver = info
                    .get_str("version")
                    .map(|s| format!("MongoDB {s}"))
                    .unwrap_or_else(|_| "MongoDB".into());
                Ok(ver)
            }
            Backend::Mssql(st) => {
                let mut client = Backend::mssql_client(st).await?;
                let rows = client
                    .simple_query("SELECT @@VERSION")
                    .await
                    .map_err(|e| format!("SQL Server: {e}"))?
                    .into_first_result()
                    .await
                    .map_err(|e| format!("SQL Server: {e}"))?;
                let ver = rows
                    .first()
                    .and_then(|r| r.get::<&str, _>(0))
                    .map(|s| s.split(" on ").next().unwrap_or(s).to_string())
                    .unwrap_or_else(|| "SQL Server".into());
                Ok(ver)
            }
            Backend::ClickHouse(st) => {
                let r = crate::clickhouse::query(st, "SELECT version()", None, 1).await?;
                let ver = r
                    .rows
                    .first()
                    .and_then(|r| r.first())
                    .and_then(|c| c.clone())
                    .unwrap_or_default();
                Ok(format!("ClickHouse {ver}"))
            }
            Backend::Kafka(st) => {
                let topics = crate::kafka::list_topics(st).await?;
                Ok(format!("Kafka · {} topics", topics.len()))
            }
            Backend::Rabbit(st) => {
                let o = crate::rabbit::overview(st).await?;
                Ok(format!(
                    "RabbitMQ {} · {} queues · {} msgs",
                    o.version, o.queue_count, o.msg_total
                ))
            }
            Backend::Mqtt(_) => Ok("MQTT broker".into()),
            Backend::Kv(st) => {
                let entries = crate::kv::list(st, "").await?;
                Ok(format!(
                    "{} · {} keys",
                    match st.kind {
                        crate::kv::KvKind::Nacos => "Nacos",
                        crate::kv::KvKind::Consul => "Consul",
                        crate::kv::KvKind::Etcd => "etcd",
                    },
                    entries.len()
                ))
            }
        }
    }
}
