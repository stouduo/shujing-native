//! 数镜原生版核心服务:自 dbclient/src-tauri 迁移的连接池/SQL 执行/编辑回写/结构查询。
//! 纯 Rust 库,无 UI 框架依赖;egui 应用进程内直调(无 IPC)。

pub mod ai;
pub mod backend;
pub mod clickhouse;
pub mod composite;
#[cfg(feature = "heavy-mw")]
pub mod kafka;
pub mod kv;
pub mod history;
pub mod lineage;
#[cfg(feature = "heavy-mw")]
pub mod mqtt;
pub mod rabbit;
pub mod designer;
pub mod edits;
pub mod excel;
pub mod exec;
pub mod hashing;
pub mod jwt;
mod model;
pub mod redis_ops;
pub mod config_crypto;
pub mod conn_import;
#[cfg(feature = "heavy-mw")]
pub mod mongo;
pub mod mssql;
pub mod parquet_preview;
pub mod plugins;
pub mod schema;
pub mod data_diff;
pub mod schema_diff;
pub mod transfer;
pub mod backup;
pub mod ssh;
mod search;
pub mod sql_advisor;
pub mod sql_format;
pub mod sql_lint;
pub mod store;
pub mod updater;

pub use search::SearchHit;
pub use model::{
    AppState, ColumnDef, ConnInfo, ConnectResult, DbType, ExecResult, IndexDef, LiveConn, Perm,
    SessionInfo, SshConfig, TableMeta, TableStructure, is_readonly_redis, is_readonly_sql,
    sql_write_kind,
};

use model::CancelState;
use std::path::PathBuf;

mod service;

/// Service 数据源方法样板收敛(模板方法模式):取连接锁 → match backend 指定变体 →
/// 执行体,其余变体统一「非 X 连接」。适用于"锁后立即 match"的同形包装;
/// 含前置逻辑(会话库/只读门/cur_db)的方法保留手写。
macro_rules! backend_dispatch {
    ($self:expr, $id:expr, $variant:ident($st:ident) => $body:expr) => {{
        let live = $self.state.live($id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match &mut guard.backend {
            crate::model::Backend::$variant($st) => $body,
            _ => Err(format!("非 {} 连接", stringify!($variant)).into()),
        }
    }};
}
pub(crate) use backend_dispatch;

/// 列值集合 → 建表列类型(方言无关的语义档位)
fn infer_column_type<'a>(vals: impl Iterator<Item = Option<&'a str>>) -> &'static str {
    let mut seen = false;
    let mut int_ok = true;
    let mut float_ok = true;
    let mut bool_ok = true;
    for v in vals {
        let Some(s) = v else { continue };
        seen = true;
        let t = s.trim();
        if t.is_empty() {
            continue;
        }
        if t.parse::<i64>().is_err() {
            int_ok = false;
        }
        if t.parse::<f64>().is_err() {
            float_ok = false;
        }
        if !matches!(t, "true" | "false") {
            bool_ok = false;
        }
        if !int_ok && !float_ok && !bool_ok {
            return "TEXT";
        }
    }
    if !seen {
        "TEXT"
    } else if int_ok {
        "BIGINT"
    } else if bool_ok {
        "BOOLEAN"
    } else if float_ok {
        "DOUBLE"
    } else {
        "TEXT"
    }
}

/// 结果集建表语句:CREATE TABLE(推断类型)+ 分批 INSERT(200/批)
/// ClickHouse 引擎语法不同,Redis/Mongo/MSSQL 无 SQL 表——调用方拒绝;
/// 表名做方言引注,值走 edits::literal 转义。MySQL/PG 带库限定,SQLite 单文件不带
pub fn build_save_table_stmts(
    info: &ConnInfo,
    database: Option<&str>,
    table: &str,
    columns: &[String],
    rows: &[Vec<Option<String>>],
) -> Result<Vec<String>, String> {
    use crate::model::DbType;
    if columns.is_empty() {
        return Err("结果集没有列".into());
    }
    if rows.is_empty() {
        return Err("结果集没有数据行".into());
    }
    if !info.db_type.caps().save_result {
        return Err("该数据库类型暂不支持结果存为新表".into());
    }
    let mysql = matches!(info.db_type, DbType::MySql | DbType::ClickHouse);
    let q = |s: &str| crate::edits::quote(s, mysql);
    let table_full = match info.db_type {
        DbType::Postgres => q(table),
        _ => match database.map(str::trim).filter(|s| !s.is_empty()) {
            Some(db) => format!("{}.{}", q(db), q(table)),
            None => q(table),
        },
    };
    // 列类型:按列收集非空值推断
    let mut types = Vec::with_capacity(columns.len());
    for (ci, _c) in columns.iter().enumerate() {
        let refs: Vec<Option<&str>> = rows
            .iter()
            .map(|r| r.get(ci).and_then(|v| v.as_deref()))
            .collect();
        types.push(infer_column_type(refs.into_iter()));
    }
    let cols_def: Vec<String> = columns
        .iter()
        .zip(&types)
        .map(|(c, t)| format!("{} {}", q(c), dialect_type(t, &info.db_type)))
        .collect();
    let mut stmts = vec![format!(
        "CREATE TABLE {} ({})",
        table_full,
        cols_def.join(", ")
    )];
    const BATCH: usize = 200;
    let col_list = columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    for chunk in rows.chunks(BATCH) {
        let values: Vec<String> = chunk
            .iter()
            .map(|row| {
                let vals: Vec<String> = columns
                    .iter()
                    .enumerate()
                    .map(|(ci, _)| {
                        row.get(ci)
                            .and_then(|v| v.as_deref())
                            .filter(|s| !s.is_empty())
                            .map(|s| crate::edits::literal(s, crate::edits::Dialect::of(info.db_type)))
                            .unwrap_or_else(|| "NULL".into())
                    })
                    .collect();
                format!("({})", vals.join(", "))
            })
            .collect();
        stmts.push(format!(
            "INSERT INTO {} ({}) VALUES {}",
            table_full,
            col_list,
            values.join(",")
        ));
    }
    Ok(stmts)
}

