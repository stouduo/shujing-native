//! 数镜原生版界面:状态 + 异步事件消费 + 数据加载。

use crate::net;
use shujing_core::{ConnInfo, Service};
use std::collections::HashMap;
use std::sync::Arc;

/// 一次异步操作的完成事件(由 tokio 后台任务投递,帧循环消费)
pub enum BgEvent {
    ConnectDone {
        conn_id: String,
        result: Result<String, String>,
    },
    TablesLoaded {
        conn_id: String,
        database: Option<String>,
        tables: Vec<shujing_core::TableMeta>,
    },
    DatabasesLoaded {
        conn_id: String,
        databases: Vec<String>,
    },
    RowsLoaded {
        tab_id: String,
        result: Result<shujing_core::ExecResult, String>,
    },
    CountLoaded {
        tab_id: String,
        total: Option<u64>,
    },
    PkLoaded {
        tab_id: String,
        cols: Vec<String>,
    },
    ChangesSaved {
        tab_id: String,
        result: Result<u64, String>,
    },
    StructureLoaded {
        tab_id: String,
        result: Result<shujing_core::TableStructure, String>,
    },
    QueryDone {
        tab_id: String,
        result: Result<Vec<shujing_core::ExecResult>, String>,
    },
    AutoTables {
        conn_id: String,
        tables: Vec<shujing_core::TableMeta>,
    },
    TestDone {
        result: Result<String, String>,
    },
    ExportDone {
        result: Result<String, String>,
    },
    StatsLoaded {
        key: String,
        result: Result<(u64, Option<u64>, Option<u64>), String>,
    },
    ImportDone {
        result: Result<usize, String>,
    },
    DdlDone {
        result: Result<(), String>,
    },
    /// 设计器编辑模式:初始结构拉取完成
    DesignerLoaded {
        tab_id: String,
        result: Result<shujing_core::TableStructure, String>,
    },
    /// 设计器保存完成(result=提示文案;附带回写的告警)
    DesignerSaved {
        tab_id: String,
        result: Result<String, String>,
        warnings: Vec<String>,
    },
    /// Redis SCAN 一页完成(reset=true 覆盖列表,否则追加)
    RedisScanDone {
        tab_id: String,
        reset: bool,
        result: Result<(u64, Vec<String>), String>,
    },
    /// Redis key 详情
    RedisDetailDone {
        tab_id: String,
        key: String,
        result: Result<shujing_core::redis_ops::RedisDetail, String>,
    },
    /// 批量键类型(key, type)
    RedisTypesDone {
        tab_id: String,
        pairs: Vec<(String, String)>,
    },
    /// 批量 TTL(key, ttl)
    RedisTtlsDone {
        tab_id: String,
        pairs: Vec<(String, i64)>,
    },
    /// Redis 写操作完成
    RedisOpDone {
        tab_id: String,
        op: RedisOp,
        result: Result<(), String>,
    },
    /// Redis 分析弹窗结果
    RedisAnalyzeDone {
        tab_id: String,
        result: Result<Vec<shujing_core::redis_ops::KeyStat>, String>,
    },
    /// SQL 优化分析(EXPLAIN)
    OptimizeDone {
        tab_id: String,
        result: Result<shujing_core::ExecResult, String>,
    },
    /// 全局数据搜索
    SearchDone {
        result: Result<Vec<shujing_core::SearchHit>, String>,
    },
    /// 一次性 SQL 执行完成(导入 SQL 文件),成功提示用 msg
    SqlDone {
        msg: String,
        result: Result<(), String>,
    },
    /// ER 图数据(表+结构+外键)
    ErLoaded {
        tab_id: String,
        result: Result<
            (
                Vec<String>,
                Vec<shujing_core::schema::FkMeta>,
                Vec<(String, shujing_core::TableStructure)>,
            ),
            String,
        >,
    },
}

/// ⌘P 快速打开
pub struct QuickOpen {
    pub query: String,
    pub sel: usize,
}

/// 表统计弹窗键
pub fn stats_key(conn: &str, table: &str) -> String {
    format!("{conn}/{table}")
}

/// CSV 导入状态
pub struct ImportState {
    pub conn_id: String,
    pub database: Option<String>,
    pub table: String,
    pub path: String,
    pub headers: Vec<String>,
    /// 源列 -> 目标表列索引(None=跳过)
    pub col_map: Vec<Option<usize>>,
    pub batch_errors: Option<String>,
    /// 目标表列名(打开导入时从表结构抓取)
    pub target_cols: Vec<String>,
}

/// 新建表弹窗状态(名称/SQL 需要跨帧保存)
pub struct NewTableState {
    pub conn_id: String,
    pub database: Option<String>,
    pub name: String,
    pub sql: String,
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
    /// 当前选中单元格(点击/复制目标)
    pub selected: Option<(usize, usize)>,
    /// 筛选输入框内容
    pub filter: String,
    /// 已应用的筛选(全列 LIKE)
    pub active_filter: Option<String>,
    /// 最近一次查询耗时
    pub last_elapsed_ms: Option<u64>,
    /// 查看完整内容弹窗 (列名, 全文)
    pub view_cell: Option<(String, String)>,
    /// 新增行表单 (列名, 输入)
    pub insert_form: Option<Vec<(String, String)>>,
    /// 行详情面板(原型默认不显示,右键"查看整行"打开)
    pub detail: bool,
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
            selected: None,
            filter: String::new(),
            active_filter: None,
            last_elapsed_ms: None,
            view_cell: None,
            insert_form: None,
            detail: false,
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
    /// 执行历史(新→旧,去重,上限 20)
    pub history: Vec<String>,
    pub results: Vec<shujing_core::ExecResult>,
    pub active_set: usize,
    pub running: bool,
    pub error: Option<String>,
    /// SQL 优化分析弹窗
    pub optimize: Option<OptimizeState>,
}

/// 优化分析弹窗状态
pub struct OptimizeState {
    pub running: bool,
    pub ran: bool,
    pub plan: Option<shujing_core::ExecResult>,
    pub error: Option<String>,
}

/// ER 图标签
pub struct ErTab {
    pub conn_id: String,
    pub database: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub tables: Vec<String>,
    pub structures: std::collections::HashMap<String, shujing_core::TableStructure>,
    pub fks: Vec<shujing_core::schema::FkMeta>,
    pub zoom: f32,
    pub pan: egui::Vec2,
    /// 用户拖过的节点位置(世界坐标)
    pub positions: std::collections::HashMap<String, egui::Vec2>,
}

/// 危险操作确认弹窗(sql 中 {v} 会被替换为用户输入)
pub struct ConfirmState {
    pub title: String,
    pub message: String,
    /// 需要输入完全匹配才能确认(删除表=表名)
    pub expect: Option<String>,
    pub input: String,
    pub conn_id: String,
    pub sql: String,
}

impl ConfirmState {
    pub fn resolved_sql(&self) -> String {
        self.sql.replace("{v}", self.input.trim())
    }
}

