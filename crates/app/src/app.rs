//! 数镜原生版界面:状态 + 异步事件消费 + 数据加载。

use crate::net;
use shujing_core::{ConnInfo, Service};
use std::collections::HashMap;
use std::sync::Arc;

/// 一次异步操作的完成事件(由 tokio 后台任务投递,帧循环消费)
// ---------------------------------------------------------------- 会话持久化

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SessionData {
    #[serde(default)]
    tabs: Vec<SessionTab>,
    #[serde(default)]
    active: usize,
    #[serde(default)]
    online: Vec<String>,
    #[serde(default)]
    expanded: HashMap<String, String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SessionTab {
    Table {
        conn: String,
        table: String,
        database: Option<String>,
    },
    Structure {
        conn: String,
        table: String,
        database: Option<String>,
    },
    Query {
        conn: String,
        sql: String,
        #[serde(default)]
        history: Vec<String>,
        /// 执行前的会话库(None=连接默认库)
        #[serde(default)]
        database: Option<String>,
    },
    Designer {
        conn: String,
        database: Option<String>,
        table: Option<String>,
        table_name: String,
    },
    Redis {
        conn: String,
        db: u8,
    },
    Er {
        conn: String,
        database: Option<String>,
    },
    Ddl {
        conn: String,
        database: Option<String>,
        name: String,
        #[serde(rename = "obj_kind")]
        kind: String,
    },
}

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
        seq: u64,
        result: Result<shujing_core::ExecResult, String>,
    },
    CountLoaded {
        tab_id: String,
        seq: u64,
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
    /// 查询结果回写:主键列
    QueryPkDone {
        tab_id: String,
        cols: Vec<String>,
    },
    /// 一次性 SQL 执行完成(导入 SQL 文件),成功提示用 msg
    SqlDone {
        msg: String,
        result: Result<(), String>,
    },
    /// 对象 DDL
    DdlLoaded {
        tab_id: String,
        result: Result<String, String>,
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
    /// 数据请求代次:快速翻页/排序时丢弃迟到的旧结果
    pub load_seq: u64,
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
    /// 已应用的筛选(全列 LIKE 关键词)
    pub active_filter: Option<String>,
    /// 筛选栏(字段/自由模式)
    pub filter_bar_open: bool,
    /// 0 字段模式 / 1 自由 WHERE
    pub filter_mode: u8,
    /// 字段模式条件 (列, 操作符, 值)
    pub conds: Vec<(String, String, String)>,
    pub free_where: String,
    /// 已应用的筛选栏 WHERE(构建好的)
    pub applied_where: Option<String>,
    pub filter_error: Option<String>,
    /// 列统计弹窗:(列名, [(指标, 值)])
    pub col_stats: Option<(String, Vec<(String, String)>)>,
    /// 隐藏的列(列选择器)
    pub hidden_cols: Vec<String>,
    /// 显示行号列
    pub show_rowno: bool,
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
            load_seq: 0,
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
            filter_bar_open: false,
            filter_mode: 0,
            conds: vec![],
            free_where: String::new(),
            applied_where: None,
            filter_error: None,
            col_stats: None,
            hidden_cols: vec![],
            show_rowno: true,
        }
    }
}

/// 列统计(当前页):行数/数值统计/NULL/去重
pub fn compute_col_stats(
    col: &str,
    rows: &[Vec<Option<String>>],
    columns: &[String],
) -> (String, Vec<(String, String)>) {
    let Some(ci) = columns.iter().position(|c| c == col) else {
        return (col.to_string(), vec![]);
    };
    let vals: Vec<Option<String>> = rows.iter().map(|r| r[ci].clone()).collect();
    let total = vals.len();
    let nulls = vals.iter().filter(|v| v.is_none()).count();
    let mut non_null: Vec<f64> = vals
        .iter()
        .filter_map(|v| v.as_ref().and_then(|s| s.parse::<f64>().ok()))
        .collect();
    let numeric = non_null.len();
    let mut out = vec![
        ("行数".to_string(), crate::icons::thousands(total as u64)),
        ("NULL".to_string(), crate::icons::thousands(nulls as u64)),
    ];
    if numeric > 0 {
        let sum: f64 = non_null.iter().sum();
        let avg = sum / numeric as f64;
        non_null.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let fmt = |x: f64| {
            if x.fract() == 0.0 && x.abs() < 1e15 {
                format!("{}", x as i64)
            } else {
                format!("{x}")
            }
        };
        out.push((
            "数值个数".to_string(),
            crate::icons::thousands(numeric as u64),
        ));
        out.push(("SUM".to_string(), fmt(sum)));
        out.push(("AVG".to_string(), fmt(avg)));
        out.push(("MIN".to_string(), fmt(non_null[0])));
        out.push(("MAX".to_string(), fmt(non_null[non_null.len() - 1])));
    } else {
        let mut distinct: Vec<&String> = vals.iter().filter_map(|v| v.as_ref()).collect();
        distinct.sort();
        distinct.dedup();
        out.push((
            "非空去重".to_string(),
            crate::icons::thousands(distinct.len() as u64),
        ));
    }
    (col.to_string(), out)
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
    /// 会话库上下文(None=连接默认库;执行前 USE,避免多库连接报 No database selected)
    pub database: Option<String>,
    pub sql: String,
    /// 执行历史(新→旧,去重,上限 20)
    pub history: Vec<String>,
    pub results: Vec<shujing_core::ExecResult>,
    pub active_set: usize,
    pub running: bool,
    pub error: Option<String>,
    /// SQL 优化分析弹窗
    pub optimize: Option<OptimizeState>,
    /// 结果集回写:单表 SELECT 检测出的表名
    pub result_table: Option<String>,
    /// 回写目标库(SQL 内显式库优先,否则执行时选中的库)
    pub result_db: Option<String>,
    pub result_pk: Vec<String>,
    pub pk_checked: bool,
    /// 结果集未保存修改:(行号, 列名, 新值)
    pub q_changes: Vec<(usize, String, Option<String>)>,
    pub q_editing: Option<(usize, usize)>,
    pub q_draft: String,
}