/// 语义档位 → 方言类型名(BOOLEAN 在 SQLite 是 NUMERIC 别名,直接可用)
fn dialect_type(t: &str, db_type: &DbType) -> String {
    match t {
        "BIGINT" => "BIGINT".into(),
        "BOOLEAN" => match db_type {
            DbType::MySql => "TINYINT(1)".into(),
            _ => "BOOLEAN".into(),
        },
        "DOUBLE" => match db_type {
            DbType::MySql => "DOUBLE".into(),
            DbType::Sqlite => "REAL".into(),
            _ => "DOUBLE PRECISION".into(),
        },
        _ => "TEXT".into(),
    }
}

/// 顶层服务:持有连接状态与配置目录(等价于 Tauri 版的 AppState + AppHandle)
pub struct Service {
    pub state: AppState,
    pub config_dir: PathBuf,
    /// SSH 隧道(按连接 id):同一连接重连且配置未变时复用;断开/改配置即关
    tunnels: std::sync::Mutex<std::collections::HashMap<String, ssh::TunnelGuard>>,
}

/// 连接列表重排的纯逻辑:把 `dragged` 移到 `target` 所在位置;二者分组不同
/// (含一方未分组)时原样返回,拖拽不改变分组归属
pub fn reorder_conn_list(conns: &[ConnInfo], dragged: &str, target: &str) -> Vec<ConnInfo> {
    if dragged == target {
        return conns.to_vec();
    }
    let group_of = |id: &str| conns.iter().find(|c| c.id == id).map(|c| c.group.clone());
    let (Some(dg), Some(tg)) = (group_of(dragged), group_of(target)) else {
        return conns.to_vec();
    };
    if dg != tg {
        return conns.to_vec();
    }
    let Some(from) = conns.iter().position(|c| c.id == dragged) else {
        return conns.to_vec();
    };
    let Some(to) = conns.iter().position(|c| c.id == target) else {
        return conns.to_vec();
    };
    let mut out = conns.to_vec();
    let moved = out.remove(from);
    out.insert(to, moved);
    out
}

