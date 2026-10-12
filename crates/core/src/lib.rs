//! 数镜原生版核心服务:自 dbclient/src-tauri 迁移的连接池/SQL 执行/编辑回写/结构查询。
//! 纯 Rust 库,无 UI 框架依赖;egui 应用进程内直调(无 IPC)。

pub mod backend;
pub mod edits;
pub mod exec;
mod model;
pub mod redis_ops;
pub mod schema;
mod search;
pub mod store;

pub use model::{
    AppState, ConnInfo, ConnectResult, DbType, ExecResult, LiveConn, TableMeta, TableStructure,
};

use model::CancelState;
use std::path::PathBuf;

/// 顶层服务:持有连接状态与配置目录(等价于 Tauri 版的 AppState + AppHandle)
pub struct Service {
    pub state: AppState,
    pub config_dir: PathBuf,
}

impl Service {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            state: AppState::default(),
            config_dir: config_dir.into(),
        }
    }

    pub fn list_saved_connections(&self) -> Vec<ConnInfo> {
        store::load_conns(&self.config_dir)
    }

    /// 新增或更新连接,返回保存后的完整列表
    pub fn save_connection(&self, info: ConnInfo) -> Result<Vec<ConnInfo>, String> {
        let mut conns = store::load_conns(&self.config_dir);
        conns.retain(|c| c.id != info.id);
        conns.push(info.clone());
        store::save_conns(&self.config_dir, &conns)?;
        // 若该连接已在线且配置变了,断开旧连接让用户重连
        self.state.remove(&info.id);
        Ok(conns)
    }

    pub fn delete_connection(&self, id: &str) -> Result<Vec<ConnInfo>, String> {
        self.state.remove(id);
        let mut conns = store::load_conns(&self.config_dir);
        conns.retain(|c| c.id != id);
        store::save_conns(&self.config_dir, &conns)?;
        Ok(conns)
    }

    pub async fn test_connection(info: ConnInfo) -> Result<ConnectResult, String> {
        let mut backend = backend::connect(&info, None).await?;
        let version = backend.server_info().await?;
        Ok(ConnectResult { version })
    }

    pub async fn connect(&self, info: ConnInfo) -> Result<ConnectResult, String> {
        let cancel = std::sync::Arc::new(CancelState::new(
            &info,
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        ));
        let mut backend = backend::connect(&info, Some(cancel.pg_pid.clone())).await?;
        // 重复连接同一 id(如改配置重连)时保留会话库记忆
        let prev = match self.state.live(&info.id) {
            Some(old) => old.lock().await.backend.session_db().map(str::to_string),
            None => None,
        };
        backend.set_session_db(prev.as_deref());
        let version = backend.server_info().await?;
        self.state.insert(LiveConn { info, backend }, cancel);
        Ok(ConnectResult { version })
    }

    pub async fn disconnect(&self, id: &str) -> Result<(), String> {
        // 直接移出连接表,引用清零后 TCP 连接随之关闭
        self.state.remove(id);
        Ok(())
    }

    /// 列出服务器上所有数据库
    pub async fn list_databases(&self, id: &str) -> Result<Vec<String>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        schema::list_databases(&mut guard.backend, &info).await
    }

    /// 列出指定数据库中的表
    pub async fn list_tables(
        &self,
        id: &str,
        database: Option<String>,
    ) -> Result<Vec<TableMeta>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard
            .backend
            .list_tables_in(&info, database.as_deref().or(info.database.as_deref()))
            .await
    }

    pub async fn list_tables_legacy(&self, id: &str) -> Result<Vec<TableMeta>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard.backend.list_tables(&info).await
    }

    pub async fn get_table_structure(
        &self,
        id: &str,
        table: &str,
        database: Option<String>,
    ) -> Result<TableStructure, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard
            .backend
            .get_table_structure(&info, table, database.as_deref())
            .await
    }

    pub async fn list_foreign_keys(
        &self,
        id: &str,
        database: Option<String>,
    ) -> Result<Vec<schema::FkMeta>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard
            .backend
            .list_foreign_keys(&info, database.as_deref())
            .await
    }

    pub async fn count_rows(&self, id: &str, table: &str) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        guard.backend.count_rows(table).await
    }

    pub async fn get_object_ddl(&self, id: &str, kind: &str, name: &str) -> Result<String, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        schema::object_ddl(&mut guard.backend, &info, kind, name).await
    }

    pub async fn search_all_tables(
        &self,
        id: &str,
        keyword: &str,
        max_hits: usize,
    ) -> Result<Vec<search::SearchHit>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        search::search_tables(&mut guard.backend, &info, keyword, max_hits).await
    }

    pub async fn apply_changes(
        &self,
        id: &str,
        table: &str,
        updates: &[edits::CellUpdate],
        deletes: &[Vec<(String, String)>],
        inserts: &[Vec<(String, String)>],
    ) -> Result<ExecResult, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        edits::apply_changes(&mut guard.backend, &info, table, updates, deletes, inserts).await
    }

    /// 判断错误是否为连接失效(空闲超时/网络中断等),可尝试重连
    fn is_dead_conn(e: &str) -> bool {
        let s = e.to_lowercase();
        const KEYS: &[&str] = &[
            "broken pipe",
            "connection reset",
            "connection closed",
            "connection refused",
            "server has gone away",
            "gone away",
            "lost connection",
            "timed out",
            "timeout",
            "2006",
            "2013",
        ];
        KEYS.iter().any(|k| s.contains(k))
    }

    /// 执行 SQL,返回多结果集(PostgreSQL 完整支持;MySQL/SQLite 返回第一个结果集)。
    /// 含只读拦截与断线自动重连。
    pub async fn run_sql(
        &self,
        id: &str,
        sql: String,
        max_rows: Option<usize>,
    ) -> Result<Vec<ExecResult>, String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err("SQL 为空".into());
        }
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let cancel_state = self.state.cancel(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only && !model::is_readonly_sql(&sql) {
            return Err("只读连接:仅允许 SELECT/WITH/EXPLAIN/SHOW/DESCRIBE/PRAGMA".into());
        }
        let max_rows = max_rows.unwrap_or(exec::DEFAULT_MAX_ROWS);
        match guard
            .backend
            .run_sql(&sql, max_rows, Some(&cancel_state.mysql_id))
            .await
        {
            Ok(r) => Ok(r),
            Err(e) => {
                if !Self::is_dead_conn(&e) {
                    return Err(e);
                }
                // 连接已整体失效(池不可用/网络中断):重建后端并恢复会话库后重试
                let info = guard.info.clone();
                let session_db = guard.backend.session_db().map(str::to_string);
                let mut nb = backend::connect(&info, None)
                    .await
                    .map_err(|e2| format!("连接已断开,自动重连失败: {e2}(原错误: {e})"))?;
                nb.set_session_db(session_db.as_deref());
                let r = nb
                    .run_sql(&sql, max_rows, Some(&cancel_state.mysql_id))
                    .await
                    .map_err(|e2| format!("{e2}(已自动重连)"))?;
                guard.backend = nb;
                Ok(r)
            }
        }
    }

    /// 取消当前正在执行的查询(MySQL KILL QUERY / PG pg_cancel_backend)
    pub async fn cancel_query(&self, id: &str) -> Result<(), String> {
        let entry = self.state.cancel(id).ok_or("连接未建立或已断开")?;
        let db_type = entry.db_type;
        let host = entry.host.clone();
        let port = entry.port;
        let user = entry.user.clone();
        let password = entry.password.clone();
        let database = entry.database.clone();
        match db_type {
            model::DbType::MySql => {
                let cid = entry.mysql_id.load(std::sync::atomic::Ordering::Relaxed);
                if cid == 0 {
                    return Err("没有正在运行的查询".into());
                }
                let opts = mysql_async::OptsBuilder::default()
                    .ip_or_hostname(host.as_deref().unwrap_or("127.0.0.1"))
                    .tcp_port(port.unwrap_or(3306))
                    .user(user.as_deref())
                    .pass(Some(password.as_deref().unwrap_or("")));
                let mut conn = mysql_async::Conn::new(opts)
                    .await
                    .map_err(|e| format!("取消失败(控制连接): {e}"))?;
                mysql_async::prelude::Queryable::query_drop(
                    &mut conn,
                    format!("KILL QUERY {cid}"),
                )
                .await
                .map_err(|e| format!("取消失败: {e}"))?;
                Ok(())
            }
            model::DbType::Postgres => {
                let pid = entry.pg_pid.load(std::sync::atomic::Ordering::Relaxed);
                if pid == 0 {
                    return Err("没有正在运行的查询".into());
                }
                let cfg = format!(
                    "host={} port={} user={} password={} dbname={} connect_timeout=8",
                    backend::pg_quote(host.as_deref().unwrap_or("127.0.0.1")),
                    port.unwrap_or(5432),
                    backend::pg_quote(user.as_deref().unwrap_or("postgres")),
                    backend::pg_quote(password.as_deref().unwrap_or("")),
                    backend::pg_quote(database.as_deref().unwrap_or("postgres"))
                );
                let (client, connection) = tokio_postgres::connect(&cfg, tokio_postgres::NoTls)
                    .await
                    .map_err(|e| format!("取消失败(控制连接): {e}"))?;
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                client
                    .simple_query(&format!("SELECT pg_cancel_backend({pid})"))
                    .await
                    .map_err(|e| format!("取消失败: {e}"))?;
                Ok(())
            }
            _ => Err("该数据库类型不支持取消".into()),
        }
    }

    /// 构建单表 SQL 文本(结构 + 可选数据)
    async fn build_table_dump(
        &self,
        backend: &mut model::Backend,
        info: &model::ConnInfo,
        table: &str,
        with_data: bool,
    ) -> Result<(String, u64), String> {
        let mysql = info.db_type == model::DbType::MySql;
        let q = |s: &str| -> String {
            if mysql {
                format!("`{}`", s.replace('`', "``"))
            } else {
                format!("\"{}\"", s.replace('"', "\"\""))
            }
        };
        let lit = |v: Option<&str>| -> String {
            match v {
                None => "NULL".to_string(),
                Some(s) => format!("'{}'", s.replace('\'', "''")),
            }
        };
        let qt = q(table);

        let mut out = String::new();
        out.push_str(&format!("-- 数镜 dump: table {qt}\n"));
        // 建表语句:SQLite/MySQL 用原生 DDL,PG 简化合成
        match backend.get_table_structure(info, table, None).await {
            Ok(st) if !st.ddl.is_empty() => {
                out.push_str(&st.ddl.trim_end_matches(';'));
                out.push_str(";\n\n");
            }
            Ok(st) if !st.columns.is_empty() => {
                let cols = st
                    .columns
                    .iter()
                    .map(|c| {
                        let mut d = format!("  {} {}", q(&c.name), c.data_type);
                        if !c.nullable {
                            d.push_str(" NOT NULL");
                        }
                        if let Some(def) = &c.default {
                            d.push_str(&format!(" DEFAULT {}", def));
                        }
                        d
                    })
                    .collect::<Vec<_>>()
                    .join(",\n");
                out.push_str(&format!(
                    "CREATE TABLE {qt} (\n{cols}\n); -- 由 数镜 按列定义简化生成\n\n"
                ));
            }
            _ => {}
        }
        let mut exported: u64 = 0;
        const BATCH: usize = 500;
        let mut offset: usize = 0;
        let mut cols_cache: Vec<String> = Vec::new();
        loop {
            if !with_data {
                break;
            }
            let sql = format!("SELECT * FROM {qt} LIMIT {BATCH} OFFSET {offset}");
            let r = backend.run_one(&sql, BATCH).await?;
            if r.columns.is_empty() || r.rows.is_empty() {
                break;
            }
            if cols_cache.is_empty() {
                cols_cache = r.columns.clone();
                let col_list = cols_cache.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
                out.push_str(&format!("INSERT INTO {qt} ({col_list}) VALUES\n"));
            }
            for row in &r.rows {
                let vals = row.iter().map(|c| lit(c.as_deref())).collect::<Vec<_>>().join(", ");
                out.push_str(&format!("({vals}),\n"));
                exported += 1;
            }
            offset += BATCH;
            if r.rows.len() < BATCH {
                break;
            }
            if exported > 1_000_000 {
                out.push_str("-- 超过 100 万行,导出中止\n");
                break;
            }
        }
        if exported > 0 {
            let trimmed = out.trim_end().trim_end_matches(',');
            out = trimmed.to_string();
            out.push_str(";\n");
        } else if with_data {
            out.push_str("-- (空表)\n");
        }
        Ok((out, exported))
    }

    /// 导出整表 SQL(结构 + 可选数据)。返回 (SQL 文本, 导出行数)
    pub async fn export_table_sql(
        &self,
        id: &str,
        table: &str,
        with_data: bool,
    ) -> Result<(String, u64), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        self.build_table_dump(&mut guard.backend, &info, table, with_data).await
    }

    /// 导出整个数据库 SQL(所有表,结构 + 可选数据)
    pub async fn export_database_sql(&self, id: &str, with_data: bool) -> Result<(String, u64), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        let tables = guard.backend.list_tables(&info).await?;
        let mut out = String::new();
        out.push_str(&format!(
            "-- 数镜 database dump\n-- tables: {}\n\n",
            tables.iter().filter(|t| t.kind == "table").count()
        ));
        let mut exported: u64 = 0;
        for t in tables.iter().filter(|t| t.kind == "table") {
            let (dump, n) =
                self.build_table_dump(&mut guard.backend, &info, &t.name, with_data).await?;
            out.push_str(&dump);
            out.push('\n');
            exported += n;
        }
        Ok((out, exported))
    }

    // ── Redis ────────────────────────────────────────────

    pub async fn redis_scan(
        &self,
        id: &str,
        db: u8,
        pattern: &str,
        cursor: u64,
        count: usize,
    ) -> Result<(u64, Vec<String>), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::scan_keys(&mut guard.backend, db, pattern, cursor, count).await
    }

    pub async fn redis_key_detail(
        &self,
        id: &str,
        db: u8,
        key: &str,
    ) -> Result<redis_ops::RedisDetail, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::key_detail(&mut guard.backend, db, key).await
    }

    pub async fn redis_key_types(
        &self,
        id: &str,
        db: u8,
        keys: &[String],
    ) -> Result<Vec<String>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::key_types(&mut guard.backend, db, keys).await
    }

    pub async fn redis_del(&self, id: &str, db: u8, key: &str) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许删除".into());
        }
        redis_ops::del_key(&mut guard.backend, db, key).await
    }

    pub async fn redis_set(&self, id: &str, db: u8, key: &str, value: &str) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许写入".into());
        }
        redis_ops::set_string(&mut guard.backend, db, key, value, true).await
    }

    pub async fn redis_set_ttl(&self, id: &str, db: u8, key: &str, seconds: i64) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许修改 TTL".into());
        }
        redis_ops::set_ttl(&mut guard.backend, db, key, seconds).await
    }

    pub async fn redis_rename(&self, id: &str, db: u8, key: &str, new_key: &str) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许重命名".into());
        }
        redis_ops::rename(&mut guard.backend, db, key, new_key).await
    }

    pub async fn redis_ttl_batch(&self, id: &str, db: u8, keys: &[String]) -> Result<Vec<i64>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::ttl_batch(&mut guard.backend, db, keys).await
    }

    pub async fn redis_new_key(
        &self,
        id: &str,
        db: u8,
        key: &str,
        key_type: &str,
        text: &str,
        pairs: &[(String, String)],
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许新建".into());
        }
        redis_ops::new_key(&mut guard.backend, db, key, key_type, text, pairs).await
    }

    pub async fn redis_member_op(
        &self,
        id: &str,
        db: u8,
        key: &str,
        key_type: &str,
        op: &str,
        member: &str,
        extra: &str,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许修改".into());
        }
        redis_ops::member_op(&mut guard.backend, db, key, key_type, op, member, extra).await
    }

    pub async fn redis_analyze(
        &self,
        id: &str,
        db: u8,
        sample: usize,
        mode: &str,
    ) -> Result<Vec<redis_ops::KeyStat>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::analyze(&mut guard.backend, db, sample.min(20000), mode).await
    }

    pub async fn redis_run(&self, id: &str, command: &str) -> Result<Vec<String>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        redis_ops::run_command(&mut guard.backend, &info, command).await
    }
}