/// 优化分析弹窗状态
pub struct OptimizeState {
    pub running: bool,
    pub ran: bool,
    pub plan: Option<shujing_core::ExecResult>,
    pub error: Option<String>,
}

/// 对象 DDL 查看标签(触发器/存储过程/函数/视图)
pub struct DdlTab {
    pub conn_id: String,
    pub database: Option<String>,
    pub name: String,
    pub kind: String,
    pub ddl: Option<String>,
    pub loading: bool,
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
    /// 首帧自动适配视野
    pub fit_done: bool,
    /// 适配尝试帧数(视口不稳时多重试几帧;内容超大的图不能无限重试)
    pub fit_tries: u32,
    /// 正在被拖拽的节点(拖节点期间禁用画布平移,防双重位移)
    pub dragging_node: Option<String>,
    /// 请求重新适配(适配按钮)
    pub refit: bool,
}

/// 库筛选弹窗状态(每连接勾选显示哪些库)
pub struct DbFilterState {
    pub conn_id: String,
    pub search: String,
    /// 该连接的全部库名(弹窗期间冻结)
    pub all: Vec<String>,
    /// 勾选中的库名
    pub selected: Vec<String>,
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
    /// 非 SQL 动作(如 delete-conn:<id>):确认后交给 app 处理而不执行 sql
    pub action: Option<String>,
}

impl ConfirmState {
    /// {v} 是用户输入的标识符(新表名等):按方言引注,防注入/防含空格报错
    pub fn resolved_sql(&self, mysql: bool) -> String {
        let v = self.input.trim();
        if v.is_empty() {
            return self.sql.clone();
        }
        let quoted = crate::app::quote_ident_by(v, mysql);
        self.sql.replace("{v}", &quoted)
    }
}

/// 全局数据搜索(⌘⇧F)
pub struct GlobalSearchState {
    pub conn_id: String,
    /// 搜索范围库(None=连接默认库;MySQL 多库连接必选,否则无默认库时搜不到表)
    pub db: Option<String>,
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
    /// 加载时的列规格快照(编辑模式 diff 基线:只对改动列出 ALTER)
    pub baseline: Vec<DesignerColumn>,
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
    Del {
        key: String,
    },
    Rename {
        from: String,
        to: String,
    },
    /// 操作后重取该 key 详情
    Refresh {
        key: String,
    },
    /// 新建后重扫并选中
    NewKey {
        key: String,
    },
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
    Ddl(DdlTab),
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
    /// 对象分组展开状态(键 = "{db}|{group}",group ∈ table/view/prog;默认只开 table)
    pub groups_open: HashMap<String, bool>,
    /// 显示的库筛选(None=全部显示;Some(列表)=只显示列表中的库)
    pub visible_dbs: Option<Vec<String>>,
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
    /// 只读模式(生产库保护)
    pub read_only: bool,
    /// 标识色板索引
    pub color_idx: Option<u8>,
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
            read_only: false,
            color_idx: None,
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
            read_only: c.read_only,
            color_idx: c.color.as_ref().and_then(|hex| {
                let rgb = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok();
                rgb.and_then(|v| {
                    let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
                    crate::table_view::CONN_COLORS
                        .iter()
                        .position(|(name, c)| {
                            *name != "无" && c.r() == r && c.g() == g && c.b() == b
                        })
                        .map(|i| i as u8)
                })
            }),
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
    /// 会话持久化
    pub snippets: Vec<String>,
    pub settings_open: bool,
    /// 库筛选弹窗(每连接勾选显示哪些库)
    pub db_filter: Option<DbFilterState>,
    /// 侧栏底部主题切换请求(绘制层无 ctx,经 update 消费)
    pub theme_toggle_requested: bool,
    pub session_dirty: bool,
    pub last_session_save: Option<std::time::Instant>,
    pub session_restored: bool,
    pub pending_online: Vec<String>,
    pub pending_expanded: HashMap<String, String>,
    pub last_session_sig: (usize, usize, bool, usize, Vec<Option<String>>),
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
        let accent = std::fs::read_to_string(config_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("accent").and_then(|a| a.as_u64()))
            .unwrap_or(0) as usize;
        crate::theme::set_accent(accent);
        crate::theme::set_dark(dark);
        crate::theme::apply(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        crate::force_appearance(dark);
        let snippets = Self::load_snippets(&config_dir);
        let mut app = Self {
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
            snippets,
            settings_open: false,
            db_filter: None,
            theme_toggle_requested: false,
            session_dirty: false,
            last_session_save: None,
            session_restored: false,
            pending_online: vec![],
            pending_expanded: HashMap::new(),
            last_session_sig: (0, 0, false, 0, vec![]),
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
        };
        app.restore_session();
        app
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
        let json =
            serde_json::json!({ "dark": dark, "accent": crate::theme::accent_idx() }).to_string();
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
            TabKind::Query(_)
            | TabKind::Designer(_)
            | TabKind::Redis(_)
            | TabKind::Er(_)
            | TabKind::Ddl(_) => return None,
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
                        baseline: vec![],
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
                        baseline: vec![],
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
        let table = d
            .original_table
            .clone()
            .unwrap_or_else(|| d.table_name.clone());
        let database = d.database.clone();
        d.loading = true;
        net::spawn(async move {
            let result = service
                .get_table_structure(&conn_id, &table, database)
                .await;
            let _ = tx.send(BgEvent::DesignerLoaded { tab_id, result });
        });
    }