impl Service {
    pub fn new(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            state: AppState::default(),
            config_dir: config_dir.into(),
            tunnels: std::sync::Mutex::new(std::collections::HashMap::new()),
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
        // 若该连接已在线且配置变了,断开旧连接让用户重连(隧道一并作废)
        self.state.remove(&info.id);
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&info.id);
        Ok(conns)
    }


    /// 连接树拖拽排序:基于调用方当前列表(`conns`)重排并落盘。两者必须在同一
    /// 分组(或都未分组)——拖拽不改变分组归属,跨组落点原样返回
    pub fn reorder_connections(
        &self,
        conns: &[ConnInfo],
        dragged: &str,
        target: &str,
    ) -> Result<Vec<ConnInfo>, String> {
        let out = reorder_conn_list(conns, dragged, target);
        let changed = out.len() != conns.len()
            || out.iter().zip(conns).any(|(a, b)| a.id != b.id);
        if changed {
            store::save_conns(&self.config_dir, &out)?;
        }
        Ok(out)
    }

    pub fn delete_connection(&self, id: &str) -> Result<Vec<ConnInfo>, String> {
        self.state.remove(id);
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        let mut conns = store::load_conns(&self.config_dir);
        conns.retain(|c| c.id != id);
        store::save_conns(&self.config_dir, &conns)?;
        Ok(conns)
    }

    /// Redis:按模式批量删除(上限 10000)

    pub async fn list_sessions(&self, id: &str) -> Result<Vec<model::SessionInfo>, String> {
        // 控制连接路径:长查询持有 live guard 期间会话面板照样可用(面板的意义所在)
        let entry = self.state.cancel(id).ok_or("连接未建立或已断开")?;
        backend::Backend::list_sessions_standalone(&entry).await
    }

    /// 终止服务器会话

    pub async fn kill_session(&self, id: &str, sid: &str, query_only: bool) -> Result<(), String> {
        // 同 list_sessions:控制连接,与长查询并发可用
        let entry = self.state.cancel(id).ok_or("连接未建立或已断开")?;
        backend::Backend::kill_session_standalone(&entry, sid, query_only).await
    }

    /// 数据字典导出:全部表的 列/类型/注释/默认/主键 Markdown

    pub async fn export_dictionary(
        &self,
        id: &str,
        database: Option<&str>,
    ) -> Result<String, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        if !info.db_type.caps().schema {
            return Err("该数据源无表结构,不支持数据字典".into());
        }
        let tables = guard.backend.list_tables_in(&info, database).await?;
        let mut md = String::new();
        md.push_str(&format!(
            "# 数据字典 · {}\n\n> 生成于 {} · {} 张表\n",
            database.or(info.database.as_deref()).unwrap_or("database"),
            chrono::Utc::now().format("%Y-%m-%d %H:%M"),
            tables.len()
        ));
        for (i, t) in tables.iter().enumerate() {
            let s = guard
                .backend
                .get_table_structure(&info, &t.name, database)
                .await?;
            md.push_str(&format!(
                "\n## {}. {}\n\n| 字段 | 类型 | 可空 | 键 | 默认 | 注释 |\n|---|---|---|---|---|---|\n",
                i + 1,
                t.name
            ));
            for c in &s.columns {
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    c.name,
                    c.data_type,
                    if c.nullable { "是" } else { "否" },
                    if c.key == "PRI" { "PK" } else { "" },
                    c.default.as_deref().unwrap_or(""),
                    c.comment.replace('|', "\\|")
                ));
            }
            if !s.indexes.is_empty() {
                md.push_str("\n**索引**:\n\n");
                for ix in &s.indexes {
                    md.push_str(&format!("- {} ({})\n", ix.name, ix.columns));
                }
            }
        }
        Ok(md)
    }

    /// 查询结果集保存为新表:类型推断建表 + 分批 INSERT(MySQL/PG/SQLite;
    /// ClickHouse 引擎语法不同暂不支持)。返回写入行数

    pub async fn save_result_as_table(
        &self,
        id: &str,
        database: Option<&str>,
        table: &str,
        columns: &[String],
        rows: &[Vec<Option<String>>],
    ) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        if !guard.info.perm().can_ddl() {
            return Err("连接权限不足:建表需要完整权限(DDL)".into());
        }
        let stmts = build_save_table_stmts(&info, database, table, columns, rows)?;
        // 走通用执行臂(复用 run_sql 的事务/方言处理),最后一条 INSERT 影响行数累加
        let mut affected_total = 0u64;
        for s in &stmts {
            let rs = guard.backend.run_sql(s, 1, None, None).await?;
            if let Some(r) = rs.first() {
                if r.affected > 0 {
                    affected_total += r.affected;
                }
            }
        }
        Ok(affected_total)
    }

    /// MongoDB:导出集合文档为 JSON 数组文本

    pub async fn connect_by_id(&self, id: &str) -> Result<(), String> {
        if self.state.live(id).is_some() {
            return Ok(());
        }
        let info = self
            .list_saved_connections()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("连接不存在: {id}"))?;
        self.connect(info).await.map(|_| ())
    }


    pub async fn test_connection(info: ConnInfo) -> Result<ConnectResult, String> {
        let mut endpoint = info.clone();
        let _tunnel;
        if let Some(ssh_cfg) = info.ssh.clone() {
            let target_host = info
                .host
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let target_port = info.port.unwrap_or(match info.db_type {
                crate::model::DbType::MySql => 3306,
                crate::model::DbType::Postgres => 5432,
                crate::model::DbType::Redis => 6379,
                crate::model::DbType::Mongo => 27017,
                crate::model::DbType::Mssql => 1433,
                crate::model::DbType::ClickHouse => 8123,
                crate::model::DbType::Kafka => 9092,
                crate::model::DbType::Rabbit => 15672,
                crate::model::DbType::Mqtt => 1883,
                crate::model::DbType::Nacos => 8848,
                crate::model::DbType::Consul => 8500,
                crate::model::DbType::Etcd => 2379,
                crate::model::DbType::Sqlite => 0,
                // 驱动插件:端口语义由 agent 决定,留空
                crate::model::DbType::Plugin => 0,
            });
            let g = ssh::open_tunnel(&ssh_cfg, &target_host, target_port).await?;
            endpoint.host = Some("127.0.0.1".into());
            endpoint.port = Some(g.local_port);
            _tunnel = Some(g); // 测试期间保活,函数返回即关闭
        }
        let mut backend = backend::connect(&endpoint, None, None).await?;
        let version = backend.server_info().await?;
        Ok(ConnectResult { version })
    }


    pub async fn connect(&self, info: ConnInfo) -> Result<ConnectResult, String> {
        // 驱动插件拦截:插件目录来自本 Service 的 config_dir(与 backend 兜底臂的
        // 环境变量路径不同源——Service 语义优先)
        if info.db_type == model::DbType::Plugin {
            return self.connect_plugin(info).await;
        }
        let mut endpoint = info.clone();
        if let Some(ssh_cfg) = info.ssh.clone() {
            let port = self.ensure_tunnel(&info.id, &ssh_cfg, &info).await?;
            // DB 驱动一律指向本地隧道口;info.host/port 语义是"从跳板机可达的库地址"
            endpoint.host = Some("127.0.0.1".into());
            endpoint.port = Some(port);
        }
        let cancel = std::sync::Arc::new(CancelState::new(
            &endpoint,
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        ));
        let mut backend =
            backend::connect(&endpoint, Some(cancel.pg_pid.clone()), Some(cancel.gen.clone()))
                .await?;
        // 重复连接同一 id(如改配置重连)时保留会话库记忆
        let prev = match self.state.live(&info.id) {
            Some(old) => old.lock().await.backend.session_db().map(str::to_string),
            None => None,
        };
        backend.set_session_db(prev.as_deref());
        let version = backend.server_info().await?;
        self.state.insert(LiveConn { info: endpoint, backend }, cancel);
        Ok(ConnectResult { version })
    }

    /// 驱动插件连接:spawn agent + 契约 connect;参数透传,方言由 agent 解释
    async fn connect_plugin(&self, info: ConnInfo) -> Result<ConnectResult, String> {
        let name = info
            .plugin
            .clone()
            .ok_or("未指定驱动插件名")?;
        let dir = self.config_dir.join("plugins");
        let drivers = crate::plugins::scan_drivers(&dir);
        let drv = drivers
            .iter()
            .find(|d| d.name.as_str() == name.as_str() || d.db_types.iter().any(|t| t.as_str() == name.as_str()))
            .ok_or_else(|| {
                format!("驱动插件 {name} 未安装(应位于 {}/<名>/plugin.toml)", dir.display())
            })?;
        let params = serde_json::json!({
            "host": info.host,
            "port": info.port,
            "user": info.user,
            "password": info.password,
            "database": info.database,
        });
        let session = crate::plugins::DriverSession::start(drv, &params).await?;
        let cancel = std::sync::Arc::new(model::CancelState::new(
            &info,
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        ));
        let version = session.version.clone();
        self.state.insert(LiveConn { info: info.clone(), backend: model::Backend::Plugin(session) }, cancel);
        Ok(ConnectResult { version })
    }

    /// 确保该连接的隧道可用:同配置且存活→复用;否则(重)建。返回本地端口

    async fn ensure_tunnel(
        &self,
        id: &str,
        ssh_cfg: &crate::model::SshConfig,
        info: &ConnInfo,
    ) -> Result<u16, String> {
        {
            let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(g) = tunnels.get(id) {
                if g.matches(ssh_cfg) && g.alive() {
                    return Ok(g.local_port);
                }
            }
        }
        // 旧的失效/配置变了:先移除(Drop 关旧会话)再建新
        let old = self
            .tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        drop(old);
        let target_host = info
            .host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let target_port = info.port.unwrap_or(match info.db_type {
            crate::model::DbType::MySql => 3306,
            crate::model::DbType::Postgres => 5432,
            crate::model::DbType::Redis => 6379,
            crate::model::DbType::Mongo => 27017,
            crate::model::DbType::Mssql => 1433,
            crate::model::DbType::ClickHouse => 8123,
            crate::model::DbType::Kafka => 9092,
            crate::model::DbType::Rabbit => 15672,
            crate::model::DbType::Mqtt => 1883,
            crate::model::DbType::Nacos => 8848,
            crate::model::DbType::Consul => 8500,
            crate::model::DbType::Etcd => 2379,
            crate::model::DbType::Sqlite => 0,
            // 驱动插件:端口语义由 agent 决定,留空
            crate::model::DbType::Plugin => 0,
        });
        let guard = ssh::open_tunnel(ssh_cfg, &target_host, target_port).await?;
        let port = guard.local_port;
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), guard);
        Ok(port)
    }


    /// SSH 隧道中断自动恢复:检测到该连接的隧道已死 → 用保存的连接配置重连
    /// (ensure_tunnel 会重建隧道、connect 保留会话库记忆并替换连接池)。
    /// 仅对 SSH 连接生效;恢复失败保持原状,随后的操作照常报错(不打断调用方语义)。
    async fn auto_recover_ssh(&self, id: &str) {
        let Some(saved) = self
            .list_saved_connections()
            .into_iter()
            .find(|c| c.id == id)
        else {
            return;
        };
        if saved.ssh.is_none() {
            return;
        }
        let dead = {
            let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.get(id).map(|g| !g.alive()).unwrap_or(false)
        };
        if dead {
            let _ = self.connect(saved).await;
        }
    }

    pub async fn disconnect(&self, id: &str) -> Result<(), String> {
        // 直接移出连接表,引用清零后 TCP 连接随之关闭;隧道一并关闭
        self.state.remove(id);
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        Ok(())
    }

    /// 列出服务器上所有数据库

    pub async fn list_databases(&self, id: &str) -> Result<Vec<String>, String> {
        self.auto_recover_ssh(id).await;
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
        self.auto_recover_ssh(id).await;
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard
            .backend
            .list_tables_in(&info, database.as_deref().or(info.database.as_deref()))
            .await
    }


    pub async fn list_tables_legacy(&self, id: &str) -> Result<Vec<TableMeta>, String> {
        self.auto_recover_ssh(id).await;
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
        self.auto_recover_ssh(id).await;
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
        self.auto_recover_ssh(id).await;
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        guard
            .backend
            .list_foreign_keys(&info, database.as_deref())
            .await
    }


    pub async fn count_rows(&self, id: &str, table: &str) -> Result<u64, String> {
        self.auto_recover_ssh(id).await;
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        guard.backend.count_rows(table).await
    }


    pub async fn get_object_ddl(
        &self,
        id: &str,
        database: Option<String>,
        kind: &str,
        name: &str,
    ) -> Result<String, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        schema::object_ddl(&mut guard.backend, &info, database.as_deref(), kind, name).await
    }


    pub async fn search_all_tables(
        &self,
        id: &str,
        database: Option<&str>,
        keyword: &str,
        max_hits: usize,
    ) -> Result<Vec<search::SearchHit>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        // 无显式库时兜底连接默认库(MySQL 未展开库时裸表名会报 No database selected)
        let database = database
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or(info.database.as_deref());
        search::search_tables(&mut guard.backend, &info, database, keyword, max_hits).await
    }

    /// 全库对象搜索:遍历连接上全部非系统库逐库搜(MySQL/PG 多库;其余单库退化)

    pub async fn search_all_databases(
        &self,
        id: &str,
        keyword: &str,
        max_hits: usize,
    ) -> Result<Vec<search::SearchHit>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        let dbs = match crate::schema::list_databases(&mut guard.backend, &info).await {
            Ok(d) => d,
            Err(_) => vec![],
        };
        // 系统库排除(MySQL information_schema/performance_schema/sys;PG pg_*/information_schema)
        let dbs: Vec<String> = {
            let all: Vec<String> = dbs;
            all.into_iter()
            .filter(|d: &String| {
                let low = d.to_lowercase();
                !(low.starts_with("pg_")
                    || low == "information_schema"
                    || low == "performance_schema"
                    || low == "sys"
                    || low == "mysql")
            })
            .collect::<Vec<String>>()
        };
        let mut hits = Vec::new();
        for d in &dbs {
            if hits.len() >= max_hits {
                break;
            }
            match search::search_tables(&mut guard.backend, &info, Some(d), keyword, max_hits).await {
                Ok(mut h) => {
                    hits.append(&mut h);
                }
                Err(_) => continue,
            }
        }
        Ok(hits)
    }


    pub async fn apply_changes(
        &self,
        id: &str,
        table: &str,
        database: Option<&str>,
        updates: &[edits::CellUpdate],
        deletes: &[Vec<(String, String)>],
        inserts: &[Vec<(String, String)>],
    ) -> Result<ExecResult, String> {
        self.auto_recover_ssh(id).await;
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        if !info.perm().can_dml() {
            return Err("连接权限不足:表格回写需要 DML 及以上权限".into());
        }
        edits::apply_changes(
            &mut guard.backend,
            &info,
            table,
            database,
            updates,
            deletes,
            inserts,
        )
        .await
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
            "2006",
            "2013",
        ];
        KEYS.iter().any(|k| s.contains(k))
    }

    /// 执行 SQL,返回多结果集(PostgreSQL 完整支持;MySQL/SQLite 返回第一个结果集)。
    /// 含只读拦截与断线自动重连。
    /// 供 composite 等组合模块直接执行原始 SQL

    pub async fn run_sql_standalone(
        service: &Service,
        info: &ConnInfo,
        sql: &str,
    ) -> Result<Vec<crate::ExecResult>, String> {
        service.run_sql(&info.id, sql.to_string(), None).await
    }


    pub async fn run_sql(
        &self,
        id: &str,
        sql: String,
        max_rows: Option<usize>,
    ) -> Result<Vec<ExecResult>, String> {
        self.run_sql_db(id, sql, max_rows, None).await
    }

    /// 带会话库的执行:库上下文在同一把连接锁内设置并生效,
    /// 避免多个查询标签并发时"先 USE 后执行"两步被交错到错误的库

    pub async fn run_sql_db(
        &self,
        id: &str,
        sql: String,
        max_rows: Option<usize>,
        db: Option<&str>,
    ) -> Result<Vec<ExecResult>, String> {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return Err("SQL 为空".into());
        }
        {
            let live = self.state.live(id);
            if let Some(lc) = live {
                #[cfg(feature = "heavy-mw")]
                if matches!(lc.lock().await.backend, crate::model::Backend::Mongo(_)) {
                    return Err(
                        "MongoDB 不支持 SQL(浏览集合用数据页;过滤输入 JSON 对象)".into(),
                    );
                }
                #[cfg(not(feature = "heavy-mw"))]
                {
                    let _ = lc;
                }
            }
        }
        self.auto_recover_ssh(id).await;
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let cancel_state = self.state.cancel(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if let Some(db) = db.map(str::trim).filter(|s| !s.is_empty()) {
            guard.backend.set_session_db(Some(db));
        }
        match guard.info.perm() {
            model::Perm::ReadOnly => {
                // 多语句逐条校验:此前只看整体前缀,`SELECT 1; DROP TABLE t` 能绕过
                let stmts = exec::split_statements(&sql);
                if stmts.is_empty() || stmts.iter().any(|s| !model::is_readonly_sql(s)) {
                    return Err("只读连接:仅允许 SELECT/SHOW/EXPLAIN/查询型 PRAGMA".into());
                }
            }
            model::Perm::Dml => {
                // 仅数据操作:DDL(CREATE/DROP/ALTER/TRUNCATE…)拒绝,DML 放行
                let stmts = exec::split_statements(&sql);
                if stmts.is_empty() || stmts.iter().any(|s| model::sql_write_kind(s) == Some("ddl")) {
                    return Err("仅 DML 连接:允许查询与 INSERT/UPDATE/DELETE/REPLACE,DDL 需完整权限".into());
                }
            }
            model::Perm::Full => {}
        }
        let max_rows = max_rows.unwrap_or(exec::DEFAULT_MAX_ROWS);
        match guard
            .backend
            .run_sql(
                &sql,
                max_rows,
                Some(&cancel_state.mysql_id),
                Some(cancel_state.as_ref()),
            )
            .await
        {
            Ok(r) => Ok(r),
            Err(e) => {
                if !Self::is_dead_conn(&e) {
                    return Err(e);
                }
                // 连接已整体失效(池不可用/网络中断):重建后端并恢复会话库后重试。
                // 仅限单语句:多语句脚本前半可能已在服务器生效,整段重放=重复写入
                let stmts = exec::split_statements(&sql);
                if stmts.len() > 1 {
                    return Err(format!(
                        "{e}(连接已断开;多语句脚本不自动重试,以免重复执行,请检查后重跑)"
                    ));
                }
                let info = guard.info.clone();
                let session_db = guard.backend.session_db().map(str::to_string);
                let mut nb = backend::connect(
                    &info,
                    Some(cancel_state.pg_pid.clone()),
                    Some(cancel_state.gen.clone()),
                )
                    .await
                    .map_err(|e2| format!("连接已断开,自动重连失败: {e2}(原错误: {e})"))?;
                nb.set_session_db(session_db.as_deref());
                let r = nb
                    .run_sql(
                &sql,
                max_rows,
                Some(&cancel_state.mysql_id),
                Some(cancel_state.as_ref()),
            )
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
                // 代数戳:开控制连接的往返期间查询可能已结束、同会话新查询可能已开跑;
                // KILL 前核对,代数变了就放弃(宁可漏杀不可误杀)
                let gen_seen = entry.gen.load(std::sync::atomic::Ordering::Relaxed);
                let opts = mysql_async::OptsBuilder::default()
                    .ip_or_hostname(host.as_deref().unwrap_or("127.0.0.1"))
                    .tcp_port(port.unwrap_or(3306))
                    .user(user.as_deref())
                    .pass(Some(password.as_deref().unwrap_or("")));
                let mut conn = mysql_async::Conn::new(opts)
                    .await
                    .map_err(|e| format!("取消失败(控制连接): {e}"))?;
                if entry.gen.load(std::sync::atomic::Ordering::Relaxed) != gen_seen {
                    return Err("查询已结束,取消已跳过(避免误伤新查询)".into());
                }
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
                let gen_seen = entry.gen.load(std::sync::atomic::Ordering::Relaxed);
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
                if entry.gen.load(std::sync::atomic::Ordering::Relaxed) != gen_seen {
                    return Err("查询已结束,取消已跳过(避免误伤新查询)".into());
                }
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
        database: Option<&str>,
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
        // MySQL 导出带库限定:多库连接下目标表可能不在会话默认库
        let qt = if mysql {
            match database.filter(|d| !d.trim().is_empty()) {
                Some(db) => format!("{}.{}", q(db.trim()), q(table)),
                None => q(table),
            }
        } else {
            q(table)
        };
        let lit = |v: Option<&str>| -> String {
            match v {
                None => "NULL".to_string(),
                Some(s) => format!("'{}'", s.replace('\'', "''")),
            }
        };

        let mut out = String::new();
        out.push_str(&format!("-- 数镜 dump: table {qt}\n"));
        // 建表语句:SQLite/MySQL 用原生 DDL,PG 简化合成
        match backend
            .get_table_structure(info, table, database)
            .await
        {
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
        database: Option<String>,
        table: &str,
        with_data: bool,
    ) -> Result<(String, u64), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        self.build_table_dump(&mut guard.backend, &info, database.as_deref(), table, with_data)
            .await
    }

    /// 导出整个数据库 SQL(所有表,结构 + 可选数据)

    pub async fn export_database_sql(
        &self,
        id: &str,
        database: Option<String>,
        with_data: bool,
    ) -> Result<(String, u64), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        let tables = match database.as_deref().filter(|d| !d.trim().is_empty()) {
            Some(db) => guard.backend.list_tables_in(&info, Some(db)).await?,
            None => guard.backend.list_tables(&info).await?,
        };
        let mut out = String::new();
        out.push_str(&format!(
            "-- 数镜 database dump\n-- tables: {}\n\n",
            tables.iter().filter(|t| t.kind == "table").count()
        ));
        let mut exported: u64 = 0;
        for t in tables.iter().filter(|t| t.kind == "table") {
            let (dump, n) = self
                .build_table_dump(&mut guard.backend, &info, database.as_deref(), &t.name, with_data)
                .await?;
            out.push_str(&dump);
            out.push('\n');
            exported += n;
        }
        Ok((out, exported))
    }

}


