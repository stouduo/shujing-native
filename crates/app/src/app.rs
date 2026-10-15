//! 数镜原生版界面:状态 + 异步事件消费 + 数据加载。

use crate::net;
use shujing_core::{ConnInfo, Service};
use std::collections::HashMap;
use std::sync::Arc;

/// 一次异步操作的完成事件(由 tokio 后台任务投递,帧循环消费)
pub enum BgEvent {
    ConnectDone { conn_id: String, result: Result<String, String> },
    TablesLoaded { conn_id: String, database: Option<String>, tables: Vec<shujing_core::TableMeta> },
    DatabasesLoaded { conn_id: String, databases: Vec<String> },
    RowsLoaded { tab_id: String, result: Result<shujing_core::ExecResult, String> },
    CountLoaded { tab_id: String, total: Option<u64> },
    PkLoaded { tab_id: String, cols: Vec<String> },
    ChangesSaved { tab_id: String, result: Result<u64, String> },
    StructureLoaded { tab_id: String, result: Result<shujing_core::TableStructure, String> },
    QueryDone { tab_id: String, result: Result<Vec<shujing_core::ExecResult>, String> },
    AutoTables { conn_id: String, tables: Vec<shujing_core::TableMeta> },
    TestDone { result: Result<String, String> },
    ExportDone { result: Result<String, String> },
    ConnsSaved { conns: Vec<ConnInfo> },
}

/// 表数据标签
pub struct TableTab {
    pub conn_id: String,
    pub table: String,
    pub database: Option<String>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total: Option<u64>,
    pub page: usize,
    pub page_size: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub sort: Option<(String, bool)>,
    pub pk_cols: Vec<String>,
    pub pk_loaded: bool,
    pub changes: Vec<(usize, String, Option<String>)>,
    pub editing: Option<(usize, usize)>,
    pub draft: String,
}

impl TableTab {
    pub fn new(conn_id: String, table: String, database: Option<String>) -> Self {
        Self {
            conn_id,
            table,
            database,
            columns: vec![],
            rows: vec![],
            total: None,
            page: 1,
            page_size: 100,
            loading: true,
            error: None,
            sort: None,
            pk_cols: vec![],
            pk_loaded: false,
            changes: vec![],
            editing: None,
            draft: String::new(),
        }
    }
}

/// 结构标签
pub struct StructureTab {
    pub conn_id: String,
    pub table: String,
    pub database: Option<String>,
    pub data: Option<shujing_core::TableStructure>,
    pub loading: bool,
    pub error: Option<String>,
}

/// 查询标签
pub struct QueryTab {
    pub conn_id: String,
    pub sql: String,
    pub results: Vec<shujing_core::ExecResult>,
    pub active_set: usize,
    pub running: bool,
    pub error: Option<String>,
}

/// 统一标签
pub struct Tab {
    pub id: String,
    pub title: String,
    pub kind: TabKind,
}

pub enum TabKind {
    Table(TableTab),
    Structure(StructureTab),
    Query(QueryTab),
}

impl Tab {
    pub fn table_data_mut(&mut self) -> Option<&mut TableTab> {
        match &mut self.kind {
            TabKind::Table(t) => Some(t),
            _ => None,
        }
    }
    pub fn table_data(&self) -> Option<&TableTab> {
        match &self.kind {
            TabKind::Table(t) => Some(t),
            _ => None,
        }
    }
}

/// 左侧树里一个连接的在线状态/库表
#[derive(Default, Clone)]
pub struct ConnTree {
    pub online: bool,
    pub version: String,
    pub databases: Vec<String>,
    pub expanded_db: Option<String>,
    pub tables: Vec<shujing_core::TableMeta>,
    pub loading: bool,
}

/// 连接编辑弹窗
pub struct ConnModal {
    pub editing_id: Option<String>,
    pub name: String,
    pub db_type: shujing_core::DbType,
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub database: String,
    pub file_path: String,
    pub testing: bool,
    pub test_result: Option<Result<String, String>>,
}

impl ConnModal {
    pub fn new() -> Self {
        Self {
            editing_id: None,
            name: String::new(),
            db_type: shujing_core::DbType::MySql,
            host: String::new(),
            port: String::new(),
            user: String::new(),
            password: String::new(),
            database: String::new(),
            file_path: String::new(),
            testing: false,
            test_result: None,
        }
    }
}