/// 全局数据搜索(⌘⇧F)
pub struct GlobalSearchState {
    pub conn_id: String,
    pub keyword: String,
    pub searching: bool,
    pub searched: bool,
    pub hits: Vec<shujing_core::SearchHit>,
    pub error: Option<String>,
}

/// 设计器一行列规格(直接复用 core 的纯数据)
pub type DesignerColumn = shujing_core::designer::ColumnSpec;

/// 表设计器标签(新建/编辑两种模式)
pub struct DesignerTab {
    pub conn_id: String,
    pub database: Option<String>,
    /// 编辑模式下的原表名(锁定);新建模式与 table_name 同步
    pub original_table: Option<String>,
    pub table_name: String,
    pub columns: Vec<DesignerColumn>,
    /// 结构已加载过一次(编辑模式初始拉取)
    pub loaded: bool,
    pub loading: bool,
    pub saving: bool,
    pub error: Option<String>,
    pub info: Option<String>,
}

impl DesignerTab {
    pub fn is_create(&self) -> bool {
        self.original_table.is_none()
    }
}

/// Redis 新建 key 弹窗
pub struct RedisNewKeyState {
    pub name: String,
    /// 0 string / 1 hash / 2 list / 3 set / 4 zset
    pub kind: u8,
    pub text: String,
    /// 多行输入(hash: field=value / zset: member score / list,set: 每行一个)
    pub lines: String,
}

/// Redis 扫描分析弹窗
pub struct RedisAnalyzeState {
    /// 0 大 key / 1 热 key
    pub mode: u8,
    pub sample: usize,
    pub running: bool,
    pub ran: bool,
    pub stats: Vec<shujing_core::redis_ops::KeyStat>,
    pub error: Option<String>,
}

/// Redis 浏览标签
pub struct RedisTab {
    pub conn_id: String,
    pub db: u8,
    pub pattern: String,
    pub cursor: u64,
    pub keys: Vec<String>,
    pub selected_key: Option<String>,
    pub detail: Option<shujing_core::redis_ops::RedisDetail>,
    pub detail_loading: bool,
    pub scanning: bool,
    pub error: Option<String>,
    /// SCAN 后批量拉取的键类型/TTL 徽标
    pub key_types: HashMap<String, String>,
    pub key_ttls: HashMap<String, i64>,
    /// 0 默认 / 1 A→Z / 2 Z→A
    pub sort: u8,
    /// 进行中的写操作(禁用按钮)
    pub op_running: bool,
    pub new_key: Option<RedisNewKeyState>,
    pub analyze: Option<RedisAnalyzeState>,
    /// 内联重命名草稿
    pub rename_draft: Option<String>,
    /// 内联 TTL 秒数草稿
    pub ttl_draft: Option<String>,
    /// string 值编辑弹窗草稿
    pub edit_value: Option<String>,
    /// 成员添加行:(成员, 附加(hash 值/zset 分数))
    pub member_add: Option<(String, String)>,
    /// member_add 处于 hash 字段编辑模式(提交走 set 而不是 add)
    pub member_set_hash: bool,
    /// list 元素内联编辑:(显示序号, 草稿)
    pub list_edit: Option<(usize, String)>,
}

/// Redis 写操作完成后的行为
#[derive(Clone)]
pub enum RedisOp {
    Del { key: String },
    Rename { from: String, to: String },
    /// 操作后重取该 key 详情
    Refresh { key: String },
    /// 新建后重扫并选中
    NewKey { key: String },
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
    Designer(DesignerTab),
    Redis(RedisTab),
    Er(ErTab),
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
    /// 各库的表缓存(此前共用一个 Vec,切库瞬间会显示上一个库的表)
    pub tables_by_db: HashMap<String, Vec<shujing_core::TableMeta>>,
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

    /// 编辑已有连接:带出原配置(含 id,保存时按 id 覆盖)
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
    /// 侧栏表名搜索
    pub table_filter: String,
    /// 自截图模式(SHUJING_SCREENSHOT=<路径>)
    pub shot: Option<ShotState>,
    /// 被未保存修改拦截、等待确认的动作
    pub pending: Option<PendingAction>,
    /// 确认丢弃后放行一次
    pub dirty_override: bool,
    /// 深色主题(持久化到 settings.json)
    pub dark_mode: bool,
    pub auto_connect_done: bool,
    pub quick_open: Option<QuickOpen>,
    pub stats: Option<(
        String,
        Option<(u64, Option<u64>, Option<u64>)>,
        Option<String>,
    )>,
    pub import: Option<ImportState>,
    pub new_table: Option<NewTableState>,
    pub global_search: Option<GlobalSearchState>,
    pub confirm: Option<ConfirmState>,
}

pub enum PendingAction {
    /// 继续执行的动作
    Proceed(Box<crate::table_view::TabAction>),
    /// 关闭标签
    CloseTab(usize),
}

/// 自截图流程状态:数据就绪后请求视口截图,落盘后退出
pub struct ShotState {
    pub path: std::path::PathBuf,
    pub frames: u32,
    pub ready_at: Option<u32>,
    pub sent: bool,
}

