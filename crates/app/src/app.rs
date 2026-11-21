//! 数镜原生版界面:状态 + 异步事件消费 + 数据加载。

use crate::net;
use egui::Color32;
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
        /// 冻结的前 N 个可见列(旧档缺省 0)
        #[serde(default)]
        frozen: usize,
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
    UpdateAvailable { tag: String, url: String },
    RedisBatchDone {
        is_delete: bool,
        count: Option<u64>,
        error: Option<String>,
    },
    TransferDone {
        result: Result<(u64, u64), String>,
    },
    DataDiffDone {
        state: DataDiffState,
        result: Result<shujing_core::data_diff::DataDiff, String>,
    },
    MongoDocDone {
        op: u8, // 0 插入 1 替换 3 批量导入
        result: Result<String, String>,
    },
    KafkaTopicCreated {
        result: Result<(), String>,
    },
    KafkaTopicDeleted {
        result: Result<(), String>,
    },
    RedisInfoLoaded {
        result: Result<Vec<shujing_core::redis_ops::InfoSection>, String>,
    },
    RedisSlowlogLoaded {
        result: Result<Vec<shujing_core::redis_ops::SlowEntry>, String>,
    },
    SessionsLoaded {
        tab_id: String,
        result: Result<Vec<shujing_core::SessionInfo>, String>,
    },
    SessionKilled {
        tab_id: String,
        result: Result<(), String>,
    },
    KafkaTopicsLoaded {
        conn_id: String,
        result: Result<Vec<shujing_core::kafka::TopicInfo>, String>,
    },
    KafkaMsgsLoaded {
        tab_id: String,
        result: Result<Vec<shujing_core::kafka::StreamMsg>, String>,
    },
    KafkaSendDone {
        tab_id: String,
        result: Result<(i32, i64), String>,
    },
    KafkaGroupsLoaded {
        tab_id: String,
        result: Result<Vec<shujing_core::kafka::GroupInfo>, String>,
    },
    RedisStreamAddDone {
        tab_id: String,
        key: String,
        result: Result<String, String>,
    },
    RedisStreamGroupsDone {
        tab_id: String,
        result: Result<Vec<shujing_core::redis_ops::StreamGroup>, String>,
    },
    MongoIndexListDone {
        result: Result<Vec<shujing_core::mongo::IndexInfo>, String>,
    },
    MongoIndexOpDone {
        op: u8, // 4 创建 5 删除
        result: Result<String, String>,
    },
    AiDone {
        tab_id: String,
        mode: u8,
        result: Result<String, String>,
    },
    SchemaDiffDone {
        state: SchemaDiffState,
        result: Result<shujing_core::schema_diff::TableDiff, String>,
    },
    ParquetPreviewDone {
        path: String,
        result: Result<shujing_core::parquet_preview::ParquetPreview, String>,
    },
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
        result: Result<Vec<String>, String>,
    },
    ChangesSaved {
        tab_id: String,
        result: Result<u64, String>,
    },
    StructureLoaded {
        tab_id: String,
        result: Result<
            (
                shujing_core::TableStructure,
                Vec<shujing_core::schema::FkMeta>,
            ),
            String,
        >,
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
    /// 导入预览:文件头解析完成(后台,防大文件卡 UI)
    ImportParsed {
        result: Result<Vec<String>, String>,
    },
    /// CSV 导入:目标表列回填(带身份,快速换表打开导入窗时迟到结果不串表)
    ImportColsDone {
        conn_id: String,
        database: String,
        table: String,
        cols: Vec<String>,
    },
    DdlDone {
        result: Result<(), String>,
        /// 操作描述(用于成功提示;空=创建表路径)
        msg: String,
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
    /// Redis 详情追加页(加载更多)
    RedisDetailMoreDone {
        tab_id: String,
        key: String,
        /// 发起时的详情代次;刷新/切 key 后迟到的旧页直接丢弃
        gen: u64,
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
        /// 发起时的 (连接, 库, 关键词):换连接/改词后迟到的旧结果直接丢弃
        conn_id: String,
        db: Option<String>,
        keyword: String,
        result: Result<Vec<shujing_core::SearchHit>, String>,
    },
    /// 查询结果回写:主键列
    QueryPkDone {
        tab_id: String,
        cols: Vec<String>,
    },
    /// 自动补全:某表的列名(懒加载)
    AcColumnsDone {
        key: String,
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

/// 表统计弹窗键(带库:同连接不同库的同名表不串台)
pub fn stats_key(conn: &str, database: &str, table: &str) -> String {
    format!("{conn}/{database}/{table}")
}

/// CSV 导入状态
pub struct ImportState {
    pub conn_id: String,
    pub database: Option<String>,
    pub table: String,
    pub path: String,
    pub headers: Vec<String>,
    /// 导入进行中(防重复提交,完成前禁用按钮)
    pub running: bool,
    /// 源列 -> 目标表列索引(None=跳过)
    pub col_map: Vec<Option<usize>>,
    pub batch_errors: Option<String>,
    /// 目标表列名(打开导入时从表结构抓取)
    pub target_cols: Vec<String>,
    /// 预览解析在途(大文件后台解析,不卡 UI;完成前禁用读预览/开始导入)
    pub parsing: bool,
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
    /// 多选行(⌘/Shift 点行号;批量删除/复制)
    pub multi_sel: Vec<usize>,
    /// 冻结的前 N 个可见列(0=不冻结;横向滚动时钉在视口左侧)
    pub frozen_vis: usize,
    /// 矩形单元格选区(可视坐标 ((r1,c1),(r2,c2));拖选/Shift 点选;批量填充·复制·置 NULL)
    pub sel_range: Option<((usize, usize), (usize, usize))>,
    /// 正在拖选的锚点格(松开清除;None=无拖选)
    pub drag_sel: Option<(usize, usize)>,
    /// 行号列拖选的锚点行(行多选范围)
    pub drag_row: Option<usize>,
    /// 被编辑行的原始主键值(行号 → (列名, 值))。
    /// 内存行在提交时已被覆写为新值,保存定位(WHERE)必须用编辑前的主键,否则静默 0 行或错行
    pub orig_pks: std::collections::HashMap<usize, Vec<(String, String)>>,
    /// 数据代次:每次 RowsLoaded 替换行数据时递增。
    /// 删行确认弹窗烘入打开时的代次,确认时不符即放弃(防确认期间慢查询落地后按旧行号删错行)
    pub rows_gen: u64,
    /// 初始列宽缓存(列数变化才重算):启发式采样每帧重算是纯浪费,
    /// 且该值只作 Column::initial 的首帧值,行数据变化不影响
    pub base_widths: Vec<f32>,
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
            multi_sel: vec![],
            frozen_vis: 0,
            sel_range: None,
            drag_sel: None,
            drag_row: None,
            orig_pks: Default::default(),
            rows_gen: 0,
            base_widths: vec![],
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
        (
            ui_kit::t!("Row count", "行数").to_string(),
            crate::icons::thousands(total as u64),
        ),
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
            ui_kit::t!("Numeric count", "数值个数").to_string(),
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
            ui_kit::t!("Distinct non-null", "非空去重").to_string(),
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
    /// 本表外键(结构页 FK 区;load_structure 拉取后过滤本表)
    pub fks: Vec<shujing_core::schema::FkMeta>,
    pub loading: bool,
    pub error: Option<String>,
}

/// 查询标签
/// SQL 自动补全弹层状态(每帧由光标位置重建候选,selected 跨帧保留)
pub struct AcState {
    /// 当前触发词(空=无触发;含 "表." 前缀时补列名)
    pub word: String,
    pub selected: usize,
    /// Ctrl+Space 强制展开(空词也显示全部候选)
    pub forced: bool,
}

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
    /// q_changes 所属的结果集下标:多语句多结果集时行号不可跨集复用,
    /// 保存/编辑只对所属集生效(切到别的集时改动暂存不可保存,防错行 UPDATE)
    pub q_changes_set: usize,
    pub q_editing: Option<(usize, usize)>,
    pub q_draft: String,
    /// SQL 自动补全(None=关闭)
    pub ac: Option<AcState>,
    /// 结果网格冻结的前 N 列(0=不冻结)
    pub q_frozen: usize,
    /// 实时诊断缓存:(sql 哈希, findings)
    pub lint_cache: Option<(u64, Vec<shujing_core::sql_lint::Finding>)>,
}

/// 优化分析弹窗状态
pub struct OptimizeState {
    pub running: bool,
    pub ran: bool,
    pub plan: Option<shujing_core::ExecResult>,
    pub error: Option<String>,
    /// 执行计划网格的冻结列数(存状态里,局部变量会每帧重置)
    pub frozen: usize,
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
    /// 上次适配尝试时的视口尺寸:同尺寸仍放不下=内容超限,立即停止覆写
    pub last_fit_size: egui::Vec2,
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
    /// 危险 SQL 必须带库执行:run_sql 走粘滞会话库,多库连接下
    /// 不带库的 DROP/TRUNCATE 可能落到上次操作残留的库(删错库)
    pub database: Option<String>,
    /// 非 SQL 动作(如 delete-conn:<id>):确认后交给 app 处理而不执行 sql
    pub action: Option<String>,
    /// 保存前 SQL 预览:弹窗里以等宽滚动区展示将执行的语句,确认后才落库
    pub preview_sql: Option<String>,
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

/// 数据传输状态(源表=右键;目标同类型连接)
#[derive(Clone)]
pub struct TransferState {
    pub src_conn: String,
    pub src_db: Option<String>,
    pub src_table: String,
    pub dst_conn: String,
    pub dst_db: String,
    pub dst_table: String,
    pub with_data: bool,
    pub drop_first: bool,
    pub running: bool,
    pub error: Option<String>,
}

/// 数据对比状态(按主键逐行对齐;目标侧同步 SQL)
#[derive(Clone)]
pub struct DataDiffState {
    pub src_conn: String,
    pub src_db: Option<String>,
    pub src_table: String,
    pub dst_conn: String,
    pub dst_db: String,
    pub dst_table: String,
    pub running: bool,
    pub result: Option<shujing_core::data_diff::DataDiff>,
    pub error: Option<String>,
}

/// 连接备份口令弹窗(导出=加密生成文件;导入=解密合并)
pub struct ConnCryptoState {
    /// true=导出 / false=导入
    pub export: bool,
    pub passphrase: String,
    pub confirm: String,
    pub error: Option<String>,
}

/// Redis 批量删除状态(pattern + 计数确认)
pub struct RedisBatchDeleteState {
    pub conn_id: String,
    pub db: u8,
    pub pattern: String,
    /// SCAN 计数结果(Some=已计数,待确认删除)
    pub counted: Option<u64>,
    pub deleting: bool,
    pub error: Option<String>,
}

/// Mongo 文档编辑弹窗(插入/编辑;删除走确认框)
pub struct MongoDocState {
    pub conn_id: String,
    pub db: Option<String>,
    pub collection: String,
    /// None=插入;Some(id)=编辑该文档
    pub doc_id: Option<String>,
    pub json: String,
    pub running: bool,
    pub error: Option<String>,
}

/// Redis 服务器信息弹窗(INFO 区块 + 慢日志)
pub struct RedisInfoState {
    pub conn_id: String,
    pub sections: Vec<shujing_core::redis_ops::InfoSection>,
    pub slowlog: Vec<shujing_core::redis_ops::SlowEntry>,
    pub loading: bool,
    pub slowlog_loaded: bool,
    pub error: Option<String>,
}

/// 服务器会话面板标签(MySQL processlist / PG pg_stat_activity)
pub struct SessionsTab {
    pub conn_id: String,
    pub db_type: shujing_core::DbType,
    pub rows: Vec<shujing_core::SessionInfo>,
    pub loading: bool,
    pub error: Option<String>,
    /// 终止按钮 armed:已点过一次的会话 id
    pub armed: Option<String>,
}

impl SessionsTab {
    /// 用于 tabs 的独立实例(状态从零)
    pub fn clone_shell(&self, _new_id: &str) -> SessionsTab {
        SessionsTab {
            conn_id: self.conn_id.clone(),
            db_type: self.db_type,
            rows: Vec::new(),
            loading: false,
            error: None,
            armed: None,
        }
    }
}

/// Kafka 控制台标签(topic 浏览/消息读取/发送/消费组)
pub struct KafkaTab {
    pub conn_id: String,
    pub topics: Vec<String>,
    pub selected: Option<String>,
    pub msgs: Vec<shujing_core::kafka::StreamMsg>,
    pub loading: bool,
    pub error: Option<String>,
    pub send_key: String,
    pub send_payload: String,
    pub sending: bool,
    pub groups: Option<Vec<shujing_core::kafka::GroupInfo>>,
    pub groups_open: bool,
    pub frozen: usize,
    /// 新建 topic 表单打开态
    pub new_open: bool,
    pub new_name: String,
    pub new_parts: String,
    pub new_repl: String,
    /// 删除选中 topic 的二次确认
    pub drop_arm: bool,
    /// topic 操作在途
    pub topic_busy: bool,
}

impl KafkaTab {
    /// 生成用于 tabs 的独立实例(状态字段复制;消息列表从零拉取)
    pub fn clone_shell(&self, new_id: &str) -> KafkaTab {
        let _ = new_id;
        KafkaTab {
            conn_id: self.conn_id.clone(),
            topics: self.topics.clone(),
            selected: self.selected.clone(),
            msgs: Vec::new(),
            loading: false,
            error: None,
            send_key: String::new(),
            send_payload: String::new(),
            sending: false,
            groups: None,
            groups_open: false,
            frozen: 0,
            new_open: false,
            new_name: String::new(),
            new_parts: "1".into(),
            new_repl: "1".into(),
            drop_arm: false,
            topic_busy: false,
        }
    }
}

/// Mongo 索引管理弹窗(列表 + 内嵌创建表单)
pub struct MongoIndexState {
    pub conn_id: String,
    pub db: Option<String>,
    pub collection: String,
    pub items: Vec<shujing_core::mongo::IndexInfo>,
    pub running: bool,
    pub error: Option<String>,
    /// 创建表单:键行(字段, 降序?)
    pub rows: Vec<(String, bool)>,
    pub unique: bool,
    /// TTL 秒文本(空=不设)
    pub ttl: String,
    /// 自定义索引名(空=Mongo 自动生成)
    pub name: String,
    /// 创建/删除请求在途
    pub busy: bool,
    /// 已点过一次删除的索引名(二次点击确认;其余动作清空)
    pub drop_arm: Option<String>,
}

/// Mongo 批量导入弹窗(粘贴/文件读入;JSON 数组或 CSV,按来源扩展名分流)
pub struct MongoImportState {
    pub conn_id: String,
    pub db: Option<String>,
    pub collection: String,
    pub json: String,
    /// 来源是 .csv 文件(解析按 CSV;粘贴区恒为 JSON)
    pub src_csv: bool,
    pub running: bool,
    pub error: Option<String>,
}

/// AI 助手弹窗(挂查询标签;模式=生成/解释/优化)
pub struct AiAssistState {
    pub tab_id: String,
    /// 0 生成 / 1 解释 / 2 优化
    pub mode: u8,
    pub input: String,
    pub running: bool,
    /// (标题, 内容)历史,最新在后
    pub history: Vec<(String, String)>,
    pub error: Option<String>,
}

/// 结构对比状态(源=右键表;目标=任一连接的任一表)
#[derive(Clone)]
pub struct SchemaDiffState {
    pub src_conn: String,
    pub src_db: Option<String>,
    pub src_table: String,
    pub dst_conn: String,
    pub dst_db: String,
    pub dst_table: String,
    pub running: bool,
    pub result: Option<shujing_core::schema_diff::TableDiff>,
    pub error: Option<String>,
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
    /// 「加载更多」在途标志(无防抖会重复追加同页,后续 LSET 按下标错位写数据)
    pub detail_more_loading: bool,
    /// 详情代次:刷新/切 key 递增,作废在途的「加载更多」(迟到旧页不能 append 到新详情)
    pub detail_gen: u64,
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
    /// stream 发送草稿(每行 field=value)
    pub stream_add_text: String,
    /// stream 消费者组列表(None=未拉取)
    pub stream_groups: Option<Vec<shujing_core::redis_ops::StreamGroup>>,
    /// stream 操作在途
    pub stream_busy: bool,
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
    /// Kafka 控制台(不参与会话恢复:重启后从侧栏重开)
    Kafka(KafkaTab),
    /// 服务器会话面板(不参与会话恢复)
    Session(SessionsTab),
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
    /// SSH 隧道开关
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_password: String,
    /// 私钥路径(填写后优先于密码;password 此时作私钥口令)
    pub ssh_key_path: String,
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
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: String::new(),
            ssh_user: String::new(),
            ssh_password: String::new(),
            ssh_key_path: String::new(),
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
            ssh_enabled: c.ssh.is_some(),
            ssh_host: c.ssh.as_ref().map(|x| x.host.clone()).unwrap_or_default(),
            ssh_port: c
                .ssh
                .as_ref()
                .map(|x| x.port.to_string())
                .unwrap_or_default(),
            ssh_user: c.ssh.as_ref().map(|x| x.user.clone()).unwrap_or_default(),
            ssh_password: c
                .ssh
                .as_ref()
                .and_then(|x| x.password.clone())
                .unwrap_or_default(),
            ssh_key_path: c
                .ssh
                .as_ref()
                .and_then(|x| x.key_path.clone())
                .unwrap_or_default(),
            color_idx: c.color.as_ref().and_then(|hex| {
                let rgb = u32::from_str_radix(hex.trim_start_matches('#'), 16).ok();
                rgb.and_then(|v| {
                    let (r, g, b) = ((v >> 16) as u8, (v >> 8) as u8, v as u8);
                    crate::table_view::CONN_COLORS
                        .iter()
                        .position(|(name, c)| {
                            *c != Color32::TRANSPARENT
                                && c.r() == r
                                && c.g() == g
                                && c.b() == b
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
    /// 保存前预览的一次性直通(确认弹窗回执后直接执行,不再二次弹预览)
    pub skip_edit_preview: bool,
    /// 深色主题(持久化到 settings.json)
    pub dark_mode: bool,
    /// 跟随系统外观(手动切换主题会自动解除)
    pub follow_system_theme: bool,
    /// 帧计数(跟随系统外观的低频轮询用)
    pub frame_count: u64,
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
    /// SQL 自动补全的列名缓存(键 "conn|db|table",懒加载)
    pub ac_columns: std::collections::HashMap<String, Vec<String>>,
    /// ac_columns 变更版本号(写入递增):查询页补全快照缓存失效判定
    pub ac_rev: u64,
    /// 查询页补全快照缓存:(版本, 前缀键, 过滤结果)——免每帧 filter+深拷贝
    pub ac_cache: Option<(
        u64,
        String,
        std::sync::Arc<std::collections::HashMap<String, Vec<String>>>,
    )>,
    pub confirm: Option<ConfirmState>,
    /// 会话持久化
    pub snippets: std::sync::Arc<Vec<String>>,
    pub settings_open: bool,
    /// Parquet 文件预览(路径,解析结果)
    pub parquet_preview: Option<(String, shujing_core::parquet_preview::ParquetPreview)>,
    /// AI 助手配置(base_url/key/model,settings.json)
    pub ai_config: shujing_core::ai::AiConfig,
    /// AI 助手弹窗(挂查询标签)
    pub ai_assist: Option<AiAssistState>,
    /// Mongo 文档编辑弹窗
    pub mongo_doc: Option<MongoDocState>,
    pub mongo_indexes: Option<MongoIndexState>,
    pub kafka_tab: Option<KafkaTab>,
    pub session_tab: Option<SessionsTab>,
    pub redis_info: Option<RedisInfoState>,
    /// 查询结果存为新表:命名弹窗(tab_id, 表名草稿)
    pub save_result_modal: Option<(String, String)>,
    pub mongo_import: Option<MongoImportState>,
    /// 数据传输弹窗
    pub transfer: Option<TransferState>,
    /// 数据对比弹窗
    pub data_diff: Option<DataDiffState>,
    /// 连接备份口令弹窗
    pub conn_crypto: Option<ConnCryptoState>,
    /// 有新版本时的提示(版本号+下载链接)
    pub update_available: Option<(String, String)>,
    /// 待写盘路径(口令确认后用)
    pub pending_crypto_path: Option<String>,
    /// Redis 批量删除弹窗
    pub redis_batch_del: Option<RedisBatchDeleteState>,
    /// 结构对比(源表来自右键,目标可跨连接)
    pub schema_diff: Option<SchemaDiffState>,
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
        let mut dark = Self::load_dark_pref(&config_dir);
        let settings = std::fs::read_to_string(config_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let follow_system = settings
            .as_ref()
            .and_then(|v| v.get("followSystem").and_then(|f| f.as_bool()))
            .unwrap_or(false);
        if follow_system {
            dark = crate::system_prefers_dark();
        }
        let accent = settings
            .as_ref()
            .and_then(|v| v.get("accent").and_then(|a| a.as_u64()))
            .unwrap_or(0) as usize;
        let settings_ai = settings
            .as_ref()
            .and_then(|v| v.get("ai"))
            .cloned()
            .and_then(|v| serde_json::from_value::<shujing_core::ai::AiConfig>(v).ok());
        if settings
            .as_ref()
            .and_then(|v| v.get("english").and_then(|f| f.as_bool()))
            .unwrap_or(false)
        {
            ui_kit::i18n::set_english(true);
        }
        crate::theme::set_accent(accent);
        crate::theme::set_dark(dark);
        crate::theme::apply(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        crate::force_appearance(dark);
        let snippets = std::sync::Arc::new(Self::load_snippets(&config_dir));
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
            skip_edit_preview: false,
            quick_open: None,
            stats: None,
            import: None,
            new_table: None,
            global_search: None,
            ac_columns: Default::default(),
            ac_rev: 0,
            ac_cache: None,
            confirm: None,
            snippets,
            settings_open: false,
            parquet_preview: None,
            ai_config: settings_ai.unwrap_or_default(),
            ai_assist: None,
            mongo_doc: None,
            mongo_indexes: None,
            kafka_tab: None,
            session_tab: None,
            redis_info: None,
            save_result_modal: None,
            mongo_import: None,
            transfer: None,
            data_diff: None,
            conn_crypto: None,
            update_available: None,
            pending_crypto_path: None,
            redis_batch_del: None,
            schema_diff: None,
            db_filter: None,
            theme_toggle_requested: false,
            session_dirty: false,
            last_session_save: None,
            session_restored: false,
            pending_online: vec![],
            pending_expanded: HashMap::new(),
            last_session_sig: (0, 0, false, 0, vec![]),
            dark_mode: dark,
            follow_system_theme: follow_system,
            frame_count: 0,
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
        // 韩文(谚文):中文/日文字体都不含谚文,不挂则显示豆腐方框
        let hangul_ok = add(
            "hangul",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            0,
            cjk_tweak,
        );
        // emoji:系统 Apple Color Emoji 是彩色位图(SBIX),egui 光栅器不支持;
        // 内嵌 Noto Emoji 矢量(黑白,OFL 许可见 assets/fonts/NotoEmoji-OFL.txt)
        let emoji_bytes: &'static [u8] =
            include_bytes!("../../../assets/fonts/NotoEmoji.ttf");
        fonts.font_data.insert(
            "emoji".to_owned(),
            std::sync::Arc::new(egui::FontData {
                font: std::borrow::Cow::Borrowed(emoji_bytes),
                index: 0,
                tweak: Default::default(),
            }),
        );
        let emoji_ok = true;

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
            if hangul_ok {
                fam.push("hangul".to_owned());
            }
            if symbols_ok {
                fam.push("symbols".to_owned());
            }
            if emoji_ok {
                fam.push("emoji".to_owned());
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
            if hangul_ok {
                fam.push("hangul".to_owned());
            }
            if symbols_ok {
                fam.push("symbols".to_owned());
            }
            if emoji_ok {
                fam.push("emoji".to_owned());
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
        self.set_theme_dark_inner(ctx, dark);
        self.save_theme_settings();
    }

    pub fn set_theme_dark_inner(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark_mode = dark;
        crate::theme::set_dark(dark);
        crate::theme::apply(ctx);
        #[cfg(target_os = "macos")]
        crate::force_appearance(dark);
    }

    pub fn save_theme_settings(&self) {
        let path = self.config_dir.join("settings.json");
        let json = serde_json::json!({
            "dark": self.dark_mode,
            "accent": crate::theme::accent_idx(),
            "followSystem": self.follow_system_theme,
            "ai": &self.ai_config,
            "english": ui_kit::i18n::is_english(),
        });
        let _ = std::fs::write(path, json.to_string());
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

    /// 导出数据字典 Markdown(连接右键;弹保存框)
    pub fn export_dictionary_for(&mut self, conn_id: &str) {
        let Some(c) = self.conns.iter().find(|c| c.id == conn_id) else {
            return;
        };
        let database = c.database.clone();
        let default_name = format!(
            "数据字典-{}.md",
            database.as_deref().unwrap_or(&c.name)
        );
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Markdown", &["md"])
            .save_file()
        else {
            return;
        };
        let conn_id = conn_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = match service
                .export_dictionary(&conn_id, database.as_deref())
                .await
            {
                Ok(md) => match tokio::fs::write(&path, md).await {
                    Ok(_) => {
                        format!("{} {}", ui_kit::t!("Exported", "已导出到"), path.display())
                    }
                    Err(e) => format!("{}: {e}", ui_kit::t!("Write failed", "写入失败")),
                },
                Err(e) => e,
            };
            let _ = tx.send(BgEvent::ExportDone { result: Ok(result) });
        });
    }

    /// 确认「结果存为新表」:表名白名单校验后建表+写入
    pub fn save_result_as_table_commit(&mut self, tab_id: &str, name: &str) {
        let name = name.trim().to_string();
        let valid = !name.is_empty()
            && !name.chars().next().unwrap().is_ascii_digit()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
        if !valid {
            self.toast_error(
                ui_kit::t!(
                    "Table name: letters/digits/underscore only, must not start with a digit",
                    "表名仅限字母/数字/下划线,且不能以数字开头"
                )
                .to_string(),
            );
            return;
        }
        let Some((tid, _)) = self.save_result_modal.clone() else {
            return;
        };
        let Some(tab) = self.tabs.iter().find(|t| t.id == tid) else {
            return;
        };
        let crate::app::TabKind::Query(q) = &tab.kind else {
            return;
        };
        let Some(r) = q.results.get(q.active_set) else {
            return;
        };
        if r.columns.is_empty() || r.rows.is_empty() {
            self.toast_error(ui_kit::t!("Result set is empty", "结果集为空").to_string());
            return;
        }
        let conn_id = q.conn_id.clone();
        let database = q.database.clone();
        let columns = r.columns.clone();
        let rows = r.rows.clone();
        self.save_result_modal = None;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = service
                .save_result_as_table(&conn_id, database.as_deref(), &name, &columns, &rows)
                .await
                .map(|n| {
                    format!(
                        "{} {name} · {} {n} {}",
                        ui_kit::t!("Created table", "已建表"),
                        ui_kit::t!("inserted", "写入"),
                        ui_kit::t!("rows", "行")
                    )
                });
            let _ = tx.send(BgEvent::ExportDone { result });
        });
    }

    /// Mongo:导出集合文档为 JSON 文件(弹保存框,上限 5 万条)
    pub fn mongo_export_json(&mut self, tab_id: &str) {
        let Some(t) = self.table_tab(tab_id) else {
            return;
        };
        let default_name = format!("{}.json", t.table);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        let conn_id = t.conn_id.clone();
        let db = t.database.clone();
        let coll = t.table.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = match service
                .mongo_export_docs(&conn_id, db.as_deref(), &coll, 50_000)
                .await
            {
                Ok((json, truncated)) => {
                    let mut note = String::new();
                    if truncated {
                        note = if ui_kit::i18n::is_english() {
                            " (truncated at 50000)"
                        } else {
                            " (已达 5 万条上限截断)"
                        }
                        .to_string();
                    }
                    tokio::fs::write(&path, json)
                        .await
                        .map(|_| {
                            format!(
                                "{} {}{note}",
                                ui_kit::t!("Exported", "已导出到"),
                                path.display()
                            )
                        })
                        .unwrap_or_else(|e| {
                            if ui_kit::i18n::is_english() {
                                format!("Write failed: {e}")
                            } else {
                                format!("写入失败: {e}")
                            }
                        })
                }
                Err(e) => e,
            };
            let _ = tx.send(BgEvent::ExportDone { result: Ok(result) });
        });
    }

    /// Redis:向 stream 发送消息(草稿每行 field=value)
    pub fn redis_stream_add(&mut self, tab_id: &str, key: &str) {
        let Some(rt) = self.redis_tab_mut(tab_id) else {
            return;
        };
        if rt.stream_busy {
            return;
        }
        let fields: Vec<(String, String)> = rt
            .stream_add_text
            .lines()
            .filter_map(|l| l.split_once('=').map(|(f, v)| (f.trim().to_string(), v.to_string())))
            .filter(|(f, _)| !f.is_empty())
            .collect();
        if fields.is_empty() {
            self.toast_error(
                ui_kit::t!("Each line must be field=value", "每行一条 field=value")
                    .to_string(),
            );
            return;
        }
        let db = rt.db;
        let conn_id = rt.conn_id.clone();
        let key = key.to_string();
        let tid = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.redis_tab_mut(tab_id).unwrap().stream_busy = true;
        net::spawn(async move {
            let result = service
                .redis_stream_xadd(&conn_id, db, &key, &fields)
                .await;
            let _ = tx.send(BgEvent::RedisStreamAddDone {
                tab_id: tid,
                key,
                result,
            });
        });
    }


    /// 打开 Redis 服务器信息弹窗(INFO+慢日志)
    pub fn open_redis_info(&mut self, conn_id: &str) {
        self.redis_info = Some(RedisInfoState {
            conn_id: conn_id.to_string(),
            sections: Vec::new(),
            slowlog: Vec::new(),
            loading: true,
            slowlog_loaded: false,
            error: None,
        });
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let cid = conn_id.to_string();
        net::spawn(async move {
            let result = service.redis_server_info(&cid).await;
            let _ = tx.send(BgEvent::RedisInfoLoaded { result });
        });
    }

    /// 拉取慢日志(弹窗内按钮)
    pub fn redis_fetch_slowlog(&mut self) {
        let Some(st) = self.redis_info.as_ref() else {
            return;
        };
        let conn_id = st.conn_id.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = service.redis_slowlog(&conn_id, 20).await;
            let _ = tx.send(BgEvent::RedisSlowlogLoaded { result });
        });
    }

    /// 打开会话面板(存在则聚焦)
    pub fn open_sessions(&mut self, conn_id: &str) {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.kind, TabKind::Session(s) if s.conn_id == conn_id))
        {
            self.active_tab = i;
            return;
        }
        let Some(c) = self.conns.iter().find(|c| c.id == conn_id) else {
            return;
        };
        let db_type = c.db_type;
        let id = format!("s{}", rand_suffix());
        self.session_tab = Some(SessionsTab {
            conn_id: conn_id.to_string(),
            db_type,
            rows: Vec::new(),
            loading: false,
            error: None,
            armed: None,
        });
        self.tabs.push(Tab {
            id: id.clone(),
            title: ui_kit::t!("Sessions", "会话面板").to_string(),
            kind: TabKind::Session(
                self.session_tab.as_ref().expect("session_tab 未初始化").clone_shell(&id),
            ),
        });
        self.active_tab = self.tabs.len() - 1;
        self.open_sessions_refresh();
    }

    /// 重拉会话列表
    pub fn open_sessions_refresh(&mut self) {
        let Some(s) = self.session_tab.as_ref() else {
            return;
        };
        let conn_id = s.conn_id.clone();
        let tab_id = self
            .tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Session(_)))
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.session_tab.as_mut().unwrap().loading = true;
        net::spawn(async move {
            let result = service.list_sessions(&conn_id).await;
            let _ = tx.send(BgEvent::SessionsLoaded { tab_id, result });
        });
    }

    /// 终止会话(query_only=仅中断当前查询)
    pub fn session_kill(&mut self, sid: &str, query_only: bool) {
        let Some(s) = self.session_tab.as_ref() else {
            return;
        };
        let conn_id = s.conn_id.clone();
        let sid = sid.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = service.kill_session(&conn_id, &sid, query_only).await;
            let _ = tx.send(BgEvent::SessionKilled { tab_id: String::new(), result });
        });
    }

    /// 打开 Kafka 控制台(拉 topic 列表)
    pub fn open_kafka(&mut self, conn_id: &str) {
        // 已开则聚焦
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.kind, TabKind::Kafka(k) if k.conn_id == conn_id))
        {
            self.active_tab = i;
            return;
        }
        let id = format!("k{}", rand_suffix());
        self.kafka_tab = Some(KafkaTab {
            conn_id: conn_id.to_string(),
            topics: Vec::new(),
            selected: None,
            msgs: Vec::new(),
            loading: false,
            error: None,
            send_key: String::new(),
            send_payload: String::new(),
            sending: false,
            groups: None,
            groups_open: false,
            frozen: 0,
            new_open: false,
            new_name: String::new(),
            new_parts: "1".into(),
            new_repl: "1".into(),
            drop_arm: false,
            topic_busy: false,
        });
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let cid = conn_id.to_string();
        net::spawn(async move {
            let result = service.kafka_list_topics(&cid).await;
            let _ = tx.send(BgEvent::KafkaTopicsLoaded { conn_id: cid, result });
        });
        self.tabs.push(Tab {
            id: id.clone(),
            title: ui_kit::t!("Kafka", "Kafka 控制台").to_string(),
            kind: TabKind::Kafka(
                self.kafka_tab
                    .as_ref()
                    .expect("kafka_tab 未初始化")
                    .clone_shell(&id),
            ),
        });
        self.active_tab = self.tabs.len() - 1;
    }

    /// 重拉 topic 列表(建/删后)
    pub fn kafka_reload_topics(&mut self) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        let conn_id = k.conn_id.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().topic_busy = true;
        net::spawn(async move {
            let result = service.kafka_list_topics(&conn_id).await;
            let _ = tx.send(BgEvent::KafkaTopicsLoaded { conn_id, result });
        });
    }

    /// 确认新建 topic(表名校验)
    pub fn kafka_create_topic_commit(&mut self, name: &str, parts: &str, repl: &str) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        if k.topic_busy {
            return;
        }
        let name = name.trim().to_string();
        let valid = !name.is_empty()
            && !name.chars().next().unwrap().is_ascii_digit()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
        if !valid {
            self.toast_error(
                ui_kit::t!(
                    "Topic name: letters/digits/underscore/dash/dot only, must not start with a digit",
                    "topic 名仅限字母/数字/下划线/中划线/点,不能以数字开头"
                )
                .to_string(),
            );
            return;
        }
        let conn_id = k.conn_id.clone();
        let parts_n: i32 = parts.trim().parse().unwrap_or(1).max(1);
        let repl_n: i32 = repl.trim().parse().unwrap_or(1).max(1);
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().topic_busy = true;
        net::spawn(async move {
            let result = service
                .kafka_create_topic(&conn_id, &name, parts_n, repl_n)
                .await;
            let _ = tx.send(BgEvent::KafkaTopicCreated { result });
        });
    }

    /// 删除选中 topic(armed 二次确认后调用)
    pub fn kafka_delete_topic_commit(&mut self) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        if k.topic_busy {
            return;
        }
        let Some(topic) = k.selected.clone() else {
            return;
        };
        let conn_id = k.conn_id.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().topic_busy = true;
        net::spawn(async move {
            let result = service.kafka_delete_topic(&conn_id, &topic).await;
            let _ = tx.send(BgEvent::KafkaTopicDeleted { result });
        });
    }

    /// Kafka:从头读取选中 topic 的 200 条消息
    pub fn kafka_load_msgs(&mut self) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        if k.loading {
            return;
        }
        let Some(topic) = k.selected.clone() else {
            return;
        };
        let conn_id = k.conn_id.clone();
        let tab_id = self
            .tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Kafka(_)))
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().loading = true;
        net::spawn(async move {
            let result = service.kafka_consume_recent(&conn_id, &topic, 200, 5).await;
            let _ = tx.send(BgEvent::KafkaMsgsLoaded { tab_id, result });
        });
    }

    /// Kafka:发送一条消息(当前选中 topic)
    pub fn kafka_send(&mut self) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        if k.sending {
            return;
        }
        let Some(topic) = k.selected.clone() else {
            return;
        };
        if k.send_payload.trim().is_empty() {
            self.toast_error(ui_kit::t!("Payload is empty", "消息体为空").to_string());
            return;
        }
        let conn_id = k.conn_id.clone();
        let key = k.send_key.trim().to_string();
        let payload = k.send_payload.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().sending = true;
        net::spawn(async move {
            let result = service
                .kafka_produce(&conn_id, &topic, (!key.is_empty()).then_some(key.as_str()), &payload)
                .await;
            let _ = tx.send(BgEvent::KafkaSendDone {
                tab_id: String::new(),
                result,
            });
        });
    }

    /// Kafka:拉取消费组列表
    pub fn kafka_load_groups(&mut self) {
        let Some(k) = self.kafka_tab.as_ref() else {
            return;
        };
        if k.loading {
            return;
        }
        let conn_id = k.conn_id.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.kafka_tab.as_mut().unwrap().loading = true;
        net::spawn(async move {
            let result = service.kafka_groups(&conn_id).await;
            let _ = tx.send(BgEvent::KafkaGroupsLoaded {
                tab_id: String::new(),
                result,
            });
        });
    }

    /// Redis:拉取 stream 消费者组
    pub fn redis_stream_groups(&mut self, tab_id: &str, key: &str) {
        let Some(rt) = self.redis_tab_mut(tab_id) else {
            return;
        };
        if rt.stream_busy {
            return;
        }
        let db = rt.db;
        let conn_id = rt.conn_id.clone();
        let key = key.to_string();
        let tid = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.redis_tab_mut(tab_id).unwrap().stream_busy = true;
        net::spawn(async move {
            let result = service.redis_stream_groups(&conn_id, db, &key).await;
            let _ = tx.send(BgEvent::RedisStreamGroupsDone { tab_id: tid, result });
        });
    }

    /// 重载当前激活标签的 Mongo 数据页(文档增删改/导入后)
    fn reload_active_mongo_rows(&mut self) {
        if let Some(tid) = self.tabs.get(self.active_tab).map(|t| t.id.clone()) {
            if let Some(t) = self.table_tab(&tid) {
                if self
                    .conns
                    .iter()
                    .any(|c| c.id == t.conn_id && c.db_type == shujing_core::DbType::Mongo)
                {
                    self.load_rows(&tid);
                }
            }
        }
    }

    pub fn redis_tab_mut(&mut self, tab_id: &str) -> Option<&mut RedisTab> {
        self.tabs
            .iter_mut()
            .find(|t| t.id == tab_id)
            .and_then(|t| match &mut t.kind {
                TabKind::Redis(rt) => Some(rt),
                _ => None,
            })
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
            TabKind::Session(s) => (s.conn_id.as_str(), "", None),
            TabKind::Kafka(k) => (k.conn_id.as_str(), "", None),
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
        let title = format!("{table} {}", ui_kit::t!("Structure", "结构"));
        self.tabs.push(Tab {
            id: id.clone(),
            title,
            kind: TabKind::Structure(StructureTab {
                conn_id: conn_id.to_string(),
                table: table.to_string(),
                database,
                data: None,
                fks: vec![],
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
                let title = format!("{} {t}", ui_kit::t!("Design", "设计"));
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
                    title: ui_kit::t!("New Table", "新表").to_string(),
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
            title: ui_kit::t!("ER Diagram", "ER 图").to_string(),
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
                        last_fit_size: egui::Vec2::ZERO,
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
            detail_more_loading: false,
            detail_gen: 0,
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
                stream_add_text: String::new(),
                stream_groups: None,
                stream_busy: false,
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
    /// Redis 详情"加载更多":追加后续一页成员
    pub fn redis_fetch_detail_more(&mut self, tab_id: &str, key: &str) {
        let tab_id = tab_id.to_string();
        let key = key.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) else {
            return;
        };
        let crate::app::TabKind::Redis(rt) = &tab.kind else {
            return;
        };
        let (conn_id, db, offset, gen) = (
            rt.conn_id.clone(),
            rt.db,
            rt.detail.as_ref().map(|d| d.pairs.len()).unwrap_or(0),
            rt.detail_gen,
        );
        if offset == 0 {
            return;
        }
        if rt.detail_more_loading {
            return; // 在途防抖:重复请求会带同一 offset,追加两份相同页
        }
        drop(tab);
        if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if let crate::app::TabKind::Redis(rt) = &mut t.kind {
                rt.detail_more_loading = true;
            }
        }
        let tid = tab_id.clone();
        let k = key.to_string();
        net::spawn(async move {
            let result = service
                .redis_key_detail_more(&conn_id, db, &k, offset)
                .await;
            let _ = tx.send(BgEvent::RedisDetailMoreDone {
                tab_id: tid,
                key,
                gen,
                result,
            });
        });
    }

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
        rt.detail_gen = rt.detail_gen.wrapping_add(1); // 作废在途「加载更多」
        rt.detail_more_loading = false;
        // 切 key 清掉内联编辑残留,否则草稿会作用到新 key 上。
        // 同 key 的刷新(写操作完成后的自动 Refresh)不清键级草稿(TTL/重命名/值编辑),
        // 否则正在输入的内容会被迟到刷新无声吞掉;行级草稿绑定行下标,刷新必清
        let key_changed = rt.selected_key.as_deref() != Some(key.as_str());
        if key_changed {
            rt.rename_draft = None;
            rt.ttl_draft = None;
            rt.edit_value = None;
        }
        rt.member_add = None;
        rt.member_set_hash = false;
        rt.list_edit = None;
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

    /// SCAN 后批量拉类型 + TTL 徽标(增量:只拉还没有类型的 key,
    /// "继续扫描"累计超过首批后新增 key 也能拿到徽标)
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
        let keys: Vec<String> = rt
            .keys
            .iter()
            .filter(|k| !rt.key_types.contains_key(*k))
            .take(200)
            .cloned()
            .collect();
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
                TabKind::Kafka(k) => k.conn_id.clone(),
                TabKind::Session(s) => s.conn_id.clone(),
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
            TabKind::Kafka(_) => None,
            TabKind::Session(_) => None,
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
        let title = format!("{} {}", ui_kit::t!("Query", "查询"), self.query_seq);
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
                q_changes_set: 0,
                q_editing: None,
                q_draft: String::new(),
                ac: None,
                q_frozen: 0,
                lint_cache: None,
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
                TabKind::Kafka(k) => k.conn_id == conn_id,
                TabKind::Session(s) => s.conn_id == conn_id,
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
        let is_mongo = self
            .conns
            .iter()
            .any(|c| c.id == conn_id && c.db_type == shujing_core::DbType::Mongo);
        if is_mongo {
            self.load_rows_mongo(&tab_id);
            return;
        }
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
        tab.orig_pks.clear();
        tab.editing = None;
        tab.multi_sel.clear();
        tab.sel_range = None;
        tab.drag_sel = None;
        tab.drag_row = None;
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
                let result = service2
                    .get_table_structure(&conn2, &table2, db2)
                    .await
                    .map(|s| {
                        s.columns
                            .iter()
                            .filter(|c| c.key == "PRI")
                            .map(|c| c.name.clone())
                            .collect()
                    });
                let _ = tx2.send(BgEvent::PkLoaded { tab_id: id2, result });
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

    /// Mongo 数据页加载:find(filter) 采样 + count(filter),无 SQL
    fn load_rows_mongo(&mut self, tab_id: &str) {
        let tab_id = tab_id.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let Some(tab) = self.table_tab_mut(&tab_id) else { return };
        let conn_id = tab.conn_id.clone();
        let database = tab.database.clone();
        let table = tab.table.clone();
        let (limit, offset) = (tab.page_size, (tab.page - 1) * tab.page_size);
        // 过滤:active_filter 须是 JSON 对象(Mongo 查询语法);SQL 片段拒绝
        let filter: Option<String> = tab.active_filter.as_ref().and_then(|q| {
            let t = q.trim();
            if t.starts_with('{') && serde_json::from_str::<serde_json::Value>(t).is_ok() {
                Some(t.to_string())
            } else {
                None
            }
        });
        let has_applied_sql = tab.applied_where.is_some();
        tab.load_seq = tab.load_seq.wrapping_add(1);
        let seq = tab.load_seq;
        tab.loading = true;
        tab.changes.clear();
        tab.orig_pks.clear();
        tab.editing = None;
        tab.multi_sel.clear();
        tab.sel_range = None;
        tab.drag_sel = None;
        tab.drag_row = None;
        net::spawn(async move {
            if has_applied_sql {
                let _ = tx.send(BgEvent::RowsLoaded {
                    tab_id: tab_id.clone(),
                    seq,
                    result: Err(
                        ui_kit::t!(
                            "Mongo only supports JSON filters: enter e.g. {\"age\": {\"$gt\": 18}} in the filter box",
                            "Mongo 仅支持 JSON 过滤:在筛选框输入如 {\"age\": {\"$gt\": 18}}"
                        )
                            .into(),
                    ),
                });
                return;
            }
            let result = service
                .mongo_sample(
                    &conn_id,
                    database.as_deref(),
                    &table,
                    filter.as_deref(),
                    limit,
                    offset as u64,
                )
                .await
                .map(|(columns, rows)| shujing_core::ExecResult {
                    columns,
                    rows,
                    affected: 0,
                    truncated: false,
                    elapsed_ms: 0,
                });
            let _ = tx.send(BgEvent::RowsLoaded {
                tab_id: tab_id.clone(),
                seq,
                result,
            });
            let total = service
                .mongo_count(&conn_id, database.as_deref(), &table)
                .await
                .ok();
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
                .get_table_structure(&conn_id, &table, database.clone())
                .await;
            // FK 过滤本表(失败不阻断结构展示)
            let fks = service
                .list_foreign_keys(&conn_id, database.clone())
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|f| f.table == table)
                .collect::<Vec<_>>();
            let _ = tx.send(BgEvent::StructureLoaded {
                tab_id,
                result: result.map(|d| (d, fks)),
            });
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
            q.error = Some(ui_kit::t!("Select a target connection in the toolbar first", "请先在工具栏选择目标连接").into());
            return;
        }
        let sql = q.sql.trim().to_string();
        if sql.is_empty() {
            q.error = Some(ui_kit::t!("SQL is empty", "SQL 为空").into());
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
            self.toast_error(ui_kit::t!("EXPLAIN only supports SELECT statements", "计划仅支持 SELECT 语句").to_string());
            return;
        }
        let Some(conn) = self.conns.iter().find(|c| c.id == q.conn_id) else {
            self.toast_error(ui_kit::t!("Select a connection first", "请先选择连接").to_string());
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
            q.error = Some(ui_kit::t!("Select a target connection in the toolbar first", "请先在工具栏选择目标连接").into());
            return;
        }
        let cmd = q.sql.trim().to_string();
        if cmd.is_empty() {
            q.error = Some(ui_kit::t!("Command is empty", "命令为空").into());
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
                    columns: vec![ui_kit::t!("Result", "结果").to_string()],
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
            // 写盘走 tokio::fs:数百 MB 的 SQL 导出不再阻塞仅有的 async worker
            let result = match service
                .export_database_sql(&conn_id, Some(db.clone()), with_data)
                .await
            {
                Ok((sql, _)) => tokio::fs::write(&path, sql)
                    .await
                    .map(|_| path.display().to_string())
                    .map_err(|e| format!("{}: {e}", ui_kit::t!("Write failed", "写入失败"))),
                Err(e) => Err(e),
            };
            let _ = tx.send(BgEvent::ExportDone { result });
        });
    }

    /// 导出连接(加密备份):选路径 → 口令弹窗 → AES-GCM 写文件
    pub fn export_connections_encrypted(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("shujing-connections.sjenc")
            .add_filter(ui_kit::t!("Shujing encrypted backup", "数镜加密备份"), &["sjenc"])
            .save_file()
        else {
            return;
        };
        self.conn_crypto = Some(ConnCryptoState {
            export: true,
            passphrase: String::new(),
            confirm: String::new(),
            error: None,
        });
        self.pending_crypto_path = Some(path.display().to_string());
    }

    /// 导入连接(加密备份):选路径 → 口令弹窗 → 解密合并
    pub fn import_connections_encrypted(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ui_kit::t!("Shujing encrypted backup", "数镜加密备份"), &["sjenc"])
            .pick_file()
        else {
            return;
        };
        self.conn_crypto = Some(ConnCryptoState {
            export: false,
            passphrase: String::new(),
            confirm: String::new(),
            error: None,
        });
        self.pending_crypto_path = Some(path.display().to_string());
    }

    /// 口令确认后执行导出/导入
    pub fn run_conn_crypto(&mut self) {
        let Some(st) = self.conn_crypto.as_mut() else { return };
        if st.passphrase.chars().count() < 8 {
            st.error = Some(ui_kit::t!("Passphrase must be at least 8 characters", "口令至少 8 个字符").into());
            return;
        }
        if st.export && st.passphrase != st.confirm {
            st.error = Some(ui_kit::t!("Passphrases do not match", "两次口令不一致").into());
            return;
        }
        let Some(path) = self.pending_crypto_path.clone() else { return };
        let export = st.export;
        let pass = st.passphrase.clone();
        let result = if export {
            let do_export = || -> Result<String, String> {
                let conns = serde_json::to_string_pretty(&self.service.list_saved_connections())
                    .map_err(|e| e.to_string())?;
                let enc = shujing_core::config_crypto::encrypt(&conns, &pass)?;
                std::fs::write(&path, enc).map_err(|e| format!("{}: {e}", ui_kit::t!("Write failed", "写入失败")))?;
                Ok(if ui_kit::i18n::is_english() {
                    format!(
                        "Exported {} connections (passwords included; the passphrase is the key)",
                        self.service.list_saved_connections().len()
                    )
                } else {
                    format!(
                        "已导出 {} 条连接(含密码,口令即钥匙)",
                        self.service.list_saved_connections().len()
                    )
                })
            };
            do_export()
        } else {
            // 非 Result 返回函数:内部错误用 let 链收集
            let mut do_import = || -> Result<usize, String> {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", ui_kit::t!("Read failed", "读取失败")))?;
                let json = shujing_core::config_crypto::decrypt(&text, &pass)?;
                let imported: Vec<shujing_core::ConnInfo> = serde_json::from_str(&json)
                    .map_err(|e| format!("{}: {e}", ui_kit::t!("Failed to parse backup", "备份内容解析失败")))?;
                let existing = self.service.list_saved_connections();
                let mut new_conns = Vec::new();
                for c in &imported {
                    if !existing
                        .iter()
                        .any(|e| e.db_type == c.db_type && e.name == c.name)
                    {
                        new_conns.push(c.clone());
                    }
                }
                let n = new_conns.len();
                for c in new_conns {
                    self.service.save_connection(c)?;
                }
                self.conns = self.service.list_saved_connections();
                Ok(n)
            };
            match do_import() {
                Ok(n) => Ok(if ui_kit::i18n::is_english() {
                    format!("Imported {n} connections (passwords are included in the backup)")
                } else {
                    format!("已导入 {n} 条连接(密码含在备份里)")
                }),
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(msg) => {
                self.conn_crypto = None;
                self.pending_crypto_path = None;
                self.toast_error(msg);
            }
            Err(e) => {
                if let Some(st) = self.conn_crypto.as_mut() {
                    st.error = Some(e);
                }
            }
        }
    }

    /// Redis 批量删除弹窗
    pub fn open_redis_batch_delete(&mut self, tab_id: &str) {
        if let Some(t) = self.tabs.iter().find(|t| t.id == tab_id) {
            if let TabKind::Redis(rt) = &t.kind {
                self.redis_batch_del = Some(RedisBatchDeleteState {
                    conn_id: rt.conn_id.clone(),
                    db: rt.db,
                    pattern: rt.pattern.clone(),
                    counted: None,
                    deleting: false,
                    error: None,
                });
            }
        }
    }

    /// 计数(SCAN-only,不删)
    pub fn run_redis_batch_count(&mut self) {
        let Some(st) = self.redis_batch_del.as_ref() else { return };
        if st.pattern.trim().is_empty() {
            return;
        }
        let conn_id = st.conn_id.clone();
        let db = st.db;
        let pattern = st.pattern.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.redis_batch_del.as_mut().unwrap().error = None;
        net::spawn(async move {
            let r = service.redis_scan_count(&conn_id, db, &pattern).await;
            let (count, error) = match r {
                Ok(n) => (Some(n), None),
                Err(e) => (None, Some(e)),
            };
            let _ = tx.send(BgEvent::RedisBatchDone {
                is_delete: false,
                count,
                error,
            });
        });
    }

    /// 执行批量删除
    pub fn run_redis_batch_delete(&mut self) {
        let Some(st) = self.redis_batch_del.as_ref() else { return };
        let conn_id = st.conn_id.clone();
        let db = st.db;
        let pattern = st.pattern.clone();
        self.redis_batch_del.as_mut().unwrap().deleting = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let r = service
                .redis_delete_by_pattern(&conn_id, db, &pattern)
                .await;
            let (count, error) = match r {
                Ok(n) => (Some(n), None),
                Err(e) => (None, Some(e)),
            };
            let _ = tx.send(BgEvent::RedisBatchDone {
                is_delete: true,
                count,
                error,
            });
        });
    }

    /// 打开数据传输(源=右键表;目标默认同类型在线连接)
    pub fn open_transfer(&mut self, conn_id: &str, db: Option<&str>, table: &str) {
        let src_info = self.conns.iter().find(|c| c.id == conn_id).cloned();
        let Some(src_info) = src_info else { return };
        let dst_conn = self
            .conns
            .iter()
            .find(|c| c.id != conn_id && c.db_type == src_info.db_type)
            .map(|c| c.id.clone());
        self.transfer = Some(TransferState {
            src_conn: conn_id.to_string(),
            src_db: db.map(str::to_string),
            src_table: table.to_string(),
            dst_conn: dst_conn.unwrap_or_default(),
            dst_db: db.unwrap_or_default().to_string(),
            dst_table: table.to_string(),
            with_data: true,
            drop_first: false,
            running: false,
            error: None,
        });
    }

    /// 运行数据传输
    pub fn run_transfer(&mut self) {
        let Some(st) = self.transfer.as_ref() else { return };
        if st.running || st.dst_conn.is_empty() || st.dst_table.trim().is_empty() {
            return;
        }
        let state = st.clone();
        self.transfer.as_mut().unwrap().running = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = shujing_core::transfer::transfer_table(
                &service,
                (&state.src_conn, state.src_db.as_deref(), &state.src_table),
                (
                    &state.dst_conn,
                    if state.dst_db.is_empty() { None } else { Some(state.dst_db.as_str()) },
                    &state.dst_table,
                ),
                state.with_data,
                state.drop_first,
            )
            .await;
            let _ = tx.send(BgEvent::TransferDone { result });
        });
    }

    /// 打开数据对比(源=右键表)
    pub fn open_data_diff(&mut self, conn_id: &str, db: Option<&str>, table: &str) {
        let src_info = self.conns.iter().find(|c| c.id == conn_id).cloned();
        let Some(src_info) = src_info else { return };
        let dst_conn = self
            .conns
            .iter()
            .find(|c| c.id != conn_id && c.db_type == src_info.db_type)
            .map(|c| c.id.clone());
        self.data_diff = Some(DataDiffState {
            src_conn: conn_id.to_string(),
            src_db: db.map(str::to_string),
            src_table: table.to_string(),
            dst_conn: dst_conn.unwrap_or_default(),
            dst_db: db.unwrap_or_default().to_string(),
            dst_table: table.to_string(),
            running: false,
            result: None,
            error: None,
        });
    }

    /// 运行数据对比
    pub fn run_data_diff(&mut self) {
        let Some(st) = self.data_diff.as_ref() else { return };
        if st.running || st.dst_conn.is_empty() || st.dst_table.trim().is_empty() {
            return;
        }
        let state = st.clone();
        self.data_diff.as_mut().unwrap().running = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = shujing_core::data_diff::diff_data(
                &service,
                (&state.src_conn, state.src_db.as_deref(), &state.src_table),
                (
                    &state.dst_conn,
                    if state.dst_db.is_empty() { None } else { Some(state.dst_db.as_str()) },
                    &state.dst_table,
                ),
                5000,
            )
            .await;
            let _ = tx.send(BgEvent::DataDiffDone { state, result });
        });
    }

    /// 打开 Mongo 文档编辑(插入)
    pub fn open_mongo_insert(&mut self, tab_id: &str) {
        if let Some(t) = self.table_tab(&tab_id) {
            self.mongo_doc = Some(MongoDocState {
                conn_id: t.conn_id.clone(),
                db: t.database.clone(),
                collection: t.table.clone(),
                doc_id: None,
                json: "{\n  \"\": null\n}".into(),
                running: false,
                error: None,
            });
        }
    }

    /// 打开 Mongo 文档编辑(编辑选中行;取该行 _id)
    pub fn open_mongo_edit(&mut self, tab_id: &str) {
        let Some(t) = self.table_tab(&tab_id) else { return };
        let Some((ri, _)) = t.selected else {
            self.toast_error(ui_kit::t!("Select a document row first", "先点选一行文档").to_string());
            return;
        };
        let Some(id_col) = t.columns.iter().position(|c| c == "_id") else {
            self.toast_error(ui_kit::t!("No _id column in the result set", "结果集中没有 _id 列").to_string());
            return;
        };
        let Some(id) = t.rows.get(ri).and_then(|r| r.get(id_col)).cloned().flatten() else {
            self.toast_error(ui_kit::t!("The _id of this row is empty and cannot be edited", "该行 _id 为空,无法编辑").to_string());
            return;
        };
        let conn_id = t.conn_id.clone();
        let db = t.database.clone();
        let coll = t.table.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let id2 = id.clone();
        self.mongo_doc = Some(MongoDocState {
            conn_id: conn_id.clone(),
            db: db.clone(),
            collection: coll.clone(),
            doc_id: Some(id),
            json: String::new(),
            running: true,
            error: None,
        });
        net::spawn(async move {
            let result = service.mongo_get_doc(&conn_id, db.as_deref(), &coll, &id2).await;
            let _ = tx.send(BgEvent::MongoDocDone { op: 9, result });
        });
    }

    /// Mongo 文档删除(确认框回执)
    pub fn run_mongo_delete(&mut self, tab_id: &str, doc_id: &str) {
        let Some(t) = self.table_tab(&tab_id) else { return };
        let conn_id = t.conn_id.clone();
        let db = t.database.clone();
        let coll = t.table.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        let id = doc_id.to_string();
        net::spawn(async move {
            let result = service
                .mongo_delete_doc(&conn_id, db.as_deref(), &coll, &id)
                .await
                .map(|_| format!("{} {id}", ui_kit::t!("Deleted document", "已删除文档")));
            let _ = tx.send(BgEvent::MongoDocDone { op: 2, result });
        });
    }

    /// 提交 Mongo 文档插入/替换
    pub fn run_mongo_save(&mut self) {
        let Some(st) = self.mongo_doc.as_ref() else { return };
        let conn_id = st.conn_id.clone();
        let db = st.db.clone();
        let coll = st.collection.clone();
        let doc_id = st.doc_id.clone();
        let json = st.json.clone();
        let running = st.running;
        drop(st);
        if running {
            return;
        }
        self.mongo_doc.as_mut().unwrap().running = true;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let op = if doc_id.is_some() { 1u8 } else { 0u8 };
            let result = if let Some(id) = doc_id {
                service
                    .mongo_replace_doc(&conn_id, db.as_deref(), &coll, &id, &json)
                    .await
                    .map(|_| format!("{} {id}", ui_kit::t!("Updated document", "已更新文档")))
            } else {
                service
                    .mongo_insert_doc(&conn_id, db.as_deref(), &coll, &json)
                    .await
            };
            let _ = tx.send(BgEvent::MongoDocDone { op, result });
        });
    }

    /// 打开 Mongo 索引管理(立即拉列表)
    pub fn open_mongo_indexes(&mut self, tab_id: &str) {
        if let Some(t) = self.table_tab(&tab_id) {
            self.mongo_indexes = Some(MongoIndexState {
                conn_id: t.conn_id.clone(),
                db: t.database.clone(),
                collection: t.table.clone(),
                items: Vec::new(),
                running: true,
                error: None,
                rows: vec![(String::new(), false)],
                unique: false,
                ttl: String::new(),
                name: String::new(),
                busy: false,
                drop_arm: None,
            });
            self.refresh_mongo_indexes();
        }
    }

    /// 重新拉取索引列表(用当前弹窗的连接上下文)
    fn refresh_mongo_indexes(&mut self) {
        let Some(st) = self.mongo_indexes.as_ref() else { return };
        let conn_id = st.conn_id.clone();
        let db = st.db.clone();
        let coll = st.collection.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.mongo_indexes.as_mut().unwrap().running = true;
        net::spawn(async move {
            let result = service.mongo_list_indexes(&conn_id, db.as_deref(), &coll).await;
            let _ = tx.send(BgEvent::MongoIndexListDone { result });
        });
    }

    /// 提交创建索引(表单校验在弹窗层做,这里只管发请求)
    pub fn run_mongo_create_index(&mut self) {
        let Some(st) = self.mongo_indexes.as_ref() else { return };
        if st.busy || st.running {
            return;
        }
        // 去掉空字段行后交给 core(空列表由 core 报错)
        let fields: Vec<(String, bool)> = st
            .rows
            .iter()
            .filter(|(f, _)| !f.trim().is_empty())
            .cloned()
            .collect();
        let unique = st.unique;
        let ttl = st
            .ttl
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|_| !st.ttl.trim().is_empty());
        let name = st.name.trim().to_string();
        let conn_id = st.conn_id.clone();
        let db = st.db.clone();
        let coll = st.collection.clone();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.mongo_indexes.as_mut().unwrap().busy = true;
        self.mongo_indexes.as_mut().unwrap().drop_arm = None;
        net::spawn(async move {
            let result = service
                .mongo_create_index(
                    &conn_id,
                    db.as_deref(),
                    &coll,
                    &fields,
                    unique,
                    ttl,
                    (!name.is_empty()).then_some(name.as_str()),
                )
                .await;
            let _ = tx.send(BgEvent::MongoIndexOpDone { op: 4, result });
        });
    }

    /// 删除索引(弹窗层二次确认后调用)
    pub fn run_mongo_drop_index(&mut self, name: &str) {
        let Some(st) = self.mongo_indexes.as_ref() else { return };
        if st.busy {
            return;
        }
        let conn_id = st.conn_id.clone();
        let db = st.db.clone();
        let coll = st.collection.clone();
        let idx = name.to_string();
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.mongo_indexes.as_mut().unwrap().busy = true;
        self.mongo_indexes.as_mut().unwrap().drop_arm = None;
        net::spawn(async move {
            let result = service
                .mongo_drop_index(&conn_id, db.as_deref(), &coll, &idx)
                .await
                .map(|_| format!("{} {idx}", ui_kit::t!("Index dropped", "已删除索引")));
            let _ = tx.send(BgEvent::MongoIndexOpDone { op: 5, result });
        });
    }

    /// 打开 Mongo 批量导入
    pub fn open_mongo_import(&mut self, tab_id: &str) {
        if let Some(t) = self.table_tab(&tab_id) {
            self.mongo_import = Some(MongoImportState {
                conn_id: t.conn_id.clone(),
                db: t.database.clone(),
                collection: t.table.clone(),
                json: String::new(),
                src_csv: false,
                running: false,
                error: None,
            });
        }
    }

    /// 执行批量导入:先本地解析(零副作用),通过才发请求
    pub fn run_mongo_import(&mut self) {
        let Some(st) = self.mongo_import.as_ref() else { return };
        if st.running {
            return;
        }
        if !st.src_csv {
            if let Err(e) = shujing_core::mongo::parse_import_docs(&st.json) {
                self.mongo_import.as_mut().unwrap().error = Some(e);
                return;
            }
        } else if let Err(e) = shujing_core::mongo::csv_to_docs(&st.json) {
            self.mongo_import.as_mut().unwrap().error = Some(e);
            return;
        }
        let conn_id = st.conn_id.clone();
        let db = st.db.clone();
        let coll = st.collection.clone();
        let payload = st.json.clone();
        let src_csv = st.src_csv;
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        self.mongo_import.as_mut().unwrap().running = true;
        self.mongo_import.as_mut().unwrap().error = None;
        net::spawn(async move {
            let result = (if src_csv {
                service
                    .mongo_import_docs_csv(&conn_id, db.as_deref(), &coll, &payload)
                    .await
            } else {
                service
                    .mongo_import_docs(&conn_id, db.as_deref(), &coll, &payload)
                    .await
            })
            .map(|n| {
                format!(
                    "{} {n} {}",
                    ui_kit::t!("Imported", "已导入"),
                    ui_kit::t!("documents", "条文档")
                )
            });
            let _ = tx.send(BgEvent::MongoDocDone { op: 3, result });
        });
    }

    /// 打开 AI 助手(mode:0 生成/1 解释/2 优化)
    pub fn open_ai_assist(&mut self, tab_id: &str, mode: u8) {
        let init_input = if mode == 0 {
            String::new()
        } else {
            self.table_of_query(tab_id).unwrap_or_default()
        };
        self.ai_assist = Some(AiAssistState {
            tab_id: tab_id.to_string(),
            mode,
            input: init_input,
            running: false,
            history: Vec::new(),
            error: None,
        });
    }

    /// 查询标签当前 SQL(解释/优化的默认对象)
    fn table_of_query(&self, tab_id: &str) -> Option<String> {
        self.tabs.iter().find(|t| t.id == tab_id).and_then(|t| match &t.kind {
            TabKind::Query(q) => Some(q.sql.trim().to_string()),
            _ => None,
        })
    }

    /// 运行 AI 请求(组装库结构上下文后调 core)
    pub fn run_ai_assist(&mut self) {
        let Some(st) = self.ai_assist.as_ref() else { return };
        if st.running || st.input.trim().is_empty() {
            return;
        }
        let Some(tab) = self.tabs.iter().find(|t| t.id == st.tab_id) else { return };
        let TabKind::Query(q) = &tab.kind else { return };
        let conn_id = q.conn_id.clone();
        let (dialect, conn_db) = self
            .conns
            .iter()
            .find(|c| c.id == conn_id)
            .map(|c| (format!("{:?}", c.db_type), c.database.clone()))
            .unwrap_or(("MySQL".into(), None));
        let cfg = self.ai_config.clone();
        let mode = st.mode;
        let input = st.input.trim().to_string();
        // 库结构上下文:侧栏已加载的表名(轻量;不逐表拉列——太慢)
        let tables: Vec<String> = self
            .trees
            .get(&conn_id)
            .map(|t| {
                t.tables_by_db
                    .get(conn_db.as_deref().unwrap_or(""))
                    .map(|ts| {
                        ts.iter()
                            .filter(|m| m.kind == "table")
                            .map(|m| m.name.clone())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let schema_ctx = if tables.is_empty() {
            ui_kit::t!(
                "(table list not loaded; expand a database in the sidebar first)",
                "(表列表未加载,请先在侧栏展开库)"
            )
            .to_string()
        } else {
            format!("{}: {}", ui_kit::t!("Tables", "表"), tables.join(", "))
        };
        let tid = st.tab_id.clone();
        drop(st);
        self.ai_assist.as_mut().unwrap().running = true;
        let tx = self.event_tx.clone();
        net::spawn(async move {
            let result = match mode {
                0 => shujing_core::ai::generate_sql(&cfg, &dialect, &schema_ctx, &input).await,
                1 => shujing_core::ai::explain_sql(&cfg, &dialect, &schema_ctx, &input).await,
                _ => shujing_core::ai::optimize_sql(&cfg, &dialect, &schema_ctx, &input).await,
            };
            let _ = tx.send(BgEvent::AiDone {
                tab_id: tid,
                mode,
                result,
            });
        });
    }

    /// 打开结构对比弹窗(源=右键表;目标预填同一连接/库/表)
    pub fn open_schema_diff(&mut self, conn_id: &str, db: Option<&str>, table: &str) {
        self.schema_diff = Some(SchemaDiffState {
            src_conn: conn_id.to_string(),
            src_db: db.map(str::to_string),
            src_table: table.to_string(),
            dst_conn: conn_id.to_string(),
            dst_db: db.unwrap_or_default().to_string(),
            dst_table: table.to_string(),
            running: false,
            result: None,
            error: None,
        });
    }

    /// 运行结构对比(异步)
    pub fn run_schema_diff(&mut self) {
        let Some(st) = self.schema_diff.as_ref() else { return };
        let state = SchemaDiffState {
            running: true,
            result: None,
            error: None,
            ..st.clone()
        };
        self.schema_diff = Some(state.clone());
        let tx = self.event_tx.clone();
        let service = self.service.clone();
        net::spawn(async move {
            let result = shujing_core::schema_diff::diff_tables(
                &service,
                (&state.src_conn, state.src_db.as_deref(), &state.src_table),
                (&state.dst_conn, if state.dst_db.is_empty() { None } else { Some(state.dst_db.as_str()) }, &state.dst_table),
            )
            .await;
            let _ = tx.send(BgEvent::SchemaDiffDone { state, result });
        });
    }

    /// Parquet 文件预览:选文件 → blocking 池解析前 200 行 → 弹窗展示
    pub fn preview_parquet_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Parquet", &["parquet"])
            .pick_file()
        else {
            return;
        };
        let path_s = path.display().to_string();
        let tx = self.event_tx.clone();
        let p = path.clone();
        net::spawn_blocking(move || {
            let result = shujing_core::parquet_preview::preview_data_file(&p, 200);
            let _ = tx.send(BgEvent::ParquetPreviewDone {
                path: path_s,
                result,
            });
        });
    }

    /// 导入连接配置(DBeaver DataSources.json / Navicat .ncx)
    pub fn import_connections_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ui_kit::t!("Connection export files", "连接导出文件"), &["json", "ncx"])
            .pick_file()
        else {
            return;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            self.toast_error(format!("{}: {}", ui_kit::t!("Read failed", "读取失败"), path.display()));
            return;
        };
        let imported = match shujing_core::conn_import::import_connections(&content) {
            Ok(v) => v,
            Err(e) => {
                self.toast_error(e);
                return;
            }
        };
        let existing = self.service.list_saved_connections();
        let (added, skipped) =
            shujing_core::conn_import::merge_into(&existing, imported.clone());
        let mut new_conns: Vec<shujing_core::ConnInfo> = Vec::new();
        for c in &imported {
            let dup = existing
                .iter()
                .any(|e| e.db_type == c.db_type && e.name == c.name);
            if !dup {
                new_conns.push(c.clone());
            }
        }
        for c in new_conns {
            if let Err(e) = self.service.save_connection(c) {
                self.toast_error(format!(
                    "{}: {e}",
                    ui_kit::t!("Failed to save imported connections", "保存导入连接失败")
                ));
                return;
            }
        }
        // 侧栏连接列表重载
        self.conns = self.service.list_saved_connections();
        if added > 0 {
            self.toast_error(if ui_kit::i18n::is_english() {
                format!(
                    "Imported {added} connections (passwords cannot be migrated, please re-enter); skipped {skipped} duplicates"
                )
            } else {
                format!(
                    "已导入 {added} 条连接(密码无法迁移,请重新输入);跳过 {skipped} 条重复"
                )
            });
        } else {
            self.toast_error(if ui_kit::i18n::is_english() {
                format!("No new connections ({skipped} duplicates skipped)")
            } else {
                format!("没有新增连接(跳过 {skipped} 条重复)")
            });
        }
        let _ = skipped; // 上文已用
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
            self.toast_error(format!("{}: {}", ui_kit::t!("Read failed", "读取失败"), path.display()));
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
                Ok(n) => {
                    if ui_kit::i18n::is_english() {
                        format!("Imported {fname} · {n} rows affected")
                    } else {
                        format!("已导入 {fname} · 影响 {n} 行")
                    }
                }
                Err(_) => format!("{} {fname}", ui_kit::t!("Imported", "已导入")),
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
            // 写盘走 tokio::fs:数百 MB 的 SQL 导出不再阻塞仅有的 async worker
            let result = match service.export_table_sql(&conn_id, database, &table, true).await {
                Ok((sql, _)) => tokio::fs::write(&path, sql)
                    .await
                    .map(|_| path.display().to_string())
                    .map_err(|e| format!("{}: {e}", ui_kit::t!("Write failed", "写入失败"))),
                Err(e) => Err(e),
            };
            let _ = tx.send(BgEvent::ExportDone { result });
        });
    }

    /// 打开危险操作确认弹窗
    pub fn open_confirm(
        &mut self,
        conn_id: &str,
        db: Option<&str>,
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
            database: db.map(str::to_string),
            action: None,
            preview_sql: None,
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
            self.toast_error(ui_kit::t!("No databases to filter (connection not ready)", "没有可筛选的库(连接未就绪)").to_string());
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
        if let Ok(j) = serde_json::to_string_pretty(&*self.snippets) {
            let _ = std::fs::write(Self::snippets_path(&self.config_dir), j);
        }
    }

    pub fn snippet_save(&mut self, sql: String) {
        let sql = sql.trim().to_string();
        if sql.is_empty() {
            return;
        }
        std::sync::Arc::make_mut(&mut self.snippets).retain(|s| s != &sql);
        {
            let v = std::sync::Arc::make_mut(&mut self.snippets);
            v.insert(0, sql);
            v.truncate(50);
        }
        self.save_snippets();
    }

    pub fn snippet_delete(&mut self, idx: usize) {
        if idx < self.snippets.len() {
            std::sync::Arc::make_mut(&mut self.snippets).remove(idx);
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
                    frozen: x.frozen_vis,
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
                // Kafka/会话面板不入会话(重启后从侧栏重开)
                TabKind::Kafka(_) => {}
                TabKind::Session(_) => {}
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
                    frozen,
                } => {
                    let mut tab = TableTab::new(conn.clone(), table.clone(), database.clone());
                    tab.frozen_vis = *frozen;
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
                        title: format!("{table} {}", ui_kit::t!("Structure", "结构")),
                        kind: TabKind::Structure(StructureTab {
                            conn_id: conn.clone(),
                            table: table.clone(),
                            database: database.clone(),
                            data: None,
                            fks: vec![],
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
                        title: format!("{} {}", ui_kit::t!("Query", "查询"), self.query_seq),
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
                q_changes_set: 0,
                            q_editing: None,
                            q_draft: String::new(),
                            ac: None,
                            q_frozen: 0,
                            lint_cache: None,
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
                            Some(t) => format!("{} {t}", ui_kit::t!("Design", "设计")),
                            None => ui_kit::t!("New Table", "新表").to_string(),
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
                            stream_add_text: String::new(),
                            stream_groups: None,
                            stream_busy: false,
                            conn_id: conn.clone(),
                            db: *db,
                            pattern: "*".to_string(),
                            cursor: 0,
                            keys: vec![],
                            selected_key: None,
                            detail: None,
                            detail_loading: false,
            detail_more_loading: false,
            detail_gen: 0,
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
                        title: ui_kit::t!("ER Diagram", "ER 图").to_string(),
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
                        last_fit_size: egui::Vec2::ZERO,
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
            self.toast_error(ui_kit::t!("No SQL in the current query tab", "当前查询页没有 SQL").to_string());
            return;
        }
        if !head.starts_with("SELECT") && !head.starts_with("WITH") {
            self.toast_error(ui_kit::t!("Optimize analysis only supports SELECT statements", "优化分析仅支持 SELECT 语句").to_string());
            return;
        }
        let Some(conn) = self.conns.iter().find(|c| c.id == q.conn_id) else {
            self.toast_error(ui_kit::t!("Select a connection first", "请先选择连接").to_string());
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
                    frozen: 0,
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
                    rs.pop().ok_or_else(|| ui_kit::t!("No execution plan", "没有执行计划").to_string())
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
            name: ui_kit::t!("Local", "本地").into(),
            db_type: shujing_core::DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(db),
            color: None,
            read_only: false,
            ssh: None,
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
                        // 启动竞态自愈:会话恢复的表标签可能在连接完成前就发起加载,
                        // 失败后错误一直挂着("连接未建立或已断开")。连接建立即自动重载
                        let stale: Vec<String> = self
                            .tabs
                            .iter()
                            .filter(|t| match &t.kind {
                                TabKind::Table(tb) => {
                                    tb.conn_id == conn_id
                                        && tb.changes.is_empty()
                                        && (tb.error.is_some() || tb.columns.is_empty())
                                }
                                _ => false,
                            })
                            .map(|t| t.id.clone())
                            .collect();
                        for tid in stale {
                            self.load_rows(&tid);
                        }
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
                        self.toast_error(format!("{}: {e}", ui_kit::t!("Connection Failed", "连接失败")));
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
                        t.sel_range = None;
                        t.drag_sel = None;
                        t.drag_row = None;
                        t.multi_sel.clear(); // 在途时用户可能 ⌘ 点选了旧页行,保留会指向新页错行
                        t.orig_pks.clear();
                        match result {
                            Ok(r) => {
                                t.last_elapsed_ms = Some(r.elapsed_ms);
                                t.columns = r.columns;
                                t.rows = r.rows;
                                t.rows_gen = t.rows_gen.wrapping_add(1);
                                t.error = None; // 成功加载清掉陈旧错误(如启动竞态期的"连接未建立")
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
                BgEvent::PkLoaded { tab_id, result } => {
                    if let Some(t) = self.table_tab_mut(&tab_id) {
                        match result {
                            Ok(cols) => t.pk_cols = cols,
                            Err(_) => {
                                // 拉取失败不等于"没有主键":复位标记,下次刷新重试(否则标签永久只读)
                                t.pk_loaded = false;
                            }
                        }
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
                                    q.q_changes_set = 0;
                                    q.q_editing = None;
                                }
                            }
                            self.toast_error(if ui_kit::i18n::is_english() {
                                format!("Saved {n} changes")
                            } else {
                                format!("已保存 {n} 处修改")
                            });
                            self.run_query(&tab_id);
                        } else if let Some(t) = self.table_tab_mut(&tab_id) {
                            t.changes.clear();
                            self.load_rows(&tab_id);
                            self.toast_error(if ui_kit::i18n::is_english() {
                                format!("Saved {n} changes")
                            } else {
                                format!("已保存 {n} 处修改")
                            });
                        }
                    }
                    Err(e) => {
                        // 保存失败要复位 loading(删行/保存路径会置 true,不复位则刷新按钮永久失效)
                        if let Some(t) = self.table_tab_mut(&tab_id) {
                            t.loading = false;
                        }
                        self.toast_error(format!("{}: {e}", ui_kit::t!("Save failed", "保存失败")));
                    }
                },
                BgEvent::StructureLoaded { tab_id, result } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Structure(st) = &mut tab.kind {
                            st.loading = false;
                            match result {
                                Ok((data, fks)) => {
                                    st.data = Some(data);
                                    st.fks = fks;
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
                                    q.q_changes_set = 0;
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
                                        q.error = Some(ui_kit::t!("Cancelled", "已取消").to_string());
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
                BgEvent::UpdateAvailable { tag, url } => {
                    self.update_available = Some((tag, url));
                    continue;
                }
                BgEvent::RedisBatchDone {
                    is_delete,
                    count,
                    error,
                } => {
                    let Some(st) = self.redis_batch_del.as_mut() else { continue };
                    if let Some(e) = error {
                        st.deleting = false;
                        st.error = Some(e);
                        continue;
                    }
                    if is_delete {
                        let n = count.unwrap_or(0);
                        self.redis_batch_del = None;
                        self.toast_error(if ui_kit::i18n::is_english() {
                            format!("Batch delete completed: {n} keys total")
                        } else {
                            format!("批量删除完成: 共 {n} 个 key")
                        });
                        if let Some(tid) = self.tabs.get(self.active_tab).map(|t| t.id.clone()) {
                            if self.tabs.get(self.active_tab).map(|t| matches!(&t.kind, TabKind::Redis(_))).unwrap_or(false) {
                                self.redis_scan(&tid, true);
                            }
                        }
                    } else {
                        st.counted = Some(count.unwrap_or(0));
                    }
                    continue;
                }
                BgEvent::TransferDone { result } => {
                    match result {
                        Ok((rows, _)) => {
                            self.toast_error(if ui_kit::i18n::is_english() {
                                format!("Transfer completed: {rows} rows total")
                            } else {
                                format!("传输完成: 共 {rows} 行")
                            });
                        }
                        Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Transfer failed", "传输失败"))),
                    }
                    self.transfer = None;
                    continue;
                }
                BgEvent::DataDiffDone { state, result } => {
                    let (ok_res, err_res) = match result {
                        Ok(d) => (Some(d), None),
                        Err(e) => (None, Some(e)),
                    };
                    self.data_diff = Some(DataDiffState {
                        running: false,
                        result: ok_res,
                        error: err_res,
                        ..state
                    });
                    continue;
                }
                BgEvent::KafkaTopicCreated { result } => {
                    if let Some(k) = self.kafka_tab.as_mut() {
                        k.topic_busy = false;
                        k.new_open = false;
                        k.drop_arm = false;
                    }
                    match &result {
                        Ok(_) => {
                            self.toast_error(ui_kit::t!("Topic created", "已创建 topic").to_string());
                            self.kafka_reload_topics();
                        }
                        Err(e) => self.toast_error(e.clone()),
                    }
                    continue;
                }
                BgEvent::KafkaTopicDeleted { result } => {
                    let keep = self
                        .kafka_tab
                        .as_ref()
                        .and_then(|k| k.selected.clone())
                        .unwrap_or_default();
                    if let Some(k) = self.kafka_tab.as_mut() {
                        k.topic_busy = false;
                        k.drop_arm = false;
                    }
                    match &result {
                        Ok(_) => {
                            self.toast_error(format!(
                                "{} {keep}",
                                ui_kit::t!("Topic deleted", "已删除 topic")
                            ));
                            if let Some(k) = self.kafka_tab.as_mut() {
                                if k.selected.as_deref() == Some(keep.as_str()) {
                                    k.selected = None;
                                    k.msgs.clear();
                                }
                            }
                            self.kafka_reload_topics();
                        }
                        Err(e) => self.toast_error(e.clone()),
                    }
                    continue;
                }
                BgEvent::RedisInfoLoaded { result } => {
                    if let Some(st) = self.redis_info.as_mut() {
                        st.loading = false;
                        match result {
                            Ok(sections) => st.sections = sections,
                            Err(e) => st.error = Some(e),
                        }
                    }
                    continue;
                }
                BgEvent::RedisSlowlogLoaded { result } => {
                    if let Some(st) = self.redis_info.as_mut() {
                        match result {
                            Ok(rows) => {
                                st.slowlog = rows;
                                st.slowlog_loaded = true;
                            }
                            Err(e) => st.error = Some(e),
                        }
                    }
                    continue;
                }
                BgEvent::SessionsLoaded { tab_id, result } => {
                    if let Some(s) = self.session_tab.as_mut() {
                        s.loading = false;
                        match result {
                            Ok(rows) => {
                                s.error = None;
                                s.rows = rows;
                            }
                            Err(e) => s.error = Some(e),
                        }
                    }
                    let _ = tab_id;
                    continue;
                }
                BgEvent::SessionKilled { result, .. } => {
                    if let Some(s) = self.session_tab.as_mut() {
                        match &result {
                            Ok(_) => self.toast_error(ui_kit::t!("Session terminated", "已终止会话").to_string()),
                            Err(e) => self.toast_error(e.clone()),
                        }
                    }
                    // 终止后自动刷新
                    self.open_sessions_refresh();
                    continue;
                }
                BgEvent::KafkaTopicsLoaded { conn_id, result } => {
                    match result {
                        Ok(topics) => {
                            let names: Vec<String> =
                                topics.into_iter().map(|t| t.name).collect();
                            if let Some(k) = self.kafka_tab.as_mut() {
                                if k.conn_id == conn_id {
                                    k.selected = names.first().cloned();
                                }
                                k.topics = names;
                                k.loading = false;
                            }
                            // topics 到位后自动读首个 topic
                            self.kafka_load_msgs();
                        }
                        Err(e) => {
                            self.toast_error(e.clone());
                            if let Some(k) = self.kafka_tab.as_mut() {
                                k.loading = false;
                                k.error = Some(e);
                            }
                        }
                    }
                    continue;
                }
                BgEvent::KafkaMsgsLoaded { result, .. } => {
                    if let Some(k) = self.kafka_tab.as_mut() {
                        k.loading = false;
                        match result {
                            Ok(msgs) => {
                                k.error = None;
                                k.msgs = msgs;
                            }
                            Err(e) => k.error = Some(e),
                        }
                    }
                    continue;
                }
                BgEvent::KafkaSendDone { result, .. } => {
                    let (conn_id, topic) = match self.kafka_tab.as_ref() {
                        Some(k) => (k.conn_id.clone(), k.selected.clone().unwrap_or_default()),
                        None => (String::new(), String::new()),
                    };
                    let tab_id = self
                        .tabs
                        .iter()
                        .find(|t| matches!(&t.kind, TabKind::Kafka(_)))
                        .map(|t| t.id.clone())
                        .unwrap_or_default();
                    if let Some(k) = self.kafka_tab.as_mut() {
                        k.sending = false;
                        match &result {
                            Ok((p, o)) => {
                                self.toast_error(format!(
                                    "{} · p{} o{}",
                                    ui_kit::t!("Sent", "已发送"),
                                    p,
                                    o
                                ));
                                // 发送后自动刷新消息列表
                                if !tab_id.is_empty() && !topic.is_empty() {
                                    let tx2 = self.event_tx.clone();
                                    let svc2 = self.service.clone();
                                    let cid2 = conn_id.clone();
                                    net::spawn(async move {
                                        let r2 =
                                            svc2.kafka_consume_recent(&cid2, &topic, 200, 5)
                                                .await;
                                        let _ = tx2.send(BgEvent::KafkaMsgsLoaded {
                                            tab_id,
                                            result: r2,
                                        });
                                    });
                                }
                            }
                            Err(e) => self.toast_error(e.clone()),
                        }
                    }
                    continue;
                }
                BgEvent::KafkaGroupsLoaded { result, .. } => {
                    if let Some(k) = self.kafka_tab.as_mut() {
                        match result {
                            Ok(g) => {
                                k.groups = Some(g);
                                k.groups_open = true;
                            }
                            Err(e) => self.toast_error(e),
                        }
                    }
                    continue;
                }
                BgEvent::RedisStreamAddDone { tab_id, key, result } => {
                    if let Some(rt) = self.redis_tab_mut(&tab_id) {
                        rt.stream_busy = false;
                    }
                    match &result {
                        Ok(id) => {
                            self.toast_error(format!(
                                "{} {id}",
                                ui_kit::t!("Message added", "已发送消息")
                            ));
                            // 刷新详情(XLEN/消息列表同步更新)
                            self.redis_fetch_detail(&tab_id, &key);
                        }
                        Err(e) => self.toast_error(e.clone()),
                    }
                    continue;
                }
                BgEvent::RedisStreamGroupsDone { tab_id, result } => {
                    if let Some(rt) = self.redis_tab_mut(&tab_id) {
                        rt.stream_busy = false;
                        match result {
                            Ok(groups) => rt.stream_groups = Some(groups),
                            Err(e) => {
                                rt.stream_groups = Some(Vec::new());
                                self.toast_error(e);
                            }
                        }
                    }
                    continue;
                }
                BgEvent::MongoIndexListDone { result } => {
                    if let Some(st) = self.mongo_indexes.as_mut() {
                        st.running = false;
                        match result {
                            Ok(items) => st.items = items,
                            Err(e) => st.error = Some(e),
                        }
                    }
                    continue;
                }
                BgEvent::MongoIndexOpDone { op, result } => {
                    match &result {
                        Ok(msg) => self.toast_error(msg.clone()),
                        Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Failed", "失败"))),
                    }
                    // 成功后重拉列表;失败停在表单(错误信息在下一次列表刷新时清掉由 busy/armed 表达)
                    if result.is_ok() {
                        self.refresh_mongo_indexes();
                    } else if let Some(st) = self.mongo_indexes.as_mut() {
                        st.busy = false;
                    }
                    let _ = op;
                    continue;
                }
                BgEvent::MongoDocDone { op, result } => {
                    // op=3 批量导入:关导入弹窗 + 重载数据页
                    if op == 3 {
                        match &result {
                            Ok(msg) => self.toast_error(msg.clone()),
                            Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Failed", "失败"))),
                        }
                        self.mongo_import = None;
                        self.reload_active_mongo_rows();
                        continue;
                    }
                    match op {
                        // op=9:get_doc 回填编辑器(不关弹窗)
                        9 => {
                            if let Some(st) = self.mongo_doc.as_mut() {
                                st.running = false;
                                match result {
                                    Ok(json) => st.json = json,
                                    Err(e) => st.error = Some(e),
                                }
                            }
                        }
                        _ => {
                            match &result {
                                Ok(msg) => self.toast_error(msg.clone()),
                                Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Failed", "失败"))),
                            }
                            self.mongo_doc = None;
                            // 重载当前 Mongo 数据页
                            self.reload_active_mongo_rows();
                        }
                    }
                    continue;
                }
                BgEvent::AiDone {
                    tab_id: _,
                    mode,
                    result,
                } => {
                    if let Some(st) = self.ai_assist.as_mut() {
                        st.running = false;
                        match result {
                            Ok(text) => {
                                let title = match mode {
                                    0 => ui_kit::t!("Generate SQL", "生成 SQL").into(),
                                    1 => ui_kit::t!("Explain", "解释").into(),
                                    _ => ui_kit::t!("Optimization suggestions", "优化建议").into(),
                                };
                                st.history.push((title, text));
                                st.error = None;
                                // 生成模式:输入清空方便连续提问
                                if mode == 0 {
                                    st.input.clear();
                                }
                            }
                            Err(e) => st.error = Some(e),
                        }
                    }
                    continue;
                }
                BgEvent::SchemaDiffDone { state, result } => {
                    let (ok_res, err_res) = match result {
                        Ok(d) => (Some(d), None),
                        Err(e) => (None, Some(e)),
                    };
                    self.schema_diff = Some(SchemaDiffState {
                        running: false,
                        result: ok_res,
                        error: err_res,
                        ..state
                    });
                    continue;
                }
                BgEvent::ParquetPreviewDone { path, result } => {
                    match result {
                        Ok(preview) => {
                            self.parquet_preview = Some((path, preview));
                        }
                        Err(e) => self.toast_error(format!(
                            "{}: {e}",
                            ui_kit::t!("Parquet preview failed", "Parquet 预览失败")
                        )),
                    }
                    continue;
                }
                BgEvent::ExportDone { result } => match result {
                    Ok(path) => {
                        self.toast_error(format!("{}: {path}", ui_kit::t!("Exported", "已导出")))
                    }
                    Err(e) => {
                        self.toast_error(format!("{}: {e}", ui_kit::t!("Export failed", "导出失败")))
                    }
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
                BgEvent::ImportColsDone {
                    conn_id,
                    database,
                    table,
                    cols,
                } => {
                    if let Some(im) = self.import.as_mut() {
                        let same_target = im.conn_id == conn_id
                            && im.database.as_deref() == Some(database.as_str())
                            && im.table == table;
                        if same_target && im.target_cols.is_empty() && !cols.is_empty() {
                            im.target_cols = cols;
                        }
                    }
                }
                BgEvent::ImportParsed { result } => {
                    if let Some(im) = self.import.as_mut() {
                        im.parsing = false;
                        match result {
                            Ok(headers) => {
                                im.col_map = headers
                                    .iter()
                                    .map(|h| im.target_cols.iter().position(|c| c.eq_ignore_ascii_case(h)))
                                    .collect();
                                im.headers = headers;
                                im.batch_errors = None;
                            }
                            Err(e) => im.batch_errors = Some(e),
                        }
                    }
                }
                BgEvent::ImportDone { result } => {
                    if let Some(im) = self.import.as_mut() {
                        im.running = false;
                    }
                    match result {
                    Ok(n) => {
                        self.import = None;
                        self.toast_error(if ui_kit::i18n::is_english() {
                            format!("Imported {n} rows")
                        } else {
                            format!("已导入 {n} 行")
                        });
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
                            self.toast_error(format!("{}: {e}", ui_kit::t!("Import failed", "导入失败")));
                        }
                    }
                    }
                },
                BgEvent::DdlDone { result, msg } => match result {
                    Ok(()) => {
                        // 建表路径(msg 空)关新建表弹窗;其余 DDL 用各自的操作名做提示
                        if msg.is_empty() {
                            self.new_table = None;
                            self.toast_error(ui_kit::t!("Created successfully", "创建成功").to_string());
                        } else {
                            self.toast_error(if ui_kit::i18n::is_english() {
                                format!("{msg} succeeded")
                            } else {
                                format!("{msg}成功")
                            });
                        } // 重新载入所有在线树的表
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
                    Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Create failed", "创建失败"))),
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
                    let mut save_err = false;
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
                                // 多语句保存可能已部分执行(如 ADD 成功 MODIFY 失败):
                                // 本地 baseline 已不可信,重拉结构同步,否则重试必撞 duplicate column
                                save_err = !d.is_create();
                                (d.conn_id.clone(), d.database.clone(), false)
                            }
                        }
                    };
                    if save_err {
                        self.load_designer_structure(&tab_id);
                        continue;
                    }
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
                                // 换 pattern 重扫:旧 key 的徽标映射清掉(新 key 同名时类型可能不同)
                                rt.key_types.clear();
                                rt.key_ttls.clear();
                            } else {
                                // HashSet 去重追加(此前 Vec::contains 逐个线性扫,O(n²))
                                let known: std::collections::HashSet<String> =
                                    rt.keys.iter().cloned().collect();
                                let fresh: Vec<String> = keys
                                    .into_iter()
                                    .filter(|k| !known.contains(k))
                                    .collect();
                                rt.keys.extend(fresh);
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
                    rt.detail_more_loading = false;
                    // 期间切换了选中 key:丢弃过期结果
                    if rt.selected_key.as_deref() != Some(key.as_str()) {
                        continue;
                    }
                    match result {
                        Ok(d) => {
                            // 回写徽标:改 TTL/类型后列表徽标不再停留旧值,直到重扫
                            rt.key_ttls.insert(key.clone(), d.ttl);
                            rt.key_types.insert(key.clone(), d.key_type.clone());
                            rt.detail = Some(d);
                        }
                        Err(e) => {
                            // key 已不存在(过期/他端删除):从列表摘掉而不是永远"加载失败"
                            if e.contains("不存在或已过期") {
                                rt.keys.retain(|k| k != &key);
                                rt.key_types.remove(&key);
                                rt.key_ttls.remove(&key);
                                if rt.selected_key.as_deref() == Some(key.as_str()) {
                                    rt.selected_key = None;
                                    rt.detail = None;
                                }
                                self.toast_error(if ui_kit::i18n::is_english() {
                                    format!(
                                        "{key} no longer exists (expired or deleted); removed from the list"
                                    )
                                } else {
                                    format!("{key} 已不存在(已过期或被删除),已从列表移除")
                                });
                            } else {
                                rt.error = Some(e);
                            }
                        }
                    }
                }
                BgEvent::RedisDetailMoreDone {
                    tab_id,
                    key,
                    gen,
                    result,
                } => {
                    let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
                        continue;
                    };
                    let TabKind::Redis(rt) = &mut tab.kind else {
                        continue;
                    };
                    // 刷新/切 key 期间迟到的旧页:丢弃。
                    // 复位放在校验之后——过期事件不复位,否则会让在途请求失去防抖保护;
                    // 不会死锁:刷新/切 key 路径(redis_fetch_detail)已复位
                    if rt.detail_gen != gen || rt.selected_key.as_deref() != Some(key.as_str()) {
                        continue;
                    }
                    rt.detail_more_loading = false;
                    match result {
                        Ok(d) => {
                            if let Some(cur) = rt.detail.as_mut() {
                                cur.pairs.extend(d.pairs);
                            }
                        }
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
                        self.toast_error(format!("{}: {e}", ui_kit::t!("Operation failed", "操作失败")));
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
                            self.toast_error(format!("{} {key}", ui_kit::t!("Deleted", "已删除")));
                        }
                        RedisOp::Rename { from, to } => {
                            // RENAME 会覆盖服务端已存在的 to:列表与徽标映射同步迁移
                            let pos = rt.keys.iter().position(|k| k == &from);
                            rt.keys.retain(|k| k != &from && k != &to);
                            if let Some(p) = pos {
                                let p = p.min(rt.keys.len());
                                rt.keys.insert(p, to.clone());
                            }
                            if let Some(t) = rt.key_types.remove(&from) {
                                rt.key_types.insert(to.clone(), t);
                            }
                            if let Some(t) = rt.key_ttls.remove(&from) {
                                rt.key_ttls.insert(to.clone(), t);
                            }
                            if rt.selected_key.as_deref() == Some(from.as_str()) {
                                rt.selected_key = Some(to.clone());
                            }
                            let tid = tab_id.clone();
                            self.redis_fetch_detail(&tid, &to);
                            self.toast_error(format!("{} {to}", ui_kit::t!("Renamed to", "已重命名为")));
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
                    Err(e) => self.toast_error(format!("{}: {e}", ui_kit::t!("Execution failed", "执行失败"))),
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
                BgEvent::AcColumnsDone { key, cols } => {
                    // 空=拉取失败:移除占位让下次输入重试(否则永远显示"加载中")
                    if cols.is_empty() {
                        self.ac_columns.remove(&key);
                    } else {
                        self.ac_columns.insert(key, cols);
                    }
                    self.ac_rev += 1;
                }
                BgEvent::QueryPkDone { tab_id, cols } => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        if let TabKind::Query(q) = &mut tab.kind {
                            q.result_pk = cols;
                            q.pk_checked = true;
                        }
                    }
                }
                BgEvent::SearchDone {
                    conn_id,
                    db,
                    keyword,
                    result,
                } => {
                    let Some(gs) = self.global_search.as_mut() else {
                        continue;
                    };
                    // 迟到的旧请求(发起后换了连接/库/词):丢弃,不串台显示
                    let stale = gs.conn_id != conn_id
                        || gs.db != db
                        || gs.keyword.trim() != keyword;
                    if stale {
                        continue;
                    }
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
        .map(|c| {
            matches!(
                c.db_type,
                shujing_core::DbType::MySql | shujing_core::DbType::ClickHouse
            )
        })
        .unwrap_or(false)
}

/// 全列 LIKE 筛选:列名按方言引注,值转义单引号
pub fn build_like_filter(columns: &[String], query: &str, mysql: bool) -> String {
    let mut escaped = query.replace('\'', "''");
    if mysql {
        escaped = escaped.replace('\\', "\\\\");
    }
    let pattern = format!("'%{escaped}%'");
    // 整体加括号:与筛选栏条件 AND 拼接时,无括号的 OR 串会被 SQL 优先级改写语义
    format!(
        "({})",
        columns
            .iter()
            .map(|c| format!("{} LIKE {}", quote_ident_by(c, mysql), pattern))
            .collect::<Vec<_>>()
            .join(" OR ")
    )
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
        let literal = if v.parse::<f64>().map(|f| f.is_finite()).unwrap_or(false) && op != "LIKE" {
            v.to_string()
        } else {
            // LIKE 语义 = 包含:两侧补通配符(用户自带 % 则原样保留)
            let wrapped = if op == "LIKE" && !v.contains('%') {
                format!("%{v}%")
            } else {
                v.to_string()
            };
            let mut quoted = wrapped.replace('\'', "''");
            if mysql {
                quoted = quoted.replace('\\', "\\\\");
            }
            format!("'{quoted}'")
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
    // ascii 小写化等长(Unicode 小写化字节可变,偏移切原串会 panic)
    let lower = sql.to_ascii_lowercase();
    let head = lower.trim_start();
    if !head.starts_with("select") {
        return None;
    }
    // 词边界黑名单:contains("update ") 会误杀 last_update 列名;
    // 逗号连接(SELECT * FROM t1, t2)的回写只会落 t1,一并拒绝
    let words: Vec<&str> = lower.split_whitespace().collect();
    let word_hit = |w: &[&str], needles: &[&str]| w.iter().any(|x| needles.contains(x));
    if word_hit(&words, &["join", "union", "group", "having", "into"])
        || word_hit(&words, &["insert", "update", "delete"])
    {
        return None;
    }
    // 子查询:select 出现两次以上即放弃
    if lower.matches("select").count() > 1 {
        return None;
    }
    let from_pos = lower.find(" from ")?;
    let rest = sql[from_pos + 6..].trim();
    // 取第一个标识符(容忍 `db`.`table` / "db"."table" / db.table,引号段可与点交错)。
    // 遇逗号立即放弃(多表);引号内双写引号是转义,不当段尾
    let mut ident = String::new();
    let mut chars = rest.chars();
    let mut comma = false;
    loop {
        match chars.next() {
            Some(',') => {
                // FROM t1, t2:回写落点歧义,放弃(仅当逗号出现在表名段)
                if !ident.is_empty() {
                    comma = true;
                }
                break;
            }
            Some(q @ ('`' | '"')) => {
                ident.push(q);
                let mut prev = q;
                while let Some(c2) = chars.next() {
                    ident.push(c2);
                    if c2 == q && prev != q {
                        break;
                    }
                    prev = c2;
                }
            }
            Some(c) if c.is_alphanumeric() || c == '_' || c == '$' || c == '.' => {
                ident.push(c);
            }
            _ => break,
        }
    }
    if comma {
        return None;
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
        if im.running {
            return; // 导入进行中,忽略重复提交
        }
        let col_map: Vec<(usize, String)> = im
            .col_map
            .iter()
            .enumerate()
            .filter_map(|(src, dst)| dst.map(|d| (src, im.target_cols[d].clone())))
            .collect();
        if col_map.is_empty() {
            self.toast_error(ui_kit::t!("No columns mapped", "未映射任何列").to_string());
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
        if let Some(im) = self.import.as_mut() {
            im.running = true;
        }
        let service = self.service.clone();
        let tx = self.event_tx.clone();
        crate::net::spawn(async move {
            let r = shujing_core::excel::import_table_file(&service, info, imp).await;
            let _ = tx.send(BgEvent::ImportDone { result: r });
        });
    }
}

impl ShujingApp {
    /// 打开表统计弹窗并异步取数
    pub fn open_stats(&mut self, conn_id: &str, database: &str, table: &str) {
        let key = crate::app::stats_key(conn_id, database, table);
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
        // 在途导入不允许重开:弹窗状态会被重建,running 门随之失效 → 并发 INSERT 重复入库
        if self.import.as_ref().is_some_and(|im| im.running) {
            self.toast_error(ui_kit::t!("Import in progress", "导入正在进行中").to_string());
            return;
        }
        // 打开系统文件选择框,读表结构填目标列
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ui_kit::t!("Data files", "表格"), &["csv", "txt", "xlsx"])
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
            .unwrap_or_default();
        // 目标表标签未打开时异步拉结构回填(此前兜底抓"任意表标签"的列,映射会指错列)
        self.import = Some(ImportState {
            conn_id: conn_id.to_string(),
            database: Some(database.to_string()),
            table: table.to_string(),
            path: path.display().to_string(),
            headers: Vec::new(),
            running: false,
            col_map: Vec::new(),
            batch_errors: None,
            target_cols: target_cols.clone(),
            parsing: false,
        });
        if target_cols.is_empty() {
            // 目标表标签未打开:异步拉结构回填列映射候选
            let (tx, service, cid, db, tbl) = (
                self.event_tx.clone(),
                self.service.clone(),
                conn_id.to_string(),
                database.to_string(),
                table.to_string(),
            );
            crate::net::spawn(async move {
                let cols = service
                    .get_table_structure(&cid, &tbl, Some(db.clone()))
                    .await
                    .map(|st| st.columns.into_iter().map(|c| c.name).collect::<Vec<_>>())
                    .unwrap_or_default();
                let _ = tx.send(BgEvent::ImportColsDone {
                    conn_id: cid,
                    database: db,
                    table: tbl,
                    cols,
                });
            });
        }
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
