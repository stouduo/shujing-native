//! 界面状态类型群:标签页/弹窗/控制台的纯数据结构(批次32 分包)。
//! 与 app.rs 通过 `pub use state::*;` 保持既有路径不变。

use egui::Color32;
use shujing_core::{ConnInfo};
use std::collections::HashMap;
use std::sync::Arc;

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
    /// 列名→类型标注(DBX 式列头第二行,按类别着色,如 int / varchar(32))
    pub col_types: std::collections::HashMap<String, String>,
    /// 列名→注释(DBX 式列头第三行灰字,有注释才显示)
    pub col_comments: std::collections::HashMap<String, String>,
    /// 当前数据对应的 SQL(状态栏预览用,DBX 式)
    pub load_sql: String,
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
    /// 列顺序(列头拖动重排;空 = 自然顺序)。存全量列名排列,隐藏列过滤在 vis_idx,
    /// schema 新增列不在其中时排尾部
    pub col_order: Vec<String>,
    /// 列头拖拽进行中:(源可视列下标, 指针相对列左缘的偏移)。画插入指示线,松开落位
    pub col_drag: Option<(usize, f32)>,
    /// 重排落位后按新显示序搬运的列宽(egui_extras 的宽度按位置存,拖宽值不跟列走):
    /// 下一帧整排按这组宽度画一帧,宽度跟着列走;用后即清
    pub col_carry: Option<Vec<f32>>,
    /// 列缘双击自适应:下一帧该列按适配宽画一帧(Column::exact 会把宽度写进
    /// egui_extras 的状态持久化),用后即清
    pub col_autofit: Option<usize>,
    /// 列缘双击判定:上一次按在哪个列缘(边界下标, 时刻, 指针 x——双击要落在同一位置)
    pub edge_click: Option<(usize, f64, f32)>,
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
            col_types: Default::default(),
            col_comments: Default::default(),
            load_sql: String::new(),
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
            col_order: vec![],
            col_drag: None,
            col_carry: None,
            col_autofit: None,
            edge_click: None,
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
    /// 单表结果集的列注释(拉主键时顺带取回;JOIN/聚合查询没有可靠来源,留空)
    pub col_comments: std::collections::HashMap<String, String>,
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
    /// 字段映射(源列→目标列文本);None=同构直插
    pub mapping: Option<Vec<(String, String)>>,
    /// 源列清单预取(打开传输时后台拉;映射按钮据此建格子)
    pub map_srcs: Option<Vec<String>>,
    pub running: bool,
    pub error: Option<String>,
}

/// 查询执行元信息(随 QueryDone 事件走,供历史记录)
#[derive(Clone)]
pub struct QueryMeta {
    pub sql: String,
    /// 连接名(展示用)
    pub conn: String,
    pub db: String,
}

/// 查询历史弹窗状态(持久化 jsonl,点击回填编辑器)
#[derive(Clone)]
pub struct HistoryState {
    pub tab_id: String,
    /// 新的在前
    pub items: Vec<shujing_core::history::HistoryEntry>,
    pub filter: String,
}

/// 插件处理弹窗状态(查询结果 → WASM 插件 → 输出)
#[derive(Clone)]
pub struct PluginRunState {
    pub tab_id: String,
    /// 行集 JSON 快照 {"columns":[..],"rows":[[..]..]}
    pub input: String,
    pub plugins: Vec<shujing_core::plugins::PluginInfo>,
    pub sel: usize,
    pub output: Option<String>,
    pub error: Option<String>,
    pub running: bool,
}

/// 备份数据库状态(连接右键;单文件 SQL dump,逐表进度)
#[derive(Clone)]
pub struct BackupState {
    pub conn_id: String,
    pub conn_name: String,
    pub db: String,
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub cur_table: String,
    /// Ok = (tables, rows, path, bytes)
    pub result: Option<Result<(usize, u64, String, u64), String>>,
    /// 备份目录最近文件(管理面板;打开弹窗时扫描)
    pub recent: Vec<shujing_core::backup::BackupFileMeta>,
}

/// 定时自动备份配置(settings.json autoBackup)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AutoBackupCfg {
    pub enabled: bool,
    /// 间隔小时(6/12/24/72)
    pub interval_hours: u32,
    /// 上次自动备份 epoch ms
    pub last_run_ms: i64,
}

impl Default for AutoBackupCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
            last_run_ms: 0,
        }
    }
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
    /// 确认输入:用户敲入的匹配数量(此前 buffer 每帧重建,打字被吞、门形同虚设)
    pub confirm_input: String,
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

/// MQTT 控制台标签(主题采样浏览/载荷/发布)
pub struct MqttTab {
    pub conn_id: String,
    pub topics: Vec<shujing_core::mqtt::MqttMsg>,
    pub selected: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub pub_topic: String,
    pub pub_payload: String,
    pub pub_retain: bool,
    pub sending: bool,
}

impl MqttTab {
    /// 用于 tabs 的独立实例(状态从零)
    pub fn clone_shell(&self, _new_id: &str) -> MqttTab {
        MqttTab {
            conn_id: self.conn_id.clone(),
            topics: self.topics.clone(),
            selected: None,
            loading: false,
            error: None,
            pub_topic: String::new(),
            pub_payload: String::new(),
            pub_retain: false,
            sending: false,
        }
    }
}