pub struct ShujingApp {
    pub service: Arc<Service>,
    pub config_dir: std::path::PathBuf,
    pub conns: Vec<ConnInfo>,
    pub trees: HashMap<String, ConnTree>,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub events: std::sync::mpsc::Receiver<BgEvent>,
    pub event_tx: std::sync::mpsc::Sender<BgEvent>,
    pub conn_modal: Option<ConnModal>,
    pub error_toast: Option<(String, std::time::Instant)>,
    pub autoopen_done: bool,
    pub query_seq: usize,
}

impl ShujingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::install_fonts(cc);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_dir = std::path::PathBuf::from(home).join(".shujing-native");
        let _ = std::fs::create_dir_all(&config_dir);
        let service = Arc::new(Service::new(&config_dir));
        let conns = service.list_saved_connections();
        let (event_tx, events) = std::sync::mpsc::channel();
        crate::theme::apply(&cc.egui_ctx);
        Self {
            service,
            config_dir,
            conns,
            trees: HashMap::new(),
            tabs: vec![],
            active_tab: 0,
            events,
            event_tx,
            conn_modal: None,
            error_toast: None,
            autoopen_done: false,
            query_seq: 0,
        }
    }

    /// CJK 字体:macOS 依次尝试常见系统中文字体(egui 默认只有拉丁字形)
    pub fn install_fonts(cc: &eframe::CreationContext<'_>) {
        // 按体积选最小可用(egui 会驻留字体原始字节,PingFang.ttc 偏大)
        let candidates = [
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        let mut fonts = egui::FontDefinitions::default();
        for path in candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts.families.entry(family).or_default().push("cjk".to_owned());
                }
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
    }

    pub fn toast_error(&mut self, msg: impl Into<String>) {
        self.error_toast = Some((msg.into(), std::time::Instant::now()));
    }

    pub fn conn_by_id(&self, id: &str) -> Option<&ConnInfo> {
        self.conns.iter().find(|c| c.id == id)
    }

    pub fn table_tab_mut(&mut self, tab_id: &str) -> Option<&mut TableTab> {
        self.tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.table_data_mut())
    }

    pub fn table_tab(&self, tab_id: &str) -> Option<&TableTab> {
        self.tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.table_data())
    }

    /// 打开表数据标签(已开则激活)
    pub fn open_table(&mut self, conn_id: &str, table: &str, database: Option<String>) {
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Table(tt)
                if tt.conn_id == conn_id && tt.table == table && tt.database == database)
        });
        if let Some(i) = existing {
            self.active_tab = i;
            return;
        }
        let tab = TableTab::new(conn_id.to_string(), table.to_string(), database);
        let id = format!("t{}", rand_suffix());
        self.tabs.push(Tab {
            id: id.clone(),
            title: tab.table.clone(),
            kind: TabKind::Table(tab),
        });
        self.active_tab = self.tabs.len() - 1;
        self.load_rows(&id);
    }

    /// 打开结构标签(已开则激活)
    pub fn open_structure(&mut self, conn_id: &str, table: &str, database: Option<String>) {
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Structure(st)
                if st.conn_id == conn_id && st.table == table && st.database == database)
        });
        if let Some(i) = existing {
            self.active_tab = i;
            return;
        }
        let id = format!("s{}", rand_suffix());
        let title = format!("{table} 结构");
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Structure(StructureTab {
                conn_id: conn_id.to_string(),
                table: table.to_string(),
                database,
                data: None,
                loading: true,
                error: None,
            }),
        });
        self.active_tab = self.tabs.len() - 1;
        self.load_structure(&id);
    }

    /// 新建查询标签
    pub fn open_query_tab(&mut self, conn_id: Option<String>) {
        let inherit = conn_id.or_else(|| {
            self.tabs.get(self.active_tab).map(|t| match &t.kind {
                TabKind::Table(t) => t.conn_id.clone(),
                TabKind::Structure(s) => s.conn_id.clone(),
                TabKind::Query(q) => q.conn_id.clone(),
            })
        });
        self.query_seq += 1;
        let id = format!("q{}", rand_suffix());
        let title = format!("查询 {}", self.query_seq);
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Query(QueryTab {
                conn_id: inherit.unwrap_or_default(),
                sql: String::new(),
                results: vec![],
                active_set: 0,
                running: false,
                error: None,
            }),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() && !self.tabs.is_empty() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    /// 拉取表数据标签的分页数据(LIMIT/OFFSET [+ ORDER BY]);首次同时拉主键
    pub fn load_rows(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.table_tab_mut(&tab_id) else { return };
        let conn_id = tab.conn_id.clone();
        let database = tab.database.clone();
        let table = tab.table.clone();
        let (limit, offset) = (tab.page_size, (tab.page - 1) * tab.page_size);
        let order = tab
            .sort
            .as_ref()
            .map(|(c, asc)| format!("{} {}", c, if *asc { "ASC" } else { "DESC" }));
        tab.loading = true;
        tab.changes.clear();
        tab.editing = None;
        if !tab.pk_loaded {
            tab.pk_loaded = true;
            let (tx2, conn2, table2, db2, id2) = (
                tx.clone(),
                conn_id.clone(),
                table.clone(),
                database.clone(),
                tab_id.clone(),
            );
            let service2 = service.clone();
            net::spawn(async move {
                let st = service2.get_table_structure(&conn2, &table2, db2).await;
                let cols = st
                    .map(|s| {
                        s.columns
                            .iter()
                            .filter(|c| c.key == "PRI")
                            .map(|c| c.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = tx2.send(BgEvent::PkLoaded { tab_id: id2, cols });
            });
        }
        net::spawn(async move {
            let full = match &database {
                Some(db) => format!("{}.{}", quote_ident(db, true), quote_ident(&table, true)),
                None => quote_ident(&table, true),
            };
            let sql = match &order {
                Some(o) => format!("SELECT * FROM {full} ORDER BY {o} LIMIT {limit} OFFSET {offset}"),
                None => format!("SELECT * FROM {full} LIMIT {limit} OFFSET {offset}"),
            };
            let result = service.run_sql(&conn_id, sql, Some(limit)).await;
            let result = result.map(|mut rs| rs.pop().unwrap_or_default());
            let _ = tx.send(BgEvent::RowsLoaded { tab_id: tab_id.clone(), result });
            let cnt = format!("SELECT COUNT(*) FROM {full}");
            let result = service.run_sql(&conn_id, cnt, Some(1)).await;
            let total = result.ok().and_then(|rs| {
                rs.first()
                    .and_then(|r| r.rows.first())
                    .and_then(|row| row.first())
                    .and_then(|v| v.as_deref())
                    .and_then(|s| s.parse::<u64>().ok())
            });
            let _ = tx.send(BgEvent::CountLoaded { tab_id, total });
        });
    }

    pub fn load_structure(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        let TabKind::Structure(st) = &mut tab.kind else { return };
        let conn_id = st.conn_id.clone();
        let table = st.table.clone();
        let database = st.database.clone();
        st.loading = true;
        net::spawn(async move {
            let result = service.get_table_structure(&conn_id, &table, database).await;
            let _ = tx.send(BgEvent::StructureLoaded { tab_id, result });
        });
    }

    /// 运行查询标签的 SQL
    pub fn run_query(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
        let TabKind::Query(q) = &mut tab.kind else { return };
        if q.running {
            return;
        }
        if q.conn_id.is_empty() {
            q.error = Some("请先在工具栏选择目标连接".into());
            return;
        }
        let sql = q.sql.trim().to_string();
        if sql.is_empty() {
            q.error = Some("SQL 为空".into());
            return;
        }
        q.running = true;
        q.error = None;
        let conn_id = q.conn_id.clone();
        net::spawn(async move {
            let result = service.run_sql(&conn_id, sql, None).await;
            let _ = tx.send(BgEvent::QueryDone { tab_id, result });
        });
    }

    /// 调试/压测:SHUJING_AUTOOPEN=<db path> 启动即连接并逐表打开(模拟多标签大数据)
    pub fn autoopen(&mut self) {
        if self.autoopen_done {
            return;
        }
        let Ok(db) = std::env::var("SHUJING_AUTOOPEN") else {
            return;
        };
        self.autoopen_done = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let info = ConnInfo {
                id: "auto".into(),
                name: "auto".into(),
                db_type: shujing_core::DbType::Sqlite,
                host: None,
                port: None,
                user: None,
                password: None,
                database: None,
                file_path: Some(db),
                color: None,
                read_only: false,
            };
            let _ = service.connect(info.clone()).await;
            let tables = service.list_tables(&info.id, None).await.unwrap_or_default();
            let _ = tx.send(BgEvent::AutoTables { conn_id: info.id, tables });
        });
    }

    /// 消费后台任务完成事件(数据落地到状态,再触发重绘)
    pub fn drain_events(&mut self, ctx: &egui::Context) {
        let mut dirty = false;
        while let Ok(ev) = self.events.try_recv() {
            dirty = true;
            match ev {
                BgEvent::ConnectDone { conn_id, result } => match result {
                    Ok(version) => {
                        let tree = self.trees.entry(conn_id.clone()).or_default();
                        tree.online = true;
                        tree.version = version;
                        let tx = self.event_tx.clone();
                        let service = self.service.clone();
                        let cid = conn_id.clone();
                        net::spawn(async move {
                            let databases = service.list_databases(&cid).await;
                            let _ = tx.send(BgEvent::DatabasesLoaded {
                                conn_id: cid,
                                databases: databases.unwrap_or_default(),
                            });
                        });
                    }
                    Err(e) => {
                        self.trees.remove(&conn_id);
                        self.toast_error(format!("连接失败: {e}"));
                    }
                },
                BgEvent::DatabasesLoaded { conn_id, databases } => {
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = false;
                        tree.databases = databases;
                        if tree.expanded_db.is_none() && !tree.databases.is_empty() {
                            let db = tree.databases[0].clone();
                            tree.expanded_db = Some(db.clone());
                            let tx = self.event_tx.clone();
                            let service = self.service.clone();
                            let cid = conn_id.clone();
                            net::spawn(async move {
                                let tables = service.list_tables(&cid, Some(db.clone())).await;
                                let _ = tx.send(BgEvent::TablesLoaded {
                                    conn_id: cid,
                                    database: Some(db),
                                    tables: tables.unwrap_or_default(),
                                });
                            });
                        }
                    }
                }
                BgEvent::TablesLoaded { conn_id, database, tables } => {
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = false;
                        tree.tables = tables;
                        let _ = database;
                    }
                }
                BgEvent::RowsLoaded { tab_id, result } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        t.loading = false;
                        match result {
                            Ok(r) => {
                                t.columns = r.columns;
                                t.rows = r.rows;
                            }
                            Err(e) => t.error = Some(e),
                        }
                    }
                }
                BgEvent::CountLoaded { tab_id, total } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        t.total = total;
                    }
                }
                BgEvent::PkLoaded { tab_id, cols } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        t.pk_cols = cols;
                    }
                }
                BgEvent::ChangesSaved { tab_id, result } => match result {
                    Ok(n) => {
                        if let Some(t) = self.table_tab_mut(&tab_id) {
                            t.changes.clear();
                        }
                        self.load_rows(&tab_id);
                        self.toast_error(format!("已保存 {n} 处修改"));
                    }
                    Err(e) => self.toast_error(format!("保存失败: {e}")),
                },
                BgEvent::StructureLoaded { tab_id, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Structure(st) = &mut tab.kind {
                            st.loading = false;
                            match result {
                                Ok(data) => st.data = Some(data),
                                Err(e) => st.error = Some(e),
                            }
                        }
                    }
                }
                BgEvent::QueryDone { tab_id, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Query(q) = &mut tab.kind {
                            q.running = false;
                            match result {
                                Ok(rs) => {
                                    q.results = rs;
                                    q.active_set = 0;
                                }
                                Err(e) => q.error = Some(e),
                            }
                        }
                    }
                }
                BgEvent::AutoTables { conn_id, tables } => {
                    self.trees.entry(conn_id.clone()).or_default();
                    let names: Vec<String> = tables
                        .iter()
                        .filter(|m| m.kind == "table")
                        .map(|m| m.name.clone())
                        .collect();
                    for name in names {
                        self.open_table(&conn_id, &name, None);
                    }
                }
                BgEvent::TestDone { result } => {
                    if let Some(m) = self.conn_modal.as_mut() {
                        m.testing = false;
                        m.test_result = Some(result);
                    }
                }
                BgEvent::ExportDone { result } => match result {
                    Ok(path) => self.toast_error(format!("已导出: {path}")),
                    Err(e) => self.toast_error(format!("导出失败: {e}")),
                },
                BgEvent::ConnsSaved { conns } => self.conns = conns,
            }
        }
        if dirty {
            ctx.request_repaint();
        }
    }
}

pub fn quote_ident(name: &str, mysql: bool) -> String {
    let q = if mysql { '`' } else { '"' };
    format!(
        "{q}{}{q}",
        name.split(q).collect::<Vec<_>>().join(&q.to_string().repeat(2))
    )
}

fn rand_suffix() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}