impl ShujingApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::install_fonts(cc);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_dir = std::path::PathBuf::from(home).join(".shujing-native");
        let _ = std::fs::create_dir_all(&config_dir);
        let service = Arc::new(Service::new(&config_dir));
        let conns = service.list_saved_connections();
        eprintln!(
            "[startup] config_dir={:?} conns={}",
            config_dir.display(),
            conns.len()
        );
        let (event_tx, events) = std::sync::mpsc::channel();
        let dark = Self::load_dark_pref(&config_dir);
        crate::theme::set_dark(dark);
        crate::theme::apply(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        crate::force_appearance(dark);
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
            table_filter: String::new(),
            pending: None,
            dirty_override: false,
            quick_open: None,
            stats: None,
            import: None,
            new_table: None,
            global_search: None,
            confirm: None,
            dark_mode: dark,
            auto_connect_done: false,
            shot: std::env::var("SHUJING_SCREENSHOT")
                .ok()
                .map(|path| ShotState {
                    path: path.into(),
                    frames: 0,
                    ready_at: None,
                    sent: false,
                }),
        }
    }

    /// CJK 字体:macOS 依次尝试常见系统中文字体(egui 默认只有拉丁字形)
    pub fn install_fonts(cc: &eframe::CreationContext<'_>) {
        Self::apply_fonts(&cc.egui_ctx);
    }

    /// 字体配置(供启动与无头渲染测试共用):
    /// 拉丁 SF Pro(Helvetica Neue 兜底)、粗体 Helvetica Neue Bold、
    /// 中文冬青黑体 W3/W6、数据格 Menlo、符号 Apple Symbols。
    pub fn apply_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let mut add = |name: &str, path: &str, index: u32, tweak: egui::FontTweak| -> bool {
            if let Some(bytes) = Self::mmap_static(path) {
                fonts.font_data.insert(
                    name.to_owned(),
                    std::sync::Arc::new(egui::FontData {
                        font: std::borrow::Cow::Borrowed(bytes),
                        index,
                        tweak,
                    }),
                );
                true
            } else {
                false
            }
        };
        let sf_ok = add(
            "sf",
            "/System/Library/Fonts/SFNS.ttf",
            0,
            Default::default(),
        );
        let sf_ok = sf_ok
            || add(
                "sf",
                "/System/Library/Fonts/HelveticaNeue.ttc",
                0,
                Default::default(),
            );
        let bold_ok = add(
            "bold",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            1,
            Default::default(),
        );
        let cjk_tweak = egui::FontTweak {
            y_offset_factor: crate::theme::CJK_BASELINE_FACTOR,
            ..Default::default()
        };
        let cjk_ok = add(
            "cjk",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            0,
            cjk_tweak,
        );
        let cjk_bold_ok = add(
            "cjk_bold",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            2,
            cjk_tweak,
        );
        let mono_ok = add(
            "mono",
            "/System/Library/Fonts/Menlo.ttc",
            0,
            Default::default(),
        );
        let mono_bold_ok = add(
            "mono_bold",
            "/System/Library/Fonts/Menlo.ttc",
            1,
            Default::default(),
        );
        let symbols_ok = add(
            "symbols",
            "/System/Library/Fonts/Apple Symbols.ttf",
            0,
            egui::FontTweak {
                y_offset: -2.5,
                ..Default::default()
            },
        );

        // 比例字体:拉丁 SF Pro 优先,中文冬青黑体,最后符号回落
        {
            let fam = fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default();
            if sf_ok {
                fam.insert(0, "sf".to_owned());
            }
            if cjk_ok {
                fam.push("cjk".to_owned());
            }
            if symbols_ok {
                fam.push("symbols".to_owned());
            }
        }
        // 等宽字体:Menlo(数据格),中文仅作回落(等宽拉丁保持)
        {
            let fam = fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default();
            if mono_ok {
                fam.insert(0, "mono".to_owned());
            }
            if cjk_ok {
                fam.push("cjk".to_owned());
            }
            if symbols_ok {
                fam.push("symbols".to_owned());
            }
        }
        // 粗体家族:Helvetica Neue Bold + 冬青 W6(中文),缺失时回落常规字族
        {
            let mut list: Vec<String> = Vec::new();
            if bold_ok {
                list.push("bold".to_owned());
            }
            if cjk_bold_ok {
                list.push("cjk_bold".to_owned());
            }
            if sf_ok {
                list.push("sf".to_owned());
            }
            if cjk_ok {
                list.push("cjk".to_owned());
            }
            if symbols_ok {
                list.push("symbols".to_owned());
            }
            if list.is_empty() {
                list = fonts
                    .families
                    .get(&egui::FontFamily::Proportional)
                    .cloned()
                    .unwrap_or_default();
            }
            fonts
                .families
                .insert(egui::FontFamily::Name(crate::theme::BOLD.into()), list);
        }
        // 等宽粗体:Menlo Bold + 冬青 W6
        {
            let mut list: Vec<String> = Vec::new();
            if mono_bold_ok {
                list.push("mono_bold".to_owned());
            }
            if cjk_bold_ok {
                list.push("cjk_bold".to_owned());
            }
            if mono_ok {
                list.push("mono".to_owned());
            }
            if cjk_ok {
                list.push("cjk".to_owned());
            }
            if symbols_ok {
                list.push("symbols".to_owned());
            }
            if list.is_empty() {
                list = fonts
                    .families
                    .get(&egui::FontFamily::Monospace)
                    .cloned()
                    .unwrap_or_default();
            }
            fonts
                .families
                .insert(egui::FontFamily::Name(crate::theme::MONO_BOLD.into()), list);
        }
        ctx.set_fonts(fonts);
    }

    /// 只读 mmap 并故意泄漏以获得 'static 生命周期(进程存续期内映射常驻)
    fn mmap_static(path: &str) -> Option<&'static [u8]> {
        let file = std::fs::File::open(path).ok()?;
        let mmap = unsafe { memmap2::Mmap::map(&file).ok()? };
        let leaked = Box::leak(Box::new(mmap));
        Some(unsafe { std::slice::from_raw_parts(leaked.as_ptr(), leaked.len()) })
    }

    fn load_dark_pref(config_dir: &std::path::Path) -> bool {
        // 优先级:环境变量 > 配置文件 > 默认深色
        if let Ok(t) = std::env::var("SHUJING_THEME") {
            if t.eq_ignore_ascii_case("light") {
                return false;
            }
            if t.eq_ignore_ascii_case("dark") {
                return true;
            }
        }
        let path = config_dir.join("settings.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("dark").and_then(|d| d.as_bool()))
            .unwrap_or(true)
    }

    pub fn set_theme_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark_mode = dark;
        crate::theme::set_dark(dark);
        crate::theme::apply(ctx);
        let path = self.config_dir.join("settings.json");
        let json = serde_json::json!({ "dark": dark }).to_string();
        let _ = std::fs::write(path, json);
        #[cfg(target_os = "macos")]
        crate::force_appearance(dark);
    }

    /// 自截图:每帧驱动重绘;表数据就绪后延时 20 帧(字体/布局稳定)请求截图,
    /// Event::Screenshot 回来后写 TGA 并关窗。10 秒兜底强制截。
    pub fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        ctx.request_repaint();
        let mut image: Option<std::sync::Arc<egui::ColorImage>> = None;
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Screenshot { image: img, .. } = e {
                    image = Some(img.clone());
                }
            }
        });
        if let Some(img) = image {
            match write_tga(&img, &shot.path) {
                Ok(()) => eprintln!("screenshot -> {}", shot.path.display()),
                Err(e) => eprintln!("screenshot write failed: {e}"),
            }
            self.shot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        shot.frames += 1;
        let data_ready = self
            .tabs
            .iter()
            .any(|t| matches!(&t.kind, TabKind::Table(tt) if !tt.rows.is_empty()));
        if data_ready && shot.ready_at.is_none() {
            shot.ready_at = Some(shot.frames);
        }
        let should_send = match shot.ready_at {
            Some(r) => shot.frames > r + 60,
            None => shot.frames > 600,
        };
        if should_send && !shot.sent {
            shot.sent = true;
            eprintln!(
                "[shot] tabs={:?} active={}",
                self.tabs
                    .iter()
                    .map(|t| t.title.clone())
                    .collect::<Vec<_>>(),
                self.active_tab
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }

    pub fn toast_error(&mut self, msg: impl Into<String>) {
        self.error_toast = Some((msg.into(), std::time::Instant::now()));
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

    /// 当前激活标签代表的 (连接, 表, 库),用于侧栏高亮;查询标签返回 None
    pub fn active_tab_info(&self) -> Option<(&str, &str, Option<&str>)> {
        let t = self.tabs.get(self.active_tab)?;
        Some(match &t.kind {
            TabKind::Table(x) => (x.conn_id.as_str(), x.table.as_str(), x.database.as_deref()),
            TabKind::Structure(x) => (x.conn_id.as_str(), x.table.as_str(), x.database.as_deref()),
            TabKind::Query(_) | TabKind::Designer(_) | TabKind::Redis(_) | TabKind::Er(_) => return None,
        })
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

    /// 打开表设计器:table=None 新建模式,Some 编辑模式(拉现有结构)
    pub fn open_designer(&mut self, conn_id: &str, table: Option<&str>, database: Option<String>) {
        let id = format!("d{}", rand_suffix());
        match table {
            Some(t) => {
                let title = format!("设计 {t}");
                self.tabs.push(Tab {
                    id: id.clone(),
                    title,
                    kind: TabKind::Designer(DesignerTab {
                        conn_id: conn_id.to_string(),
                        database,
                        original_table: Some(t.to_string()),
                        table_name: t.to_string(),
                        columns: vec![],
                        loaded: false,
                        loading: true,
                        saving: false,
                        error: None,
                        info: None,
                    }),
                });
                self.active_tab = self.tabs.len() - 1;
                self.load_designer_structure(&id);
            }
            None => {
                self.tabs.push(Tab {
                    id: id.clone(),
                    title: "新表".to_string(),
                    kind: TabKind::Designer(DesignerTab {
                        conn_id: conn_id.to_string(),
                        database,
                        original_table: None,
                        table_name: String::new(),
                        columns: vec![DesignerColumn {
                            name: "id".into(),
                            data_type: "INTEGER".into(),
                            length: String::new(),
                            nullable: false,
                            pk: true,
                            auto_inc: true,
                            ..Default::default()
                        }],
                        loaded: true,
                        loading: false,
                        saving: false,
                        error: None,
                        info: None,
                    }),
                });
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    /// 拉取结构填充设计器列快照(编辑模式初始 + 保存后刷新)
    pub fn load_designer_structure(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Designer(d) = &mut tab.kind else {
            return;
        };
        let conn_id = d.conn_id.clone();
        let table = d.original_table.clone().unwrap_or_else(|| d.table_name.clone());
        let database = d.database.clone();
        d.loading = true;
        net::spawn(async move {
            let result = service
                .get_table_structure(&conn_id, &table, database)
                .await;
            let _ = tx.send(BgEvent::DesignerLoaded { tab_id, result });
        });
    }

    /// 打开 ER 图标签(同连接同库已开则激活)
    pub fn open_er(&mut self, conn_id: &str, database: Option<String>) {
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Er(e) if e.conn_id == conn_id && e.database == database)
        });
        if let Some(i) = existing {
            self.active_tab = i;
            return;
        }
        let id = format!("e{}", rand_suffix());
        self.tabs.push(Tab {
            id: id.clone(),
            title: "ER 图".to_string(),
            kind: TabKind::Er(ErTab {
                conn_id: conn_id.to_string(),
                database,
                loading: true,
                error: None,
                tables: vec![],
                structures: Default::default(),
                fks: vec![],
                zoom: 0.9,
                pan: egui::vec2(60.0, 40.0),
                positions: Default::default(),
            }),
        });
        self.active_tab = self.tabs.len() - 1;
        self.load_er(&id);
    }

    /// 拉 ER 图数据:表清单 + 全部结构(上限 40 张)+ 外键
    pub fn load_er(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Er(e) = &mut tab.kind else {
            return;
        };
        let conn_id = e.conn_id.clone();
        let database = e.database.clone();
        e.loading = true;
        e.error = None;
        net::spawn(async move {
            let tables = service
                .list_tables(&conn_id, database.clone())
                .await
                .map(|v| v.into_iter().filter(|t| t.kind == "table").map(|t| t.name).collect::<Vec<_>>())
                .unwrap_or_default();
            let fks = service
                .list_foreign_keys(&conn_id, database.clone())
                .await
                .unwrap_or_default();
            let mut structures = Vec::new();
            for name in tables.iter().take(40) {
                if let Ok(st) = service
                    .get_table_structure(&conn_id, name, database.clone())
                    .await
                {
                    structures.push((name.clone(), st));
                }
            }
            let _ = tx.send(BgEvent::ErLoaded {
                tab_id,
                result: Ok((tables, fks, structures)),
            });
        });
    }

    /// 打开 Redis 浏览标签(同连接同 db 已开则激活)
    pub fn open_redis(&mut self, conn_id: &str, db: u8) {
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Redis(rt) if rt.conn_id == conn_id && rt.db == db)
        });
        if let Some(i) = existing {
            self.active_tab = i;
            return;
        }
        let id = format!("r{}", rand_suffix());
        self.tabs.push(Tab {
            id: id.clone(),
            title: format!("db{db}"),
            kind: TabKind::Redis(RedisTab {
                conn_id: conn_id.to_string(),
                db,
                pattern: "*".to_string(),
                cursor: 0,
                keys: vec![],
                selected_key: None,
                detail: None,
                detail_loading: false,
                scanning: false,
                error: None,
                key_types: HashMap::new(),
                key_ttls: HashMap::new(),
                sort: 0,
                op_running: false,
                new_key: None,
                analyze: None,
                rename_draft: None,
                ttl_draft: None,
                edit_value: None,
                member_add: None,
                member_set_hash: false,
                list_edit: None,
            }),
        });
        self.active_tab = self.tabs.len() - 1;
        self.redis_scan(&id, true);
    }

    /// SCAN 一页 key(count=200,与 dbclient 一致)
    pub fn redis_scan(&mut self, tab_id: &str, reset: bool) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Redis(rt) = &mut tab.kind else {
            return;
        };
        let conn_id = rt.conn_id.clone();
        let db = rt.db;
        let pattern = if rt.pattern.trim().is_empty() {
            "*".to_string()
        } else {
            rt.pattern.clone()
        };
        let cursor = if reset { 0 } else { rt.cursor };
        rt.scanning = true;
        rt.error = None;
        let tid = tab_id.clone();
        net::spawn(async move {
            let result = service.redis_scan(&conn_id, db, &pattern, cursor, 200).await;
            let _ = tx.send(BgEvent::RedisScanDone {
                tab_id: tid,
                reset,
                result,
            });
        });
    }

    /// 取 key 详情
    pub fn redis_fetch_detail(&mut self, tab_id: &str, key: &str) {
        let tab_id = tab_id.to_string();
        let key = key.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Redis(rt) = &mut tab.kind else {
            return;
        };
        let conn_id = rt.conn_id.clone();
        let db = rt.db;
        rt.selected_key = Some(key.clone());
        rt.detail = None;
        rt.detail_loading = true;
        rt.rename_draft = None;
        rt.member_add = None;
        let tid = tab_id.clone();
        let k = key.clone();
        net::spawn(async move {
            let result = service.redis_key_detail(&conn_id, db, &k).await;
            let _ = tx.send(BgEvent::RedisDetailDone {
                tab_id: tid,
                key,
                result,
            });
        });
    }

    /// Redis 写操作进行中(按钮禁用)
    pub fn tabs_mark_redis_op(&mut self, tab_id: &str) {
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if let TabKind::Redis(rt) = &mut t.kind {
                rt.op_running = true;
            }
        }
    }

    /// SCAN 后批量拉类型 + TTL 徽标
    pub fn redis_fetch_badges(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Redis(rt) = &tab.kind else {
            return;
        };
        let keys: Vec<String> = rt.keys.iter().take(200).cloned().collect();
        if keys.is_empty() {
            return;
        }
        let conn_id = rt.conn_id.clone();
        let db = rt.db;
        let tid = tab_id.clone();
        net::spawn(async move {
            if let Ok(types) = service.redis_key_types(&conn_id, db, &keys).await {
                let pairs: Vec<(String, String)> =
                    keys.iter().cloned().zip(types.into_iter()).collect();
                let _ = tx.send(BgEvent::RedisTypesDone { tab_id: tid.clone(), pairs });
            }
            if let Ok(ttls) = service.redis_ttl_batch(&conn_id, db, &keys).await {
                let pairs: Vec<(String, i64)> =
                    keys.iter().cloned().zip(ttls.into_iter()).collect();
                let _ = tx.send(BgEvent::RedisTtlsDone { tab_id: tid, pairs });
            }
        });
    }

    /// 新建查询标签
    pub fn open_query_tab(&mut self, conn_id: Option<String>) {
        let inherit = conn_id.or_else(|| {
            self.tabs.get(self.active_tab).map(|t| match &t.kind {
                TabKind::Table(t) => t.conn_id.clone(),
                TabKind::Structure(s) => s.conn_id.clone(),
                TabKind::Query(q) => q.conn_id.clone(),
                TabKind::Designer(d) => d.conn_id.clone(),
                TabKind::Redis(r) => r.conn_id.clone(),
                TabKind::Er(e) => e.conn_id.clone(),
            })
        });
        let id = format!("q{}", rand_suffix());
        self.query_seq += 1;
        let title = format!("查询 {}", self.query_seq);
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Query(QueryTab {
                conn_id: inherit.unwrap_or_default(),
                sql: String::new(),
                history: vec![],
                results: vec![],
                active_set: 0,
                running: false,
                error: None,
                optimize: None,
            }),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, idx: usize) {
        // 未保存修改的表标签:先弹确认
        if !self.dirty_override && self.pending.is_none() {
            let dirty = matches!(self.tabs.get(idx), Some(t) if matches!(&t.kind, TabKind::Table(tt) if !tt.changes.is_empty()));
            if dirty {
                self.pending = Some(PendingAction::CloseTab(idx));
                return;
            }
        }
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
        let mysql = mysql_quote(&self.conns, &tab_id);
        let Some(tab) = self.table_tab_mut(&tab_id) else {
            return;
        };
        let conn_id = tab.conn_id.clone();
        let database = tab.database.clone();
        let table = tab.table.clone();
        let (limit, offset) = (tab.page_size, (tab.page - 1) * tab.page_size);
        let cols_snapshot = tab.columns.clone();
        let order = tab.sort.as_ref().map(|(c, asc)| {
            format!(
                "{} {}",
                quote_ident_by(c, mysql),
                if *asc { "ASC" } else { "DESC" }
            )
        });
        let where_clause = tab
            .active_filter
            .as_ref()
            .map(|q| build_like_filter(&cols_snapshot, q, mysql));
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
                Some(db) => format!(
                    "{}.{}",
                    quote_ident_by(db, mysql),
                    quote_ident_by(&table, mysql)
                ),
                None => quote_ident_by(&table, mysql),
            };
            let mut sql = format!("SELECT * FROM {full}");
            if let Some(w) = &where_clause {
                sql.push_str(&format!(" WHERE {w}"));
            }
            if let Some(o) = &order {
                sql.push_str(&format!(" ORDER BY {o}"));
            }
            sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
            let result = service.run_sql(&conn_id, sql, Some(limit)).await;
            let result = result.map(|mut rs| rs.pop().unwrap_or_default());
            let _ = tx.send(BgEvent::RowsLoaded {
                tab_id: tab_id.clone(),
                result,
            });
            let mut cnt = format!("SELECT COUNT(*) FROM {full}");
            if let Some(w) = &where_clause {
                cnt.push_str(&format!(" WHERE {w}"));
            }
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
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Structure(st) = &mut tab.kind else {
            return;
        };
        let conn_id = st.conn_id.clone();
        let table = st.table.clone();
        let database = st.database.clone();
        st.loading = true;
        net::spawn(async move {
            let result = service
                .get_table_structure(&conn_id, &table, database)
                .await;
            let _ = tx.send(BgEvent::StructureLoaded { tab_id, result });
        });
    }

    /// 运行查询标签的 SQL
    pub fn run_query(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Query(q) = &mut tab.kind else {
            return;
        };
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
        q.history.retain(|h| h != &sql);
        q.history.insert(0, sql.clone());
        q.history.truncate(20);
        let conn_id = q.conn_id.clone();
        net::spawn(async move {
            let result = service.run_sql(&conn_id, sql, None).await;
            let _ = tx.send(BgEvent::QueryDone { tab_id, result });
        });
    }

    /// 查询页 Redis 命令行模式:redis_run 执行,结果渲染成单列结果集
    pub fn run_redis_cmd(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Query(q) = &mut tab.kind else {
            return;
        };
        if q.running {
            return;
        }
        if q.conn_id.is_empty() {
            q.error = Some("请先在工具栏选择目标连接".into());
            return;
        }
        let cmd = q.sql.trim().to_string();
        if cmd.is_empty() {
            q.error = Some("命令为空".into());
            return;
        }
        q.running = true;
        q.error = None;
        q.history.retain(|h| h != &cmd);
        q.history.insert(0, cmd.clone());
        q.history.truncate(20);
        let conn_id = q.conn_id.clone();
        net::spawn(async move {
            let result = service.redis_run(&conn_id, &cmd).await.map(|lines| {
                vec![shujing_core::ExecResult {
                    columns: vec!["结果".to_string()],
                    rows: lines.into_iter().map(|l| vec![Some(l)]).collect(),
                    affected: 0,
                    elapsed_ms: 0,
                    truncated: false,
                }]
            });
            let _ = tx.send(BgEvent::QueryDone { tab_id, result });
        });
    }

    /// 导入 SQL 文件:选文件 → 逐语句执行 → 刷新树
    pub fn import_sql_file(&mut self, conn_id: &str) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SQL", &["sql", "txt"])
            .pick_file()
        else {
            return;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            self.toast_error(format!("读取失败: {}", path.display()));
            return;
        };
        let conn_id = conn_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let fname = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        net::spawn(async move {
            let result = service
                .run_sql(&conn_id, content, None)
                .await
                .map(|rs| rs.iter().map(|r| r.affected).sum::<u64>());
            let msg = match &result {
                Ok(n) => format!("已导入 {fname} · 影响 {n} 行"),
                Err(_) => format!("已导入 {fname}"),
            };
            let result = result.map(|_| ());
            let _ = tx.send(BgEvent::SqlDone { msg, result });
        });
    }

    /// 把表名插入到该连接的查询标签(没有则新建)
    pub fn insert_name_to_query(&mut self, conn_id: &str, name: &str) {
        let mysql = mysql_quote(self.conns.as_slice(), conn_id);
        let ident = quote_ident_by(name, mysql);
        // 已有该连接的查询标签就用它
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Query(q) if q.conn_id == conn_id)
        });
        let idx = match existing {
            Some(i) => {
                self.active_tab = i;
                i
            }
            None => {
                self.open_query_tab(Some(conn_id.to_string()));
                self.tabs.len() - 1
            }
        };
        if let Some(t) = self.tabs.get_mut(idx) {
            if let TabKind::Query(q) = &mut t.kind {
                if !q.sql.trim().is_empty() {
                    q.sql.push(' ');
                }
                q.sql.push_str(&ident);
            }
        }
    }

    /// 侧栏触发的单表 SQL 导出(结构与数据)
    pub fn export_table_sql(&mut self, conn_id: &str, _db: &str, table: &str) {
        let default_name = format!("{table}_schema.sql");
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("SQL", &["sql"])
            .save_file()
        else {
            return;
        };
        let conn_id = conn_id.to_string();
        let table = table.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        crate::net::spawn(async move {
            let result = service
                .export_table_sql(&conn_id, &table, true)
                .await
                .and_then(|(sql, _)| {
                    std::fs::write(&path, sql).map_err(|e| format!("写入失败: {e}"))
                })
                .map(|_| path.display().to_string());
            let _ = tx.send(BgEvent::ExportDone { result });
        });
    }

    /// 打开危险操作确认弹窗
    pub fn open_confirm(
        &mut self,
        conn_id: &str,
        title: &str,
        message: &str,
        expect: Option<String>,
        sql: String,
    ) {
        self.confirm = Some(ConfirmState {
            title: title.to_string(),
            message: message.to_string(),
            expect,
            input: String::new(),
            conn_id: conn_id.to_string(),
            sql,
        });
    }

    /// SQL 优化分析:对当前查询页 SQL 跑 EXPLAIN(仅 SELECT)
    pub fn run_optimize(&mut self, tab_id: &str) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Query(q) = &tab.kind else {
            return;
        };
        let sql = q.sql.trim().to_string();
        let head = sql.get(..8).unwrap_or("").to_uppercase();
        if sql.is_empty() {
            self.toast_error("当前查询页没有 SQL".to_string());
            return;
        }
        if !head.starts_with("SELECT") && !head.starts_with("WITH") {
            self.toast_error("优化分析仅支持 SELECT 语句".to_string());
            return;
        }
        let Some(conn) = self.conns.iter().find(|c| c.id == q.conn_id) else {
            self.toast_error("请先选择连接".to_string());
            return;
        };
        let explain = shujing_core::sql_advisor::explain_sql(conn.db_type, &sql);
        let conn_id = q.conn_id.clone();
        let tid = tab_id.to_string();
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tid) {
            if let TabKind::Query(q) = &mut t.kind {
                let o = q.optimize.get_or_insert_with(|| crate::app::OptimizeState {
                    running: false,
                    ran: false,
                    plan: None,
                    error: None,
                });
                o.running = true;
                o.error = None;
            }
        }
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = service
                .run_sql(&conn_id, explain, Some(200))
                .await
                .and_then(|mut rs| {
                    rs.truncate(1);
                    rs.pop().ok_or_else(|| "没有执行计划".to_string())
                });
            let _ = tx.send(BgEvent::OptimizeDone { tab_id: tid, result });
        });
    }

    /// 调试/压测:SHUJING_AUTOOPEN=<db path> 启动即连接并逐表打开(模拟多标签大数据)
    pub fn autoopen(&mut self) {
        if self.autoopen_done {
            return;
        }
        let Ok(db) = std::env::var("SHUJING_AUTOOPEN") else {
            eprintln!("[autoopen] no env, skip");
            return;
        };
        eprintln!("[autoopen] env={db}");
        self.autoopen_done = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let info = ConnInfo {
            id: "auto".into(),
            name: "本地".into(),
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
        // 让压测/演示入口也出现在侧栏(否则只有标签页,看不到连接树)
        if !self.conns.iter().any(|c| c.id == info.id) {
            self.conns.push(info.clone());
        }
        net::spawn(async move {
            let _ = service.connect(info.clone()).await;
            let tables = service
                .list_tables(&info.id, None)
                .await
                .unwrap_or_default();
            let _ = tx.send(BgEvent::AutoTables {
                conn_id: info.id,
                tables,
            });
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
                BgEvent::DatabasesLoaded {
                    conn_id,
                    mut databases,
                } => {
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = false;
                        // SQLite 无库概念:合成 main 节点,让侧栏树/表搜索可用
                        if databases.is_empty() {
                            databases = vec!["main".to_string()];
                        }
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
                BgEvent::TablesLoaded {
                    conn_id,
                    database,
                    tables,
                } => {
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = false;
                        // 无库名(如 SQLite 单库)落到 "main" 槽
                        let key = database.unwrap_or_else(|| "main".to_string());
                        tree.tables_by_db.insert(key, tables);
                    }
                }
                BgEvent::RowsLoaded { tab_id, result } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        t.loading = false;
                        t.selected = None; // 数据翻页后旧选中会指错行
                        match result {
                            Ok(r) => {
                                t.last_elapsed_ms = Some(r.elapsed_ms);
                                t.columns = r.columns;
                                t.rows = r.rows;
                            }
                            Err(e) => {
                                t.last_elapsed_ms = None;
                                t.error = Some(e);
                            }
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
                    let tree = self.trees.entry(conn_id.clone()).or_default();
                    tree.online = true;
                    if tree.databases.is_empty() {
                        tree.databases = vec!["main".to_string()];
                    }
                    if tree.expanded_db.is_none() {
                        tree.expanded_db = Some("main".to_string());
                    }
                    tree.tables_by_db.insert("main".to_string(), tables.clone());
                    let names: Vec<String> = tables
                        .iter()
                        .filter(|m| m.kind == "table")
                        .map(|m| m.name.clone())
                        .collect();
                    let struct_first = std::env::var("SHUJING_OPEN_STRUCT")
                        .ok()
                        .and_then(|_| names.first().cloned());
                    for name in names {
                        self.open_table(&conn_id, &name, Some("main".to_string()));
                    }
                    if let Some(first) = struct_first {
                        self.open_structure(&conn_id, &first, Some("main".to_string()));
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
                BgEvent::StatsLoaded { key, result } => {
                    // 用户可能已关闭统计弹窗:只更新仍打开同一张表的统计,避免迟到结果重新弹窗
                    let still_open = self
                        .stats
                        .as_ref()
                        .map(|(k, _, _)| k == &key)
                        .unwrap_or(false);
                    if still_open {
                        match &result {
                            Ok(_) => self.stats = Some((key, result.ok(), None)),
                            Err(e) => self.stats = Some((key, None, Some(e.clone()))),
                        }
                    }
                }
                BgEvent::ImportDone { result } => match result {
                    Ok(n) => {
                        self.import = None;
                        self.toast_error(format!("已导入 {n} 行"));
                        // 刷新当前表标签(若是导入目标)——简单起见全部刷新
                        let ids: Vec<String> = self
                            .tabs
                            .iter()
                            .filter(|t| matches!(t.kind, TabKind::Table(_)))
                            .map(|t| t.id.clone())
                            .collect();
                        for id in ids {
                            self.load_rows(&id);
                        }
                    }
                    Err(e) => {
                        if let Some(im) = self.import.as_mut() {
                            im.batch_errors = Some(e);
                        } else {
                            self.toast_error(format!("导入失败: {e}"));
                        }
                    }
                },
                BgEvent::DdlDone { result } => match result {
                    Ok(()) => {
                        self.new_table = None;
                        self.toast_error("创建成功".to_string());                        // 重新载入所有在线树的表
                        let ids: Vec<String> = self.trees.keys().cloned().collect();
                        for cid in ids {
                            let dbs = self
                                .trees
                                .get(&cid)
                                .map(|t| (t.databases.clone(), t.expanded_db.clone()));
                            if let Some((dbs, exp)) = dbs {
                                if let Some(db) = exp.or_else(|| dbs.first().cloned()) {
                                    let tx = self.event_tx.clone();
                                    let service = self.service.clone();
                                    let (c2, d2) = (cid.clone(), db.clone());
                                    net::spawn(async move {
                                        let tables = service
                                            .list_tables(&c2, Some(d2))
                                            .await
                                            .unwrap_or_default();
                                        let _ = tx.send(BgEvent::TablesLoaded {
                                            conn_id: c2,
                                            database: Some(db),
                                            tables,
                                        });
                                    });
                                }
                            }
                        }
                    }
                    Err(e) => self.toast_error(format!("创建失败: {e}")),
                },
                BgEvent::DesignerLoaded { tab_id, result } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Designer(d) = &mut tab.kind else {
                        return;
                    };
                    d.loading = false;
                    match result {
                        Ok(st) => {
                            d.columns = shujing_core::designer::columns_from_structure(&st);
                            d.loaded = true;
                            d.error = None;
                        }
                        Err(e) => d.error = Some(e),
                    }
                }
                BgEvent::DesignerSaved {
                    tab_id,
                    result,
                    warnings,
                } => {
                    // 先落地结果,再刷新树/结构(避免拿着 tabs 借用调 self)
                    let (conn_id, db, reload_struct) = {
                        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                            return;
                        };
                        let TabKind::Designer(d) = &mut tab.kind else {
                            return;
                        };
                        d.saving = false;
                        match result {
                            Ok(msg) => {
                                d.info = Some(if warnings.is_empty() {
                                    msg
                                } else {
                                    format!("{msg} · {}", warnings.join(";"))
                                });
                                d.error = None;
                                let r = !d.is_create();
                                (d.conn_id.clone(), d.database.clone(), r)
                            }
                            Err(e) => {
                                d.error = Some(e);
                                return;
                            }
                        }
                    };
                    // 保存成功:刷新在线树的表列表(新建表出现)
                    if let Some(tree) = self.trees.get_mut(&conn_id) {
                        tree.loading = true;
                    }
                    let tx = self.event_tx.clone();
                    let service = self.service.clone();
                    let c2 = conn_id.clone();
                    net::spawn(async move {
                        let tables = service.list_tables(&c2, db.clone()).await.unwrap_or_default();
                        let _ = tx.send(BgEvent::TablesLoaded {
                            conn_id: c2,
                            database: db,
                            tables,
                        });
                    });
                    // 编辑模式:刷新列快照,避免重复执行 ALTER
                    if reload_struct {
                        self.load_designer_structure(&tab_id);
                    }
                }
                BgEvent::RedisScanDone {
                    tab_id,
                    reset,
                    result,
                } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    rt.scanning = false;
                    match result {
                        Ok((cursor, keys)) => {
                            rt.cursor = cursor;
                            if reset {
                                rt.keys = keys;
                            } else {
                                for k in keys {
                                    if !rt.keys.contains(&k) {
                                        rt.keys.push(k);
                                    }
                                }
                            }
                            self.redis_fetch_badges(&tab_id);
                        }
                        Err(e) => rt.error = Some(e),
                    }
                }
                BgEvent::RedisDetailDone {
                    tab_id,
                    key,
                    result,
                } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    rt.detail_loading = false;
                    // 期间切换了选中 key:丢弃过期结果
                    if rt.selected_key.as_deref() != Some(key.as_str()) {
                        return;
                    }
                    match result {
                        Ok(d) => rt.detail = Some(d),
                        Err(e) => rt.error = Some(e),
                    }
                }
                BgEvent::RedisTypesDone { tab_id, pairs } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    for (k, t) in pairs {
                        rt.key_types.insert(k, t);
                    }
                }
                BgEvent::RedisTtlsDone { tab_id, pairs } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    for (k, t) in pairs {
                        rt.key_ttls.insert(k, t);
                    }
                }
                BgEvent::RedisOpDone {
                    tab_id,
                    op,
                    result,
                } => {
                    if let Err(e) = &result {
                        self.toast_error(format!("操作失败: {e}"));
                    }
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    rt.op_running = false;
                    if result.is_err() {
                        return;
                    }
                    match op {
                        RedisOp::Del { key } => {
                            rt.keys.retain(|k| *k != key);
                            rt.key_types.remove(&key);
                            rt.key_ttls.remove(&key);
                            rt.selected_key = None;
                            rt.detail = None;
                            self.toast_error(format!("已删除 {key}"));
                        }
                        RedisOp::Rename { from, to } => {
                            for k in rt.keys.iter_mut() {
                                if *k == from {
                                    *k = to.clone();
                                }
                            }
                            if rt.selected_key.as_deref() == Some(from.as_str()) {
                                rt.selected_key = Some(to.clone());
                            }
                            let tid = tab_id.clone();
                            self.redis_fetch_detail(&tid, &to);
                            self.toast_error(format!("已重命名为 {to}"));
                        }
                        RedisOp::Refresh { key } => {
                            let tid = tab_id.clone();
                            self.redis_fetch_detail(&tid, &key);
                        }
                        RedisOp::NewKey { key } => {
                            self.redis_scan(&tab_id, true);
                            let tid = tab_id.clone();
                            self.redis_fetch_detail(&tid, &key);
                        }
                    }
                }
                BgEvent::SqlDone { msg, result } => match result {
                    Ok(()) => self.toast_error(msg),
                    Err(e) => self.toast_error(format!("执行失败: {e}")),
                },
                BgEvent::ErLoaded { tab_id, result } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Er(e) = &mut tab.kind else {
                        return;
                    };
                    e.loading = false;
                    match result {
                        Ok((tables, fks, structures)) => {
                            e.tables = tables;
                            e.fks = fks;
                            e.structures = structures.into_iter().collect();
                        }
                        Err(err) => e.error = Some(err),
                    }
                }
                BgEvent::OptimizeDone { tab_id, result } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Query(q) = &mut tab.kind else {
                        return;
                    };
                    let Some(o) = q.optimize.as_mut() else {
                        return;
                    };
                    o.running = false;
                    o.ran = true;
                    match result {
                        Ok(plan) => o.plan = Some(plan),
                        Err(e) => o.error = Some(e),
                    }
                }
                BgEvent::SearchDone { result } => {
                    let Some(gs) = self.global_search.as_mut() else {
                        return;
                    };
                    gs.searching = false;
                    gs.searched = true;
                    match result {
                        Ok(hits) => gs.hits = hits,
                        Err(e) => gs.error = Some(e),
                    }
                }
                BgEvent::RedisAnalyzeDone { tab_id, result } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        return;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        return;
                    };
                    let Some(a) = rt.analyze.as_mut() else {
                        return;
                    };
                    a.running = false;
                    a.ran = true;
                    match result {
                        Ok(stats) => a.stats = stats,
                        Err(e) => a.error = Some(e),
                    }
                }
            }
        }
        if dirty {
            ctx.request_repaint();
        }
    }
}