/// 配置中心控制台标签(Nacos/Consul/Etcd 三类型共用)
pub struct KvTab {
    pub conn_id: String,
    pub entries: Vec<shujing_core::kv::KvEntry>,
    pub selected: Option<String>,
    pub edit_value: Option<String>,
    pub filter: String,
    pub new_key: String,
    pub loading: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub drop_arm: bool,
}

impl KvTab {
    /// 用于 tabs 的独立实例(状态从零)
    pub fn clone_shell(&self, _new_id: &str) -> KvTab {
        KvTab {
            conn_id: self.conn_id.clone(),
            entries: self.entries.clone(),
            selected: None,
            edit_value: None,
            filter: String::new(),
            new_key: String::new(),
            loading: false,
            busy: false,
            error: None,
            drop_arm: false,
        }
    }
}

/// RabbitMQ 控制台标签(概览/队列/消息取回/发送/清空)
pub struct RabbitTab {
    pub conn_id: String,
    pub queues: Vec<shujing_core::rabbit::RabbitQueue>,
    /// (vhost, 队列名)
    pub selected: Option<(String, String)>,
    pub msgs: Vec<shujing_core::rabbit::StreamMsg>,
    pub overview: Option<shujing_core::rabbit::RabbitOverview>,
    pub loading: bool,
    pub error: Option<String>,
    pub send_key: String,
    pub send_payload: String,
    pub sending: bool,
    pub drop_arm: bool,
}

impl RabbitTab {
    /// 用于 tabs 的独立实例(状态从零)
    pub fn clone_shell(&self, _new_id: &str) -> RabbitTab {
        RabbitTab {
            conn_id: self.conn_id.clone(),
            queues: self.queues.clone(),
            selected: None,
            msgs: Vec::new(),
            overview: self.overview.clone(),
            loading: false,
            error: None,
            send_key: String::new(),
            send_payload: String::new(),
            sending: false,
            drop_arm: false,
        }
    }
}

/// 开发工具箱标签(左侧工具导航 + 右侧工具页)
pub struct ToolboxTab {
    pub tool: crate::toolbox::Tool,
    pub codec: crate::toolbox::CodecTool,
    pub ts: crate::toolbox::TsTool,
    pub uuid: crate::toolbox::UuidTool,
    pub json: crate::toolbox::JsonTool,
    pub hash: crate::toolbox::HashTool,
    pub cron: crate::toolbox::CronTool,
    pub regex: crate::toolbox::RegexTool,
    pub jwt: crate::toolbox::JwtTool,
}

impl Drop for ToolboxTab {
    fn drop(&mut self) {
        // 标签关闭(任何路径)即叫停在途的文件摘要任务,免得后台把大文件读完
        self.hash.stop_running();
    }
}

impl ToolboxTab {
    /// 用于 tabs 的独立实例(状态从零)
    pub fn clone_shell(&self, _new_id: &str) -> ToolboxTab {
        ToolboxTab::default()
    }
}

impl Default for ToolboxTab {
    fn default() -> Self {
        ToolboxTab {
            tool: crate::toolbox::Tool::Codec,
            codec: Default::default(),
            ts: Default::default(),
            uuid: Default::default(),
            json: Default::default(),
            hash: Default::default(),
            jwt: Default::default(),
            cron: Default::default(),
            regex: Default::default(),
        }
    }
}

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
    /// 全库搜索(遍历连接全部非系统库;db 字段此时忽略)
    pub all_dbs: bool,
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
    /// RabbitMQ 控制台(不参与会话恢复)
    Rabbit(RabbitTab),
    /// MQTT 控制台(不参与会话恢复)
    Mqtt(MqttTab),
    /// 配置中心控制台(不参与会话恢复)
    Kv(KvTab),
    /// 服务器会话面板(不参与会话恢复)
    Session(SessionsTab),
    /// 开发工具箱(不参与会话恢复)
    Toolbox(ToolboxTab),
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
    /// 当前分区 tab(0 连接信息 / 1 SSH 隧道 / 末位 高级;本地文件库没有隧道位)
    pub tab: u8,
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
    /// 权限档位(生产库保护;只读/仅 DML/完整)
    pub perm: shujing_core::Perm,
    /// 侧栏分组(空 = 不分组)
    pub group: String,
    /// 驱动插件名(连接类型 = 插件驱动 时填)
    pub plugin: String,
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
            tab: 0,
            name: String::new(),
            db_type: shujing_core::DbType::MySql,
            host: String::new(),
            // 预填默认类型的端口:新连接不用手敲,切换类型时再跟随默认值
            port: "3306".to_string(),
            user: String::new(),
            password: String::new(),
            database: String::new(),
            file_path: String::new(),
            testing: false,
            test_result: None,
            perm: shujing_core::Perm::Full,
            group: String::new(),
            plugin: String::new(),
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
            tab: 0,
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
            perm: c.perm(),
            group: c.group.clone().unwrap_or_default(),
            plugin: c.plugin.clone().unwrap_or_default(),
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