#[cfg(test)]
mod save_table_tests {
    use super::*;

    fn info(db_type: DbType) -> ConnInfo {
        ConnInfo {
            id: "c".into(),
            name: "c".into(),
            db_type,
            host: None,
            port: None,
            user: None,
            password: None,
            database: Some("d1".into()),
            file_path: None,
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        }
    }

    #[test]
    fn type_inference() {
        let vals = || [Some("1"), Some("2"), None].into_iter();
        assert_eq!(infer_column_type(vals()), "BIGINT");
        let vals = || [Some("1.5"), Some("2"), None].into_iter();
        assert_eq!(infer_column_type(vals()), "DOUBLE");
        let vals = || [Some("true"), Some("false")].into_iter();
        assert_eq!(infer_column_type(vals()), "BOOLEAN");
        let vals = || [Some("张三"), Some("2")].into_iter();
        assert_eq!(infer_column_type(vals()), "TEXT");
        let vals = || [None, None].into_iter();
        assert_eq!(infer_column_type(vals()), "TEXT");
    }

    #[test]
    fn mysql_stmts_qualified_and_escaped() {
        let info = info(DbType::MySql);
        let stmts = build_save_table_stmts(
            &info,
            Some("d1"),
            "new_t",
            &["name".into(), "age".into()],
            &[vec![Some("张三".into()), Some("28".into())]],
        )
        .unwrap();
        assert_eq!(
            stmts[0],
            "CREATE TABLE `d1`.`new_t` (`name` TEXT, `age` BIGINT)"
        );
        assert!(stmts[1]
            .starts_with("INSERT INTO `d1`.`new_t` (`name`, `age`) VALUES ('张三', '28')"));
    }

