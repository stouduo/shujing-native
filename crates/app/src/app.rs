//! 数镜原生版界面(egui)。


use crate::net;
use shujing_core::{ConnInfo, Service};
use std::collections::HashMap;
use std::sync::Arc;

/// 一次异步操作的完成事件(由 tokio 后台任务投递,帧循环消费)
pub enum BgEvent {
    ConnectDone { conn_id: String, result: Result<String, String> },
    TablesLoaded { conn_id: String, database: Option<String>, result: Result<Vec<shujing_core::TableMeta>, String> },
    RowsLoaded { key: String, result: Result<shujing_core::ExecResult, String> },
    CountLoaded { key: String, result: Result<u64, String> },
    TestDone { result: Result<String, String> },
    AutoTables { conn_id: String, tables: Vec<shujing_core::TableMeta> },
    PkLoaded { key: String, cols: Vec<String> },
    ChangesSaved { key: String, result: Result<u64, String> },
    Saved { result: Result<Vec<ConnInfo>, String> },
    Exported { result: Result<String, String> },
}

/// 一个已打开的表数据标签
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
    /// 列排序:(列名, 升序)
    pub sort: Option<(String, bool)>,
    /// 主键列名(决定可编辑性;空 = 只读)
    pub pk_cols: Vec<String>,
    pub pk_loaded: bool,
    /// 未保存修改:(页内行号, 列名, 新值;None = NULL)
    pub changes: Vec<(usize, String, Option<String>)>,
    /// 正在编辑的格(页内行号, 列下标)与草稿
    pub editing: Option<(usize, usize)>,
    pub draft: String,
}

impl TableTab {
    fn new(conn_id: String, table: String, database: Option<String>) -> Self {
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

    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.conn_id, self.database.as_deref().unwrap_or(""), self.table)
    }
}

/// 左侧树里一个连接的表列表(按库缓存)
#[derive(Default)]
pub struct ConnTree {
    pub online: bool,
    pub version: String,
    pub databases: Vec<String>,
    pub expanded_db: Option<String>,
    pub tables: Vec<shujing_core::TableMeta>,
    pub loading: bool,
}

pub struct ShujingApp {
    pub service: Arc<Service>,
    pub config_dir: std::path::PathBuf,
    /// 保存的连接定义
    pub conns: Vec<ConnInfo>,
    /// 每连接的在线状态/树
    pub trees: HashMap<String, ConnTree>,
    /// 打开的表标签(顺序即标签顺序)
    pub tabs: Vec<TableTab>,
    pub active_tab: usize,
    /// 异步事件队列(net.rs 投递)
    pub events: std::sync::mpsc::Receiver<BgEvent>,
    pub event_tx: std::sync::mpsc::Sender<BgEvent>,
    /// 连接弹窗(新建/编辑)的输入状态
    pub conn_modal: Option<ConnModal>,
    autoopen_done: bool,
    pub error_toast: Option<(String, std::time::Instant)>,
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

    pub fn from_conn(c: &ConnInfo) -> Self {
        Self {
            editing_id: Some(c.id.clone()),
            name: c.name.clone(),
            db_type: c.db_type,
            host: c.host.clone().unwrap_or_default(),
            port: c.port.map(|p| p.to_string()).unwrap_or_default(),
            user: c.user.clone().unwrap_or_default(),
            password: c.password.clone().unwrap_or_default(),
            database: c.database.clone().unwrap_or_default(),
            file_path: c.file_path.clone().unwrap_or_default(),
            testing: false,
            test_result: None,
        }
    }
}