    /// 打开对象 DDL 标签
    pub fn open_ddl(&mut self, conn_id: &str, database: Option<String>, kind: &str, name: &str) {
        let existing = self.tabs.iter().position(|t| {
            matches!(&t.kind, TabKind::Ddl(d)
                if d.conn_id == conn_id && d.name == name && d.kind == kind && d.database == database)
        });
        if let Some(i) = existing {
            self.active_tab = i;
            return;
        }
        let id = format!("o{}", rand_suffix());
        let title = format!("{name} DDL");
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Ddl(DdlTab {
                conn_id: conn_id.to_string(),
                database,
                name: name.to_string(),
                kind: kind.to_string(),
                ddl: None,
                loading: true,
                error: None,
            }),
        });
        self.active_tab = self.tabs.len() - 1;
        self.load_ddl(&id);
    }

    pub fn load_ddl(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Ddl(d) = &mut tab.kind else {
            return;
        };
        d.loading = true;
        d.error = None;
        let conn_id = d.conn_id.clone();
        let database = d.database.clone();
        let kind = d.kind.clone();
        let name = d.name.clone();
        net::spawn(async move {
            let result = service
                .get_object_ddl(&conn_id, database, &kind, &name)
                .await;
            let _ = tx.send(BgEvent::DdlLoaded { tab_id, result });
        });
    }

    /// 打开 ER 图标签(同连接同库已开则激活)
    pub fn open_er(&mut self, conn_id: &str, database: Option<String>) {
        let existing = self.tabs.iter().position(
            |t| matches!(&t.kind, TabKind::Er(e) if e.conn_id == conn_id && e.database == database),
        );
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
                fit_done: false,
                fit_tries: 0,
                dragging_node: None,
                refit: false,
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
            // 表/外键失败要报出来(此前 unwrap_or_default 吞掉,失败被当成"没有表")
            let tables = service
                .list_tables(&conn_id, database.clone())
                .await
                .map(|v| {
                    v.into_iter()
                        .filter(|t| t.kind == "table")
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                });
            let (tables, fks) = match (
                tables,
                service.list_foreign_keys(&conn_id, database.clone()).await,
            ) {
                (Ok(t), Ok(f)) => (t, f),
                (Err(e), _) | (_, Err(e)) => {
                    let _ = tx.send(BgEvent::ErLoaded {
                        tab_id,
                        result: Err(e),
                    });
                    return;
                }
            };
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
        let existing = self.tabs.iter().position(
            |t| matches!(&t.kind, TabKind::Redis(rt) if rt.conn_id == conn_id && rt.db == db),
        );
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
            let result = service
                .redis_scan(&conn_id, db, &pattern, cursor, 200)
                .await;
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
        // 切 key 清掉所有内联编辑残留(TTL/重命名/成员/list),否则草稿会作用到新 key 上
        rt.rename_draft = None;
        rt.member_add = None;
        rt.member_set_hash = false;
        rt.ttl_draft = None;
        rt.list_edit = None;
        rt.edit_value = None;
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
                let _ = tx.send(BgEvent::RedisTypesDone {
                    tab_id: tid.clone(),
                    pairs,
                });
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
        // 库上下文跟随当前激活标签(正在看 shop 库的表 → 新查询默认 shop)
        let inherit = conn_id.or_else(|| {
            self.tabs.get(self.active_tab).map(|t| match &t.kind {
                TabKind::Table(t) => t.conn_id.clone(),
                TabKind::Structure(s) => s.conn_id.clone(),
                TabKind::Query(q) => q.conn_id.clone(),
                TabKind::Designer(d) => d.conn_id.clone(),
                TabKind::Redis(r) => r.conn_id.clone(),
                TabKind::Er(e) => e.conn_id.clone(),
                TabKind::Ddl(d) => d.conn_id.clone(),
            })
        });
        let inherit_db = self.tabs.get(self.active_tab).and_then(|t| match &t.kind {
            TabKind::Table(t) => t.database.clone(),
            TabKind::Structure(s) => s.database.clone(),
            TabKind::Query(q) => q.database.clone(),
            TabKind::Designer(d) => d.database.clone(),
            TabKind::Redis(_) => None,
            TabKind::Er(e) => e.database.clone(),
            TabKind::Ddl(d) => d.database.clone(),
        });
        // 指定了连接(侧栏右键"新建查询")时,取侧栏展开的库;
        // 仅限无默认库的 MySQL 连接——有默认库的连接保持 None 走默认,
        // databases.first() 往往是 information_schema,不能当默认值
        let inherit_db = inherit_db.or_else(|| {
            let cid = inherit.as_ref()?;
            let no_default = self
                .conns
                .iter()
                .find(|c| &c.id == cid)
                .map(|c| {
                    c.db_type == shujing_core::DbType::MySql
                        && c
                            .database
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .is_none()
                })
                .unwrap_or(false);
            if !no_default {
                return None;
            }
            self.trees.get(cid)?.expanded_db.clone()
        });
        let id = format!("q{}", rand_suffix());
        self.query_seq += 1;
        let title = format!("查询 {}", self.query_seq);
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Query(QueryTab {
                conn_id: inherit.unwrap_or_default(),
                database: inherit_db,
                sql: String::new(),
                history: vec![],
                results: vec![],
                active_set: 0,
                running: false,
                error: None,
                optimize: None,
                result_table: None,
                result_db: None,
                result_pk: vec![],
                pk_checked: false,
                q_changes: vec![],
                q_editing: None,
                q_draft: String::new(),
            }),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    pub fn close_tab(&mut self, idx: usize) {
        // 未保存修改(表标签单元格改动/查询结果回写改动):先弹确认
        if !self.dirty_override && self.pending.is_none() {
            let dirty = matches!(self.tabs.get(idx), Some(t) if match &t.kind {
                TabKind::Table(tt) => !tt.changes.is_empty(),
                TabKind::Query(q) => !q.q_changes.is_empty(),
                _ => false,
            });
            if dirty {
                self.pending = Some(PendingAction::CloseTab(idx));
                return;
            }
        }
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            // 关闭活动标签之前的标签时,活动下标要跟着左移;否则会静默切到错误的标签
            if idx < self.active_tab {
                self.active_tab -= 1;
            }
            if self.tabs.is_empty() {
                self.active_tab = 0;
            } else if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }

    /// 关闭某个连接下的全部标签(断开/删除连接时用)
    /// 该连接名下是否有未保存修改的标签(断开前确认用)
    pub fn conn_has_dirty_tabs(&self, conn_id: &str) -> bool {
        self.tabs.iter().any(|t| match &t.kind {
            TabKind::Table(tt) => tt.conn_id == conn_id && !tt.changes.is_empty(),
            TabKind::Query(q) => q.conn_id == conn_id && !q.q_changes.is_empty(),
            _ => false,
        })
    }

    pub fn close_tabs_for_connection(&mut self, conn_id: &str) {
        let mut removed_before_active = 0usize;
        let mut i = 0usize;
        while i < self.tabs.len() {
            let belongs = match &self.tabs[i].kind {
                TabKind::Table(t) => t.conn_id == conn_id,
                TabKind::Structure(t) => t.conn_id == conn_id,
                TabKind::Query(q) => q.conn_id == conn_id,
                TabKind::Designer(d) => d.conn_id == conn_id,
                TabKind::Redis(r) => r.conn_id == conn_id,
                TabKind::Er(e) => e.conn_id == conn_id,
                TabKind::Ddl(d) => d.conn_id == conn_id,
            };
            if belongs {
                if i < self.active_tab {
                    removed_before_active += 1;
                }
                self.tabs.remove(i);
            } else {
                i += 1;
            }
        }
        self.active_tab = self
            .active_tab
            .saturating_sub(removed_before_active)
            .min(self.tabs.len().saturating_sub(1));
    }

    /// 拉取表数据标签的分页数据(LIMIT/OFFSET [+ ORDER BY]);首次同时拉主键
    pub fn load_rows(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let (mysql, conn_id) = match self.table_tab(&tab_id) {
            Some(t) => (mysql_quote(&self.conns, &t.conn_id), t.conn_id.clone()),
            None => return,
        };
        let Some(tab) = self.table_tab_mut(&tab_id) else {
            return;
        };
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
        let mut where_parts: Vec<String> = Vec::new();
        if let Some(q) = &tab.active_filter {
            where_parts.push(build_like_filter(&cols_snapshot, q, mysql));
        }
        if let Some(w) = &tab.applied_where {
            where_parts.push(w.clone());
        }
        let where_clause = (!where_parts.is_empty()).then(|| where_parts.join(" AND "));
        tab.load_seq = tab.load_seq.wrapping_add(1);
        let seq = tab.load_seq;
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
                seq,
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
            let _ = tx.send(BgEvent::CountLoaded { tab_id, seq, total });
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
        let db = q.database.clone();
        net::spawn(async move {
            let result = service.run_sql_db(&conn_id, sql, None, db.as_deref()).await;
            let _ = tx.send(BgEvent::QueryDone { tab_id, result });
        });
    }

    /// EXPLAIN 到结果区(不进历史)
    pub fn run_explain(&mut self, tab_id: &str) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Query(q) = &tab.kind else {
            return;
        };
        let sql = q.sql.trim().to_string();
        let head = sql.get(..8).unwrap_or("").to_uppercase();
        if sql.is_empty() || (!head.starts_with("SELECT") && !head.starts_with("WITH")) {
            self.toast_error("计划仅支持 SELECT 语句".to_string());
            return;
        }
        let Some(conn) = self.conns.iter().find(|c| c.id == q.conn_id) else {
            self.toast_error("请先选择连接".to_string());
            return;
        };
        let explain = shujing_core::sql_advisor::explain_sql(conn.db_type, &sql);
        let conn_id = q.conn_id.clone();
        let db = q.database.clone();
        let tid = tab_id.to_string();
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tid) {
            if let TabKind::Query(q) = &mut t.kind {
                q.running = true;
                q.error = None;
            }
        }
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = service
                .run_sql_db(&conn_id, explain, Some(200), db.as_deref())
                .await;
            let _ = tx.send(BgEvent::QueryDone {
                tab_id: tid,
                result,
            });
        });
    }

    /// 执行指定 SQL(⌘↵ 选区)
    pub fn run_query_sql(&mut self, tab_id: &str, sql: String) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let TabKind::Query(q) = &mut tab.kind else {
            return;
        };
        if q.running || sql.trim().is_empty() {
            return;
        }
        q.running = true;
        q.error = None;
        q.history.retain(|h| h != &sql);
        q.history.insert(0, sql.clone());
        q.history.truncate(20);
        let conn_id = q.conn_id.clone();
        let db = q.database.clone();
        net::spawn(async move {
            let result = service.run_sql_db(&conn_id, sql, None, db.as_deref()).await;
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

    /// 整库导出 SQL(结构+可选数据)
    pub fn export_database_sql_file(&mut self, conn_id: &str, db: &str, with_data: bool) {
        let default_name = format!("{db}_{}.sql", if with_data { "full" } else { "schema" });
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("SQL", &["sql"])
            .save_file()
        else {
            return;
        };
        let conn_id = conn_id.to_string();
        let db = db.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        crate::net::spawn(async move {
            let result = service
                .export_database_sql(&conn_id, Some(db.clone()), with_data)
                .await
                .and_then(|(sql, _)| {
                    std::fs::write(&path, sql).map_err(|e| format!("写入失败: {e}"))
                })
                .map(|_| path.display().to_string());
            let _ = tx.send(BgEvent::ExportDone { result });
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
        let existing = self
            .tabs
            .iter()
            .position(|t| matches!(&t.kind, TabKind::Query(q) if q.conn_id == conn_id));
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
        let database = Some(_db.to_string());
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        crate::net::spawn(async move {
            let result = service
                .export_table_sql(&conn_id, database, &table, true)
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
            action: None,
        });
    }

    // ---------------------------------------------------------------- 会话

    // ---------------------------------------------------------------- 库筛选

    fn db_filters_path(config_dir: &std::path::Path) -> std::path::PathBuf {
        config_dir.join("db_filters.json")
    }

    /// 读取持久化的库筛选(连接 id → 显示的库名列表)
    pub fn load_db_filters(config_dir: &std::path::Path) -> HashMap<String, Vec<String>> {
        std::fs::read_to_string(Self::db_filters_path(config_dir))
            .ok()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }

    fn save_db_filters(&self) {
        // 以当前各连接树上的筛选为准落盘
        let mut map: HashMap<String, Vec<String>> = Self::load_db_filters(&self.config_dir);
        for (id, tree) in &self.trees {
            if let Some(list) = &tree.visible_dbs {
                map.insert(id.clone(), list.clone());
            } else {
                map.remove(id);
            }
        }
        if let Ok(j) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::write(Self::db_filters_path(&self.config_dir), j);
        }
    }

    /// 连接成功时应用已持久化的库筛选
    pub fn apply_saved_db_filter(&mut self, conn_id: &str) {
        let saved = Self::load_db_filters(&self.config_dir);
        if let Some(list) = saved.get(conn_id) {
            if let Some(tree) = self.trees.get_mut(conn_id) {
                tree.visible_dbs = Some(list.clone());
            }
        }
    }

    /// 打开库筛选弹窗
    pub fn open_db_filter(&mut self, conn_id: &str) {
        let Some(tree) = self.trees.get(conn_id) else { return };
        if tree.databases.is_empty() {
            self.toast_error("没有可筛选的库(连接未就绪)".to_string());
            return;
        }
        let selected = tree.visible_dbs.clone().unwrap_or_else(|| tree.databases.clone());
        self.db_filter = Some(DbFilterState {
            conn_id: conn_id.to_string(),
            search: String::new(),
            all: tree.databases.clone(),
            selected,
        });
    }

    /// 应用筛选:写入树 + 落盘;若当前展开的库被隐藏则收起
    pub fn apply_db_filter(&mut self, conn_id: &str, selected: Vec<String>) {
        let expanded_hidden = self.trees.get(conn_id).and_then(|t| {
            t.expanded_db
                .as_ref()
                .map(|db| !selected.contains(db))
        });
        if let Some(tree) = self.trees.get_mut(conn_id) {
            tree.visible_dbs = Some(selected);
            if expanded_hidden == Some(true) {
                tree.expanded_db = None;
            }
        }
        self.save_db_filters();
    }

    fn snippets_path(config_dir: &std::path::Path) -> std::path::PathBuf {
        config_dir.join("snippets.json")
    }

    fn load_snippets(config_dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(Self::snippets_path(config_dir))
            .ok()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }

    fn save_snippets(&self) {
        if let Ok(j) = serde_json::to_string_pretty(&self.snippets) {
            let _ = std::fs::write(Self::snippets_path(&self.config_dir), j);
        }
    }

    pub fn snippet_save(&mut self, sql: String) {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return;
        }
        self.snippets.retain(|s| s != &sql);
        self.snippets.insert(0, sql);
        self.snippets.truncate(50);
        self.save_snippets();
    }

    pub fn snippet_delete(&mut self, idx: usize) {
        if idx < self.snippets.len() {
            self.snippets.remove(idx);
            self.save_snippets();
        }
    }

    pub fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
    }

    fn session_path(&self) -> std::path::PathBuf {
        self.config_dir.join("session.json")
    }

    pub fn save_session(&mut self) {
        let mut tabs = Vec::new();
        for t in &self.tabs {
            match &t.kind {
                TabKind::Table(x) => tabs.push(SessionTab::Table {
                    conn: x.conn_id.clone(),
                    table: x.table.clone(),
                    database: x.database.clone(),
                }),
                TabKind::Structure(x) => tabs.push(SessionTab::Structure {
                    conn: x.conn_id.clone(),
                    table: x.table.clone(),
                    database: x.database.clone(),
                }),
                TabKind::Query(q) => tabs.push(SessionTab::Query {
                    conn: q.conn_id.clone(),
                    sql: q.sql.clone(),
                    history: q.history.clone(),
                    database: q.database.clone(),
                }),
                TabKind::Designer(d) => tabs.push(SessionTab::Designer {
                    conn: d.conn_id.clone(),
                    database: d.database.clone(),
                    table: d.original_table.clone(),
                    table_name: d.table_name.clone(),
                }),
                TabKind::Redis(r) => tabs.push(SessionTab::Redis {
                    conn: r.conn_id.clone(),
                    db: r.db,
                }),
                TabKind::Er(e) => tabs.push(SessionTab::Er {
                    conn: e.conn_id.clone(),
                    database: e.database.clone(),
                }),
                TabKind::Ddl(d) => tabs.push(SessionTab::Ddl {
                    conn: d.conn_id.clone(),
                    database: d.database.clone(),
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                }),
            }
        }
        let data = SessionData {
            tabs,
            active: self.active_tab,
            online: self
                .trees
                .iter()
                .filter(|(_, t)| t.online)
                .map(|(id, _)| id.clone())
                .collect(),
            expanded: self
                .trees
                .iter()
                .filter_map(|(id, t)| t.expanded_db.clone().map(|db| (id.clone(), db)))
                .collect(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = std::fs::write(self.session_path(), json);
        }
        self.session_dirty = false;
        self.last_session_save = Some(std::time::Instant::now());
    }

    /// 距上次保存超过 1 秒且有变更时落盘(update 循环调用)
    pub fn save_session_if_due(&mut self) {
        if !self.session_dirty {
            return;
        }
        let due = self
            .last_session_save
            .map(|t| t.elapsed().as_secs_f32() > 1.0)
            .unwrap_or(true);
        if due {
            self.save_session();
        }
    }

    /// 启动时恢复会话(标签结构/在线连接/展开库);压测模式跳过
    pub fn restore_session(&mut self) -> bool {
        if std::env::var("SHUJING_AUTOOPEN").is_ok() {
            return false;
        }
        let Ok(json) = std::fs::read_to_string(self.session_path()) else {
            return false;
        };
        let Ok(data) = serde_json::from_str::<SessionData>(&json) else {
            return false;
        };
        let mut loaders: Vec<(String, String)> = Vec::new(); // (tab_id, kind)
        for st in &data.tabs {
            match st {
                SessionTab::Table {
                    conn,
                    table,
                    database,
                } => {
                    let tab = TableTab::new(conn.clone(), table.clone(), database.clone());
                    let id = format!("t{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: tab.table.clone(),
                        kind: TabKind::Table(tab),
                    });
                    loaders.push((id.clone(), "table".into()));
                }
                SessionTab::Structure {
                    conn,
                    table,
                    database,
                } => {
                    let id = format!("s{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: format!("{table} 结构"),
                        kind: TabKind::Structure(StructureTab {
                            conn_id: conn.clone(),
                            table: table.clone(),
                            database: database.clone(),
                            data: None,
                            loading: true,
                            error: None,
                        }),
                    });
                    loaders.push((id.clone(), "structure".into()));
                }
                SessionTab::Query {
                    conn,
                    sql,
                    history,
                    database,
                } => {
                    self.query_seq += 1;
                    let id = format!("q{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: format!("查询 {}", self.query_seq),
                        kind: TabKind::Query(QueryTab {
                            conn_id: conn.clone(),
                            database: database.clone(),
                            sql: sql.clone(),
                            history: history.clone(),
                            results: vec![],
                            active_set: 0,
                            running: false,
                            error: None,
                            optimize: None,
                            result_table: None,
                            result_db: None,
                            result_pk: vec![],
                            pk_checked: false,
                            q_changes: vec![],
                            q_editing: None,
                            q_draft: String::new(),
                        }),
                    });
                }
                SessionTab::Designer {
                    conn,
                    database,
                    table,
                    table_name,
                } => {
                    let id = format!("d{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: match table {
                            Some(t) => format!("设计 {t}"),
                            None => "新表".to_string(),
                        },
                        kind: TabKind::Designer(DesignerTab {
                            conn_id: conn.clone(),
                            database: database.clone(),
                            original_table: table.clone(),
                            table_name: table_name.clone(),
                            columns: if table.is_some() {
                                vec![]
                            } else {
                                vec![DesignerColumn {
                                    name: "id".into(),
                                    data_type: "INTEGER".into(),
                                    nullable: false,
                                    pk: true,
                                    auto_inc: true,
                                    ..Default::default()
                                }]
                            },
                            baseline: vec![],
                            loaded: table.is_none(),
                            loading: table.is_some(),
                            saving: false,
                            error: None,
                            info: None,
                        }),
                    });
                    if table.is_some() {
                        loaders.push((id.clone(), "designer".into()));
                    }
                }
                SessionTab::Redis { conn, db } => {
                    let id = format!("r{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: format!("db{db}"),
                        kind: TabKind::Redis(RedisTab {
                            conn_id: conn.clone(),
                            db: *db,
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
                    loaders.push((id.clone(), "redis".into()));
                }
                SessionTab::Er { conn, database } => {
                    let id = format!("e{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: "ER 图".to_string(),
                        kind: TabKind::Er(ErTab {
                            conn_id: conn.clone(),
                            database: database.clone(),
                            loading: true,
                            error: None,
                            tables: vec![],
                            structures: Default::default(),
                            fks: vec![],
                            zoom: 0.9,
                            pan: egui::vec2(60.0, 40.0),
                            positions: Default::default(),
                            fit_done: false,
                            fit_tries: 0,
                            dragging_node: None,
                            refit: false,
                        }),
                    });
                    loaders.push((id.clone(), "er".into()));
                }
                SessionTab::Ddl {
                    conn,
                    database,
                    name,
                    kind,
                } => {
                    let id = format!("o{}", rand_suffix());
                    self.tabs.push(Tab {
                        id: id.clone(),
                        title: format!("{name} DDL"),
                        kind: TabKind::Ddl(DdlTab {
                            conn_id: conn.clone(),
                            database: database.clone(),
                            name: name.clone(),
                            kind: kind.clone(),
                            ddl: None,
                            loading: true,
                            error: None,
                        }),
                    });
                    loaders.push((id.clone(), "ddl".into()));
                }
            }
        }
        // 触发各标签的数据加载
        for (id, kind) in loaders {
            match kind.as_str() {
                "table" => self.load_rows(&id),
                "structure" => self.load_structure(&id),
                "designer" => self.load_designer_structure(&id),
                "redis" => self.redis_scan(&id, true),
                "er" => self.load_er(&id),
                "ddl" => self.load_ddl(&id),
                _ => {}
            }
        }
        self.active_tab = data.active.min(self.tabs.len().saturating_sub(1));
        self.pending_online = data.online.clone();
        self.pending_expanded = data.expanded.clone();
        self.session_restored = !data.tabs.is_empty() || !data.online.is_empty();
        self.session_restored
    }

    /// 应用筛选栏(字段/自由模式)
    pub fn apply_filter_bar(&mut self, tab_id: &str) {
        let mysql = self
            .table_tab(tab_id)
            .map(|t| mysql_quote(&self.conns, &t.conn_id))
            .unwrap_or(true);
        let (conds, free) = {
            let Some(t) = self.table_tab(tab_id) else {
                return;
            };
            (t.conds.clone(), t.free_where.clone())
        };
        let built = build_cond_where(&conds, &free, mysql);
        if let Some(t) = self.table_tab_mut(tab_id) {
            t.filter_error = None;
            t.applied_where = built;
            t.page = 1;
        }
        self.load_rows(tab_id);
    }

    /// 清除筛选栏
    pub fn clear_filter_bar(&mut self, tab_id: &str) {
        if let Some(t) = self.table_tab_mut(tab_id) {
            t.applied_where = None;
            t.conds.clear();
            t.free_where.clear();
            t.filter_error = None;
            t.page = 1;
        }
        self.load_rows(tab_id);
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
        let db = q.database.clone();
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
                .run_sql_db(&conn_id, explain, Some(200), db.as_deref())
                .await
                .and_then(|mut rs| {
                    rs.truncate(1);
                    rs.pop().ok_or_else(|| "没有执行计划".to_string())
                });
            let _ = tx.send(BgEvent::OptimizeDone {
                tab_id: tid,
                result,
            });
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
                        // 用户已断开(树被移除)后迟到的 ConnectDone:丢弃,不复活在线状态
                        let Some(tree) = self.trees.get_mut(&conn_id) else {
                            continue;
                        };
                        tree.online = true;
                        tree.version = version;
                        self.apply_saved_db_filter(&conn_id);
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
                        // 会话恢复:优先展开上次使用的库(被筛选隐藏的库除外)
                        let visible = tree.visible_dbs.clone();
                        let shown = |db: &str| {
                            visible
                                .as_ref()
                                .map(|v| v.iter().any(|d| d == db))
                                .unwrap_or(true)
                        };
                        if let Some(db) = self.pending_expanded.remove(&conn_id) {
                            if tree.databases.contains(&db) && shown(&db) {
                                tree.expanded_db = Some(db);
                            }
                        }
                        if tree.expanded_db.is_none() {
                            if let Some(db) = tree.databases.iter().find(|d| shown(d)).cloned() {
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
                BgEvent::RowsLoaded {
                    tab_id,
                    seq,
                    result,
                } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        // 丢弃过期请求(快速翻页/排序时旧请求可能后到)
                        if t.load_seq != seq {
                            continue;
                        }
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
                BgEvent::CountLoaded { tab_id, seq, total } => {
                    let mut clamp = false;
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        if t.load_seq == seq {
                            t.total = total;
                            // 筛选/删行后总页数缩水:当前页超出末页时钳回并重载
                            if let Some(total) = total {
                                let max_page = (total.div_ceil(t.page_size as u64) as usize).max(1);
                                if t.page > max_page {
                                    t.page = max_page;
                                    clamp = true;
                                }
                            }
                        }
                    }
                    if clamp {
                        self.load_rows(&tab_id);
                    }
                }
                BgEvent::PkLoaded { tab_id, cols } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        t.pk_cols = cols;
                    }
                }
                BgEvent::ChangesSaved { tab_id, result } => match result {
                    Ok(n) => {
                        // 查询结果回写:清改动并重跑查询
                        let is_query = self
                            .tabs
                            .iter()
                            .any(|t| t.id == tab_id && matches!(t.kind, TabKind::Query(_)));
                        if is_query {
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                                if let TabKind::Query(q) = &mut tab.kind {
                                    q.q_changes.clear();
                                    q.q_editing = None;
                                }
                            }
                            self.toast_error(format!("已保存 {n} 处修改"));
                            self.run_query(&tab_id);
                        } else if let Some(t) = self.table_tab_mut(&tab_id) {
                            t.changes.clear();
                            self.load_rows(&tab_id);
                            self.toast_error(format!("已保存 {n} 处修改"));
                        }
                    }
                    Err(e) => self.toast_error(format!("保存失败: {e}")),
                },
                BgEvent::StructureLoaded { tab_id, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Structure(st) = &mut tab.kind {
                            st.loading = false;
                            match result {
                                Ok(data) => {
                                    st.data = Some(data);
                                    st.error = None;
                                }
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
                                    q.q_changes.clear();
                                    q.q_editing = None;
                                    let det = detect_single_table(&q.sql);
                                    q.result_table = det.as_ref().map(|(_, t)| t.clone());
                                    // SQL 里显式写了库就用它,否则用执行时选中的库
                                    q.result_db = det
                                        .and_then(|(d, _)| d)
                                        .or_else(|| q.database.clone());
                                    q.pk_checked = false;
                                    // 单表 SELECT:拉主键判断结果集可否回写
                                    if let Some(t) = q.result_table.clone() {
                                        let conn_id = q.conn_id.clone();
                                        let db = q.result_db.clone();
                                        let tid = tab_id.clone();
                                        let tx = self.event_tx.clone();
                                        let service = self.service.clone();
                                        net::spawn(async move {
                                            let cols = service
                                                .get_table_structure(&conn_id, &t, db)
                                                .await
                                                .map(|st| {
                                                    st.columns
                                                        .iter()
                                                        .filter(|c| c.key == "PRI")
                                                        .map(|c| c.name.clone())
                                                        .collect::<Vec<_>>()
                                                })
                                                .unwrap_or_default();
                                            let _ =
                                                tx.send(BgEvent::QueryPkDone { tab_id: tid, cols });
                                        });
                                    }
                                }
                                Err(e) => {
                                    let low = e.to_lowercase();
                                    if low.contains("interrupt")
                                        || low.contains("cancel")
                                        || low.contains("kill")
                                        || low.contains("中止")
                                    {
                                        q.error = Some("已取消".to_string());
                                    } else {
                                        q.error = Some(e);
                                    }
                                }
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
                        self.toast_error("创建成功".to_string()); // 重新载入所有在线树的表
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
                        continue;
                    };
                    let TabKind::Designer(d) = &mut tab.kind else {
                        continue;
                    };
                    d.loading = false;
                    match result {
                        Ok(st) => {
                            d.columns = shujing_core::designer::columns_from_structure(&st);
                            d.baseline = d.columns.clone();
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
                            continue;
                        };
                        let TabKind::Designer(d) = &mut tab.kind else {
                            continue;
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
                                continue;
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
                        let tables = service
                            .list_tables(&c2, db.clone())
                            .await
                            .unwrap_or_default();
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
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
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
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    rt.detail_loading = false;
                    // 期间切换了选中 key:丢弃过期结果
                    if rt.selected_key.as_deref() != Some(key.as_str()) {
                        continue;
                    }
                    match result {
                        Ok(d) => rt.detail = Some(d),
                        Err(e) => rt.error = Some(e),
                    }
                }
                BgEvent::RedisTypesDone { tab_id, pairs } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    for (k, t) in pairs {
                        rt.key_types.insert(k, t);
                    }
                }
                BgEvent::RedisTtlsDone { tab_id, pairs } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    for (k, t) in pairs {
                        rt.key_ttls.insert(k, t);
                    }
                }
                BgEvent::RedisOpDone { tab_id, op, result } => {
                    if let Err(e) = &result {
                        self.toast_error(format!("操作失败: {e}"));
                    }
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    rt.op_running = false;
                    if result.is_err() {
                        continue;
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
                BgEvent::DdlLoaded { tab_id, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Ddl(d) = &mut tab.kind {
                            d.loading = false;
                            match result {
                                Ok(ddl) => {
                                    d.ddl = Some(ddl);
                                    d.error = None;
                                }
                                Err(e) => d.error = Some(e),
                            }
                        }
                    }
                }
                BgEvent::ErLoaded { tab_id, result } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    let TabKind::Er(e) = &mut tab.kind else {
                        continue;
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
                        continue;
                    };
                    let TabKind::Query(q) = &mut tab.kind else {
                        continue;
                    };
                    let Some(o) = q.optimize.as_mut() else {
                        continue;
                    };
                    o.running = false;
                    o.ran = true;
                    match result {
                        Ok(plan) => o.plan = Some(plan),
                        Err(e) => o.error = Some(e),
                    }
                }
                BgEvent::QueryPkDone { tab_id, cols } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Query(q) = &mut tab.kind {
                            q.result_pk = cols;
                            q.pk_checked = true;
                        }
                    }
                }
                BgEvent::SearchDone { result } => {
                    let Some(gs) = self.global_search.as_mut() else {
                        continue;
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
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    let Some(a) = rt.analyze.as_mut() else {
                        continue;
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

/// 筛选栏条件 → WHERE 片段(数字裸值,其余引号转义;自由模式原文并入)
pub fn build_cond_where(
    conds: &[(String, String, String)],
    free: &str,
    mysql: bool,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for (col, op, val) in conds {
        let op = op.trim();
        if op.is_empty() {
            continue;
        }
        let ident = quote_ident_by(col, mysql);
        if op == "IS NULL" || op == "IS NOT NULL" {
            parts.push(format!("{ident} {op}"));
            continue;
        }
        let v = val.trim();
        if v.is_empty() {
            continue;
        }
        let literal = if v.parse::<f64>().is_ok() && op != "LIKE" {
            v.to_string()
        } else {
            // LIKE 语义 = 包含:两侧补通配符(用户自带 % 则原样保留)
            let wrapped = if op == "LIKE" && !v.contains('%') {
                format!("%{v}%")
            } else {
                v.to_string()
            };
            format!("'{}'", wrapped.replace('\'', "''"))
        };
        parts.push(format!("{ident} {op} {literal}"));
    }
    let free_t = free.trim();
    if !free_t.is_empty() {
        parts.push(free_t.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// 检测 SQL 是否为可回写的单表 SELECT,返回表名(取最后一段标识符)
pub fn detect_single_table(sql: &str) -> Option<(Option<String>, String)> {
    let lower = sql.to_lowercase();
    let head = lower.trim_start();
    if !head.starts_with("select") {
        return None;
    }
    for bad in [
        " join ",
        " union ",
        " group by ",
        " having ",
        " into ",
        "insert ",
        "update ",
        "delete ",
        ") select ",
        ")select",
    ] {
        if lower.contains(bad) {
            return None;
        }
    }
    // 子查询:select 出现两次以上即放弃
    if lower.matches("select").count() > 1 {
        return None;
    }
    let from_pos = lower.find(" from ")?;
    let rest = sql[from_pos + 6..].trim();
    // 取第一个标识符(容忍 `db`.`table` / "db"."table" / db.table,引号段可与点交错)
    let mut ident = String::new();
    let mut chars = rest.chars();
    loop {
        match chars.next() {
            Some(q @ ('`' | '"')) => {
                ident.push(q);
                for c2 in chars.by_ref() {
                    ident.push(c2);
                    if c2 == q {
                        break;
                    }
                }
            }
            Some(c) if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' => {
                ident.push(c);
            }
            _ => break,
        }
    }
    let cleaned: String = ident.chars().filter(|c| *c != '`' && *c != '"').collect();
    let parts: Vec<&str> = cleaned.split('.').filter(|s| !s.trim().is_empty()).collect();
    match parts.as_slice() {
        // 裸表名:不带库
        [t] => Some((None, t.trim().to_string())),
        // `db`.`table` / db.table / PG schema.table:显式库
        [d, t] => Some((Some(d.trim().to_string()), t.trim().to_string())),
        // 三段及以上(db.schema.table 等)过于歧义,放弃回写
        _ => None,
    }
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

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn detect_single_table_basic() {
        assert_eq!(
            detect_single_table("SELECT * FROM users WHERE id = 1"),
            Some((None, "users".into()))
        );
        assert_eq!(
            detect_single_table("SELECT `u`.`id` FROM `dramato`.`users` LIMIT 10"),
            Some((Some("dramato".into()), "users".into()))
        );
        assert_eq!(
            detect_single_table("SELECT * FROM shop.orders LIMIT 5"),
            Some((Some("shop".into()), "orders".into()))
        );
        assert_eq!(detect_single_table("SELECT a FROM t1 JOIN t2 ON 1"), None);
        assert_eq!(detect_single_table("SELECT (SELECT 1) FROM t"), None);
        assert_eq!(detect_single_table("UPDATE t SET a = 1"), None);
        assert_eq!(
            detect_single_table("select id, count(*) from orders group by id"),
            None
        );
    }

    #[test]
    fn cond_where_builder() {
        let conds = vec![
            ("age".to_string(), ">".to_string(), "18".to_string()),
            ("name".to_string(), "LIKE".to_string(), "张%".to_string()),
            ("del".to_string(), "IS NULL".to_string(), String::new()),
            ("empty".to_string(), "=".to_string(), String::new()),
        ];
        let w = build_cond_where(&conds, "", true).unwrap();
        assert!(w.contains("`age` > 18"), "{w}");
        assert!(w.contains("`name` LIKE '张%'"), "{w}");
        assert!(w.contains("`del` IS NULL"), "{w}");
        assert!(!w.contains("empty"), "{w}");
        // 自由 WHERE 并入
        let w2 = build_cond_where(&[], "status = 1 AND amount > 0", false).unwrap();
        assert_eq!(w2, "status = 1 AND amount > 0");
        assert!(build_cond_where(&[], "", true).is_none());
    }
}