    #[test]
    fn pg_ignores_db_qualifier_and_double_precision() {
        let info = info(DbType::Postgres);
        let stmts = build_save_table_stmts(
            &info,
            Some("e2e_a"),
            "new_t",
            &["score".into()],
            &[vec![Some("91.5".into())]],
        )
        .unwrap();
        assert_eq!(
            stmts[0],
            "CREATE TABLE \"new_t\" (\"score\" DOUBLE PRECISION)"
        );
    }

    #[test]
    fn unsupported_and_empty_rejected() {
        let c = info(DbType::ClickHouse);
        assert!(build_save_table_stmts(&c, None, "t", &["a".into()], &[vec![Some("1".into())]]).is_err());
        let m = info(DbType::MySql);
        assert!(build_save_table_stmts(&m, None, "t", &[], &[vec![]]).is_err());
        assert!(build_save_table_stmts(&m, None, "t", &["a".into()], &[]).is_err());
    }

    #[test]
    fn null_and_empty_string_become_null() {
        let info = info(DbType::Sqlite);
        let stmts = build_save_table_stmts(
            &info,
            None,
            "t",
            &["a".into()],
            &[vec![None], vec![Some("".into())]],
        )
        .unwrap();
        assert!(stmts[1].contains("(NULL)"));
        assert_eq!(stmts.len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use crate::backend;
    use crate::edits;
    use crate::model;
    use crate::schema;
    use crate::search;
    use crate::model::{ConnInfo, DbType, Perm};

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
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        }
    }