impl ShujingApp {
    /// CJK 字体:macOS 依次尝试常见系统中文字体(egui 默认只有拉丁字形)
    pub(crate) fn install_fonts(cc: &eframe::CreationContext<'_>) {
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
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts.families.entry(family).or_default().push("cjk".to_owned());
                }
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::install_fonts(cc);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_dir = std::path::PathBuf::from(home).join(".shujing-native");
        let _ = std::fs::create_dir_all(&config_dir);
        let service = Arc::new(Service::new(&config_dir));
        let conns = service.list_saved_connections();
        let (event_tx, events) = std::sync::mpsc::channel();
        // 深色主题基调(接近 Tauri 版 zinc 风)
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
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
        }
    }

    pub fn toast_error(&mut self, msg: impl Into<String>) {
        self.error_toast = Some((msg.into(), std::time::Instant::now()));
    }

    pub fn conn_by_id(&self, id: &str) -> Option<&ConnInfo> {
        self.conns.iter().find(|c| c.id == id)
    }

    /// 打开表标签(已开则激活)
    pub fn open_table(&mut self, conn_id: &str, table: &str, database: Option<String>) {
        let key = format!("{}|{}|{}", conn_id, database.as_deref().unwrap_or(""), table);
        if let Some(i) = self.tabs.iter().position(|t| t.key() == key) {
            self.active_tab = i;
            return;
        }
        let tab = TableTab::new(conn_id.to_string(), table.to_string(), database);
        let key = tab.key();
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.load_rows(&key);
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
            let info = shujing_core::ConnInfo {
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
                            let result = service.list_databases(&cid).await;
                            let _ = tx.send(BgEvent::TablesLoaded {
                                conn_id: cid,
                                database: None,
                                result: result.map(|dbs| {
                                    dbs.into_iter()
                                        .map(|d| shujing_core::TableMeta {
                                            name: d,
                                            kind: "__database".into(),
                                        })
                                        .collect()
                                }),
                            });
                        });
                    }
                    Err(e) => {
                        self.trees.remove(&conn_id);
                        self.toast_error(format!("连接失败: {e}"));
                    }
                },
                BgEvent::TablesLoaded { conn_id, database, result } => {
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = false;
                        match result {
                            Ok(metas) => {
                                if database.is_none() {
                                    tree.databases = metas.iter().map(|m| m.name.clone()).collect();
                                    if tree.expanded_db.is_none() && !tree.databases.is_empty() {
                                        tree.expanded_db = Some(tree.databases[0].clone());
                                    }
                                    // 库列表加载后自动展开第一个库并拉表
                                    if let Some(db) = tree.expanded_db.clone() {
                                        let tx = self.event_tx.clone();
                                        let service = self.service.clone();
                                        let cid = conn_id.clone();
                                        let db2 = db.clone();
                                        net::spawn(async move {
                                            let result = service.list_tables(&cid, Some(db2.clone())).await;
                                            let _ = tx.send(BgEvent::TablesLoaded {
                                                conn_id: cid,
                                                database: Some(db2),
                                                result,
                                            });
                                        });
                                    }
                                } else {
                                    tree.tables = metas
                                        .into_iter()
                                        .filter(|m| m.kind == "table")
                                        .collect();
                                }
                            }
                            Err(e) => self.toast_error(format!("加载列表失败: {e}")),
                        }
                    }
                }
                BgEvent::RowsLoaded { key, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.key() == key) {
                        tab.loading = false;
                        match result {
                            Ok(r) => {
                                tab.columns = r.columns;
                                tab.rows = r.rows;
                            }
                            Err(e) => tab.error = Some(e),
                        }
                    }
                }
                BgEvent::CountLoaded { key, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.key() == key) {
                        tab.total = result.ok();
                    }
                }
                BgEvent::AutoTables { conn_id, tables } => {
                    self.trees.entry(conn_id.clone()).or_default();
                    let table_names: Vec<String> = tables
                        .iter()
                        .filter(|m| m.kind == "table")
                        .map(|m| m.name.clone())
                        .collect();
                    for name in table_names {
                        self.open_table(&conn_id, &name, None);
                    }
                }
                BgEvent::PkLoaded { key, cols } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.key() == key) {
                        tab.pk_cols = cols;
                    }
                }
                BgEvent::ChangesSaved { key, result } => match result {
                    Ok(n) => {
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.key() == key) {
                            tab.changes.clear();
                            let cid = tab.conn_id.clone();
                            self.load_rows(&key);
                            let _ = (cid, n);
                        }
                        self.toast_error(format!("已保存 {n} 处修改"));
                    }
                    Err(e) => self.toast_error(format!("保存失败: {e}")),
                },
                BgEvent::TestDone { result } => {
                    if let Some(m) = self.conn_modal.as_mut() {
                        m.testing = false;
                        m.test_result = Some(result);
                    }
                }
                BgEvent::Saved { result } => match result {
                    Ok(conns) => self.conns = conns,
                    Err(e) => self.toast_error(format!("保存失败: {e}")),
                },
                BgEvent::Exported { result } => match result {
                    Ok(path) => self.toast_error(format!("已导出: {path}")),
                    Err(e) => self.toast_error(format!("导出失败: {e}")),
                },
            }
        }
        if dirty {
            ctx.request_repaint();
        }
    }

    /// 拉取当前标签的分页数据(LIMIT/OFFSET [+ ORDER BY])
    pub fn load_rows(&mut self, key: &str) {
        let key = key.to_string();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.key() == key) else { return };
        let conn_id = tab.conn_id.clone();
        let database = tab.database.clone();
        let table = tab.table.clone();
        let (limit, offset) = (tab.page_size, (tab.page - 1) * tab.page_size);
        let order = tab
            .sort
            .as_ref()
            .map(|(c, asc)| format!("{} {}", c, if *asc { "ASC" } else { "DESC" }));
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        tab.loading = true;
        tab.changes.clear();
        tab.editing = None;
        // 首次加载时拉主键列(决定可编辑性)
        if !tab.pk_loaded {
            tab.pk_loaded = true;
            let (tx2, conn2, table2, db2, key2) = (
                tx.clone(),
                conn_id.clone(),
                table.clone(),
                database.clone(),
                key.clone(),
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
                let _ = tx2.send(BgEvent::PkLoaded { key: key2, cols });
            });
        }
        tab.changes.clear();
        tab.editing = None;
        if !tab.pk_loaded {
            tab.pk_loaded = true;
            let (tx2, conn_id2, table2, database2, key2) = (
                tx.clone(),
                conn_id.clone(),
                table.clone(),
                database.clone(),
                key.clone(),
            );
            let service2 = service.clone();
            net::spawn(async move {
                let st = service2
                    .get_table_structure(&conn_id2, &table2, database2.clone())
                    .await;
                let cols = st
                    .map(|s| s.columns.iter().filter(|c| c.key == "PRI").map(|c| c.name.clone()).collect())
                    .unwrap_or_default();
                let _ = tx2.send(BgEvent::PkLoaded { key: key2, cols });
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
            let _ = tx.send(BgEvent::RowsLoaded { key: key.to_string(), result });
            let cnt = format!("SELECT COUNT(*) FROM {full}");
            let result = service.run_sql(&conn_id, cnt, Some(1)).await;
            let result = result.map(|rs| {
                rs.first()
                    .and_then(|r| r.rows.first())
                    .and_then(|row| row.first())
                    .and_then(|v| v.as_deref())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
            });
            let _ = tx.send(BgEvent::CountLoaded { key: key.to_string(), result });
        });
    }
}

pub fn quote_ident(name: &str, mysql: bool) -> String {
    let q = if mysql { '`' } else { '"' };
    format!("{q}{}{q}", name.split(q).collect::<Vec<_>>().join(&q.to_string().repeat(2)))
}