/// 该连接是否用 MySQL 反引号引注标识符(SQLite 接受反引号,Postgres 必须双引号)
pub fn mysql_quote(conns: &[ConnInfo], conn_id: &str) -> bool {
    conns
        .iter()
        .find(|c| c.id == conn_id)
        .map(|c| c.db_type == shujing_core::DbType::MySql)
        .unwrap_or(true)
}

/// 全列 LIKE 筛选:列名按方言引注,值转义单引号
pub fn build_like_filter(columns: &[String], query: &str, mysql: bool) -> String {
    let escaped = query.replace('\'', "''");
    let pattern = format!("'%{escaped}%'");
    columns
        .iter()
        .map(|c| format!("{} LIKE {}", quote_ident_by(c, mysql), pattern))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub fn quote_ident_by(name: &str, mysql: bool) -> String {
    let q = if mysql { '`' } else { '"' };
    format!(
        "{q}{}{q}",
        name.split(q)
            .collect::<Vec<_>>()
            .join(&q.to_string().repeat(2))
    )
}

fn rand_suffix() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_filter_escapes_and_quotes() {
        let cols = vec!["id".to_string(), "na`me".to_string()];
        // 值里的单引号翻倍;列名里的方言引号翻倍
        let mysql = build_like_filter(&cols, "o'brien", true);
        assert!(mysql.contains("`id` LIKE '%o''brien%'"), "{mysql}");
        assert!(mysql.contains("`na``me` LIKE '%o''brien%'"), "{mysql}");
        let pg_cols = vec!["na\"me".to_string()];
        let pg = build_like_filter(&pg_cols, "x", false);
        assert!(pg.contains("\"na\"\"me\" LIKE '%x%'"), "{pg}");
    }
}