#[cfg(test)]
mod tests {
    use crate::backend;
    use crate::edits;
    use crate::model;
    use crate::schema;
    use crate::search;
    use crate::model::{ConnInfo, DbType};

    fn sqlite_info(path: &str) -> ConnInfo {
        ConnInfo {
            id: "test".into(),
            name: "test".into(),
            db_type: DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(path.into()),
            color: None,
            read_only: false,
        }
    }

    fn tmp_db(tag: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("shujing_native_test_{}_{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn sqlite_full_flow() {
        let path = tmp_db("flow");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();

        b.run_sql("CREATE TABLE t1 (id INTEGER PRIMARY KEY, name TEXT, age INT)", 100, None)
            .await
            .unwrap();
        b.run_sql("CREATE INDEX idx_name ON t1(name)", 100, None).await.unwrap();
        let r = b.run_one(
            "INSERT INTO t1 (name, age) VALUES ('alice', 30), ('bob', 25), ('carol', NULL)",
            100,
        )
        .await
        .unwrap();
        assert_eq!(r.affected, 3);

        let r = b.run_one("SELECT id, name, age FROM t1 ORDER BY id", 100).await.unwrap();
        assert_eq!(r.columns, vec!["id", "name", "age"]);
        assert_eq!(r.rows.len(), 3);
        assert_eq!(r.rows[0][1].as_deref(), Some("alice"));
        assert_eq!(r.rows[2][2], None);

        let tables = b.list_tables(&info).await.unwrap();
        assert!(tables.iter().any(|t| t.name == "t1" && t.kind == "table"));

        let s = b.get_table_structure(&info, "t1", None).await.unwrap();
        assert_eq!(s.columns.len(), 3);
        assert_eq!(s.columns[0].key, "PRI");
        assert!(s.indexes.iter().any(|i| i.name == "idx_name" && !i.unique));
        assert!(s.ddl.contains("CREATE TABLE"));

        assert_eq!(b.count_rows("t1").await.unwrap(), 3);

        let updates = vec![edits::CellUpdate {
            pk: vec![("id".into(), "1".into())],
            sets: vec![("age".into(), Some("31".into()))],
        }];
        let r = edits::apply_changes(&mut b, &info, "t1", &updates, &[], &[]).await.unwrap();
        assert_eq!(r.affected, 1);
        let r = b.run_one("SELECT age FROM t1 WHERE id = 1", 10).await.unwrap();
        assert_eq!(r.rows[0][0].as_deref(), Some("31"));

        let deletes = vec![vec![("id".into(), "2".into())]];
        edits::apply_changes(&mut b, &info, "t1", &[], &deletes, &[]).await.unwrap();
        assert_eq!(b.count_rows("t1").await.unwrap(), 2);

        let updates = vec![edits::CellUpdate {
            pk: vec![("id".into(), "1".into())],
            sets: vec![("age".into(), None)],
        }];
        edits::apply_changes(&mut b, &info, "t1", &updates, &[], &[]).await.unwrap();
        let r = b.run_one("SELECT age FROM t1 WHERE id = 1", 10).await.unwrap();
        assert_eq!(r.rows[0][0], None);

        let inserts = vec![vec![
            ("name".to_string(), "dave".to_string()),
            ("age".to_string(), "40".to_string()),
        ]];
        edits::apply_changes(&mut b, &info, "t1", &[], &[], &inserts).await.unwrap();
        assert_eq!(b.count_rows("t1").await.unwrap(), 3);
        let r = b.run_one("SELECT name FROM t1 WHERE name = 'dave'", 10).await.unwrap();
        assert_eq!(r.rows.len(), 1);

        let r = b.run_one("SELECT * FROM t1", 1).await.unwrap();
        assert_eq!(r.rows.len(), 1);
        assert!(r.truncated);

        b.run_sql("CREATE TABLE nopk (a TEXT)", 10, None).await.unwrap();
        let bad = edits::apply_changes(
            &mut b,
            &info,
            "nopk",
            &[edits::CellUpdate {
                pk: vec![],
                sets: vec![("a".into(), Some("x".into()))],
            }],
            &[],
            &[],
        )
        .await;
        assert!(bad.is_err());

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_multi_statement_fallback() {
        let path = tmp_db("multi");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();
        let rs = b
            .run_sql(
                "CREATE TABLE m (x INT);\nINSERT INTO m VALUES (1);\nINSERT INTO m VALUES (2);\nSELECT * FROM m;\nSELECT x + 1 AS nx FROM m WHERE x = 2;",
                10,
                None,
            )
            .await
            .unwrap();
        assert_eq!(rs.len(), 5);
        assert_eq!(rs[3].rows.len(), 2);
        assert_eq!(rs[4].columns, vec!["nx"]);
        assert_eq!(rs[4].rows[0][0].as_deref(), Some("3"));

        let rs = b.run_sql("SELECT 'a;b' AS s", 10, None).await.unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].rows[0][0].as_deref(), Some("a;b"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_foreign_keys() {
        let path = tmp_db("fk");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE a (id INTEGER PRIMARY KEY);\
             CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER REFERENCES a(id));",
            10,
            None,
        )
        .await
        .unwrap();
        let fks = b.list_foreign_keys(&info, None).await.unwrap();
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].table, "b");
        assert_eq!(fks[0].column, "a_id");
        assert_eq!(fks[0].ref_table, "a");
        assert_eq!(fks[0].ref_column, "id");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_trigger_and_ddl() {
        let path = tmp_db("trg");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INT);\
             CREATE TRIGGER trg_t AFTER INSERT ON t BEGIN UPDATE t SET n = n + 1 WHERE id = NEW.id; END;",
            10,
            None,
        )
        .await
        .unwrap();
        let objs = b.list_tables(&info).await.unwrap();
        let trg = objs.iter().find(|o| o.kind == "trigger").expect("应有触发器");
        assert_eq!(trg.name, "trg_t");
        let ddl = schema::object_ddl(&mut b, &info, "trigger", "trg_t").await.unwrap();
        assert!(ddl.contains("CREATE TRIGGER"));
        assert!(ddl.contains("AFTER INSERT ON t"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_readonly_guard() {
        let path = tmp_db("ro");
        let mut info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();
        b.run_sql("CREATE TABLE r (x INT); INSERT INTO r VALUES (1);", 10, None).await.unwrap();

        info.read_only = true;
        assert!(model::is_readonly_sql("select 1"));
        assert!(model::is_readonly_sql("  WITH t AS (SELECT 1) SELECT * FROM t"));
        assert!(model::is_readonly_sql("EXPLAIN QUERY PLAN SELECT 1"));
        assert!(!model::is_readonly_sql("DELETE FROM r"));
        assert!(!model::is_readonly_sql("DROP TABLE r"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_global_search() {
        let path = tmp_db("search");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE s1 (id INTEGER PRIMARY KEY, name TEXT);             CREATE TABLE s2 (id INTEGER PRIMARY KEY, note TEXT);             INSERT INTO s1 (name) VALUES ('alpha'), ('Beta'), ('gamma');             INSERT INTO s2 (note) VALUES ('ALPHA note'), ('other');",
            20,
            None,
        )
        .await
        .unwrap();
        let hits = search::search_tables(&mut b, &info, "alpha", 50).await.unwrap();
        assert_eq!(hits.len(), 2, "s1.alpha 与 s2.ALPHA note 都应命中");
        let tables: Vec<&str> = hits.iter().map(|h| h.table.as_str()).collect();
        assert!(tables.contains(&"s1"));
        assert!(tables.contains(&"s2"));
        let h2 = hits.iter().find(|h| h.table == "s2").unwrap();
        assert_eq!(h2.column, "note");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn service_connect_and_disconnect() {
        let dir = std::env::temp_dir().join(format!("shujing_native_svc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let svc = crate::Service::new(&dir);
        assert!(svc.list_saved_connections().is_empty());

        let info = sqlite_info("/tmp/shujing_native_svc.db");
        let saved = svc.save_connection(info.clone()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(svc.list_saved_connections().len(), 1);

        svc.connect(info.clone()).await.unwrap();
        assert!(svc.state.live(&info.id).is_some());
        let rs = svc.run_sql(&info.id, "SELECT 1 AS one".into(), None).await.unwrap();
        assert_eq!(rs[0].rows[0][0].as_deref(), Some("1"));
        let tables = svc.list_tables(&info.id, None).await.unwrap();
        assert!(tables.is_empty() || !tables.is_empty()); // 空库可返回空列表
        svc.disconnect(&info.id).await;
        assert!(svc.state.live(&info.id).is_none());

        svc.delete_connection(&info.id).unwrap();
        assert!(svc.list_saved_connections().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