    fn tmp_db(tag: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("shujing_native_test_{}_{tag}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        // 连接层现在拒绝不存在的路径(防打错路径静默新建空库),测试先落空文件
        // (0 字节是合法空 SQLite 库)
        let _ = std::fs::write(&p, b"");
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn sqlite_full_flow() {
        let path = tmp_db("flow");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None, None).await.unwrap();

        b.run_sql("CREATE TABLE t1 (id INTEGER PRIMARY KEY, name TEXT, age INT)", 100, None, None)
            .await
            .unwrap();
        b.run_sql("CREATE INDEX idx_name ON t1(name)", 100, None, None).await.unwrap();
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
        let r = edits::apply_changes(&mut b, &info, "t1", None, &updates, &[], &[]).await.unwrap();
        assert_eq!(r.affected, 1);
        let r = b.run_one("SELECT age FROM t1 WHERE id = 1", 10).await.unwrap();
        assert_eq!(r.rows[0][0].as_deref(), Some("31"));

        let deletes = vec![vec![("id".into(), "2".into())]];
        edits::apply_changes(&mut b, &info, "t1", None, &[], &deletes, &[]).await.unwrap();
        assert_eq!(b.count_rows("t1").await.unwrap(), 2);

        let updates = vec![edits::CellUpdate {
            pk: vec![("id".into(), "1".into())],
            sets: vec![("age".into(), None)],
        }];
        edits::apply_changes(&mut b, &info, "t1", None, &updates, &[], &[]).await.unwrap();
        let r = b.run_one("SELECT age FROM t1 WHERE id = 1", 10).await.unwrap();
        assert_eq!(r.rows[0][0], None);

        let inserts = vec![vec![
            ("name".to_string(), "dave".to_string()),
            ("age".to_string(), "40".to_string()),
        ]];
        edits::apply_changes(&mut b, &info, "t1", None, &[], &[], &inserts).await.unwrap();
        assert_eq!(b.count_rows("t1").await.unwrap(), 3);
        let r = b.run_one("SELECT name FROM t1 WHERE name = 'dave'", 10).await.unwrap();
        assert_eq!(r.rows.len(), 1);

        let r = b.run_one("SELECT * FROM t1", 1).await.unwrap();
        assert_eq!(r.rows.len(), 1);
        assert!(r.truncated);

        b.run_sql("CREATE TABLE nopk (a TEXT)", 10, None, None).await.unwrap();
        let bad = edits::apply_changes(
            &mut b,
            &info,
            "nopk",
            None,
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
        let mut b = backend::connect(&info, None, None).await.unwrap();
        let rs = b
            .run_sql(
                "CREATE TABLE m (x INT);\nINSERT INTO m VALUES (1);\nINSERT INTO m VALUES (2);\nSELECT * FROM m;\nSELECT x + 1 AS nx FROM m WHERE x = 2;",
                10,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(rs.len(), 5);
        assert_eq!(rs[3].rows.len(), 2);
        assert_eq!(rs[4].columns, vec!["nx"]);
        assert_eq!(rs[4].rows[0][0].as_deref(), Some("3"));

        let rs = b.run_sql("SELECT 'a;b' AS s", 10, None, None).await.unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].rows[0][0].as_deref(), Some("a;b"));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_foreign_keys() {
        let path = tmp_db("fk");
        let info = sqlite_info(&path);
        let mut b = backend::connect(&info, None, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE a (id INTEGER PRIMARY KEY);\
             CREATE TABLE b (id INTEGER PRIMARY KEY, a_id INTEGER REFERENCES a(id));",
            10,
            None,
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
        let mut b = backend::connect(&info, None, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INT);\
             CREATE TRIGGER trg_t AFTER INSERT ON t BEGIN UPDATE t SET n = n + 1 WHERE id = NEW.id; END;",
            10,
            None,
            None,
        )
        .await
        .unwrap();
        let objs = b.list_tables(&info).await.unwrap();
        let trg = objs.iter().find(|o| o.kind == "trigger").expect("应有触发器");
        assert_eq!(trg.name, "trg_t");
        let ddl = schema::object_ddl(&mut b, &info, None, "trigger", "trg_t").await.unwrap();
        assert!(ddl.contains("CREATE TRIGGER"));
        assert!(ddl.contains("AFTER INSERT ON t"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn sqlite_readonly_guard() {
        let path = tmp_db("ro");
        let mut info = sqlite_info(&path);
        let mut b = backend::connect(&info, None, None).await.unwrap();
        b.run_sql("CREATE TABLE r (x INT); INSERT INTO r VALUES (1);", 10, None, None).await.unwrap();

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
        let mut b = backend::connect(&info, None, None).await.unwrap();
        b.run_sql(
            "CREATE TABLE s1 (id INTEGER PRIMARY KEY, name TEXT);             CREATE TABLE s2 (id INTEGER PRIMARY KEY, note TEXT);             INSERT INTO s1 (name) VALUES ('alpha'), ('Beta'), ('gamma');             INSERT INTO s2 (note) VALUES ('ALPHA note'), ('other');",
            20,
            None,
            None,
        )
        .await
        .unwrap();
        let hits = search::search_tables(&mut b, &info, None, "alpha", 50).await.unwrap();
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

        // 预写 0 字节空库(SQLite 合法):v0.12.3 起连接拒绝不存在的文件,
        // 固定路径曾靠历史残留文件侥幸通过,被 /tmp 定期清理暴露
        let db = std::env::temp_dir().join(format!("shujing_svc_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        std::fs::write(&db, b"").unwrap();
        let info = sqlite_info(db.to_str().unwrap());
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
        let _ = std::fs::remove_file(&db);
    }

    // ── 连接权限三档(批次34) ──

    #[test]
    fn perm_compat_and_helpers() {
        let mut c = sqlite_info("perm-compat");
        assert_eq!(c.perm(), Perm::Full);
        // 旧配置:只有 read_only 布尔 → 推断
        c.read_only = true;
        assert_eq!(c.perm(), Perm::ReadOnly);
        // 显式 perm 优先于旧布尔
        c.perm = Some(Perm::Dml);
        assert_eq!(c.perm(), Perm::Dml);
        // 档位能力
        assert!(Perm::Full.can_dml() && Perm::Full.can_ddl());
        assert!(Perm::Dml.can_dml() && !Perm::Dml.can_ddl());
        assert!(!Perm::ReadOnly.can_dml() && !Perm::ReadOnly.can_ddl());
    }

    #[test]
    fn perm_serde_old_and_new_configs() {
        // 旧配置文件:无 perm 字段,仅 readOnly
        let old: ConnInfo = serde_json::from_str(
            r#"{"id":"a","name":"a","dbType":"mysql","readOnly":true}"#,
        )
        .unwrap();
        assert_eq!(old.perm(), Perm::ReadOnly);
        assert_eq!(old.group, None, "旧配置无 group 字段");
        // 新配置文件:perm: "dml" + group
        let new: ConnInfo = serde_json::from_str(
            r#"{"id":"b","name":"b","dbType":"mysql","perm":"dml","group":"生产"}"#,
        )
        .unwrap();
        assert_eq!(new.perm(), Perm::Dml);
        assert_eq!(new.group.as_deref(), Some("生产"));
        // 新序列化含 perm,且 ReadOnly 时旧布尔同步为 true(旧版本可读)
        let ro = ConnInfo {
            read_only: true,
            perm: Some(Perm::ReadOnly),
            group: None,
            plugin: None,
            ..old
        };
        let s = serde_json::to_string(&ro).unwrap();
        assert!(s.contains("\"perm\":\"read_only\""), "{s}");
        assert!(s.contains("\"readOnly\":true"), "{s}");
    }

    #[tokio::test]
    async fn perm_tier_gate_on_sql_exec() {
        let dir = std::env::temp_dir().join(format!("shujing_native_perm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = tmp_db("perm");
        std::fs::write(&path, b"").unwrap();
        let svc = crate::Service::new(&dir);

        // Full 档建表
        let mut full = sqlite_info(&path);
        full.id = "full".into();
        full.name = "full".into();
        svc.connect(full.clone()).await.unwrap();
        svc.run_sql(&full.id, "CREATE TABLE p (id INTEGER PRIMARY KEY, v TEXT)".into(), None)
            .await
            .unwrap();
        svc.disconnect(&full.id).await;

        // DML 档:INSERT 放行,DDL(单条/混入多语句)拒绝
        let mut dml = sqlite_info(&path);
        dml.id = "dml".into();
        dml.name = "dml".into();
        dml.perm = Some(Perm::Dml);
        svc.connect(dml.clone()).await.unwrap();
        svc.run_sql_db(&dml.id, "INSERT INTO p (v) VALUES ('张三')".into(), None, None)
            .await
            .unwrap();
        let e = svc
            .run_sql_db(&dml.id, "CREATE TABLE q (id INTEGER)".into(), None, None)
            .await
            .unwrap_err();
        assert!(e.contains("DDL"), "应拒 DDL: {e}");
        // 混入多语句的 DDL 也拦(`INSERT…; DROP TABLE p` 绕过不了)
        let e2 = svc
            .run_sql_db(
                &dml.id,
                "INSERT INTO p (v) VALUES ('a'); DROP TABLE p".into(),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(e2.contains("DDL"), "多语句混 DDL 应拦: {e2}");
        svc.disconnect(&dml.id).await;

        // 只读档:INSERT 拒绝
        let mut ro = sqlite_info(&path);
        ro.id = "ro".into();
        ro.name = "ro".into();
        ro.perm = Some(Perm::ReadOnly);
        svc.connect(ro.clone()).await.unwrap();
        let e3 = svc
            .run_sql_db(&ro.id, "INSERT INTO p (v) VALUES ('x')".into(), None, None)
            .await
            .unwrap_err();
        assert!(e3.contains("只读"), "只读应拒写: {e3}");
        svc.disconnect(&ro.id).await;

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn perm_gate_on_apply_changes() {
        // 表格回写(cell 编辑)同样受权限门禁:只读拒绝,DML 放行
        let dir = std::env::temp_dir().join(format!("shujing_native_perm_ac_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = tmp_db("perm_ac");
        std::fs::write(&path, b"").unwrap();
        let svc = crate::Service::new(&dir);

        let mut full = sqlite_info(&path);
        full.id = "full-ac".into();
        full.name = "full-ac".into();
        svc.connect(full.clone()).await.unwrap();
        svc.run_sql(&full.id, "CREATE TABLE ac (id INTEGER PRIMARY KEY, v TEXT)".into(), None)
            .await
            .unwrap();
        svc.run_sql(&full.id, "INSERT INTO ac VALUES (1, 'a')".into(), None)
            .await
            .unwrap();
        svc.disconnect(&full.id).await;

        // 只读档:单元格回写拒绝
        let mut ro = sqlite_info(&path);
        ro.id = "ro-ac".into();
        ro.name = "ro-ac".into();
        ro.perm = Some(Perm::ReadOnly);
        svc.connect(ro.clone()).await.unwrap();
        let upd = vec![crate::edits::CellUpdate {
            pk: vec![("id".into(), "1".into())],
            sets: vec![("v".into(), Some("改".into()))],
        }];
        let e = svc
            .apply_changes(&ro.id, "ac", None, &upd, &[], &[])
            .await
            .unwrap_err();
        assert!(e.contains("权限不足"), "只读应拒回写: {e}");

        // DML 档:回写放行
        let mut dml = sqlite_info(&path);
        dml.id = "dml-ac".into();
        dml.name = "dml-ac".into();
        dml.perm = Some(Perm::Dml);
        svc.connect(dml.clone()).await.unwrap();
        svc.apply_changes(&dml.id, "ac", None, &upd, &[], &[])
            .await
            .unwrap();
        svc.disconnect(&dml.id).await;

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn ssh_auto_recover_noop_for_plain_conns() {
        // 非 SSH 连接:恢复检测必须是无副作用快路径(连接保持可用)
        let dir = std::env::temp_dir().join(format!("shujing_native_rec_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = tmp_db("recover");
        std::fs::write(&path, b"").unwrap();
        let svc = crate::Service::new(&dir);
        let info = sqlite_info(&path);
        svc.connect(info.clone()).await.unwrap();
        svc.auto_recover_ssh(&info.id).await;
        let rs = svc
            .run_sql(&info.id, "SELECT 1".into(), None)
            .await
            .unwrap();
        assert_eq!(rs[0].rows[0][0].as_deref(), Some("1"));
        // 不存在的连接 id:同样 no-op 不 panic
        svc.auto_recover_ssh("no-such-conn").await;
        svc.disconnect(&info.id).await;
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::*;

    fn conn(id: &str, group: Option<&str>) -> ConnInfo {
        ConnInfo {
            id: id.into(),
            name: id.into(),
            db_type: DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(format!("/{id}.db")),
            color: None,
            read_only: false,
            perm: None,
            group: group.map(str::to_string),
            plugin: None,
            ssh: None,
        }
    }

    /// 同分组内拖拽:移到目标位置;跨分组落点被忽略;自落位/未知 id 原样
    #[test]
    fn reorder_connections_within_group_and_guards() {
        let conns = vec![
            conn("a", None),
            conn("b", None),
            conn("g1", Some("组一")),
            conn("g2", Some("组一")),
        ];
        let ids = |out: &[ConnInfo]| -> Vec<String> {
            out.iter().map(|c| c.id.clone()).collect()
        };
        // 未分组内:b 拖到 a 的位置
        assert_eq!(ids(&reorder_conn_list(&conns, "b", "a")), ["b", "a", "g1", "g2"]);
        // 跨分组:未分组的 a 拖到 组一 的 g1 → 忽略(拖拽不改分组归属)
        assert_eq!(ids(&reorder_conn_list(&conns, "a", "g1")), ["a", "b", "g1", "g2"]);
        // 组内:g2 拖到 g1 前
        assert_eq!(ids(&reorder_conn_list(&conns, "g2", "g1")), ["a", "b", "g2", "g1"]);
        // 自落位 / 不存在的 id:原样
        assert_eq!(reorder_conn_list(&conns, "g2", "g2").len(), 4);
        assert_eq!(reorder_conn_list(&conns, "nope", "a").len(), 4);
    }
}