/// 写未压缩 TGA(32bit BGRA,顶左原点)
fn write_tga(img: &egui::ColorImage, path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    let (w, h) = (img.width() as u16, img.height() as u16);
    let mut header = [0u8; 18];
    header[2] = 2; // 未压缩真彩
    header[12] = (w & 0xff) as u8;
    header[13] = (w >> 8) as u8;
    header[14] = (h & 0xff) as u8;
    header[15] = (h >> 8) as u8;
    header[16] = 32; // bpp
    header[17] = 0x28; // 8bit alpha + 顶左原点
    f.write_all(&header)?;
    let mut buf = Vec::with_capacity(img.pixels.len() * 4);
    for p in &img.pixels {
        let [r, g, b, a] = p.to_array();
        buf.extend_from_slice(&[b, g, r, a]);
    }
    f.write_all(&buf)?;
    Ok(())
}

impl ShujingApp {
    /// 启动 CSV 导入(按当前 col_map 拼 composite::CsvImport)
    pub fn start_import(&mut self) {
        let Some(im) = self.import.as_ref() else {
            return;
        };
        let col_map: Vec<(usize, String)> = im
            .col_map
            .iter()
            .enumerate()
            .filter_map(|(src, dst)| dst.map(|d| (src, im.target_cols[d].clone())))
            .collect();
        if col_map.is_empty() {
            self.toast_error("未映射任何列".to_string());
            return;
        }
        let imp = shujing_core::composite::CsvImport {
            path: im.path.clone(),
            database: im.database.clone(),
            table: im.table.clone(),
            col_map,
            has_header: true,
        };
        let info = match self.conns.iter().find(|c| c.id == im.conn_id) {
            Some(c) => c.clone(),
            None => return,
        };
        let service = self.service.clone();
        let tx = self.event_tx.clone();
        crate::net::spawn(async move {
            let r = shujing_core::composite::import_csv(&service, info, imp).await;
            let _ = tx.send(BgEvent::ImportDone { result: r });
        });
    }
}

impl ShujingApp {
    /// 打开表统计弹窗并异步取数
    pub fn open_stats(&mut self, conn_id: &str, database: &str, table: &str) {
        let key = crate::app::stats_key(conn_id, table);
        self.stats = Some((key.clone(), None, None));
        let Some(info) = self.conns.iter().find(|c| c.id == conn_id).cloned() else {
            return;
        };
        let service = self.service.clone();
        let tx = self.event_tx.clone();
        let db = database.to_string();
        let tbl = table.to_string();
        let key2 = key.clone();
        crate::net::spawn(async move {
            let r = shujing_core::composite::table_stats(&service, &info, Some(db), &tbl).await;
            let _ = tx.send(BgEvent::StatsLoaded {
                key: key2,
                result: r,
            });
        });
    }

    pub fn open_import(&mut self, conn_id: &str, database: &str, table: &str) {
        // 打开系统文件选择框,读表结构填目标列
        let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv", "txt"])
            .pick_file()
        else {
            return;
        };
        let target_cols = self
            .tabs
            .iter()
            .find_map(|t| {
                t.table_data()
                    .filter(|tt| {
                        tt.conn_id == conn_id
                            && tt.table == table
                            && tt.database.as_deref() == Some(database)
                    })
                    .map(|tt| tt.columns.clone())
            })
            .or_else(|| {
                self.tabs
                    .iter()
                    .find_map(|t| t.table_data().map(|tt| tt.columns.clone()))
            })
            .unwrap_or_default();
        self.import = Some(ImportState {
            conn_id: conn_id.to_string(),
            database: Some(database.to_string()),
            table: table.to_string(),
            path: path.display().to_string(),
            headers: Vec::new(),
            col_map: Vec::new(),
            batch_errors: None,
            target_cols,
        });
    }
}
