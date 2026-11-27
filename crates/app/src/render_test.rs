//! 无头渲染自检:离屏渲染各视图存 PNG 到 /tmp/shujing_ui/,用于像素级自验收。
//! 运行:cargo test -p shujing-app --渲染即生成,不比对快照(纯生成供人工/代理查看)。

use crate::app::{ConnTree, QueryTab, ShujingApp, StructureTab, Tab, TabKind, TableTab};
use crate::query;
use crate::structure;
use crate::table_view;
use egui_kittest::Harness;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 主题是全局状态;测试并行执行会互相覆盖,渲染全部串行化
pub(crate) static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// 走真实 eframe::App::update 路径的测试:顶部应用工具栏把其下所有内容下推的高度。
/// 这些测试里的点击坐标一律写成"原逻辑坐标 + CHROME_TOP"(弹窗居中、不随面板移动的除外);
/// draw_app 夹具不含工具栏,走它的测试不加
const CHROME_TOP: f32 = crate::appbar::BAR_H;

fn render(name: &str, w: f32, h: f32, dark: bool, draw: impl FnMut(&egui::Context) + 'static) {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    render_locked(name, w, h, dark, draw);
}

/// 渲染并落盘(调用方须已持有 RENDER_LOCK)。要把"锁外的全局状态切换"也纳入
/// 同一把锁的保护范围时用它——例如英文测试:语言是进程级全局,切语言若发生在
/// 锁外,并行运行的其他渲染测试会随机渲染成英文,落盘的图就不可复现、不可对比
fn render_locked(
    name: &str,
    w: f32,
    h: f32,
    dark: bool,
    mut draw: impl FnMut(&egui::Context) + 'static,
) {
    let out_dir = std::path::PathBuf::from("/tmp/shujing_ui");
    let _ = std::fs::create_dir_all(&out_dir);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(2.0)
        .build(move |ctx| {
            if !setup_done.get() {
                setup_done.set(true);
                ShujingApp::apply_fonts(ctx);
                crate::theme::set_dark(dark);
                crate::theme::apply(ctx);
                // 字体/样式在下一帧生效:首帧只做安装,避免 bold 家族尚未绑定
                return;
            }
            draw(ctx);
        });
    harness.run_steps(4);
    let img = harness.render().expect("render 失败");
    img.save(out_dir.join(format!("{name}.png")))
        .expect("保存 PNG 失败");
}

// ---------------------------------------------------------------- 假数据

fn test_app(tabs: Vec<Tab>, active_tab: usize) -> ShujingApp {
    let dir = std::env::temp_dir().join("shujing_ui_cfg");
    let _ = std::fs::create_dir_all(&dir);
    let (tx, rx) = std::sync::mpsc::channel();
    let conn = shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: Some("dramato".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };
    let conn2 = shujing_core::ConnInfo {
        id: "c2".into(),
        name: "本地".into(),
        db_type: shujing_core::DbType::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        database: None,
        file_path: Some("~/data/test.db".into()),
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };
    let tables: Vec<shujing_core::TableMeta> = [
        "ads_report_youni",
        "ali_vod_trans_log",
        "backend_role_privilege",
        "campaign_report",
        "drama_episode",
        "user_role_map",
        "abnormal_account",
        "abtest_strategy",
        "access_deny_info",
        "active_event_button",
        "activity_card",
        "actor_heat_score",
    ]
    .iter()
    .map(|n| shujing_core::TableMeta {
        name: n.to_string(),
        kind: "table".into(),
        owner: None,
    })
    .collect();
    let mut tables = tables;
    tables.push(shujing_core::TableMeta {
        name: "v_user_summary".into(),
        kind: "view".into(),
        owner: None,
    });
    tables.push(shujing_core::TableMeta {
        name: "trg_touch_user".into(),
        kind: "trigger".into(),
        owner: None,
    });
    tables.push(shujing_core::TableMeta {
        name: "fn_heat_score".into(),
        kind: "function".into(),
        owner: None,
    });
    let archive: Vec<shujing_core::TableMeta> = (0..7)
        .map(|i| shujing_core::TableMeta {
            name: format!("archive_table_{i}"),
            kind: "table".into(),
            owner: None,
        })
        .collect();
    let comic: Vec<shujing_core::TableMeta> = (0..4)
        .map(|i| shujing_core::TableMeta {
            name: format!("comic_table_{i}"),
            kind: "table".into(),
            owner: None,
        })
        .collect();
    let tree = ConnTree {
        online: true,
        version: "8.0.36".into(),
        databases: vec!["dramato".into(), "archive".into(), "comic".into()],
        expanded_db: Some("dramato".into()),
        groups_open: Default::default(),
        tables_by_db: HashMap::from([
            ("dramato".to_string(), tables),
            ("archive".to_string(), archive),
            ("comic".to_string(), comic),
        ]),
        loading: false,
        visible_dbs: None,
    };
    ShujingApp {
        service: Arc::new(shujing_core::Service::new(&dir)),
        config_dir: dir.clone(),
        conns: vec![conn, conn2],
        trees: HashMap::from([("c1".to_string(), tree)]),
        tabs,
        active_tab,
        events: rx,
        event_tx: tx,
        conn_modal: None,
        error_toast: None,
        autoopen_done: true,
        query_seq: 1,
        table_filter: String::new(),
        pending: None,
        dirty_override: false,
        skip_edit_preview: false,
        parquet_preview: None,
        schema_diff: None,
        ai_config: Default::default(),
        ai_assist: None,
        mongo_doc: None,
        mongo_indexes: None,
        mongo_import: None,
        save_result_modal: None,
        redis_info: None,
        rabbit_tab: None,
        kv_tab: None,
        mqtt_tab: None,
        kafka_tab: None,
        session_tab: None,
        transfer: None,
        backup: None,
        plugin_run: None,
        query_history: None,
        auto_backup: Default::default(),
        auto_backup_queue: Vec::new(),
        auto_backup_running: false,
        plugin_manager_open: false,
        plugins: std::sync::Arc::new(shujing_core::plugins::PluginManager::load_all(std::path::Path::new("/tmp/definitely-missing-plugins-dir"))),
        data_diff: None,
        conn_crypto: None,
        pending_crypto_path: None,
        redis_batch_del: None,
        update_available: None,
        update_check_done: true,
        shot: None,
        dark_mode: true,
        follow_system_theme: false,
        frame_count: 0,
        auto_connect_done: true,
        quick_open: None,
        stats: None,
        import: None,
        new_table: None,
        global_search: None,
        ac_columns: Default::default(),
        ac_rev: 0,
        ac_cache: None,
        confirm: None,
        snippets: std::sync::Arc::new(vec![]),
        settings_open: false,
        db_filter: None,
        theme_toggle_requested: false,
        session_dirty: false,
        last_session_save: None,
        session_restored: false,
        pending_online: vec![],
        pending_expanded: HashMap::new(),
        last_session_sig: (0, 0, false, 0, vec![]),
    }
}

fn stub_tab(title: &str, table: &str) -> Tab {
    let mut t = TableTab::new("c1".into(), table.into(), Some("dramato".into()));
    t.loading = false;
    Tab {
        id: format!("t-{table}"),
        title: title.into(),
        kind: TabKind::Table(t),
    }
}

fn role_tab() -> TableTab {
    let mut tab = TableTab::new(
        "c1".into(),
        "backend_role_privilege".into(),
        Some("dramato".into()),
    );
    tab.columns = vec![
        "id".into(),
        "role".into(),
        "role_name".into(),
        "access_urls".into(),
        "status".into(),
        "created_at".into(),
    ];
    let rows = [
        (
            "admin",
            "超级管理员",
            "/admin/backend/menu_v2,/admin/backend/user_role_list",
            "1",
            "1731383330",
        ),
        (
            "editor",
            "内容编辑",
            "/admin/series_resource/search,/admin/series_resource/detail",
            "1",
            "1731383560",
        ),
        (
            "auditor",
            "内容审核员",
            "/admin/audit/queue,/admin/audit/history",
            "2",
            "1731389120",
        ),
        (
            "viewer",
            "只读访客",
            "/admin/series_resource/search",
            "1",
            "1731400230",
        ),
        ("ops", "运维值班", "", "1", "1731411680"),
        (
            "finance",
            "财务对账",
            "/admin/report/daily,/admin/report/settle",
            "1",
            "1731418800",
        ),
        ("guest", "", "/public/landing", "2", "1731425540"),
        (
            "support",
            "客服工单",
            "/admin/ticket/list,/admin/ticket/reply",
            "1",
            "1731432950",
        ),
    ];
    tab.rows = rows
        .iter()
        .enumerate()
        .map(|(i, (role, name, urls, status, created))| {
            vec![
                Some((i + 1).to_string()),
                Some((*role).to_string()),
                (!name.is_empty()).then(|| (*name).to_string()),
                Some((*urls).to_string()),
                Some((*status).to_string()),
                Some((*created).to_string()),
            ]
        })
        .collect();
    tab.loading = false;
    tab.pk_cols = vec!["id".into()];
    tab.pk_loaded = true;
    tab.total = Some(312);
    tab.page = 1;
    tab.page_size = 100;
    tab.sort = Some(("id".into(), true));
    tab.selected = Some((2, 0));
    tab.active_filter = Some("auditor".into());
    tab.last_elapsed_ms = Some(24);
    tab.detail = false;
    tab.multi_sel = vec![2, 3, 4];
    tab
}

fn campaign_tab() -> TableTab {
    let mut tab = TableTab::new(
        "c1".into(),
        "campaign_report".into(),
        Some("dramato".into()),
    );
    tab.columns = vec![
        "campaign".into(),
        "channel".into(),
        "impressions".into(),
        "clicks".into(),
        "ctr".into(),
        "cost_cents".into(),
    ];
    let rows = [
        (
            "春节档投放 A",
            "抖音",
            "1,204,551",
            "38,201",
            "3.17%",
            "182,033",
        ),
        (
            "春节档投放 B",
            "快手",
            "862,330",
            "21,905",
            "2.54%",
            "97,441",
        ),
        (
            "暑期预热",
            "小红书",
            "512,004",
            "28,930",
            "5.65%",
            "141,720",
        ),
        ("暑期预热", "B站", "298,177", "12,038", "4.04%", "66,208"),
        ("品牌联动", "微信", "120,993", "8,412", "6.95%", "52,900"),
        (
            "拉新-应用商店",
            "ASA",
            "64,551",
            "9,120",
            "14.13%",
            "203,411",
        ),
    ];
    tab.rows = rows
        .iter()
        .map(|r| {
            let vals = [r.0, r.1, r.2, r.3, r.4, r.5];
            vals.iter()
                .map(|v| Some((*v).to_string()))
                .collect::<Vec<_>>()
        })
        .collect();
    tab.loading = false;
    tab.pk_loaded = true;
    tab.total = Some(86_241);
    tab.page = 12;
    tab.page_size = 100;
    tab.last_elapsed_ms = Some(38);
    tab.detail = false;
    tab
}

fn full_app(which: &str) -> ShujingApp {
    match which {
        "role" => test_app(
            vec![
                stub_tab("ads_report_youni", "ads_report_youni"),
                stub_tab("campaign_report", "campaign_report"),
                Tab {
                    id: "t-role".into(),
                    title: "backend_role_privilege".into(),
                    kind: TabKind::Table(role_tab()),
                },
            ],
            2,
        ),
        _ => test_app(
            vec![
                stub_tab("ads_report_youni", "ads_report_youni"),
                Tab {
                    id: "t-campaign".into(),
                    title: "campaign_report".into(),
                    kind: TabKind::Table(campaign_tab()),
                },
            ],
            1,
        ),
    }
}

fn draw_app(app: &mut ShujingApp, ctx: &egui::Context) {
    app.draw_sidebar_panel(ctx);
    table_view::draw_tabs(app, ctx);
    if app.tabs.is_empty() {
        return;
    }
    let idx = app.active_tab.min(app.tabs.len() - 1);
    let tab_id = app.tabs[idx].id.clone();
    match &mut app.tabs[idx].kind {
        TabKind::Table(t) => {
            let conns = app.conns.clone();
            let _ = table_view::draw_table_view(ctx, &tab_id, t, &conns);
        }
        TabKind::Structure(s) => {
            let _ = structure::draw_structure_view(ctx, &tab_id, s, None);
        }
        TabKind::Query(q) => {
            let conns = app.conns.clone();
            let _ = query::draw_query_view(
                &conns,
                ctx,
                &tab_id,
                q,
                &[],
                &[],
                false,
                &[],
                &Default::default(),
                None,
                false,
            );
        }
        TabKind::Designer(d) => {
            crate::designer::stash_dialect(ctx, shujing_core::DbType::MySql);
            let _ = crate::designer::draw_designer_view(ctx, &tab_id, d);
        }
        TabKind::Redis(rt) => {
            let _ = crate::redis::draw_redis_view(ctx, &tab_id, rt, false);
        }
        TabKind::Er(e) => {
            let _ = crate::er::draw_er_view(ctx, &tab_id, e);
        }
        TabKind::Kafka(k) => {
            let _ = crate::kafka::draw_kafka_view(ctx, &tab_id, k);
        }
        TabKind::Session(s) => {
            let _ = crate::sessions::draw_sessions_view(ctx, &tab_id, s);
        }
        TabKind::Rabbit(r) => {
            let _ = crate::rabbit::draw_rabbit_view(ctx, &tab_id, r);
        }
        TabKind::Kv(k) => {
            let _ = crate::kv_console::draw_kv_view(ctx, &tab_id, k);
        }
        TabKind::Mqtt(m) => {
            let _ = crate::mqtt_console::draw_mqtt_view(ctx, &tab_id, m);
        }
        TabKind::Kv(k) => {
            let _ = crate::kv_console::draw_kv_view(ctx, &tab_id, k);
        }
        TabKind::Ddl(d) => {
            let _ = crate::structure::draw_ddl_view(ctx, &tab_id, d);
        }
    }
}

// ---------------------------------------------------------------- 快照

#[test]
fn render_full_dark() {
    let app = std::cell::RefCell::new(full_app("role"));
    render("full_dark", 1200.0, 640.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn render_compact_dirty() {
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.changes
            .push((1, "role_name".into(), Some("内容编辑".into())));
        t.filter = "admin".into();
        t.active_filter = Some("admin".into());
    }
    let app = std::cell::RefCell::new(app);
    render("compact_dirty", 1024.0, 640.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn render_empty_state() {
    let app = std::cell::RefCell::new(test_app(vec![], 0));
    render("empty_state", 1200.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        let app = &mut *b;
        app.draw_sidebar_panel(ctx);
        table_view::draw_tabs(app, ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::theme::c().bg))
            .show(ctx, |ui| {
                table_view::draw_empty_state(ui);
            });
    });
}

#[test]
fn render_quick_open() {
    let mut app = full_app("role");
    app.quick_open = Some(crate::app::QuickOpen {
        query: "role".into(),
        sel: 0,
    });
    let app = std::cell::RefCell::new(app);
    render("quick_open", 1000.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        let app = &mut *b;
        draw_app(app, ctx);
        crate::modals::draw_quick_open(app, ctx);
    });
}

#[test]
fn render_full_light() {
    let app = std::cell::RefCell::new(full_app("campaign"));
    render("full_light", 1200.0, 640.0, false, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

/// 真实整帧:顶部应用工具栏 + 侧栏 + 标签栏 + 内容。其余"全窗口"快照为了让测试坐标
/// 不随工具栏变而不画它(坐标按"原逻辑坐标 + CHROME_TOP"写),和用户实际看到的差一排——
/// 审视觉、做前后对比以这组为准
fn draw_shell(app: &mut ShujingApp, ctx: &egui::Context) {
    app.draw_app_toolbar(ctx);
    draw_app(app, ctx);
}

/// 整帧夹具:表页按真实加载后的样子补上列类型/注释(表头第二行的小字、悬停注释),
/// 并回到中性状态(不预置多选行和已应用的筛选)
fn shell_app(which: &str) -> ShujingApp {
    let mut app = full_app(which);
    for tab in app.tabs.iter_mut() {
        let TabKind::Table(t) = &mut tab.kind else {
            continue;
        };
        let cols: &[(&str, &str, &str)] = if t.table == "backend_role_privilege" {
            &[
                ("id", "int unsigned", ""),
                ("role", "varchar(64)", "角色编码"),
                ("role_name", "varchar(128)", "角色名称"),
                ("access_urls", "text", "可访问路径,逗号分隔"),
                ("status", "tinyint", "1启用 2禁用"),
                ("created_at", "bigint", "创建时间戳"),
            ]
        } else {
            &[
                ("campaign", "varchar(128)", "投放计划"),
                ("channel", "varchar(32)", "渠道"),
                ("impressions", "bigint unsigned", "曝光"),
                ("clicks", "bigint unsigned", "点击"),
                ("ctr", "decimal(6,4)", "点击率"),
                ("cost_cents", "bigint", "花费(分)"),
            ]
        };
        for (c, ty, cm) in cols {
            t.col_types.insert((*c).to_string(), (*ty).to_string());
            if !cm.is_empty() {
                t.col_comments.insert((*c).to_string(), (*cm).to_string());
            }
        }
        t.multi_sel.clear();
        t.active_filter = None;
        t.filter.clear();
        t.sel_range = None;
    }
    app
}

#[test]
fn render_shell_dark() {
    let app = std::cell::RefCell::new(shell_app("role"));
    render("shell_dark", 1440.0, 900.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_shell(&mut b, ctx);
    });
}

#[test]
fn render_shell_light() {
    let app = std::cell::RefCell::new(shell_app("campaign"));
    render("shell_light", 1440.0, 900.0, false, move |ctx| {
        let mut b = app.borrow_mut();
        draw_shell(&mut b, ctx);
    });
}

#[test]
fn render_shell_query() {
    let (q, ..) = query_view_fixture();
    let app = std::cell::RefCell::new(test_app(
        vec![Tab {
            id: "q1".into(),
            title: "查询".into(),
            kind: TabKind::Query(q),
        }],
        0,
    ));
    render("shell_query", 1440.0, 900.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_shell(&mut b, ctx);
    });
}

#[test]
fn render_shell_empty() {
    let app = std::cell::RefCell::new(test_app(vec![], 0));
    render("shell_empty", 1440.0, 900.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        b.draw_app_toolbar(ctx);
        draw_app(&mut b, ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(crate::theme::c().bg))
            .show(ctx, |ui| {
                table_view::draw_empty_state(ui);
            });
    });
}

#[test]
fn render_data_table() {
    let mut tab = role_tab();
    tab.changes
        .push((1, "role_name".into(), Some("内容编辑".into())));
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    render("data_table", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _actions = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
}

#[test]
fn render_english_mode() {
    // i18n 冒烟:英文开关下渲染同一数据表场景,输出 data_table_en.png 供比对;
    // 结束复位中文(语言是进程级全局,切换全程在本测试持有的 RENDER_LOCK 内)
    assert_eq!(ui_kit::i18n::t("Tables", "表"), "表");
    // 全程持 RENDER_LOCK:语言是进程级全局,切语言若在锁外,并行运行的其他渲染
    // 测试会随机渲染成英文(同一次运行里前几张图英文、后几张中文),落盘的图
    // 既不可复现也不可用于前后版本的像素对比
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ui_kit::i18n::set_english(true);
    assert_eq!(ui_kit::i18n::t("Tables", "表"), "Tables");
    let mut tab = role_tab();
    tab.changes
        .push((1, "role_name".into(), Some("内容编辑".into())));
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    render_locked("data_table_en", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _actions = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
    ui_kit::i18n::set_english(false);
    assert_eq!(ui_kit::i18n::t("Tables", "表"), "表");
}

#[test]
fn render_mongo_v2_modals() {
    // 1) 索引管理:3 条索引(含 _id_ 不可删/TTL)+ 两条创建键行
    let mut app = test_app(vec![], 0);
    app.mongo_indexes = Some(crate::app::MongoIndexState {
        conn_id: "c1".into(),
        db: Some("dramato".into()),
        collection: "users".into(),
        items: vec![
            shujing_core::mongo::IndexInfo {
                name: "_id_".into(),
                keys: "_id asc".into(),
                unique: true,
                ttl: None,
            },
            shujing_core::mongo::IndexInfo {
                name: "email_unique".into(),
                keys: "email asc".into(),
                unique: true,
                ttl: None,
            },
            shujing_core::mongo::IndexInfo {
                name: "created_ttl".into(),
                keys: "created_at asc".into(),
                unique: false,
                ttl: Some(86400),
            },
        ],
        running: false,
        error: None,
        rows: vec![("user_id".into(), false), ("created_at".into(), true)],
        unique: false,
        ttl: "2592000".into(),
        name: String::new(),
        busy: false,
        drop_arm: None,
    });
    let app = std::cell::RefCell::new(app);
    render("modal_mongo_indexes", 900.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_mongo_indexes_modal(&mut b, ctx);
    });

    // 1b) Kafka 控制台空态(topic 下拉+发送区+空消息提示)
    let mut app = test_app(vec![], 0);
    app.kafka_tab = Some(crate::app::KafkaTab {
        conn_id: "c1".into(),
        topics: vec!["orders".into(), "payments".into()],
        selected: Some("orders".into()),
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
    let tabs = vec![Tab {
        id: "k1".into(),
        title: "Kafka 控制台".into(),
        kind: TabKind::Kafka(
            app.kafka_tab.as_ref().unwrap().clone_shell("k1"),
        ),
    }];
    let app2 = test_app(tabs, 0);
    let app2 = std::cell::RefCell::new(app2);
    render("kafka_console", 900.0, 620.0, true, move |ctx| {
        let mut b = app2.borrow_mut();
        draw_app(&mut b, ctx);
    });

    // 2) 批量导入:预填 JSON 数组
    let mut app = test_app(vec![], 0);
    app.mongo_import = Some(crate::app::MongoImportState {
        conn_id: "c1".into(),
        db: Some("dramato".into()),
        collection: "users".into(),
        json: "[\n  {\"name\": \"张三\", \"age\": 28},\n  {\"name\": \"李四\", \"tags\": [\"vip\"]}\n]".into(),
        src_csv: false,
        running: false,
        error: None,
    });
    let app = std::cell::RefCell::new(app);
    render("modal_mongo_import", 900.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_mongo_import_modal(&mut b, ctx);
    });
}

/// MQTT 控制台:采样列表 + 底部发布表单(与 Kafka 发送区同一套表单语言)
#[test]
fn render_mqtt_console() {
    let mut app = test_app(vec![], 0);
    let mqtt_tab = crate::app::MqttTab {
        conn_id: "c1".into(),
        topics: vec![shujing_core::mqtt::MqttMsg {
            topic: "sensor/temperature".into(),
            payload: "{\"c\": 23.5}".into(),
            bytes: 12,
            retained: false,
            binary: false,
        }],
        selected: Some("sensor/temperature".into()),
        loading: false,
        error: None,
        pub_topic: String::new(),
        pub_payload: String::new(),
        pub_retain: false,
        sending: false,
    };
    let tabs = vec![Tab {
        id: "m1".into(),
        title: "MQTT 控制台".into(),
        kind: TabKind::Mqtt(mqtt_tab),
    }];
    let app = test_app(tabs, 0);
    let app = std::cell::RefCell::new(app);
    render("mqtt_console", 1100.0, 640.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

/// RabbitMQ 控制台:队列列表(选中一行)+ 底部发送表单
#[test]
fn render_rabbit_console() {
    let mut app = test_app(vec![], 0);
    let rabbit_tab = crate::app::RabbitTab {
        conn_id: "c1".into(),
        queues: vec![
            shujing_core::rabbit::RabbitQueue {
                name: "orders".into(),
                vhost: "/".into(),
                messages: 3,
                messages_ready: 3,
                messages_unacked: 0,
                consumers: 1,
                queue_type: "classic".into(),
            },
            shujing_core::rabbit::RabbitQueue {
                name: "payments".into(),
                vhost: "/".into(),
                messages: 0,
                messages_ready: 0,
                messages_unacked: 0,
                consumers: 2,
                queue_type: "quorum".into(),
            },
        ],
        selected: Some(("/".into(), "orders".into())),
        msgs: vec![],
        overview: None,
        loading: false,
        error: None,
        send_key: String::new(),
        send_payload: String::new(),
        sending: false,
        drop_arm: false,
    };
    let tabs = vec![Tab {
        id: "r1".into(),
        title: "RabbitMQ 控制台".into(),
        kind: TabKind::Rabbit(rabbit_tab),
    }];
    let app = test_app(tabs, 0);
    let app = std::cell::RefCell::new(app);
    render("rabbit_console", 1100.0, 640.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

/// Kafka 控制台读过消息之后:消息网格(两行列头 + 类型小字)与底部发送区
#[test]
fn render_kafka_console_msgs() {
    let msg = |offset: i64, key: Option<&str>, payload: &str, time: &str| shujing_core::kafka::StreamMsg {
        topic: "orders".into(),
        partition: 0,
        offset,
        key: key.map(str::to_string),
        payload: Some(payload.to_string()),
        ts_millis: 0,
        time: time.to_string(),
    };
    // 直接把带消息的状态放进标签页:clone_shell 是"状态从零"的空壳,不带消息
    let kafka_tab = crate::app::KafkaTab {
        conn_id: "c1".into(),
        topics: vec!["orders".into(), "payments".into()],
        selected: Some("orders".into()),
        msgs: vec![
            msg(7, Some("order-1001"), "{\"id\": 1001, \"amount\": 199.00}", "11-24 09:12:01"),
            msg(8, None, "{\"id\": 1002, \"amount\": 88.50}", "11-24 09:12:05"),
            msg(9, Some("order-1003"), "{\"id\": 1003, \"amount\": 1200.00}", "11-24 09:12:09"),
        ],
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
    };
    let tabs = vec![Tab {
        id: "k1".into(),
        title: "Kafka 控制台".into(),
        kind: TabKind::Kafka(kafka_tab),
    }];
    let app = test_app(tabs, 0);
    let app = std::cell::RefCell::new(app);
    render("kafka_console_msgs", 1100.0, 620.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn render_data_table_narrow() {
    // 窄视口 + 长 access_urls:验证表格整体溢出时底部出现横向滚动条
    let tab = role_tab();
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    render("data_table_narrow", 760.0, 700.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _ = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
}

/// 宽表夹具:11 列带类型(照 SQL Server 实测那张表),列多到窗口放不下,
/// 每列都会被压到最小宽度 —— 列头最挤的场景
fn wide_tab() -> TableTab {
    let mut tab = TableTab::new("c1".into(), "users".into(), Some("sj_gui".into()));
    tab.columns = [
        "id", "name", "age", "score", "active", "note", "created", "birthday", "amount", "uid",
        "big_col",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    tab.col_types = [
        ("id", "int"),
        ("name", "nvarchar(64)"),
        ("age", "int"),
        ("score", "float"),
        ("active", "bit"),
        ("note", "varchar(100)"),
        ("created", "datetime2"),
        ("birthday", "date"),
        ("amount", "decimal(10,2)"),
        ("uid", "uniqueidentifier"),
        ("big_col", "bigint"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    tab.col_comments.insert("note".to_string(), "备注说明".to_string());
    tab.pk_cols = vec!["id".to_string()];
    tab.pk_loaded = true;
    tab.rows = (0..6)
        .map(|i| {
            vec![
                Some((i + 1).to_string()),
                Some(format!("张三{}", i + 1)),
                Some((18 + i).to_string()),
                Some(format!("{:.1}", 60.0 + i as f32 * 0.75)),
                Some((i % 2).to_string()),
                Some(format!("note {i}")),
                Some("2026-11-01 08:00:30.500".to_string()),
                Some("1990-01-15".to_string()),
                Some(format!("{:.2}", 1000.5 + i as f32)),
                Some("57e10b1c-2402-4aea-9a93-1f2c0f5ab7f0".to_string()),
                Some("9007199254740993".to_string()),
            ]
        })
        .collect();
    tab
}

#[test]
fn render_wide_table() {
    let tab = wide_tab();
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    render("wide_table", 920.0, 480.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _ = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
}

#[test]
fn render_row_detail() {
    let mut tab = role_tab();
    tab.detail = true;
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    render("row_detail", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _ = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
}

#[test]
fn render_structure_view() {
    use shujing_core::{ColumnDef, IndexDef, TableStructure};
    let mut s = StructureTab {
        conn_id: "c1".into(),
        table: "backend_role_privilege".into(),
        database: Some("dramato".into()),
        data: None,
        fks: vec![],
        loading: false,
        error: None,
    };
    s.data = Some(TableStructure {
        columns: vec![
            ColumnDef { name: "id".into(), data_type: "int unsigned".into(), nullable: false, key: "PRI".into(), default: None, extra: "auto_increment".into(), comment: "".into() },
            ColumnDef { name: "role".into(), data_type: "varchar(64)".into(), nullable: false, key: "MUL".into(), default: None, extra: "".into(), comment: "角色编码".into() },
            ColumnDef { name: "role_name".into(), data_type: "varchar(128)".into(), nullable: false, key: "".into(), default: Some("".into()), extra: "".into(), comment: "角色名称".into() },
            ColumnDef { name: "access_urls".into(), data_type: "text".into(), nullable: true, key: "".into(), default: None, extra: "".into(), comment: "可访问路径,逗号分隔".into() },
            ColumnDef { name: "status".into(), data_type: "tinyint".into(), nullable: false, key: "".into(), default: Some("1".into()), extra: "".into(), comment: "1启用 2禁用".into() },
            ColumnDef { name: "created".into(), data_type: "bigint".into(), nullable: false, key: "".into(), default: None, extra: "".into(), comment: "创建时间戳".into() },
        ],
        indexes: vec![
            IndexDef { name: "PRIMARY".into(), columns: "id".into(), unique: true },
            IndexDef { name: "idx_role".into(), columns: "role".into(), unique: false },
            IndexDef { name: "uk_role_name".into(), columns: "role_name".into(), unique: true },
        ],
        ddl: "CREATE TABLE `backend_role_privilege` (\n  `id` int unsigned NOT NULL AUTO_INCREMENT,\n  `role` varchar(64) NOT NULL COMMENT '角色编码',\n  `role_name` varchar(128) NOT NULL COMMENT '角色名称',\n  `access_urls` text COMMENT '可访问路径,逗号分隔',\n  `status` tinyint NOT NULL DEFAULT '1',\n  `created` bigint NOT NULL,\n  `updated` bigint NOT NULL,\n  PRIMARY KEY (`id`),\n  KEY `idx_role` (`role`),\n  UNIQUE KEY `uk_role_name` (`role_name`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;".into(),
    });
    let cell = std::cell::RefCell::new(Some(s));
    render("structure", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let s = b.as_mut().unwrap();
        let _ = structure::draw_structure_view(ctx, "s1", s, None);
    });
}

/// 查询页夹具:带类型的结果集(类型名是 MySQL 文本协议的写法)
fn query_view_fixture() -> (QueryTab, Vec<shujing_core::ConnInfo>, Vec<String>) {
    let mut q = QueryTab {
        conn_id: "c1".into(),
        database: None,
        sql: "SELECT id, role, role_name, status\nFROM backend_role_privilege\nWHERE status = 1\nORDER BY created DESC\nLIMIT 50;".into(),
        history: vec!["SELECT * FROM backend_role_privilege LIMIT 10;".into()],
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
    };
    let rows: Vec<Vec<Option<String>>> = (0..24)
        .map(|i| {
            vec![
                Some((i + 1).to_string()),
                Some(format!("role_{i}")),
                if i % 4 == 0 {
                    None
                } else {
                    Some(format!("角色 {i}"))
                },
                Some(if i % 3 == 0 { "1" } else { "2" }.to_string()),
                Some("1731383330".to_string()),
            ]
        })
        .collect();
    q.results = vec![shujing_core::ExecResult {
        columns: vec![
            "id".into(),
            "role".into(),
            "role_name".into(),
            "status".into(),
            "created".into(),
        ],
        // 驱动自报的类型(MySQL 文本协议写法):表头第二行的小字
        column_types: vec![
            "int unsigned".into(),
            "varchar".into(),
            "varchar".into(),
            "tinyint".into(),
            "datetime".into(),
        ],
        rows,
        affected: 0,
        truncated: false,
        elapsed_ms: 42,
    }];
    let conns = vec![shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: Some("dramato".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    }];
    let dbs = vec![
        "dramato".to_string(),
        "shop".to_string(),
        "stats".to_string(),
    ];
    (q, conns, dbs)
}

fn render_query_view_in(name: &str, dark: bool) {
    let (q, conns, dbs) = query_view_fixture();
    let cell = std::cell::RefCell::new(Some(q));
    render(name, 1280.0, 780.0, dark, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(
            &conns,
            ctx,
            "q1",
            q,
            &[],
            &dbs,
            true,
            &[],
            &Default::default(),
            None,
            false,
        );
    });
}

#[test]
fn render_query_view() {
    render_query_view_in("query", true);
}

/// 浅色主题:类型小字的浅色色板(深而饱和)与表头/斑马纹的浅灰层次
#[test]
fn render_query_view_light() {
    render_query_view_in("query_light", false);
}

/// 带库下拉的查询工具栏在窄窗口下不应溢出/换行塌陷
#[test]
fn render_query_view_narrow_with_db() {
    let mut q = QueryTab {
        conn_id: "c1".into(),
        database: Some("dramato".into()),
        sql: "SELECT 1".into(),
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
    };
    let conns = vec![shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: None,
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    }];
    let cell = std::cell::RefCell::new(Some(q));
    let dbs = vec!["dramato".to_string(), "shop".to_string()];
    render("query_narrow_db", 1024.0, 700.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(
            &conns,
            ctx,
            "q1",
            q,
            &[],
            &dbs,
            true,
            &[],
            &Default::default(),
            None,
            false,
        );
    });
}

#[test]
fn render_conn_modal() {
    // 三个分区 tab 各出一张:连接信息 / SSH 隧道(展开) / 高级
    for (tab, name) in [(0u8, "conn_modal"), (1, "conn_modal_ssh"), (2, "conn_modal_adv")] {
        let mut m = crate::app::ConnModal::new();
        m.name = "dramato-prod".into();
        m.host = "10.0.0.8".into();
        m.port = "3306".into();
        m.user = "readonly".into();
        m.password = "secret".into();
        m.database = "dramato".into();
        m.test_result = Some(Ok("连接成功: MySQL 8.0.36".into()));
        m.tab = tab;
        m.ssh_enabled = tab == 1;
        m.ssh_host = "jump.example.com".into();
        m.ssh_port = "22".into();
        m.ssh_user = "deploy".into();
        let cell = std::cell::RefCell::new(Some(m));
        let app = std::cell::RefCell::new(full_app("role"));
        render(name, 1000.0, 720.0, true, move |ctx| {
            let mut b = app.borrow_mut();
            draw_app(&mut b, ctx);
            let mut mb = cell.borrow_mut();
            let _ = table_view::draw_conn_modal(ctx, mb.as_mut().unwrap());
        });
    }
}

#[test]
fn render_app_forms() {
    // 依赖应用状态的表单弹窗各出一张:数据传输 / 数据对比 / 结构对比 / 加密备份
    for (name, kind) in [
        ("modal_transfer", 0u8),
        ("modal_data_diff", 1),
        ("modal_schema_diff", 2),
        ("modal_conn_crypto", 3),
    ] {
        let mut app = full_app("role");
        let cid = app
            .conns
            .iter()
            .find(|c| c.db_type == shujing_core::DbType::MySql)
            .map(|c| c.id.clone())
            .unwrap_or_default();
        match kind {
            0 => app.open_transfer(&cid, Some("dramato"), "backend_role_privilege"),
            1 => app.open_data_diff(&cid, Some("dramato"), "backend_role_privilege"),
            2 => app.open_schema_diff(&cid, Some("dramato"), "backend_role_privilege"),
            // 不能走 export_connections_encrypted():它会立刻弹原生保存对话框,测试环境起不来
            _ => {
                app.conn_crypto = Some(crate::app::ConnCryptoState {
                    export: true,
                    passphrase: String::new(),
                    confirm: String::new(),
                    error: None,
                })
            }
        }
        let cell = std::cell::RefCell::new(app);
        render(name, 1000.0, 720.0, true, move |ctx| {
            let mut b = cell.borrow_mut();
            draw_app(&mut b, ctx);
            match kind {
                0 => crate::modals::draw_transfer_modal(&mut b, ctx),
                1 => crate::modals::draw_data_diff_modal(&mut b, ctx),
                2 => crate::modals::draw_schema_diff_modal(&mut b, ctx),
                _ => crate::modals::draw_conn_crypto_modal(&mut b, ctx),
            }
        });
    }
}

#[test]
fn render_insert_form() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.pk_cols = vec!["id".into()];
        t.pk_loaded = true;
        t.col_types = [
            ("id", "int unsigned"),
            ("role", "varchar(64)"),
            ("role_name", "varchar(128)"),
            ("access_urls", "text"),
            ("status", "tinyint"),
            ("created", "bigint"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        t.insert_form = Some(
            t.columns
                .iter()
                .map(|c| (c.clone(), String::new()))
                .collect(),
        );
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let img = harness.render().expect("render 失败");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/insert_form.png")
        .expect("保存 PNG 失败");
}

#[test]
fn render_db_filter_modal() {
    let mut app = full_app("role");
    app.db_filter = Some(crate::app::DbFilterState {
        conn_id: "c1".into(),
        search: String::new(),
        all: vec![
            "dramato".into(),
            "archive".into(),
            "comic".into(),
            "mysql".into(),
            "information_schema".into(),
            "performance_schema".into(),
        ],
        selected: vec!["dramato".into(), "archive".into(), "comic".into()],
    });
    let cell = std::cell::RefCell::new(app);
    render("db_filter", 1000.0, 720.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_db_filter_modal(&mut b, ctx);
    });
}

#[test]
fn render_designer_edit() {
    let mut app = test_app(vec![], 0);
    // 复用已有 c1;不能 push 一个同 id 的连接,否则侧栏漏斗会触发重复 widget ID 警告
    if let Some(c) = app.conns.iter_mut().find(|c| c.id == "c1") {
        c.name = "dramato-dev".into();
        c.db_type = shujing_core::DbType::MySql;
        c.host = Some("127.0.0.1".into());
        c.port = Some(3306);
        c.user = Some("root".into());
        c.password = None;
        c.database = None;
        c.file_path = None;
    }
    app.open_designer("c1", Some("users"), Some("dramato".to_string()));
    if let TabKind::Designer(d) = &mut app.tabs.last_mut().unwrap().kind {
        d.loading = false;
        d.loaded = true;
        d.columns = vec![
            crate::app::DesignerColumn {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                pk: true,
                auto_inc: true,
                existing: true,
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "name".into(),
                data_type: "VARCHAR".into(),
                length: "255".into(),
                nullable: true,
                comment: "姓名".into(),
                existing: true,
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "email".into(),
                data_type: "VARCHAR".into(),
                length: "320".into(),
                nullable: true,
                ..Default::default()
            },
        ];
    }
    let app = std::cell::RefCell::new(app);
    render("designer_edit", 1200.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        crate::designer::stash_dialect(ctx, shujing_core::DbType::MySql);
        draw_app(&mut b, ctx);
    });
}



/// 交互验证:整页滚动到底后,长 DDL 应自然展开(不再被 320px 内嵌滚动区压缩)
/// 结构页没有索引/建表语句时的两种口径:数据源不提供(ClickHouse 暂未做;
/// SQL Server 批次 78 起已支持)与 真的没有(如 SQLite 的无索引表)。
/// 都不能画成空框或"索引 0"
#[test]
fn render_structure_empty_states() {
    use shujing_core::{ColumnDef, TableStructure};
    let mk = || StructureTab {
        conn_id: "c1".into(),
        table: "users".into(),
        database: Some("sj_gui".into()),
        data: Some(TableStructure {
            columns: vec![
                ColumnDef { name: "id".into(), data_type: "int".into(), nullable: false, key: "PRI".into(), default: None, extra: "".into(), comment: "".into() },
                ColumnDef { name: "name".into(), data_type: "nvarchar(64)".into(), nullable: true, key: "".into(), default: None, extra: "".into(), comment: "".into() },
            ],
            indexes: vec![],
            ddl: String::new(),
        }),
        fks: vec![],
        loading: false,
        error: None,
    };
    let cell = std::cell::RefCell::new(Some(mk()));
    render("structure_unsupported", 900.0, 420.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let s = b.as_mut().unwrap();
        let _ = structure::draw_structure_view(ctx, "s2", s, Some(shujing_core::DbType::ClickHouse));
    });
    let cell = std::cell::RefCell::new(Some(mk()));
    render("structure_no_index", 900.0, 420.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let s = b.as_mut().unwrap();
        let _ = structure::draw_structure_view(ctx, "s3", s, Some(shujing_core::DbType::Sqlite));
    });
}

#[test]
fn structure_long_ddl_scroll_to_bottom() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cols: Vec<shujing_core::ColumnDef> = (0..30)
        .map(|i| shujing_core::ColumnDef {
            name: format!("col_{i}"),
            data_type: "int".into(),
            nullable: true,
            key: if i == 0 { "PRI".into() } else { String::new() },
            default: None,
            extra: String::new(),
            comment: format!("列 {i}"),
        })
        .collect();
    let ddl = format!(
        "CREATE TABLE `wide_table` (\n{}\n) ENGINE=InnoDB;",
        (0..30)
            .map(|i| format!("  `col_{i}` int DEFAULT NULL COMMENT '列 {i}'"))
            .collect::<Vec<_>>()
            .join(",\n")
    );
    let cell = std::cell::RefCell::new(test_app(
        vec![Tab {
            id: "t-struct2".into(),
            title: "wide_table 结构".into(),
            kind: TabKind::Structure(StructureTab {
                conn_id: "c1".into(),
                table: "wide_table".into(),
                database: Some("dramato".into()),
                data: Some(shujing_core::TableStructure {
                    columns: cols,
                    indexes: vec![],
                    ddl,
                }),
                fks: vec![],
                loading: false,
                error: None,
            }),
        }],
        0,
    ));
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1100.0, 720.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    // 指针放内容区,滚轮滚到底(30 字段+DDL 约 30 行,滚 6000px 足够)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(700.0, 400.0)));
    harness.step();
    for _ in 0..50 {
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            modifiers: Default::default(),
        });
        harness.step();
    }
    let img = harness.render().expect("render 失败");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/structure_ddl_bottom.png")
        .expect("保存 PNG 失败");
}

/// 结构页长 DDL:全高展开由整页滚动(原内嵌 320px 小滚动区已删),渲染自检

/// 弹窗渲染巡检第二轮:Redis 新建 key(多行预览)/Redis 扫描分析(20 条结果条形图)/
/// 外观设置/快速打开——同"内容多被压小/截断"类问题的批量视觉自检
#[test]
fn render_modals_visual_sweep2() {
    // 1) Redis 新建 key:hash 类型多行输入 + 计数
    let mut app = test_app(vec![], 0);
    app.open_redis("c1", 0);
    if let TabKind::Redis(rt) = &mut app.tabs.last_mut().unwrap().kind {
        rt.keys = vec!["user:1001".into()];
        rt.selected_key = Some("user:1001".into());
        rt.new_key = Some(crate::app::RedisNewKeyState {
            name: "session:cart:9f2".into(),
            kind: 1,
            text: String::new(),
            lines: (0..8)
                .map(|i| format!("field_{i}=value_{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }
    let app = std::cell::RefCell::new(app);
    render("modal_redis_newkey", 900.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });

    // 2) Redis 扫描分析:大 key 模式 20 条结果
    let mut app = test_app(vec![], 0);
    app.open_redis("c1", 0);
    if let TabKind::Redis(rt) = &mut app.tabs.last_mut().unwrap().kind {
        rt.analyze = Some(crate::app::RedisAnalyzeState {
            mode: 0,
            sample: 1000,
            running: false,
            ran: true,
            stats: (0..20)
                .map(|i| shujing_core::redis_ops::KeyStat {
                    key: format!("big:cache:user:{i}:profile"),
                    key_type: "hash".into(),
                    mem: 900_000 - i * 40_000,
                    len: 5000 - i * 200,
                    freq: -1,
                })
                .collect(),
            error: None,
        });
    }
    let app = std::cell::RefCell::new(app);
    render("modal_redis_analyze", 900.0, 760.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });

    // 3) 外观设置
    let mut app = test_app(vec![], 0);
    app.settings_open = true;
    let app = std::cell::RefCell::new(app);
    render("modal_settings", 700.0, 560.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_settings_modal(&mut b, ctx);
    });

    // 3b) 备份弹窗(进行中态)
    let mut app = test_app(vec![], 0);
    app.backup = Some(crate::app::BackupState {
        conn_id: "c1".into(),
        conn_name: "本地库".into(),
        db: "main".into(),
        running: true,
        done: 2,
        total: 7,
        cur_table: "users".into(),
        result: None,
        recent: vec![shujing_core::backup::BackupFileMeta {
            name: "本地库-main-20261121-120000.sql".into(),
            path: "/tmp/demo.sql".into(),
            bytes: 12_345,
            modified_ms: 1_763_700_000_000,
        }],
    });
    let app = std::cell::RefCell::new(app);
    render("modal_backup", 700.0, 560.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_backup_modal(&mut b, ctx);
    });

    // 3c) 插件处理弹窗(输出态)
    let mut app = test_app(vec![], 0);
    app.plugin_run = Some(crate::app::PluginRunState {
        tab_id: "q1".into(),
        input: r#"{"columns":["name"],"rows":[["张三"]]}"#.into(),
        plugins: vec![shujing_core::plugins::PluginInfo {
            name: "rows-to-markdown".into(),
            version: "0.1.0".into(),
            description: "行集转 Markdown".into(),
            dir: "/tmp".into(),
            ok: true,
            err: None,
        }],
        sel: 0,
        output: Some("| name |\n|---|\n| 张三 |".into()),
        error: None,
        running: false,
    });
    let app = std::cell::RefCell::new(app);
    render("modal_plugin", 700.0, 560.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_plugin_modal(&mut b, ctx);
    });

    // 3d) 查询历史弹窗
    let mut app = test_app(vec![], 0);
    app.query_history = Some(crate::app::HistoryState {
        tab_id: "q1".into(),
        items: vec![
            shujing_core::history::HistoryEntry {
                ts: 1_763_700_000_000,
                conn: "dramato-dev".into(),
                db: "dramato".into(),
                sql: "SELECT id, name FROM users WHERE name = '张三' LIMIT 10".into(),
                ok: true,
                rows: 3,
                elapsed_ms: 12,
                ..Default::default()
            },
            shujing_core::history::HistoryEntry {
                ts: 1_763_690_000_000,
                conn: "dramato-dev".into(),
                db: "dramato".into(),
                sql: "UPDATE users SET score = 0 WHERE id = 5".into(),
                ok: false,
                err: "权限不足".into(),
                ..Default::default()
            },
        ],
        filter: String::new(),
    });
    let app = std::cell::RefCell::new(app);
    render("modal_query_history", 760.0, 560.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_history_modal(&mut b, ctx);
    });

    // 4) 快速打开(输入过滤态)
    let mut app = test_app(vec![], 0);
    app.quick_open = Some(crate::app::QuickOpen {
        query: "us".into(),
        sel: 0,
    });
    let app = std::cell::RefCell::new(app);
    render("modal_quick_open", 800.0, 620.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_quick_open(&mut b, ctx);
    });
}

#[test]
fn render_structure_long_ddl() {
    let cols: Vec<shujing_core::ColumnDef> = (0..30)
        .map(|i| shujing_core::ColumnDef {
            name: format!("col_{i}"),
            data_type: if i % 3 == 0 { "varchar(64)".into() } else { "int".into() },
            nullable: i % 2 == 0,
            key: if i == 0 { "PRI".into() } else { String::new() },
            default: None,
            extra: if i == 0 { "auto_increment".into() } else { String::new() },
            comment: format!("列 {i}"),
        })
        .collect();
    let ddl = format!(
        "CREATE TABLE `wide_table` (\n{}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;",
        (0..30)
            .map(|i| format!("  `col_{i}` int DEFAULT NULL COMMENT '列 {i}'"))
            .collect::<Vec<_>>()
            .join(",\n")
    );
    let mut app = test_app(
        vec![Tab {
            id: "t-struct1".into(),
            title: "wide_table 结构".into(),
            kind: TabKind::Structure(StructureTab {
                conn_id: "c1".into(),
                table: "wide_table".into(),
                database: Some("dramato".into()),
                data: Some(shujing_core::TableStructure {
                    columns: cols,
                    indexes: vec![],
                    ddl,
                }),
                fks: vec![],
                loading: false,
                error: None,
            }),
        }],
        0,
    );
    let _ = &mut app;
    let app = std::cell::RefCell::new(app);
    render("structure_long_ddl", 1100.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}


/// 弹窗渲染巡检:优化分析(大执行计划 20 行)/列统计(12 行)/导入预览(12 列映射)/
/// 新建表(长 SQL)——同类"内容多被压小/不可滚"问题的批量视觉自检
#[test]
fn render_modals_visual_sweep() {
    // 1) 优化分析弹窗:MySQL 计划 8 列 x 20 行
    let plan = shujing_core::ExecResult {
        columns: vec![
            "id".into(), "select_type".into(), "table".into(), "type".into(),
            "key".into(), "rows".into(), "filtered".into(), "Extra".into(),
        ],
        rows: (0..20)
            .map(|i| {
                vec![
                    Some(format!("{i}")),
                    Some("SIMPLE".into()),
                    Some(format!("big_table_{i}")),
                    Some("ALL".into()),
                    None,
                    Some(format!("{}", 10000 + i * 137)),
                    Some("100.00".into()),
                    Some("Using where; Using temporary; Using filesort".into()),
                ]
            })
            .collect(),
        affected: 0,
        truncated: false,
        elapsed_ms: 42,
        column_types: vec![],
    };
    let mut q = test_query_tab();
    q.sql = "SELECT * FROM big_table_0 WHERE a = 1 ORDER BY b".into();
    q.optimize = Some(crate::app::OptimizeState {
        running: false,
        ran: true,
        plan: Some(plan),
        error: None,
        frozen: 0,
    });
    let conns = test_conns();
    let q = std::cell::RefCell::new(q);
    render("modal_optimize", 1100.0, 760.0, true, move |ctx| {
        let mut q = q.borrow_mut();
        let _ = query::draw_query_view(&conns, ctx, "tq1", &mut q, &[], &[], false, &[], &Default::default(), None, false);
    });
    // 1b) 实时诊断条:UPDATE 无 WHERE + 括号不配对 → 红/橙提示(恒定占位不跳布局)
    let mut q = test_query_tab();
    q.sql = "UPDATE t SET a=1;\nSELECT (1 FROM b".into();
    let conns = test_conns();
    let q = std::cell::RefCell::new(q);
    render("query_lint_bar", 900.0, 620.0, true, move |ctx| {
        let mut q = q.borrow_mut();
        let _ = query::draw_query_view(&conns, ctx, "tq1b", &mut q, &[], &[], false, &[], &Default::default(), None, false);
    });
    // 1c) 干净 SQL:诊断条只占位无文字
    let mut q = test_query_tab();
    q.sql = "SELECT id, name FROM users WHERE id = 1".into();
    let conns = test_conns();
    let q = std::cell::RefCell::new(q);
    render("query_lint_clean", 900.0, 620.0, true, move |ctx| {
        let mut q = q.borrow_mut();
        let _ = query::draw_query_view(&conns, ctx, "tq1c", &mut q, &[], &[], false, &[], &Default::default(), None, false);
    });

    // 2) 列统计弹窗(12 行)
    let mut app = test_app(vec![], 0);
    let mut t = crate::app::TableTab::new("c1".into(), "users".into(), Some("dramato".into()));
    t.loading = false;
    t.col_stats = Some((
        "status 列统计(当前页 500 行)".into(),
        (0..12)
            .map(|i| (format!("取值_{i}"), format!("{} 次 ({}%)", 40 - i * 3, 8 - i / 2)))
            .collect(),
    ));
    app.tabs.push(Tab {
        id: "t-stats".into(),
        title: "users".into(),
        kind: TabKind::Table(t),
    });
    app.active_tab = app.tabs.len() - 1;
    let app = std::cell::RefCell::new(app);
    render("modal_col_stats", 700.0, 640.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        // draw_app 简化路径不含弹窗,直接调绘制函数
        crate::modals::draw_col_stats_modal(&mut b, ctx);
    });

    // 3) 导入预览弹窗(12 源列 -> 12 目标列映射)
    let mut app = test_app(vec![], 0);
    let headers: Vec<String> = (0..12).map(|i| format!("src_col_{i}")).collect();
    let n = headers.len();
    app.import = Some(crate::app::ImportState {
        conn_id: "c1".into(),
        database: Some("dramato".into()),
        table: "users".into(),
        path: "/tmp/big_export.csv".into(),
        headers,
        running: false,
        col_map: (0..n).map(|i| Some(i)).collect(),
        batch_errors: None,
        target_cols: (0..n).map(|i| format!("target_col_{i}")).collect(),
        parsing: false,
    });
    let app = std::cell::RefCell::new(app);
    render("modal_import", 900.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_import_modal(&mut b, ctx);
    });

    // 4) 新建表弹窗(长 SQL 模板)
    let mut app = test_app(vec![], 0);
    app.new_table = Some(crate::app::NewTableState {
        conn_id: "c1".into(),
        database: Some("dramato".into()),
        name: String::new(),
        sql: "CREATE TABLE `new_table` (\n  `id` int NOT NULL AUTO_INCREMENT,\n  `name` varchar(255) DEFAULT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;".into(),
    });
    let app = std::cell::RefCell::new(app);
    render("modal_new_table", 700.0, 600.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        crate::modals::draw_new_table_modal(&mut b, ctx);
    });
}

fn test_query_tab() -> QueryTab {
    QueryTab {
        conn_id: "c1".into(),
        database: None,
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
    }
}

fn test_conns() -> Vec<shujing_core::ConnInfo> {
    vec![shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: Some("dramato".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    }]
}

#[test]
fn render_designer_create() {
    let mut app = test_app(vec![], 0);
    app.open_designer("c1", None, Some("dramato".to_string()));
    if let TabKind::Designer(d) = &mut app.tabs.last_mut().unwrap().kind {
        d.loading = false;
        d.loaded = true;
        d.columns = vec![
            crate::app::DesignerColumn {
                name: "id".into(),
                data_type: "INT".into(),
                nullable: false,
                pk: true,
                auto_inc: true,
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "title".into(),
                data_type: "VARCHAR".into(),
                length: "200".into(),
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "created_at".into(),
                data_type: "DATETIME".into(),
                ..Default::default()
            },
        ];
    }
    let app = std::cell::RefCell::new(app);
    render("designer_create", 1200.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        crate::designer::stash_dialect(ctx, shujing_core::DbType::MySql);
        draw_app(&mut b, ctx);
    });
}

/// 回归:新建模式删除中间列曾因 for-range 上限定死而越界 panic
/// 删除按钮逻辑坐标(从 designer_create.png 校准,1200×700 视口)
const DESIGNER_DEL_X: f32 = 1134.0;
const DESIGNER_DEL_Y0: f32 = 140.0;
#[test]
fn designer_delete_column_no_panic() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app(vec![], 0);
    app.open_designer("c1", None, Some("dramato".to_string()));
    if let TabKind::Designer(d) = &mut app.tabs.last_mut().unwrap().kind {
        d.loading = false;
        d.loaded = true;
        d.columns = vec![
            crate::app::DesignerColumn {
                name: "id".into(),
                data_type: "INT".into(),
                nullable: false,
                pk: true,
                auto_inc: true,
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "title".into(),
                data_type: "VARCHAR".into(),
                length: "200".into(),
                ..Default::default()
            },
            crate::app::DesignerColumn {
                name: "created_at".into(),
                data_type: "DATETIME".into(),
                ..Default::default()
            },
        ];
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                crate::designer::stash_dialect(ctx, shujing_core::DbType::MySql);
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    // 坐标从 designer_create.png 校准:第 0 行删除按钮(逻辑坐标)
    let click = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    click(&mut harness, egui::pos2(DESIGNER_DEL_X, DESIGNER_DEL_Y0));
    harness.run_steps(3);
    let b = harness.state();
    let n = b
        .borrow()
        .tabs
        .last()
        .map(|t| match &t.kind {
            TabKind::Designer(d) => d.columns.len(),
            _ => usize::MAX,
        })
        .unwrap_or(usize::MAX);
    assert_eq!(n, 2, "点击第 0 行删除后应剩 2 列且不 panic");
}

#[test]
fn render_redis_panel() {
    let mut app = test_app(vec![], 0);
    app.open_redis("c1", 0);
    if let TabKind::Redis(rt) = &mut app.tabs.last_mut().unwrap().kind {
        rt.keys = vec![
            "user:1001".into(),
            "user:1002".into(),
            "session:abc".into(),
            "cache:home:banner".into(),
        ];
        rt.selected_key = Some("user:1001".into());
        rt.key_types = std::collections::HashMap::from([
            ("user:1001".to_string(), "hash".to_string()),
            ("user:1002".to_string(), "string".to_string()),
            ("session:abc".to_string(), "string".to_string()),
            ("cache:home:banner".to_string(), "list".to_string()),
        ]);
        rt.key_ttls = std::collections::HashMap::from([
            ("user:1001".to_string(), -1i64),
            ("session:abc".to_string(), 120i64),
        ]);
        rt.detail = Some(shujing_core::redis_ops::RedisDetail {
            key_type: "hash".into(),
            ttl: -1,
            len: 3,
            text: None,
            pairs: vec![
                ("name".into(), "张三".into()),
                ("city".into(), "北京".into()),
                ("level".into(), "9".into()),
            ],
        });
    }
    let app = std::cell::RefCell::new(app);
    render("redis_panel", 1200.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

/// 同一面板的浅色主题:类型徽标的淡彩底在浅色下要能看清又不刺眼
#[test]
fn render_redis_panel_light() {
    let mut app = test_app(vec![], 0);
    app.open_redis("c1", 0);
    if let TabKind::Redis(rt) = &mut app.tabs.last_mut().unwrap().kind {
        rt.keys = vec![
            "user:1001".into(),
            "user:1002".into(),
            "session:abc".into(),
            "cache:home:banner".into(),
        ];
        rt.selected_key = Some("user:1002".into());
        rt.key_types = std::collections::HashMap::from([
            ("user:1001".to_string(), "hash".to_string()),
            ("user:1002".to_string(), "string".to_string()),
            ("session:abc".to_string(), "zset".to_string()),
            ("cache:home:banner".to_string(), "stream".to_string()),
        ]);
        rt.key_ttls = std::collections::HashMap::from([
            ("user:1001".to_string(), -1i64),
            ("session:abc".to_string(), 120i64),
        ]);
        rt.detail = Some(shujing_core::redis_ops::RedisDetail {
            key_type: "string".into(),
            ttl: 3600,
            len: 0,
            text: Some("hello".into()),
            pairs: vec![],
        });
    }
    let app = std::cell::RefCell::new(app);
    render("redis_panel_light", 1200.0, 700.0, false, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn render_er_diagram() {
    let mut app = test_app(vec![], 0);
    app.open_er("c1", Some("dramato".to_string()));
    use shujing_core::schema::FkMeta;
    let col = |n: &str, key: &str| shujing_core::ColumnDef {
        name: n.into(),
        data_type: "int".into(),
        nullable: false,
        key: key.into(),
        default: None,
        extra: String::new(),
        comment: String::new(),
    };
    if let TabKind::Er(e) = &mut app.tabs.last_mut().unwrap().kind {
        e.loading = false;
        e.tables = vec![
            "users".into(),
            "orders".into(),
            "order_items".into(),
            "products".into(),
        ];
        e.fks = vec![
            FkMeta {
                table: "orders".into(),
                column: "user_id".into(),
                ref_table: "users".into(),
                ref_column: "id".into(),
            },
            FkMeta {
                table: "order_items".into(),
                column: "order_id".into(),
                ref_table: "orders".into(),
                ref_column: "id".into(),
            },
            FkMeta {
                table: "order_items".into(),
                column: "product_id".into(),
                ref_table: "products".into(),
                ref_column: "id".into(),
            },
        ];
        e.structures = std::collections::HashMap::from([
            (
                "users".to_string(),
                shujing_core::TableStructure {
                    columns: vec![
                        col("id", "PRI"),
                        col("name", ""),
                        col("email", ""),
                        col("created", ""),
                    ],
                    indexes: vec![],
                    ddl: String::new(),
                },
            ),
            (
                "orders".to_string(),
                shujing_core::TableStructure {
                    columns: vec![
                        col("id", "PRI"),
                        col("user_id", ""),
                        col("total", ""),
                        col("status", ""),
                    ],
                    indexes: vec![],
                    ddl: String::new(),
                },
            ),
            (
                "order_items".to_string(),
                shujing_core::TableStructure {
                    columns: vec![
                        col("id", "PRI"),
                        col("order_id", ""),
                        col("product_id", ""),
                        col("qty", ""),
                    ],
                    indexes: vec![],
                    ddl: String::new(),
                },
            ),
            (
                "products".to_string(),
                shujing_core::TableStructure {
                    columns: vec![col("id", "PRI"), col("title", ""), col("price", "")],
                    indexes: vec![],
                    ddl: String::new(),
                },
            ),
        ]);
    }
    let app = std::cell::RefCell::new(app);
    render("er_diagram", 1200.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn render_filter_bar() {
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.filter_bar_open = true;
        t.filter_mode = 0;
        t.conds = vec![
            ("status".into(), "=".into(), "1".into()),
            ("role_name".into(), "LIKE".into(), "admin%".into()),
        ];
    }
    let app = std::cell::RefCell::new(app);
    render("filter_bar", 1200.0, 700.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
    });
}

#[test]
fn close_tab_keeps_active_tab() {
    let mut app = test_app(
        vec![
            stub_tab("a", "ads_report_youni"),
            stub_tab("b", "campaign_report"),
            stub_tab("c", "backend_role_privilege"),
        ],
        2,
    );
    let active_id = app.tabs[app.active_tab].id.clone();
    app.close_tab(0);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs[app.active_tab].id, active_id);
}

#[test]
fn deleted_badge_is_inverted() {
    use crate::theme::badge_value;
    assert_eq!(
        badge_value("deleted", &Some("1".into())),
        Some(("1".to_string(), false))
    );
    assert_eq!(
        badge_value("deleted", &Some("0".into())),
        Some(("0".to_string(), true))
    );
    assert_eq!(
        badge_value("status", &Some("2".into())),
        Some(("2".to_string(), false))
    );
    assert_eq!(
        badge_value("status", &Some("1".into())),
        Some(("1".to_string(), true))
    );
}

#[test]
fn session_save_restore_roundtrip() {
    let dir = std::env::temp_dir().join(format!("shujing_sess_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let mut app = test_app(vec![], 0);
    app.config_dir = dir.clone();
    // 一个查询标签 + 一个表标签
    let mut q = QueryTab {
        conn_id: "c1".into(),
        database: None,
        sql: "SELECT 1".into(),
        history: vec!["SELECT 2".into()],
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
    };
    q.sql = "SELECT id FROM backend_role_privilege".into();
    app.tabs.push(Tab {
        id: "q1".into(),
        title: "查询 1".into(),
        kind: TabKind::Query(q),
    });
    app.tabs.push(stub_tab("role", "backend_role_privilege"));
    // 冻结列随会话持久化
    if let TabKind::Table(t) = &mut app.tabs.last_mut().unwrap().kind {
        t.frozen_vis = 2;
    }
    app.active_tab = 1;
    app.trees.entry("c1".to_string()).or_default().online = true;
    app.save_session();
    assert!(dir.join("session.json").exists());

    // 还原:直接解析(不走 new(),避免真正连库)
    let json = std::fs::read_to_string(dir.join("session.json")).unwrap();
    #[derive(serde::Deserialize)]
    struct SD {
        tabs: Vec<serde_json::Value>,
        active: usize,
        online: Vec<String>,
    }
    let d: SD = serde_json::from_str(&json).unwrap();
    assert_eq!(d.tabs.len(), 2);
    assert_eq!(d.active, 1);
    assert_eq!(d.online, vec!["c1".to_string()]);
    assert_eq!(d.tabs[0]["kind"], "query");
    assert_eq!(d.tabs[1]["kind"], "table");
    assert_eq!(d.tabs[1]["frozen"], 2, "冻结列数应随会话持久化");
    assert_eq!(d.tabs[0]["sql"], "SELECT id FROM backend_role_privilege");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tabbar_wheel_ignored_when_pointer_over_table() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 标签栏的“滚轮横滚”只能在指针位于标签栏时生效;
    // 否则滚表格时标签栏也会跟着横移(用户反馈的问题)。
    let mut app = full_app("role");
    for i in 0..8 {
        app.tabs.push(stub_tab(
            &format!("辅助表 {i}"),
            &format!("extra_table_{i}"),
        ));
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let before = harness.render().expect("render before");
    let width = before.width() as usize;
    let top_len = (crate::theme::tabbar_h() * 2.0) as usize * width * 4;
    let before_top = before.as_raw()[..top_len].to_vec();

    // 指针放到表格内容上,注入纵向滚轮(标签栏不应响应)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(600.0, 380.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let after = harness.render().expect("render after");
    assert_eq!(
        before_top,
        &after.as_raw()[..top_len],
        "滚动表格时标签栏不应跟着横移"
    );
}

#[test]
fn data_grid_hscroll_works() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 宽表:每个格塞长内容,总宽远超视口
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        let filler = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string();
        let new_rows: Vec<Vec<Option<String>>> = t
            .rows
            .iter()
            .map(|r| r.iter().map(|_| Some(filler.clone())).collect())
            .collect();
        t.rows = new_rows;
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let before = harness.render().expect("render before");

    // 指针移到网格中心,注入横向滚轮
    let center = egui::pos2(500.0, 350.0);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(center));
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-400.0, 0.0),
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let after = harness.render().expect("render after");
    assert_ne!(
        before.into_raw(),
        after.into_raw(),
        "横向滚轮后画面应发生变化(能左右滚动)"
    );

    // 数据网格按住拖动 = 矩形选区(v0.11.4 起手势让位:平移走滚轮/触控板/滚动条)
    // 断言拖动后横向偏移不变(不滚动),选区语义由 cell_range_drag_select_and_type_fill 覆盖
    {
        // 空转数帧:等上一步滚轮的平滑滚动动画停稳,基线才是纯拖动前的状态
        for _ in 0..12 {
            harness.step();
        }
        let x_before_drag = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(500.0, 350.0)));
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(500.0, 350.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        for x in [460.0f32, 420.0, 380.0, 340.0, 300.0, 250.0] {
            harness
                .input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, 350.0)));
            harness.step();
        }
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(250.0, 350.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
        harness.step();
        harness.step();
        let x_after_drag = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            x_before_drag, x_after_drag,
            "数据网格按住拖动应是选区,不应连带平移"
        );
        {
            let app = harness.state().borrow();
            let t = app.tabs[2].table_data().unwrap();
            assert!(t.sel_range.is_some(), "按住拖动应产生矩形选区");
        }
    }

    // 组合 xy 滚轮(模拟触控板斜向滑动):横向分量必须仍然生效
    let x_before_combined = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, 350.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-300.0, 90.0),
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let x_after_combined = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    assert_ne!(
        x_before_combined, x_after_combined,
        "组合 xy 滚轮的横向分量应驱动横向滚动(触控板斜滑场景)"
    );
}

#[test]
fn filter_bar_clicks_work() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.filter_bar_open = true;
        t.filter_mode = 0;
        t.conds = vec![("status".into(), "=".into(), "1".into())];
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);

    // 点击操作符下拉(fop):x ≈ 侧栏236+14(边距)+170+6+130+6+56+18+6 = 大约 640,行中心 y ≈ 82+4+26+6+13 ≈ 131
    let click = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    // 从截图核对的实际逻辑坐标:操作符下拉 ≈ (440, 175)。
    // 表页头部现为两行(标签栏 38 + 内容头 34 + 查询栏 36 = 108),筛选栏比旧的单行
    // 工具条布局整体下移 26pt,下面的弹层/选项坐标同步 +26
    click(&mut harness, egui::pos2(440.0, 175.0));
    harness.step();
    let out = harness.output().clone();
    let popup_rect = egui::Rect::from_min_size(egui::pos2(399.0, 196.0), egui::vec2(96.0, 250.0));
    let popup_shapes = out
        .shapes
        .iter()
        .filter(|cs| cs.clip_rect.intersects(popup_rect))
        .count();
    eprintln!("[popup-shapes] {popup_shapes}");
    // 弹层选项第一项(= )在按钮下方 ~30px;第二项(!=)≈ +28
    click(&mut harness, egui::pos2(440.0, 231.0));
    let b = harness.state();
    let conds_op = b.borrow().tabs[2]
        .table_data()
        .map(|t| t.conds[0].1.clone())
        .unwrap_or_default();
    assert_eq!(conds_op, "!=", "操作符下拉应可选,实际为 {conds_op:?}");
}

#[test]
fn where_chip_toggles_filter_bar() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.filter_bar_open = false;
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let img = harness.render().expect("render 失败");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/where_chip.png")
        .expect("保存 PNG 失败");
    // 查询栏 WHERE 芯片(取代了旧的"高级筛选"按钮):芯片紧贴查询栏左侧,
    // x = 侧栏 220 + 边距 14 + 芯片宽 76 / 2 ≈ 272;y = 标签栏 38 + 内容头 34 + 查询栏 36 / 2 = 90
    let pos = egui::pos2(272.0, 90.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let open = harness.state()
        .borrow()
        .tabs[2]
        .table_data()
        .map(|t| t.filter_bar_open)
        .unwrap_or(true);
    assert!(open, "点击 WHERE 芯片应打开筛选栏");
}

#[test]
fn appbar_buttons_open_modals() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    app.conn_modal = None;
    app.settings_open = false;
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                b.draw_app_toolbar(ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let img = harness.render().expect("render 失败");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/appbar.png")
        .expect("保存 PNG 失败");
    let tap = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    // 工具栏高 40:按钮中心 y = 20。「新建连接」是应用名(62) + 间距后的第一个按钮;
    // 齿轮在最右(右边距 12 + 按钮半宽 14)
    tap(&mut harness, egui::pos2(146.0, 20.0));
    assert!(
        harness.state().borrow().conn_modal.is_some(),
        "点击「新建连接」应打开连接弹窗"
    );
    harness.state().borrow_mut().conn_modal = None;
    tap(&mut harness, egui::pos2(1174.0, 20.0));
    assert!(
        harness.state().borrow().settings_open,
        "点击齿轮应打开设置(此前设置只能从表页 ··· 菜单进)"
    );
}

/// 入口唯一:一个功能一个入口,放在它作用的层级(全局=顶栏,对象=侧栏右键菜单,
/// 页面=页工具栏,状态栏只放状态)。此前切换外观 4 处、设置 3 处、导入导出连接 3 处、
/// 数据传输/对比 2 处、导出 2 处、新建查询 5 处——新入口加上去时旧的没删。
/// 这条测试盯住删掉的重复入口不回来
#[test]
fn entry_points_are_unique() {
    use egui_kittest::kittest::Queryable as _;
    type H<'a> = egui_kittest::Harness<'a, std::cell::RefCell<ShujingApp>>;
    fn has(h: &H<'_>, label: &str) -> bool {
        h.query_by_label(label).is_some()
    }
    fn open_menu(h: &mut H<'_>, label: &str) {
        {
            h.get_by_label(label).simulate_click();
        }
        h.run_steps(3);
    }

    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let cell = std::cell::RefCell::new(shell_app("role"));
    let setup_done = std::cell::Cell::new(false);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_shell(&mut b, ctx);
            },
            cell,
        );
    h.run_steps(6);

    // 顶栏「更多」:只有连接的导入导出;设置由右侧齿轮进
    open_menu(&mut h, "更多");
    for keep in [
        "导入连接(DBeaver/Navicat)…",
        "导出连接(加密备份)…",
        "导入连接(加密备份)…",
    ] {
        assert!(has(&h, keep), "「更多」应保留:{keep}");
    }
    assert!(!has(&h, "设置…"), "「更多」里不该再有设置(右侧齿轮是唯一入口)");
    key_once(&mut h, egui::Key::Escape, plain);
    assert!(!has(&h, "导入连接(DBeaver/Navicat)…"), "Esc 应关掉菜单");

    // 表页「···」:只留"这一页怎么看"(列显隐、复制格式、Mongo 文档操作)
    open_menu(&mut h, "更多操作");
    for keep in ["显示列", "显示全部列", "复制本页为 TSV", "复制本页为 INSERT 语句"] {
        assert!(has(&h, keep), "表页 ··· 应保留:{keep}");
    }
    for gone in [
        "设置…",
        "浅色外观",
        "深色外观",
        "数据传输到…",
        "查看整行详情",
        "导出 CSV",
        "导出 Excel(.xlsx)",
        "导出 SQL",
        "高级筛选…",
    ] {
        assert!(!has(&h, gone), "表页 ··· 不该再有:{gone}(别处已有唯一入口)");
    }
    // 与顶栏按钮同名的两项:菜单开着时全窗口也只能各有一个(就是顶栏那个)
    for dup in ["数据对比…", "结构对比…"] {
        let n = h.query_all_by_label(dup).count();
        assert_eq!(n, 1, "「{dup}」应只有顶栏这一个入口,实际找到 {n} 个");
    }
    key_once(&mut h, egui::Key::Escape, plain);

    // 导出只有页面工具栏这一处
    open_menu(&mut h, "导出");
    for keep in ["导出 CSV", "导出 Excel(.xlsx)", "导出 SQL"] {
        assert!(has(&h, keep), "工具栏「导出」应包含:{keep}");
    }
    key_once(&mut h, egui::Key::Escape, plain);

    // 顶栏的传输/对比带上当前标签的表:表页 ··· 里的重复入口删掉后,预选表的便利不丢
    {
        h.get_by_label("数据传输…").simulate_click();
    }
    h.run_steps(3);
    let src = h.state().borrow().transfer.as_ref().map(|t| t.src_table.clone());
    assert_eq!(src.as_deref(), Some("backend_role_privilege"));
}

/// 侧栏右键菜单按层级分工:连接节点才有"新建查询""导入 SQL 文件",库节点是建表/ER 图/
/// 导出库 SQL,表节点是表自己的操作。此前"新建查询"在连接/库/表三级菜单里各有一份、
/// "导入 SQL 文件"在连接和库两级各有一份,调用的还都是同一个不带库参数的函数
#[test]
fn sidebar_context_menus_are_layered() {
    use egui_kittest::kittest::Queryable as _;
    type H<'a> = egui_kittest::Harness<'a, std::cell::RefCell<ShujingApp>>;
    fn menu_has(h: &H<'_>, label: &str) -> bool {
        h.query_by_label(label).is_some()
    }
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let cell = std::cell::RefCell::new(shell_app("role"));
    let setup_done = std::cell::Cell::new(false);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_shell(&mut b, ctx);
            },
            cell,
        );
    h.run_steps(6);

    // 行位置(shell_app 夹具、标准密度):侧栏没有头部行,搜索框与标签栏同行
    let (conn_row, db_row, table_row) = (
        egui::pos2(150.0, 102.0),
        egui::pos2(150.0, 130.0),
        egui::pos2(150.0, 230.0),
    );
    // 表节点
    right_click_at(&mut h, table_row);
    // "设计表"在面包屑工具栏也有一份(有意保留的两个入口),菜单开着时全窗口
    // 恰好两处;表节点菜单专有的项用唯一性断言
    for keep in ["复制表名", "删除表…", "复制 SELECT *"] {
        assert!(menu_has(&h, keep), "表节点右键菜单应有:{keep}");
    }
    assert_eq!(
        h.query_all_by_label("设计表").count(),
        2,
        "「设计表」= 面包屑按钮 + 表节点菜单,共两处"
    );
    assert!(!menu_has(&h, "新建查询"), "表节点右键菜单不该有新建查询(与连接节点重复)");
    key_once(&mut h, egui::Key::Escape, plain);
    assert!(!menu_has(&h, "复制表名"), "Esc 应关掉菜单");

    // 库节点
    right_click_at(&mut h, db_row);
    for keep in ["新建表…", "查看 ER 图", "导出数据库 SQL(结构+数据)"] {
        assert!(menu_has(&h, keep), "库节点右键菜单应有:{keep}");
    }
    for gone in ["新建查询", "导入 SQL 文件…"] {
        assert!(
            !menu_has(&h, gone),
            "库节点右键菜单不该有:{gone}(与连接节点是同一个调用)"
        );
    }
    key_once(&mut h, egui::Key::Escape, plain);

    // 连接节点:点在行内空白处(名字右侧)——整行都可右键,与库行/表行一致
    right_click_at(&mut h, conn_row);
    for keep in ["新建查询", "导入 SQL 文件…", "编辑连接…"] {
        assert!(menu_has(&h, keep), "连接节点右键菜单应有:{keep}");
    }
    key_once(&mut h, egui::Key::Escape, plain);
    assert!(!menu_has(&h, "编辑连接…"), "Esc 应关掉菜单");
    // 正好点在名字上也一样出菜单(名字是行内上层控件,菜单只挂整行时这里会点不出来)
    right_click_at(&mut h, egui::pos2(60.0, 102.0));
    for keep in ["新建查询", "导入 SQL 文件…", "编辑连接…"] {
        assert!(menu_has(&h, keep), "右键点在连接名上应出同一个菜单,缺:{keep}");
    }
    key_once(&mut h, egui::Key::Escape, plain);
}

/// 71 批重排过的弹窗底栏按钮:此前只看过渲染图,没点过。
/// 按无障碍标签找按钮(theme::button 已补 widget_info),指针模拟点击,
/// 断言各自的状态变化。设置弹窗的语言开关会改全局 i18n、"打开文件夹"会真开
/// Finder,不能点;传输的"开始传输"/对比的"运行对比"要走异步任务,也不点。
#[test]
fn modal_footer_buttons_work() {
    use egui_kittest::kittest::Queryable as _;
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let harness_with = |app: ShujingApp,
                        draw_modal: fn(&mut ShujingApp, &egui::Context),
                        height: f32|
     -> egui_kittest::Harness<std::cell::RefCell<ShujingApp>> {
        let cell = std::cell::RefCell::new(app);
        let setup_done = std::cell::Cell::new(false);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, height))
            .with_pixels_per_point(2.0)
            .build_state(
                move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                    if !setup_done.get() {
                        setup_done.set(true);
                        ShujingApp::apply_fonts(ctx);
                        crate::theme::set_dark(true);
                        crate::theme::apply(ctx);
                        return;
                    }
                    let mut b = app.borrow_mut();
                    draw_app(&mut b, ctx);
                    draw_modal(&mut b, ctx);
                },
                cell,
            );
        harness.run_steps(6);
        harness
    };
    let click = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>, label: &str| {
        {
            let node = h.get_by_label(label);
            node.simulate_click();
        }
        h.run_steps(3);
    };

    // 新增行:取消 → 表单关
    {
        let mut app = full_app("role");
        if let TabKind::Table(t) = &mut app.tabs[2].kind {
            t.pk_cols = vec!["id".into()];
            t.pk_loaded = true;
            t.insert_form = Some(vec![
                ("id".to_string(), String::new()),
                ("role".to_string(), String::new()),
            ]);
        }
        // 表单由 draw_table_view 自己画,不用额外叠弹窗
        let mut h = harness_with(app, |_, _| {}, 720.0);
        click(&mut h, "取消");
        assert!(
            matches!(&h.state().borrow().tabs[2].kind, TabKind::Table(t) if t.insert_form.is_none()),
            "点「取消」应关闭新增行表单"
        );
    }
    // 新增行:插入(有值) → 表单关并提交(draw_app 丢弃动作,不触发异步写库)
    {
        let mut app = full_app("role");
        if let TabKind::Table(t) = &mut app.tabs[2].kind {
            t.pk_cols = vec!["id".into()];
            t.pk_loaded = true;
            t.insert_form = Some(vec![
                ("id".to_string(), String::new()),
                ("role".to_string(), "admin".to_string()),
            ]);
        }
        let mut h = harness_with(app, |_, _| {}, 720.0);
        click(&mut h, "插入");
        assert!(
            matches!(&h.state().borrow().tabs[2].kind, TabKind::Table(t) if t.insert_form.is_none()),
            "点「插入」应提交并关闭新增行表单"
        );
    }
    // 设置:插件管理按钮 + 完成
    {
        let mut app = full_app("role");
        app.settings_open = true;
        // 高视口让整份设置内容可见:插件区在 720 下已滚出 ScrollArea,模拟点不到
        let mut h = harness_with(app, crate::modals::draw_settings_modal, 1400.0);
        click(&mut h, "插件管理…");
        assert!(
            h.state().borrow().plugin_manager_open,
            "点「插件管理…」应打开插件管理"
        );
        // 插件管理浮层本 harness 没画,直接收掉它再点设置的完成
        h.state().borrow_mut().plugin_manager_open = false;
        click(&mut h, "完成");
        assert!(!h.state().borrow().settings_open, "点「完成」应关闭设置弹窗");
    }
    // 设置:密度切换(紧凑 ↔ 标准)。皮肤 = 色彩预设 × 密度,这是换装入按;
    // 全局状态必须复原,否则同进程的后续坐标测试全被紧凑档带偏
    {
        let mut app = full_app("role");
        app.settings_open = true;
        let mut h = harness_with(app, crate::modals::draw_settings_modal, 1400.0);
        // 落盘的值:启动时按同一个键恢复(见 SETTINGS_KEY_DENSITY),内存对了文件没对等于白点
        let saved_density = |h: &egui_kittest::Harness<std::cell::RefCell<ShujingApp>>| {
            let cfg = h.state().borrow().config_dir.clone();
            let text = std::fs::read_to_string(cfg.join("settings.json"))
                .expect("切换密度后应写出 settings.json");
            let v: serde_json::Value =
                serde_json::from_str(&text).expect("settings.json 应是合法 JSON");
            v.get(crate::app::SETTINGS_KEY_DENSITY)
                .and_then(|x| x.as_u64())
        };
        click(&mut h, "紧凑");
        assert_eq!(
            crate::theme::density_idx(),
            1,
            "点「紧凑」应切到紧凑密度"
        );
        assert_eq!(saved_density(&h), Some(1), "点「紧凑」后 settings.json 应记录 density=1");
        click(&mut h, "标准");
        assert_eq!(
            crate::theme::density_idx(),
            0,
            "点「标准」应切回标准密度"
        );
        assert_eq!(saved_density(&h), Some(0), "点「标准」后 settings.json 应记录 density=0");
    }
    // 数据传输:取消
    {
        let mut app = full_app("role");
        let cid = app
            .conns
            .iter()
            .find(|c| c.db_type == shujing_core::DbType::MySql)
            .map(|c| c.id.clone())
            .unwrap_or_default();
        app.open_transfer(&cid, Some("dramato"), "backend_role_privilege");
        assert!(app.transfer.is_some());
        let mut h = harness_with(app, crate::modals::draw_transfer_modal, 720.0);
        click(&mut h, "取消");
        assert!(h.state().borrow().transfer.is_none(), "点「取消」应关闭数据传输弹窗");
    }
    // 数据对比 / 结构对比:关闭
    {
        let mut app = full_app("role");
        let cid = app
            .conns
            .iter()
            .find(|c| c.db_type == shujing_core::DbType::MySql)
            .map(|c| c.id.clone())
            .unwrap_or_default();
        app.open_data_diff(&cid, Some("dramato"), "backend_role_privilege");
        let mut h = harness_with(app, crate::modals::draw_data_diff_modal, 720.0);
        click(&mut h, "关闭");
        assert!(h.state().borrow().data_diff.is_none(), "点「关闭」应关闭数据对比弹窗");
    }
    {
        let mut app = full_app("role");
        let cid = app
            .conns
            .iter()
            .find(|c| c.db_type == shujing_core::DbType::MySql)
            .map(|c| c.id.clone())
            .unwrap_or_default();
        app.open_schema_diff(&cid, Some("dramato"), "backend_role_privilege");
        let mut h = harness_with(app, crate::modals::draw_schema_diff_modal, 720.0);
        click(&mut h, "关闭");
        assert!(h.state().borrow().schema_diff.is_none(), "点「关闭」应关闭结构对比弹窗");
    }
}

#[test]
fn render_query_autocomplete() {
    let mut q = QueryTab {
        conn_id: "c1".into(),
        database: None,
        sql: "SELECT * FROM us".into(),
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
        ac: Some(crate::app::AcState {
            word: "us".into(),
            selected: 0,
            forced: false,
        }),
        q_frozen: 0,
        lint_cache: None,
    };
    let conns = vec![shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: Some("dramato".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    }];
    let cell = std::cell::RefCell::new(Some(q));
    let tables = vec![
        "users".to_string(),
        "user_logs".to_string(),
        "user_settings".to_string(),
    ];
    let mut cols = std::collections::HashMap::new();
    cols.insert(
        "c1|dramato|users".to_string(),
        vec![
            "id".to_string(),
            "user_name".to_string(),
            "status".to_string(),
        ],
    );
    render("query_autocomplete", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(
            &conns,
            ctx,
            "q1",
            q,
            &[],
            &[],
            false,
            &tables,
            &cols,
            Some("dramato"),
            false,
        );
    });
}

#[test]
fn frozen_columns_pin_while_hscrolling() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 冻结前 2 列:横向滚动多段后,冻结区(行号+前 2 列)必须逐像素钉在原位
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        let filler = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string();
        t.rows = t
            .rows
            .iter()
            .map(|r| r.iter().map(|_| Some(filler.clone())).collect())
            .collect();
        t.frozen_vis = 2;
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);

    let wheel = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>, dx: f32| {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(500.0, 350.0)));
        h.step();
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-dx, 0.0),
            modifiers: Default::default(),
        });
        h.step();
        h.step();
        // 指针移开:冻结区不留悬停高亮,像素差只能来自滚动
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
        h.step();
        h.step();
    };
    let crop_frozen_region =
        |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>| -> Vec<u8> {
            let img = h.render().expect("render");
            let raw = img.as_raw();
            let width = img.width() as usize;
            // 逻辑 (260..420, 200..520) → 物理 ×2:落在冻结区内、表头之下
            let (x0, x1, y0, y1) = (260usize * 2, 420usize * 2, 200usize * 2, 520usize * 2);
            let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for y in y0..y1 {
                let base = y * width * 4;
                out.extend_from_slice(&raw[base + x0 * 4..base + x1 * 4]);
            }
            out
        };

    wheel(&mut harness, 600.0);
    let crop1 = crop_frozen_region(&mut harness);
    let x1 = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    wheel(&mut harness, 600.0);
    let crop2 = crop_frozen_region(&mut harness);
    let x2 = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    assert!(x1 > 0, "第一次横向滚轮后应产生滚动偏移,实际 {x1}");
    assert!(x2 > x1, "第二次滚动应继续前进,{x1} → {x2}");
    assert_eq!(crop1, crop2, "冻结区在两次滚动位置应逐像素一致(钉住)");

    // 存一张滚动后的全图供人工/代理目检
    let img = harness.render().expect("render final");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/frozen_scrolled.png")
        .expect("保存 PNG 失败");
}

#[test]
fn frozen_overlay_editor_and_unfreeze() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 冻结区滚出后:编辑框由覆盖层持有;取消冻结后回到底层
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        let filler = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string();
        t.rows = t
            .rows
            .iter()
            .map(|r| r.iter().map(|_| Some(filler.clone())).collect())
            .collect();
        t.frozen_vis = 1;
        t.editing = Some((0, 0)); // 编辑第 0 行第 0 个可见列(冻结列)
        t.draft = "hello".into();
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);

    // 滚一段使冻结列滚出视口 → 覆盖层接管编辑框
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, 350.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-400.0, 0.0),
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();

    // 注入回车提交:覆盖层编辑框应产出 cr 事件(改动落 changes)
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
        physical_key: None,
    };
    harness.input_mut().events.push(enter);
    harness.step();
    harness.step();
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        assert!(
            t.editing.is_none(),
            "回车后应退出编辑态(覆盖层编辑框持有焦点)"
        );
    }

    // 取消冻结:视觉回到底层渲染,不再有覆盖层
    let img = harness.render().expect("render");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/frozen_edit.png")
        .expect("保存 PNG 失败");
}

#[test]
fn query_grid_frozen_columns_pin() {
    // 必须持 RENDER_LOCK:坐标按标准密度写死,而紧凑档渲染测试会在锁内把
    // 进程级密度切到紧凑——不持锁时若与之重叠,就在紧凑布局上按标准坐标点击
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 查询结果网格冻结 1 列:横向滚动两段后,冻结区逐像素钉住;整幅画面必须已变化
    let filler = "value-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let mut q = QueryTab {
        conn_id: "c1".into(),
        database: None,
        sql: "SELECT * FROM wide_table".into(),
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
        q_frozen: 1,
        lint_cache: None,
    };
    q.results = vec![shujing_core::ExecResult {
        columns: (0..6).map(|i| format!("col_{i}")).collect(),
        rows: (0..14)
            .map(|i| (0..6).map(|_| Some(format!("{filler}-{i}"))).collect())
            .collect(),
        affected: 0,
        truncated: false,
        elapsed_ms: 12,
        column_types: vec![],
    }];
    let conns = vec![shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("10.0.0.3".into()),
        port: Some(3306),
        user: Some("dev".into()),
        password: None,
        database: Some("dramato".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    }];
    let cell = std::cell::RefCell::new(Some(q));
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1280.0, 780.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, cell: &mut std::cell::RefCell<Option<QueryTab>>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = cell.borrow_mut();
                let q = b.as_mut().unwrap();
                let _ = query::draw_query_view(
                    &conns,
                    ctx,
                    "q1",
                    q,
                    &[],
                    &[],
                    false,
                    &[],
                    &Default::default(),
                    None,
                    false,
                );
            },
            cell,
        );
    harness.run_steps(6);

    let crop_frozen =
        |h: &mut egui_kittest::Harness<std::cell::RefCell<Option<QueryTab>>>| -> Vec<u8> {
            let img = h.render().expect("render");
            let raw = img.as_raw();
            let width = img.width() as usize;
            // 逻辑 (250..360, 460..700) → 物理 ×2:冻结区内、分界线(≈x381)左侧、表头下、滚动条上
            let (x0, x1, y0, y1) = (250usize * 2, 360usize * 2, 460usize * 2, 700usize * 2);
            let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for y in y0..y1 {
                let base = y * width * 4;
                out.extend_from_slice(&raw[base + x0 * 4..base + x1 * 4]);
            }
            out
        };
    let wheel = |h: &mut egui_kittest::Harness<std::cell::RefCell<Option<QueryTab>>>, dx: f32| {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(640.0, 620.0)));
        h.step();
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-dx, 0.0),
            modifiers: Default::default(),
        });
        h.step();
        h.step();
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
        h.step();
        h.step();
    };

    wheel(&mut harness, 500.0);
    let crop1 = crop_frozen(&mut harness);
    let full1 = harness.render().expect("render").into_raw();
    wheel(&mut harness, 500.0);
    let crop2 = crop_frozen(&mut harness);
    let full2 = harness.render().expect("render").into_raw();
    assert_ne!(full1, full2, "第二次滚动后整幅画面应变化(网格在滚动)");
    assert_eq!(crop1, crop2, "查询网格冻结区两次滚动位置应逐像素一致");

    let img = harness.render().expect("render final");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/query_frozen.png")
        .expect("保存 PNG 失败");
}

#[test]
fn cell_range_drag_select_and_type_fill() {
    // 直接操作链路:按住单元格拖动 = 矩形选区 → 直接打字 = 锚点格进入编辑 → 回车 = 批量填充
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.active_filter = None;
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);

    // 拖选:role 列(≈380,220)拖到 role_name 列(≈500,290)
    let drag = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>,
                from: egui::Pos2,
                to: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(from));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        for k in 1..=4 {
            let x = from.x + (to.x - from.x) * k as f32 / 4.0;
            let y = from.y + (to.y - from.y) * k as f32 / 4.0;
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, y)));
            h.step();
        }
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    drag(
        &mut harness,
        egui::pos2(380.0, 220.0),
        egui::pos2(500.0, 290.0),
    );
    let expected = {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        let r = t.sel_range.expect("拖选后应有矩形选区");
        assert!(!t.changes.is_empty() == false, "拖选本身不应产生改动");
        r
    };

    // 直接打字:锚点格进入编辑
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("9".to_string()));
    harness.step();
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        assert_eq!(t.editing, Some(expected.0), "打字应从锚点格进入编辑");
        assert_eq!(t.draft, "9");
    }

    // 回车:批量填充整个选区(主键列跳过)
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
        physical_key: None,
    });
    harness.step();
    harness.step();
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        assert!(t.editing.is_none(), "回车后应退出编辑");
        let ((r1, c1), (r2, c2)) = expected;
        let cells = (r2 - r1 + 1) * (c2 - c1 + 1);
        assert_eq!(
            t.changes.len(),
            cells,
            "选区 {}×{} 应产生 {} 处改动,实际 {:?}",
            r2 - r1 + 1,
            c2 - c1 + 1,
            cells,
            t.changes
        );
        assert!(
            t.changes
                .iter()
                .all(|(_, cn, v)| cn != "id" && v.as_deref() == Some("9")),
            "改动应填 9 且跳过主键列"
        );
    }

    let img = harness.render().expect("render");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/range_fill.png")
        .expect("保存 PNG 失败");
}

#[test]
fn cmd_a_selects_page_range() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.active_filter = None;
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        // role_tab:8 行 × 6 列全可见
        assert_eq!(t.sel_range, Some(((0, 0), (7, 5))), "⌘A 应全选本页");
        assert!(t.multi_sel.is_empty(), "⌘A 后行多选应让位");
    }
    // Delete:全选置 NULL 是批量操作,应产生暂存改动(不直接写库)
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Delete,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
        physical_key: None,
    });
    harness.step();
    harness.step();
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        // 8 行 × 5 个非主键列(id 是主键,跳过)= 40 处暂存
        assert_eq!(
            t.changes.len(),
            40,
            "Delete 全选置 NULL 应暂存 40 处非主键改动"
        );
        assert!(t.changes.iter().all(|(_, cn, _)| cn != "id"));
    }
}

#[test]
fn double_click_edit_opens_editor() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let app = full_app("role");
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        // 默认 step_dt=1/4s:两步就超过 egui 的 0.3s 双击窗,双击永远不触发
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    // 双击 role 列第一个数据格:两次完整按下/释放
    for _ in 0..2 {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(380.0, 182.0)));
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(380.0, 182.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(380.0, 182.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.step();
    }
    // 逐帧观察:第二次按下当帧 editing 是否短暂出现
    for step in 0..4 {
        harness.step();
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        eprintln!(
            "[dblclick step{step}] editing={:?} detail={} sel={:?} range={:?}",
            t.editing, t.detail, t.selected, t.sel_range
        );
    }
    {
        let app = harness.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        let pk = !t.pk_cols.is_empty();
        eprintln!(
            "[dblclick final] pk_cols={:?} editing={:?}",
            t.pk_cols, t.editing
        );
        assert!(pk, "测试表应有主键");
        assert!(t.editing.is_some(), "双击应进入编辑态");
    }
}

#[test]
fn tab_close_and_context_menu_work() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let app = full_app("role");
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let tab_x = |i: usize| 222.0 + i as f32 * 190.0 + 60.0;

    // 右键第一个标签 → 应弹出菜单(菜单区域出现新 shape)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(tab_x(0), 14.0)));
    harness.step();
    let before = harness.render().expect("r").into_raw();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(tab_x(0), 14.0),
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(tab_x(0), 14.0),
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let after_menu = harness.render().expect("r").into_raw();
    assert_ne!(before, after_menu, "右键标签应弹出菜单");

    // 点击「关闭其他」→ 只剩激活标签
    // 菜单条目从标签下方开始,第一项 ≈ (tab_x(0)+40, 40)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(tab_x(0) + 40.0, 40.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(tab_x(0) + 40.0, 40.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(tab_x(0) + 40.0, 40.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    {
        let app = harness.state().borrow();
        eprintln!("[tabmenu] tabs={}", app.tabs.len());
        assert_eq!(app.tabs.len(), 1, "「关闭其他」应只留激活标签");
    }
}

#[test]
fn tab_overflow_click_close_context_menu_work() {
    // 标签多到横向溢出时,ScrollArea 的 drag_to_scroll 曾把“按下+轻微移动”
    // 吞成拖拽滚动,导致 tab 点击/×/右键间歇失效。这里用 11 个标签复现。
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    for i in 0..8 {
        app.tabs.push(stub_tab(
            &format!("溢出标签 {i}"),
            &format!("overflow_table_{i}"),
        ));
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let click = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>,
                 pos: egui::Pos2,
                 button: egui::PointerButton,
                 jitter: f32| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        if jitter != 0.0 {
            h.input_mut()
                .events
                .push(egui::Event::PointerMoved(egui::pos2(pos.x + jitter, pos.y)));
            h.step();
        }
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(pos.x + jitter, pos.y),
            button,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };

    // 带 8px 手抖点击第 1 个标签 → 应激活
    click(
        &mut harness,
        egui::pos2(300.0, 14.0),
        egui::PointerButton::Primary,
        8.0,
    );
    assert_eq!(
        harness.state().borrow().active_tab,
        0,
        "溢出标签栏中带手抖点击也应激活 tab"
    );

    // 点第 1 个标签的 ×(标题宽度实测右缘≈394.7,× 中心≈378.7)
    click(
        &mut harness,
        egui::pos2(378.7, 14.0),
        egui::PointerButton::Primary,
        0.0,
    );
    assert_eq!(
        harness.state().borrow().tabs.len(),
        10,
        "溢出标签栏中点 × 应关闭标签"
    );

    // 右键第 1 个标签 → 菜单 →「关闭其他」
    click(
        &mut harness,
        egui::pos2(300.0, 14.0),
        egui::PointerButton::Secondary,
        0.0,
    );
    click(
        &mut harness,
        egui::pos2(340.0, 40.0),
        egui::PointerButton::Primary,
        0.0,
    );
    assert_eq!(
        harness.state().borrow().tabs.len(),
        1,
        "溢出标签栏右键「关闭其他」应生效"
    );
}

/// 真根因复现:标签条在指针按压/惯性期间移动 → egui 要求点击的按下/松开
/// 命中同一 widget,内容一滑松开就落进邻格,点击被静默取消(="点了没反应")。
/// 断言用 TABBAR_OFFSET_X 镜像:一次滚轮后偏移必须在极短帧数内停稳
/// (旧实现读 smooth_scroll_delta,停滚后还要放 0.1-0.3s 尾巴,标签一直在滑)。

/// 数据网格滚轮一步到位(全应用 smooth=raw 覆写的代表用例):
/// 滚完立刻点单元格/行,内容不能还在滑(旧 smooth 停滚后尾巴 ~0.18s,
/// 期间点击被 egui 静默取消——与标签栏 v0.12.4 同病同源)。
#[test]
fn grid_wheel_scroll_settles_fast() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role"); // role_tab 是 12 列宽表 fixture,保证横向可滚
                                    // 真实 update 路径(全局 smooth=raw 覆写在 eframe update 里,draw_app 简化路径不含)
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = app;
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let off = || table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    // 指针放到网格内容区,横向大步进滚轮(shift 语义直接给 x)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(600.0, 380.0 + CHROME_TOP)));
    harness.step();
    let before = off();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-80.0, 0.0),
        modifiers: Default::default(),
    });
    harness.step();
    let mut drift = 0;
    let mut last = off();
    for _ in 0..12 {
        harness.step();
        let cur = off();
        if (cur - last).abs() > 0 {
            drift += 1;
        }
        last = cur;
    }
    assert!(
        (last - before).abs() > 30,
        "横向滚轮应实际滚动数据网格(before={before}, after={last})"
    );
    assert!(
        drift <= 2,
        "滚轮后数据网格必须在 2 帧内停稳(实际漂移 {drift} 帧)—— \
        尾巴期间点单元格/行会被 egui 静默取消"
    );
}

#[test]
fn tabbar_wheel_scroll_settles_fast() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    for i in 0..8 {
        app.tabs.push(stub_tab(
            &format!("溢出标签 {i}"),
            &format!("overflow_table_{i}"),
        ));
    }
    // 真实 update 路径(全局 smooth=raw 覆写在 eframe update 里,draw_app 简化路径不含)
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = app;
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let off =
        || f32::from_bits(table_view::TABBAR_OFFSET_X.load(std::sync::atomic::Ordering::Relaxed));
    // 指针先进标签栏(滚轮处理有 rect_contains_pointer 门,指针不在栏内不动)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, 14.0 + CHROME_TOP)));
    harness.step();
    let before = off();
    // 一格大步进滚轮(负向=滚进溢出区;Point |120| ≥ 8 → egui 归入
    // unprocessed 分帧释放,尾巴最重)
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -120.0),
        modifiers: Default::default(),
    });
    harness.step();
    // 记录随后 12 帧的偏移:统计"仍在变化"的帧数
    let mut drift_frames = 0;
    let mut last = off();
    for _ in 0..12 {
        harness.step();
        let cur = off();
        if (cur - last).abs() > 0.5 {
            drift_frames += 1;
        }
        last = cur;
    }
    assert!(
        last > before + 50.0,
        "滚轮应实际滚动标签栏(before={before}, after={last})"
    );
    assert!(
        drift_frames <= 2,
        "滚轮后标签栏必须在 2 帧内停稳(实际漂移 {drift_frames} 帧)—— \
        尾巴期间任何点击的按下/松开都会落在不同标签上被 egui 静默取消"
    );
}

/// 激活标签自动滚入必须瞬跳:旧实现 scroll_to_rect 默认动画 0.1-0.3s,
/// 期间标签条在滑,紧跟着的 ×/切换点击全部作废("切完马上关不掉"的根源)。
#[test]
fn tabbar_autoscroll_is_instant() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    for i in 0..8 {
        app.tabs.push(stub_tab(
            &format!("溢出标签 {i}"),
            &format!("overflow_table_{i}"),
        ));
    }
    // 真实 update 路径(全局 smooth=raw 覆写在 eframe update 里,draw_app 简化路径不含)
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = app;
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let off =
        || f32::from_bits(table_view::TABBAR_OFFSET_X.load(std::sync::atomic::Ordering::Relaxed));
    // 激活远端标签(x=800 实测命中溢出区)→ 触发 scroll_to_rect
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(800.0, 14.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(800.0, 14.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(800.0, 14.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    assert!(harness.state().active_tab >= 3, "前置:应激活了溢出区的标签");
    // 随后 12 帧:自动滚入产生的偏移变化帧数必须 ≤3(瞬跳=1-2 帧;动画=6-18 帧)
    let mut drift_frames = 0;
    let mut last = off();
    for _ in 0..12 {
        harness.step();
        let cur = off();
        if (cur - last).abs() > 0.5 {
            drift_frames += 1;
        }
        last = cur;
    }
    assert!(
        drift_frames <= 3,
        "激活标签自动滚入必须瞬跳(实际漂移 {drift_frames} 帧)—— \
        动画期间紧跟着的关闭/切换点击会被 egui 静默取消"
    );
}

#[test]
fn tab_close_button_works() {
    // 用真实 eframe::App::update 路径(draw_app 不含 update 的弹窗/快捷键块)
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    // 第一个(非激活)标签的 ×:标签右缘 -16;标签宽度随标题 galley 变化,按候选右缘试探
    let mut closed_to = None;
    for right in [176.0f32, 190.0, 204.0, 218.0, 232.0, 246.0, 260.0] {
        let x = 222.0 + right - 16.0;
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(x, 14.0 + CHROME_TOP)));
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(x, 14.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(x, 14.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.step();
        harness.step();
        let n = harness.state().tabs.len();
        eprintln!("[tabx] right={right} tabs={n}");
        if n == 2 {
            closed_to = Some(n);
            break;
        }
    }
    assert_eq!(closed_to, Some(2), "点 × 应关闭标签");
}

#[test]
fn tab_close_dirty_shows_confirm_and_closes() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            // 激活标签带未保存修改(模拟用户测试编辑后残留)
            if let TabKind::Table(t) = &mut app.tabs[2].kind {
                t.changes.push((0, "role".into(), Some("x".into())));
            }
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    // 单元路径:直接 close_tab 触发 pending(几何无关),验证确认弹窗可见且按钮可用
    // 走真实用户路径:⌘W 关闭脏标签 → 弹未保存确认
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::W,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    {
        let app = harness.state();
        eprintln!(
            "[dirtyx] tabs={} pending={}",
            app.tabs.len(),
            app.pending.is_some()
        );
        assert!(app.pending.is_some(), "脏标签关闭应弹未保存确认");
        assert_eq!(app.tabs.len(), 3, "确认前不应关闭");
    }
    let img = harness.render().expect("r");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/dirty_confirm.png").unwrap();
    let img = harness.render().expect("r");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/dirty_confirm.png").unwrap();
    // Escape 关闭弹窗(取消)→ 标签保留
    // 点「丢弃并继续」→ 标签应关闭(弹窗居中,主按钮在右下 ≈ (644, 405))
    let click = |h: &mut egui_kittest::Harness<ShujingApp>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    click(&mut harness, egui::pos2(644.0, 405.0));
    {
        let app = harness.state();
        eprintln!("[dirtyx after] tabs={}", app.tabs.len());
        assert!(app.pending.is_none(), "点击丢弃并继续后确认应结束");
        assert_eq!(app.tabs.len(), 2, "丢弃并继续后应关闭标签");
    }
}

#[test]
fn tab_rename_sticky_focus_and_exit() {
    // 双击标签标题进入重命名:此前点击别处不会退出重命名(TextEdit 每帧抢焦点),
    // 导致 ⌘W 被焦点门永久拦截、右键点到重命名输入框上没有标签菜单
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);

    // 基线:⌘W 直接关闭激活标签
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::W,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    let base = harness.state().tabs.len();
    eprintln!("[rename baseline] tabs after cmd-w = {base}");
    // 重命名入口已迁右键菜单(双击误触率高):直接播种重命名态测交互行为
    let t0 = harness.state().tabs[0].title.clone();
    harness.ctx.memory_mut(|m| {
        m.data
            .insert_temp(egui::Id::new("tab-rename"), (0usize, t0))
    });
    harness.step();
    harness.step();
    eprintln!(
        "[rename dbg] focused after dbl = {:?}",
        harness
            .ctx
            .memory(|m| m.focused().map(|f| f.short_debug_format()))
    );

    // 单击空白处:重命名应失焦退出(此前点击别处不退出,焦点被每帧抢回,⌘W 永久被拦)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(5.0, 5.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(5.0, 5.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();

    // ⌘W:应能关闭激活标签(若重命名仍粘着焦点,这里关不掉)。
    // 真实 winit 会持续上报修饰键状态;egui 0.31 的 InputState.modifiers 来自
    // RawInput.modifiers 字段(kittest 合成键事件自身携带的 modifiers 不会回填),
    // 测试里必须先设置它,否则 i.modifiers.command 恒为 false(与 ⌘A 测试同源问题)
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::W,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    eprintln!(
        "[rename dbg] focused after click-away = {:?}",
        harness
            .ctx
            .memory(|m| m.focused().map(|f| f.short_debug_format()))
    );
    let n = harness.state().tabs.len();
    eprintln!("[rename] after cmd-w tabs={n} (baseline {base})");
    assert_eq!(n, base - 1, "点击别处应退出重命名,⌘W 应能关标签");
}





/// 真机日志实锤的死点击模式:激活标签强制居中滚动,快速连点时上一次激活的
/// 瞬跳把下一个目标挪走(max_d=0 的 lp-outside 死亡)。修复=激活标签已可见时
/// 不再滚动;断言:点击一个已完全可见的标签,标签条偏移必须纹丝不动。
#[test]
fn activating_visible_tab_does_not_scroll_strip() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    for i in 0..8 {
        app.tabs.push(stub_tab(
            &format!("溢出标签 {i}"),
            &format!("overflow_table_{i}"),
        ));
    }
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = app;
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let off = || {
        f32::from_bits(table_view::TABBAR_OFFSET_X.load(std::sync::atomic::Ordering::Relaxed))
    };
    // 先滚一点制造溢出态(滚动条存在),随后点一个完全可见的中间标签
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, 19.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -60.0),
        modifiers: Default::default(),
    });
    for _ in 0..6 {
        harness.step();
    }
    let before = off();
    // 点击 tab1(x≈450 完全可见)
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(450.0, 19.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    for _ in 0..5 {
        harness.step();
    }
    assert_eq!(
        off(),
        before,
        "激活完全可见的标签不得滚动标签条(before={}, after={})——它会把快速连点的下一个目标挪走",
        before,
        off()
    );
    // 反向保证:激活一个出视野的标签仍会滚入(功能不回归)
    // 滚到最右,点 tab0(出视野)
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Num1,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    assert_eq!(harness.state().active_tab, 0, "⌘1 应激活 tab0");
    assert!(
        off() < before - 50.0,
        "出视野标签激活仍应滚入(偏移应左移,实际 before={before} after={})",
        off()
    );
}

/// 真实输入时序模糊测试:真机 winit 会合并事件到同一帧(快点的按下+松开同帧、
/// 松开前无移动事件、多事件单帧),脚本化测试从不覆盖这些形态。
/// 全部形态都必须照常激活标签。
#[test]
fn tab_click_realistic_timing_patterns() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let active = |h: &egui_kittest::Harness<ShujingApp>| h.state().active_tab;
    let reset = |h: &mut egui_kittest::Harness<ShujingApp>| {
        // 切回 tab1(点 tab1 标题中部,稳妥坐标)
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(450.0, 19.0 + CHROME_TOP)));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    // tab0 标题坐标(此前多个测试校准过 (300,19) 有效命中 tab0)
    let (x, y) = (300.0, 19.0 + CHROME_TOP);

    // 形态1:按下与松开合并进同一帧(快点)
    reset(&mut harness);
    assert_ne!(active(&harness), 0);
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    assert_eq!(active(&harness), 0, "同帧按下+松开(快点)应激活");

    // 形态2:按下→长时间无任何事件(30 帧)→松开(无移动事件,松开事件带原坐标)
    reset(&mut harness);
    assert_ne!(active(&harness), 0);
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for _ in 0..30 {
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    assert_eq!(active(&harness), 0, "长按 30 帧后松开应激活(max_click_duration=3s)");

    // 形态3:按下→移动→松开,移动与松开同帧(拖尾合并)
    reset(&mut harness);
    assert_ne!(active(&harness), 0);
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x + 8.0, y + 3.0)));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(x + 8.0, y + 3.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    assert_eq!(active(&harness), 0, "移动与松开同帧(8px 手抖)应激活");

    // 形态4:松开后紧接下一帧立即再点(快速连点第二次)
    harness.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(450.0, 19.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(450.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    assert_ne!(active(&harness), 0, "快速连点第二次应照常切换");
}

/// 复现:侧栏树行是位置型 widget id——按压期间上方库的表列表异步到达插入,
/// 松开落在"当前位置的行"上,点错目标(点 archive 变成打开 dramato 的表)。
/// 稳定 id(push_id)后:点错降级为点击落空。
#[test]
fn sidebar_async_insert_no_wrong_target() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = full_app("role");
    if let Some(t) = app.trees.get_mut("c1") {
        t.databases = vec!["dramato".into(), "archive".into()];
        // dramato 展开但表列表未到(空);archive 收起且表未加载
        t.tables_by_db = HashMap::from([("dramato".to_string(), vec![])]);
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let tabs_before = harness.state().borrow().tabs.len();
    // 在 archive 行上按下(坐标由 [calib] 插桩校准)
    let pos = egui::pos2(110.0, 175.0); // [calib] 插桩实测 archive 行中心
    harness.input_mut().events.push(egui::Event::PointerMoved(pos));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    // 按压期间:dramato 的表列表异步到达(上方插入 5 行,archive 下移)
    {
        let mut b = harness.state().borrow_mut();
        let t = b.trees.get_mut("c1").unwrap();
        t.tables_by_db.insert(
            "dramato".to_string(),
            (0..5)
                .map(|i| shujing_core::TableMeta {
                    name: format!("late_table_{i}"),
                    kind: "table".into(),
                    owner: None,
                })
                .collect(),
        );
    }
    harness.step();
    // 原位置松开
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let tabs_after = harness.state().borrow().tabs.len();
    assert_eq!(
        tabs_after, tabs_before,
        "按压期间上方异步插入行,松开不得点开'当前位置的行'(应落空而不是点错目标)"
    );
}

/// 复现"顶 tab 点击始终不灵敏"的三个残余根源(2026-11-10 用户实测报告):
/// ① 20-44px 手抖两个通道全判死(egui max_click_dist=20,手势层 24)
/// ② 拖拽排序 temp 松开不清零:30px 手抖把 acc 留在 temp,下一次 25px 手抖
///    点击叠加 ≥48px → 普通点击触发标签换位(标签自己跳走)
/// ③ 底部 7px 手势死区无条件生效(无滚动条时也排除)
#[test]
fn tab_click_survives_wobble_and_no_phantom_swap() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let titles = |h: &egui_kittest::Harness<ShujingApp>| {
        h.state()
            .tabs
            .iter()
            .map(|t| t.title.clone())
            .collect::<Vec<_>>()
    };
    // 带手抖的点击:按下后水平晃 ±amount 再松开(模拟触控板/魔术鼠标点按)
    let wobble_click = |h: &mut egui_kittest::Harness<ShujingApp>, x: f32, y: f32, amount: f32| {
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        for (i, dx) in [0.45f32, -0.35, 0.5, -0.6, 1.0].iter().enumerate() {
            let _ = i;
            h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(
                x + dx * amount,
                y + 0.3 * amount,
            )));
            h.step();
        }
        h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(
            x + amount * 0.7,
            y + 2.0,
        )));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(x + amount * 0.7, y + 2.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };
    let before = titles(&harness);
    let active_before = harness.state().active_tab;

    // 场景 1:40px 手抖点击 tab0(松开位移 28px,超出 egui 20px 与手势 24px 双阈值)
    // → 应激活,标签顺序不变
    wobble_click(&mut harness, 300.0, 19.0 + CHROME_TOP, 40.0);
    assert_eq!(
        harness.state().active_tab, 0,
        "30px 手抖的点击应照常激活标签(egui 20px 阈值过严,手势层须放宽)"
    );
    assert_eq!(titles(&harness), before, "手抖点击不得触发换位");

    // 场景 2:紧接着 25px 手抖再点 tab0 → 不得因残留累计触发换位/失灵
    wobble_click(&mut harness, 300.0, 19.0 + CHROME_TOP, 25.0);
    assert_eq!(
        harness.state().active_tab, 0,
        "第二次手抖点击应照常激活"
    );
    assert_eq!(
        titles(&harness), before,
        "拖拽累计不得跨手势残留(松开必须清零),普通点击不得触发换位"
    );

    // 场景 3:无溢出时低带点击(tab 底缘上 3px = y35,在 × 区 y≤32.5 之外)带 12px
    // 手抖 → 应激活 tab1;不得误触右侧 × 关闭区(× 为 22×22 居中矩形),不得换位
    // 标签条紧贴顶栏下缘(条顶 0)、标签高 TABBAR_H-1:底缘上 3px = 34,在 × 区(居中 22 高,7.5~29.5)之外
    let low_y = CHROME_TOP + (ui_kit::theme::tabbar_h() - 1.0) - 3.0;
    let order_low = titles(&harness);
    wobble_click(&mut harness, 430.0, low_y, 12.0);
    assert_eq!(
        harness.state().active_tab, 1,
        "低带 12px 手抖点击应激活目标标签(tab1)"
    );
    assert_eq!(
        titles(&harness), order_low,
        "低带点击不得误触 × 关闭区(此前 y=30 落在 × 纵向范围内把标签点没了)"
    );

    // 场景 4(核心回归):大幅拖拽(60px)仍是换位而非点击
    let order_pre = titles(&harness);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(300.0, 19.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(300.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for x in [320.0f32, 345.0, 370.0, 370.0, 370.0] {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(x, 19.0 + CHROME_TOP)));
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(370.0, 19.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    assert_ne!(
        titles(&harness),
        order_pre,
        "60px 水平拖拽应触发换位(不因手势层放宽而吞掉拖拽)"
    );
}

/// 回归:双击重命名已删——快连点两下(300ms 双击窗内)必须照常切换标签,
/// 不得弹重命名、不得取消切换(用户"点击切换不太好用"的主诉根源之一)
#[test]
fn tab_double_click_switches_not_renames() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    assert_ne!(harness.state().active_tab, 0, "前置:第 0 个标签初始未激活");
    for _ in 0..2 {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(260.0, 14.0 + CHROME_TOP)));
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(260.0, 14.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        harness.step();
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(260.0, 14.0 + CHROME_TOP),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        harness.step();
    }
    harness.step();
    assert_eq!(
        harness.state().active_tab,
        0,
        "双击第 0 个标签应照常切换过去(不得被重命名取消)"
    );
    let renaming = harness.ctx.memory(|m| {
        m.data
            .get_temp::<(usize, String)>(egui::Id::new("tab-rename"))
            .is_some()
    });
    assert!(!renaming, "双击不得再进入重命名");
    assert!(
        harness.ctx.memory(|m| m.focused().is_none()),
        "双击后不应有输入框持焦"
    );
}

/// 回归:右键菜单浮层盖在标签条上方,点菜单项(几何上落在某标签矩形内)
/// 不得被手势层隔空判成"点了那个标签"(误切/误关)。
/// 手势层 v2 用 layer_id_at 判定浮层:浮层上的点击不路由到背景层的标签。
#[test]
fn menu_overlay_click_does_not_hit_underlying_tab() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let active_before = harness.state().active_tab;
    let tabs_before = harness.state().tabs.len();
    let click =
        |h: &mut egui_kittest::Harness<ShujingApp>, pos: egui::Pos2, btn: egui::PointerButton| {
            h.input_mut().events.push(egui::Event::PointerMoved(pos));
            h.step();
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: btn,
                pressed: true,
                modifiers: Default::default(),
            });
            h.step();
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: btn,
                pressed: false,
                modifiers: Default::default(),
            });
            h.step();
        };
    // 右键第 0 个标签 → 菜单浮层展开(菜单项几何上覆盖标签条底部)
    click(
        &mut harness,
        egui::pos2(260.0, 14.0 + CHROME_TOP),
        egui::PointerButton::Secondary,
    );
    // 点菜单第 4 项「重命名标签…」(菜单项 y≈30 附近,同时也在标签矩形内)
    let mut entered = false;
    for y in [120.0f32, 96.0, 144.0, 30.0, 108.0, 132.0] {
        click(
            &mut harness,
            egui::pos2(272.0, y + CHROME_TOP),
            egui::PointerButton::Primary,
        );
        let renaming = harness.ctx.memory(|m| {
            m.data
                .get_temp::<(usize, String)>(egui::Id::new("tab-rename"))
                .is_some()
        });
        if renaming {
            entered = true;
            break;
        }
    }
    assert!(entered, "菜单「重命名标签…」应进入重命名");
    assert_eq!(
        harness.state().tabs.len(),
        tabs_before,
        "点菜单项不得误关底层标签"
    );
    assert_ne!(
        harness.state().active_tab,
        0,
        "点菜单项不得隔空激活菜单下方的标签(原 active={active_before})"
    );
}

/// 右键菜单「重命名标签…」应能进入重命名(双击入口删除后的唯一入口)
#[test]
fn tab_rename_via_context_menu() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    let right_click = |h: &mut egui_kittest::Harness<ShujingApp>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
    };
    let click = |h: &mut egui_kittest::Harness<ShujingApp>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
    };
    // 菜单项 y 位置不确定:扫候选,未命中菜单已关(CloseOnClick)则重开再试
    let mut entered = false;
    for y in [120.0f32, 96.0, 144.0, 108.0, 132.0, 84.0] {
        right_click(&mut harness, egui::pos2(260.0, 14.0 + CHROME_TOP));
        click(&mut harness, egui::pos2(272.0, y + CHROME_TOP));
        let renaming = harness.ctx.memory(|m| {
            m.data
                .get_temp::<(usize, String)>(egui::Id::new("tab-rename"))
                .is_some()
        });
        if renaming {
            entered = true;
            break;
        }
    }
    assert!(entered, "右键菜单「重命名标签…」应能进入重命名");
    harness.step(); // 重命名态落盘后,TextEdit 在下一帧渲染并 request_focus
    assert!(
        harness.ctx.memory(|m| m.focused().is_some()),
        "进入重命名后输入框应持焦点"
    );
    // Escape 退出
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
        physical_key: None,
    });
    harness.step();
    harness.step();
    let renaming = harness.ctx.memory(|m| {
        m.data
            .get_temp::<(usize, String)>(egui::Id::new("tab-rename"))
            .is_some()
    });
    assert!(!renaming, "Escape 应退出重命名");
}

#[test]
fn tab_close_button_real_path() {
    // 真实 update 路径点 ×:干净标签 / 脏标签两种
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);

    // 激活标签(第 3 个,role)标题最长 → 宽 248;× 在标签右缘-16
    // 侧栏 222 + 标签0(126)+标签1(126) 起点不确定,按几个右缘试探第 1 个标签的 ×
    let click = |h: &mut egui_kittest::Harness<ShujingApp>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.step();
    };

    // 干净的非激活标签:× 中心 ≈ 标签右缘-16(插桩实测标签0 右缘 394.7)
    let mut ok = false;
    for right in [394.0f32, 392.0, 396.0, 384.0] {
        click(&mut harness, egui::pos2(right - 16.0, 14.0 + CHROME_TOP));
        let n = harness.state().tabs.len();
        eprintln!("[x-real] right={right} tabs={n}");
        if n == 2 {
            ok = true;
            break;
        }
        // 没关掉可能误点了激活标签的标签体:恢复(若弹了确认就 Escape)
        harness.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Escape,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
            physical_key: None,
        });
        harness.step();
        harness.step();
    }
    assert!(ok, "真实路径点 × 应能关闭非激活干净标签");
}

#[test]
fn tab_rename_exit_by_drag_and_enter_commit() {
    // 拖动退出(点击网格后轻微拖动 = drag 而非 click,any_click 判定覆盖不到)+ 回车提交
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);

    // 场景1:播种重命名态 → 在网格上按下并拖动(drag,不是 click)→ 重命名应退出
    let t0 = harness.state().tabs[0].title.clone();
    harness.ctx.memory_mut(|m| {
        m.data
            .insert_temp(egui::Id::new("tab-rename"), (0usize, t0))
    });
    harness.step();
    assert!(
        harness.ctx.memory(|m| m.focused().is_some()),
        "播种重命名态后输入框应持有焦点"
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(500.0, 350.0)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(500.0, 350.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for x in [480.0f32, 460.0, 440.0] {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(x, 350.0)));
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(440.0, 350.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    assert!(
        harness.ctx.memory(|m| m.focused().is_none()),
        "拖动后重命名应退出(焦点释放)"
    );
}

#[test]
fn tab_rename_enter_commits() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    // 播种重命名态(入口已迁右键菜单)→ 打字 + 回车 → 标题应更新
    harness.ctx.memory_mut(|m| {
        m.data.insert_temp(
            egui::Id::new("tab-rename"),
            (0usize, "ads_report_youni".to_string()),
        )
    });
    harness.step();
    assert!(
        harness.ctx.memory(|m| m.focused().is_some()),
        "播种重命名态后输入框应持有焦点"
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("X".into()));
    harness.step();
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
        physical_key: None,
    });
    harness.step();
    harness.step();
    let title = harness.state().tabs[0].title.clone();
    eprintln!("[rename-enter] title after enter = {title}");
    // draft 预填原标题,输入 X 追加在光标处(末尾)
    assert_eq!(title, "ads_report_youniX", "回车应提交重命名");
}

#[test]
fn cmd_r_works_while_field_focused_but_not_during_cell_edit() {
    // v0.11.5 的焦点门拦一切文本框,导致 SQL 编辑器打字时 ⌘R 跑不了查询;
    // 收窄后:普通输入框(如筛选框)持焦不影响 ⌘R,单元格编辑中仍拦截
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);

    // 场景1:筛选框持焦 → ⌘R 应触发重载(loading 置位)
    harness
        .ctx
        .memory_mut(|m| m.request_focus(egui::Id::new("table-filter")));
    harness.step();
    assert!(
        harness.ctx.memory(|m| m.focused().is_some()),
        "筛选框应持有焦点"
    );
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::R,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    let seq_before = harness.state().tabs[2].table_data().unwrap().load_seq;
    harness.step();
    harness.step();
    // kittest 里后台任务瞬间失败返回会把 loading 复位,用 load_seq(单调递增)断言
    let seq_after = harness.state().tabs[2].table_data().unwrap().load_seq;
    eprintln!("[cmd-r filter-focused] load_seq {seq_before} -> {seq_after}");
    assert!(seq_after > seq_before, "筛选框持焦时 ⌘R 应触发重载");

    // 场景2:单元格编辑中 → ⌘R 不应触发(保护未提交草稿)
    if let TabKind::Table(t) = &mut harness.state_mut().tabs[2].kind {
        t.editing = Some((0, 0));
    }
    harness.input_mut().modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    let seq0 = harness.state().tabs[2].table_data().unwrap().load_seq;
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::R,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
        physical_key: None,
    });
    harness.step();
    harness.step();
    let t = harness.state().tabs[2].table_data().unwrap();
    eprintln!(
        "[cmd-r cell-editing] load_seq {} -> {} editing={:?}",
        seq0, t.load_seq, t.editing
    );
    assert_eq!(t.load_seq, seq0, "单元格编辑中 ⌘R 应被拦截");
}

#[test]
fn tab_click_with_hand_slopp_activates() {
    // 手抖点击(按下→滑动 8px 超过 egui 默认 6px 阈值→释放)应仍激活标签。
    // 修复前:被判 drag 不算 click,"点了没反应"(用户实测间歇复现)
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| {
            let mut app = full_app("role");
            app.auto_connect_done = true;
            ShujingApp::apply_fonts(&cc.egui_ctx);
            crate::theme::set_dark(true);
            app
        });
    harness.run_steps(8);
    // 初始激活第 2 个标签(role);手抖点第 0 个标签:按下→移 8px→释放
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(250.0, 14.0 + CHROME_TOP)));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(250.0, 14.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.step();
    for x in [252.0f32, 254.0, 256.0, 258.0] {
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(egui::pos2(x, 14.0 + CHROME_TOP)));
        harness.step();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: egui::pos2(258.0, 14.0 + CHROME_TOP),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.step();
    harness.step();
    let active = harness.state().active_tab;
    eprintln!("[sloppy-click] active_tab={active}");
    assert_eq!(active, 0, "带 8px 位移的点击应激活标签(手抖容忍)");
}

#[test]
fn korean_and_emoji_glyphs_covered() {
    // 韩文(谚文)与 emoji 不再是豆腐:字体栈必须覆盖这些字形
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(600.0, 300.0))
        .with_pixels_per_point(2.0)
        .build(move |ctx| {
            if !setup_done.get() {
                setup_done.set(true);
                ShujingApp::apply_fonts(ctx);
                crate::theme::set_dark(true);
                crate::theme::apply(ctx);
            }
        });
    harness.run_steps(3);
    let ctx = harness.ctx.clone();
    let prop = egui::FontId::proportional(12.0);
    let mono = egui::FontId::monospace(12.0);
    for (label, s) in [
        ("韩文谚文", "한국어 텍스트 안녕하세요"),
        ("emoji", "😀🚀✅❤️"),
        ("中文(回归)", "数镜连接查询"),
        ("日文假名(回归)", "あいうエオ"),
    ] {
        let ok_p = ctx.fonts(|f| f.has_glyphs(&prop, s));
        let ok_m = ctx.fonts(|f| f.has_glyphs(&mono, s));
        eprintln!("[glyphs] {label}: prop={ok_p} mono={ok_m}");
        assert!(ok_p, "{label} 比例字体应覆盖");
        assert!(ok_m, "{label} 等宽字体应覆盖");
    }
    // 快照目检(基线对齐/字号观感)
    let cell = std::cell::RefCell::new(());
    let _ = cell;
    let done = std::cell::Cell::new(false);
    let mut h2 = egui_kittest::Harness::builder()
        .with_size(egui::vec2(700.0, 240.0))
        .with_pixels_per_point(2.0)
        .build(move |ctx| {
            if !done.get() {
                done.set(true);
                ShujingApp::apply_fonts(ctx);
                crate::theme::set_dark(true);
                crate::theme::apply(ctx);
                return;
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("한국어 데이터 안녕하세요");
                ui.label("emoji: 😀 🚀 ✅ ❤️ 已完成");
                ui.monospace("한글 mono 😀 완료");
                ui.label("混合: 状态OK 상태 😀 完成");
            });
        });
    h2.run_steps(4);
    let img = h2.render().expect("render");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/korean_emoji.png").unwrap();
}

/// 紧凑密度整页渲染:Metrics token 真实换装的视觉验证(行高 26/字号 ×0.94)。
/// 全程持锁 + 结束复原:密度是进程级全局,别泄漏给并行的坐标测试
#[test]
fn render_compact_density() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let app = full_app("role");
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 760.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::set_density(1);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(6);
    let img = harness.render().expect("render 失败");
    let _ = std::fs::create_dir_all("/tmp/shujing_ui");
    img.save("/tmp/shujing_ui/compact_density.png")
        .expect("保存 PNG 失败");
    crate::theme::set_density(0);
}

/// 紧凑密度的多页核对:查询结果网格与 Redis 控制台(结构性高度全走 token,
/// 页面里写死的小件——WHERE 芯片 76×24、类型徽标 16 高——要能在紧凑档里放得下)
#[test]
fn render_compact_pages() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 查询页(带类型的结果集)
    let (q, conns, dbs) = query_view_fixture();
    let cell = std::cell::RefCell::new(Some(q));
    let setup_done = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 760.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, st: &mut std::cell::RefCell<Option<QueryTab>>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::set_density(1);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = st.borrow_mut();
                let q = b.as_mut().unwrap();
                let _ = query::draw_query_view(
                    &conns,
                    ctx,
                    "q1",
                    q,
                    &[],
                    &dbs,
                    true,
                    &[],
                    &Default::default(),
                    None,
                    false,
                );
            },
            cell,
        );
    harness.run_steps(6);
    harness
        .render()
        .expect("render 失败")
        .save("/tmp/shujing_ui/compact_query.png")
        .expect("保存 PNG 失败");

    // Redis 控制台
    let mut app = test_app(vec![], 0);
    app.open_redis("c1", 0);
    if let TabKind::Redis(rt) = &mut app.tabs.last_mut().unwrap().kind {
        rt.keys = vec![
            "user:1001".into(),
            "user:1002".into(),
            "session:abc".into(),
            "cache:home:banner".into(),
        ];
        rt.selected_key = Some("user:1001".into());
        rt.key_types = std::collections::HashMap::from([
            ("user:1001".to_string(), "hash".to_string()),
            ("user:1002".to_string(), "string".to_string()),
            ("session:abc".to_string(), "zset".to_string()),
            ("cache:home:banner".to_string(), "stream".to_string()),
        ]);
        rt.detail = Some(shujing_core::redis_ops::RedisDetail {
            key_type: "hash".into(),
            ttl: 3600,
            len: 2,
            text: None,
            pairs: vec![("name".into(), "张三".into()), ("city".into(), "北京".into())],
        });
    }
    let app = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1200.0, 700.0))
        .with_pixels_per_point(2.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            app,
        );
    harness.run_steps(6);
    harness
        .render()
        .expect("render 失败")
        .save("/tmp/shujing_ui/compact_redis.png")
        .expect("保存 PNG 失败");
    crate::theme::set_density(0);
}

// ---------------------------------------------------------------- 几何的像素断言

/// 在 RGBA8 原始像素里量某种颜色的纵向色带高度:逐列取最长连续段,返回全图里
/// 出现最多的段高 (众数, 命中列数)。不取"第一条够长的"——网格最右缘的列头会被
/// 上方控件和面板边缘切掉一截,残段高度参差不齐,先撞上它就量错了
fn color_band_height(
    w: u32,
    h: u32,
    raw: &[u8],
    color: [u8; 3],
    min_len: u32,
) -> Option<(u32, u32)> {
    let px = |x: u32, y: u32| -> [u8; 3] {
        let i = ((y * w + x) * 4) as usize;
        [raw[i], raw[i + 1], raw[i + 2]]
    };
    let mut counts: std::collections::HashMap<u32, u32> = Default::default();
    for x in (0..w).step_by(3) {
        let (mut best, mut cur) = (0u32, 0u32);
        for y in 0..h {
            if px(x, y) == color {
                cur += 1;
                best = best.max(cur);
            } else {
                cur = 0;
            }
        }
        if best >= min_len {
            *counts.entry(best).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n)
}

/// 渲染一个场景并返回 (宽, 高, RGBA8 原始像素)。调用方须已持有 RENDER_LOCK
fn render_pixels(
    w: f32,
    h: f32,
    mut draw: impl FnMut(&egui::Context),
) -> (u32, u32, Vec<u8>) {
    let setup_done = std::cell::Cell::new(false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(2.0)
        .build(move |ctx| {
            if !setup_done.get() {
                setup_done.set(true);
                ShujingApp::apply_fonts(ctx);
                crate::theme::set_dark(true);
                crate::theme::apply(ctx);
                return;
            }
            draw(ctx);
        });
    harness.run_steps(6);
    let img = harness.render().expect("render 失败");
    (img.width(), img.height(), img.into_raw())
}

/// 结果网格的表头必须与数据页同高(两行列头:名称 + 类型)。
///
/// 74·一 迁移度量常量时,grid.rs 的表头高度被换成了名字相近的"单行列头"token,
/// 查询结果 / 执行计划 / Parquet 预览 / Kafka 消息共用的结果网格表头从 52pt
/// 压到 36pt——两行文字仍画得下,所以肉眼和不量几何的测试都没看出来,还装了机。
/// 这里直接量渲染出来的列头底色段,不看 token 叫什么。
#[test]
fn result_grid_header_matches_data_page_header() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let header = crate::theme::c().header;
    let color = [header.r(), header.g(), header.b()];
    const MIN: u32 = 24;

    // 数据页
    let mut tab = role_tab();
    tab.pk_cols = vec!["id".into()];
    let cell = std::cell::RefCell::new(Some(tab));
    let conns: Vec<shujing_core::ConnInfo> = vec![];
    let (w, h, raw) = render_pixels(1280.0, 780.0, |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _ = table_view::draw_table_view(ctx, "t1", tab, &conns);
    });
    let (data_page, data_page_cols) = color_band_height(w, h, &raw, color, MIN)
        .expect("数据页应能找到列头底色段");

    // 查询结果网格(带类型的结果集)
    let (q, qconns, dbs) = query_view_fixture();
    let cell = std::cell::RefCell::new(Some(q));
    let (w, h, raw) = render_pixels(1280.0, 780.0, |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(
            &qconns, ctx, "q1", q, &[], &dbs, true, &[], &Default::default(), None, false,
        );
    });
    let (result_grid, result_grid_cols) = color_band_height(w, h, &raw, color, MIN)
        .expect("查询结果网格应能找到列头底色段");
    assert!(
        data_page_cols >= 8 && result_grid_cols >= 8,
        "命中列数太少(数据页 {data_page_cols},结果网格 {result_grid_cols}),众数不可信"
    );

    // 底部那条 1pt 横线不是列头底色,所以量到的比"高度×2"略短
    let expect = (crate::theme::data_header_h() * 2.0).round() as i64;
    for (what, measured) in [("数据页", data_page), ("结果网格", result_grid)] {
        let short = expect - measured as i64;
        assert!(
            (0..=4).contains(&short),
            "{what}表头量到 {measured}px;data_header_h={} 应约 {expect}px(只允许底部横线占 0~4px)",
            crate::theme::data_header_h()
        );
    }
    // 两处底线的抗锯齿可能差 1px,允许 1px
    assert!(
        result_grid.abs_diff(data_page) <= 1,
        "结果网格表头 {result_grid}px 应与数据页 {data_page}px 同高(两行列头对齐)"
    );
}


// ---------------------------------------------------------------- 网格键盘与剪贴板

type GridHarness<'a> = egui_kittest::Harness<'a, std::cell::RefCell<ShujingApp>>;

/// 数据网格夹具:role 表(id 主键,6 列),行数可扩到 `rows`(验证滚动)。
/// 列宽超出视口一截,横向可滚
fn keyboard_grid_harness(rows: usize) -> GridHarness<'static> {
    let mut app = full_app("role");
    if let TabKind::Table(t) = &mut app.tabs[2].kind {
        t.active_filter = None;
        while t.rows.len() < rows {
            let r = t.rows[t.rows.len() % 8].clone();
            t.rows.push(r);
        }
    }
    let cell = std::cell::RefCell::new(app);
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, app: &mut std::cell::RefCell<ShujingApp>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = app.borrow_mut();
                draw_app(&mut b, ctx);
            },
            cell,
        );
    harness.run_steps(8);
    harness
}

fn press_key(h: &mut GridHarness<'_>, key: egui::Key, m: egui::Modifiers) {
    h.input_mut().modifiers = m;
    h.input_mut().events.push(egui::Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers: m,
        physical_key: None,
    });
    h.step();
    h.step();
    h.input_mut().modifiers = Default::default();
}

fn type_text(h: &mut GridHarness<'_>, s: &str) {
    h.input_mut().events.push(egui::Event::Text(s.to_string()));
    h.step();
    h.step();
}

/// 单击(完整的按下/释放),坐标是网格内的逻辑坐标
fn click_grid(h: &mut GridHarness<'_>, pos: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.step();
    h.step();
}

/// (选中格, 矩形选区)
fn selection(h: &GridHarness<'_>) -> (Option<(usize, usize)>, Option<((usize, usize), (usize, usize))>) {
    let app = h.state().borrow();
    let t = app.tabs[2].table_data().unwrap();
    (t.selected, t.sel_range)
}

/// 网格里的状态快照(借用结束后再断言)
struct GridSnap {
    editing: Option<(usize, usize)>,
    draft: String,
    changes: Vec<(usize, String, Option<String>)>,
    cells: Vec<Vec<Option<String>>>,
}

fn snap(h: &GridHarness<'_>) -> GridSnap {
    let app = h.state().borrow();
    let t = app.tabs[2].table_data().unwrap();
    GridSnap {
        editing: t.editing,
        draft: t.draft.clone(),
        changes: t.changes.clone(),
        cells: t.rows.clone(),
    }
}

#[test]
fn grid_keyboard_navigation_moves_selection() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    let plain = egui::Modifiers::NONE;
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let cmd = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    use egui::Key::*;
    // 点选 role 列第一行 =(0,1)
    click_grid(&mut h, egui::pos2(380.0, 182.0));
    // 点选本身也落一个 1×1 的矩形选区(拖选状态机的起点,既有行为)
    assert_eq!(selection(&h), (Some((0, 1)), Some(((0, 1), (0, 1)))), "点选后应选中 (0,1)");

    // 方向键逐格移动
    press_key(&mut h, ArrowDown, plain);
    assert_eq!(selection(&h), (Some((1, 1)), None), "↓ 收拢为单格");
    press_key(&mut h, ArrowRight, plain);
    assert_eq!(selection(&h).0, Some((1, 2)), "→");
    press_key(&mut h, ArrowUp, plain);
    assert_eq!(selection(&h).0, Some((0, 2)), "↑");
    press_key(&mut h, ArrowLeft, plain);
    assert_eq!(selection(&h).0, Some((0, 1)), "←");
    press_key(&mut h, ArrowUp, plain);
    assert_eq!(selection(&h).0, Some((0, 1)), "顶边不越界");

    // Shift+方向:以原格为锚点扩展矩形选区
    press_key(&mut h, ArrowDown, shift);
    assert_eq!(selection(&h), (Some((1, 1)), Some(((0, 1), (1, 1)))), "Shift+↓ 扩展");
    press_key(&mut h, ArrowDown, shift);
    assert_eq!(selection(&h).1, Some(((0, 1), (2, 1))), "继续扩展");
    press_key(&mut h, ArrowRight, plain);
    assert_eq!(selection(&h), (Some((2, 2)), None), "不带 Shift 收拢为单格");

    // Home/End 到行首行尾;Tab 行尾折到下一行,Shift+Tab 行首折到上一行
    press_key(&mut h, End, plain);
    assert_eq!(selection(&h).0, Some((2, 5)), "End");
    press_key(&mut h, Tab, plain);
    assert_eq!(selection(&h).0, Some((3, 0)), "Tab 行尾折返");
    press_key(&mut h, Home, plain);
    assert_eq!(selection(&h).0, Some((3, 0)), "Home");
    press_key(&mut h, Tab, shift);
    assert_eq!(selection(&h).0, Some((2, 5)), "Shift+Tab 行首折返");

    // ⌘+方向:跳到行/列首尾;翻页越界夹到最后一行
    press_key(&mut h, ArrowDown, cmd);
    assert_eq!(selection(&h).0, Some((7, 5)), "⌘↓ 最后一行");
    press_key(&mut h, ArrowUp, cmd);
    assert_eq!(selection(&h).0, Some((0, 5)), "⌘↑ 第一行");
    press_key(&mut h, ArrowLeft, cmd);
    assert_eq!(selection(&h).0, Some((0, 0)), "⌘← 第一列");
    press_key(&mut h, ArrowRight, cmd);
    assert_eq!(selection(&h).0, Some((0, 5)), "⌘→ 最后一列");
    press_key(&mut h, PageDown, plain);
    assert_eq!(selection(&h).0, Some((7, 5)), "PageDown 夹到最后一行");
    press_key(&mut h, PageUp, plain);
    assert_eq!(selection(&h).0, Some((0, 5)), "PageUp 回到第一行");
}

#[test]
fn grid_keyboard_edit_then_commit_moves_on() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    let plain = egui::Modifiers::NONE;
    use egui::Key::*;
    click_grid(&mut h, egui::pos2(380.0, 182.0));
    assert_eq!(selection(&h).0, Some((0, 1)));

    // 选中格上直接打字 = 进入编辑,首字符就是打的内容
    type_text(&mut h, "x");
    let s = snap(&h);
    assert_eq!(s.editing, Some((0, 1)), "打字应进入编辑");
    assert_eq!(s.draft, "x");
    type_text(&mut h, "y");
    assert_eq!(snap(&h).draft, "xy");

    // Tab:提交并移到右边一格
    press_key(&mut h, Tab, plain);
    let s = snap(&h);
    assert_eq!(s.editing, None, "Tab 应提交并退出编辑");
    assert_eq!(s.cells[0][1].as_deref(), Some("xy"));
    assert!(s.changes.contains(&(0, "role".to_string(), Some("xy".to_string()))));
    assert_eq!(selection(&h).0, Some((0, 2)), "Tab 后移到右边一格");

    // 回车:进入编辑(带着原值);再回车:提交并移到下一行
    let original = snap(&h).cells[0][2].clone();
    press_key(&mut h, Enter, plain);
    let s = snap(&h);
    assert_eq!(s.editing, Some((0, 2)), "回车应进入编辑");
    assert_eq!(s.draft, original.clone().unwrap_or_default(), "编辑框带原值");
    type_text(&mut h, "!");
    press_key(&mut h, Enter, plain);
    let s = snap(&h);
    assert_eq!(s.editing, None);
    let expect = format!("{}!", original.unwrap_or_default());
    assert_eq!(s.cells[0][2].as_deref(), Some(expect.as_str()));
    assert_eq!(selection(&h).0, Some((1, 2)), "回车后移到下一行");

    // F2 进入编辑,Esc 取消:值不变,选中格不动
    press_key(&mut h, F2, plain);
    assert_eq!(snap(&h).editing, Some((1, 2)), "F2 应进入编辑");
    type_text(&mut h, "Z");
    assert_eq!(snap(&h).draft, "内容编辑Z");
    press_key(&mut h, Escape, plain);
    let s = snap(&h);
    assert_eq!(s.editing, None, "Esc 退出编辑");
    assert_eq!(s.cells[1][2].as_deref(), Some("内容编辑"), "Esc 不提交");
    assert_eq!(selection(&h).0, Some((1, 2)));

    // Esc 之后网格继续接管键盘:Delete 把选中格置 NULL(暂存)
    press_key(&mut h, Delete, plain);
    let s = snap(&h);
    assert_eq!(s.cells[1][2], None, "Delete 置 NULL");
    assert!(s.changes.contains(&(1, "role_name".to_string(), None)));
    // Shift+Tab:回到左边一格
    press_key(&mut h, Tab, egui::Modifiers { shift: true, ..Default::default() });
    assert_eq!(selection(&h).0, Some((1, 1)));

    // Delete 不动主键列(与右键菜单"设为 NULL"同一条规则)
    click_grid(&mut h, egui::pos2(285.0, 212.0));
    assert_eq!(selection(&h).0, Some((1, 0)), "点选 id 列第二行");
    let before = snap(&h).changes.len();
    press_key(&mut h, Delete, plain);
    let s = snap(&h);
    assert_eq!(s.cells[1][0].as_deref(), Some("2"), "主键不被 Delete 置 NULL");
    assert_eq!(s.changes.len(), before, "主键格上 Delete 不产生改动");
}

#[test]
fn grid_paste_block_and_single_value_fill() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    click_grid(&mut h, egui::pos2(380.0, 182.0));
    assert_eq!(selection(&h).0, Some((0, 1)));

    // 从 Excel 那样复制来的 3×2 块:贴到以选中格为左上角的位置
    h.input_mut()
        .events
        .push(egui::Event::Paste("a\tb\nc\td\ne\tf".to_string()));
    h.step();
    h.step();
    let s = snap(&h);
    assert_eq!(s.cells[0][1].as_deref(), Some("a"));
    assert_eq!(s.cells[0][2].as_deref(), Some("b"));
    assert_eq!(s.cells[2][1].as_deref(), Some("e"));
    assert_eq!(s.cells[2][2].as_deref(), Some("f"));
    assert_eq!(s.changes.len(), 6, "3×2 块应产生 6 处改动: {:?}", s.changes);
    assert_eq!(selection(&h), (Some((0, 1)), Some(((0, 1), (2, 2)))), "贴完选中贴入的区域");

    // 单值贴进多格选区 = 填充整个选区(与"打字后回车"同一语义,主键列跳过)
    press_key(
        &mut h,
        egui::Key::A,
        egui::Modifiers { command: true, ..Default::default() },
    );
    assert_eq!(selection(&h).1, Some(((0, 0), (7, 5))));
    h.input_mut().events.push(egui::Event::Paste("z".to_string()));
    h.step();
    h.step();
    let s = snap(&h);
    assert_eq!(s.changes.len(), 8 * 5, "8 行 × 5 个非主键列");
    assert!(
        s.changes.iter().all(|(_, c, v)| c != "id" && v.as_deref() == Some("z")),
        "主键列跳过,其余都是 z"
    );
    assert_eq!(s.cells[7][4].as_deref(), Some("z"));
    assert_eq!(s.cells[7][0].as_deref(), Some("8"), "主键不动");
}

/// 两幅渲染图(RGBA 原始字节)在给定矩形内的差异像素数;矩形坐标是物理像素
fn diff_pixels(a: &[u8], b: &[u8], w: usize, rect: (usize, usize, usize, usize)) -> usize {
    let (x0, y0, x1, y1) = rect;
    (y0..y1)
        .map(|y| {
            (x0..x1)
                .filter(|&x| {
                    let i = (y * w + x) * 4;
                    a[i..i + 3] != b[i..i + 3]
                })
                .count()
        })
        .sum()
}

#[test]
fn grid_keyboard_scrolls_selection_into_view() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(100);
    let plain = egui::Modifiers::NONE;
    click_grid(&mut h, egui::pos2(380.0, 182.0));
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(60.0, 660.0)));
    h.step();
    let before = h.render().expect("render").into_raw();
    assert_eq!(table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed), 0);

    // 翻到看不见的行:网格要跟着滚
    press_key(&mut h, egui::Key::PageDown, plain);
    press_key(&mut h, egui::Key::PageDown, plain);
    let row = selection(&h).0.expect("选中格").0;
    assert!(row >= 20, "两次翻页后应到了第 {row} 行");
    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(60.0, 660.0)));
    h.step();
    let after = h.render().expect("render").into_raw();
    // 数据区(避开侧栏/表头/底栏):x 240..900,y 200..620(逻辑坐标,×2)
    let moved = diff_pixels(&before, &after, 2000, (480, 400, 1800, 1240));
    assert!(moved > 100_000, "翻页后数据区应滚动了,差异像素 {moved}");

    // End 到最后一列:横向也要滚进视野
    press_key(&mut h, egui::Key::End, plain);
    let hx = table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
    assert!(hx > 0, "End 到最右列后横向滚动条应离开原点,实际 {hx}");
}

// ---------------------------------------------------------------- 查询结果网格:选中 / 复制 / 键盘 / 右键菜单 / 编辑

type ResultHarness<'a> = egui_kittest::Harness<'a, std::cell::RefCell<Option<QueryTab>>>;

const RG_ID: &str = "query-results-q1";
/// 查询结果网格夹具:5 列(id/role/role_name/status/created)× 24 行,role_name 每 4 行一个 NULL。
/// `editable` = 单表带主键的结果集,网格可回写
fn result_grid_harness(w: f32, h: f32, editable: bool) -> ResultHarness<'static> {
    let (mut q, conns, dbs) = query_view_fixture();
    if editable {
        q.result_table = Some("backend_role_privilege".into());
        q.pk_checked = true;
        q.result_pk = vec!["id".into()];
    }
    let cell = std::cell::RefCell::new(Some(q));
    let setup_done = std::cell::Cell::new(false);
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(2.0)
        .with_step_dt(1.0 / 60.0)
        .build_state(
            move |ctx, cell: &mut std::cell::RefCell<Option<QueryTab>>| {
                if !setup_done.get() {
                    setup_done.set(true);
                    ShujingApp::apply_fonts(ctx);
                    crate::theme::set_dark(true);
                    crate::theme::apply(ctx);
                    return;
                }
                let mut b = cell.borrow_mut();
                let q = b.as_mut().unwrap();
                let _ = query::draw_query_view(
                    &conns,
                    ctx,
                    "q1",
                    q,
                    &[],
                    &dbs,
                    true,
                    &[],
                    &Default::default(),
                    None,
                    false,
                );
            },
            cell,
        );
    harness.run_steps(8);
    harness
}

fn result_sel(h: &ResultHarness<'_>) -> Option<((usize, usize), (usize, usize))> {
    crate::grid::selection(&h.ctx, RG_ID, result_shape(h))
}

/// 结果集当前的真实形状(行数, 列数)
fn result_shape(h: &ResultHarness<'_>) -> (usize, usize) {
    let cell = h.state().borrow();
    let q = cell.as_ref().expect("query tab");
    let r = &q.results[q.active_set.min(q.results.len().saturating_sub(1))];
    (r.rows.len(), r.columns.len())
}

/// 单元格中心的逻辑坐标:按网格上一帧的真实列宽算(窗口宽度、密度变了都跟着对)
fn cell_pos(h: &ResultHarness<'_>, r: usize, c: usize) -> egui::Pos2 {
    let (ws, left, body_top) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    let mut x = left + ws[0];
    for w in &ws[1..=c] {
        x += w;
    }
    egui::pos2(
        x + ws[1 + c] / 2.0,
        body_top + crate::theme::row_h() * (r as f32 + 0.5),
    )
}

/// 行号列中心
fn rowno_pos(h: &ResultHarness<'_>, r: usize) -> egui::Pos2 {
    let (_, left, body_top) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    egui::pos2(
        left + crate::theme::ROWNO_W / 2.0,
        body_top + crate::theme::row_h() * (r as f32 + 0.5),
    )
}

/// 点/拖/右键/双击单元格(坐标在函数内算好,免得借用打架)
fn click_cell(h: &mut ResultHarness<'_>, r: usize, c: usize) {
    let pos = cell_pos(h, r, c);
    click_at(h, pos);
}

fn click_cell_with(h: &mut ResultHarness<'_>, r: usize, c: usize, m: egui::Modifiers) {
    let pos = cell_pos(h, r, c);
    click_at_with(h, pos, m);
}

fn drag_cells(h: &mut ResultHarness<'_>, a: (usize, usize), b: (usize, usize)) {
    let (from, to) = (cell_pos(h, a.0, a.1), cell_pos(h, b.0, b.1));
    drag_at(h, from, to);
}

fn right_click_cell(h: &mut ResultHarness<'_>, r: usize, c: usize) {
    let pos = cell_pos(h, r, c);
    right_click_at(h, pos);
}

fn double_click_cell(h: &mut ResultHarness<'_>, r: usize, c: usize) {
    let pos = cell_pos(h, r, c);
    double_click_at(h, pos);
}

/// 选区收拢成单格时的那个格(键盘导航断言用);多格选区返回 None
fn result_cur(h: &ResultHarness<'_>) -> Option<(usize, usize)> {
    result_sel(h).filter(|(a, b)| a == b).map(|(a, _)| a)
}

/// 结果网格当前的横向滚动偏移
fn result_hscroll(h: &ResultHarness<'_>) -> f32 {
    crate::grid::debug_hscroll(&h.ctx, RG_ID)
}

/// 网格里的状态快照(借用结束后再断言)
struct QSnap {
    editing: Option<(usize, usize)>,
    draft: String,
    changes: Vec<(usize, String, Option<String>)>,
    sql: String,
}

fn q_snap(h: &ResultHarness<'_>) -> QSnap {
    let cell = h.state().borrow();
    let q = cell.as_ref().expect("query tab");
    QSnap {
        editing: q.q_editing,
        draft: q.q_draft.clone(),
        changes: q.q_changes.clone(),
        sql: q.sql.clone(),
    }
}

/// 这一帧应用写进剪贴板的文本
fn copied_text<S>(h: &egui_kittest::Harness<'_, S>) -> String {
    h.output()
        .platform_output
        .commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 完整的一次按键;返回按键那一帧写进剪贴板的文本
fn key_once<S>(h: &mut egui_kittest::Harness<'_, S>, key: egui::Key, m: egui::Modifiers) -> String {
    h.input_mut().modifiers = m;
    h.input_mut().events.push(egui::Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers: m,
        physical_key: None,
    });
    h.step();
    let copied = copied_text(h);
    h.input_mut().modifiers = Default::default();
    h.step();
    copied
}

fn pointer_button<S>(
    h: &mut egui_kittest::Harness<'_, S>,
    pos: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Default::default(),
    });
}

/// 单击(完整的按下/释放)
fn click_at<S>(h: &mut egui_kittest::Harness<'_, S>, pos: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
    pointer_button(h, pos, egui::PointerButton::Primary, true);
    h.step();
    pointer_button(h, pos, egui::PointerButton::Primary, false);
    h.step();
    h.step();
}

/// 双击(第二次按下落在系统双击间隔内)
fn double_click_at<S>(h: &mut egui_kittest::Harness<'_, S>, pos: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
    for _ in 0..2 {
        pointer_button(h, pos, egui::PointerButton::Primary, true);
        h.step();
        pointer_button(h, pos, egui::PointerButton::Primary, false);
        h.step();
    }
    h.run_steps(3);
}

/// 右键(按下/释放 → 弹出右键菜单)
fn right_click_at<S>(h: &mut egui_kittest::Harness<'_, S>, pos: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
    pointer_button(h, pos, egui::PointerButton::Secondary, true);
    h.step();
    pointer_button(h, pos, egui::PointerButton::Secondary, false);
    h.step();
    h.run_steps(3);
}

/// 按住左键从 from 拖到 to(分 4 步移动)
fn drag_at<S>(h: &mut egui_kittest::Harness<'_, S>, from: egui::Pos2, to: egui::Pos2) {
    h.input_mut().events.push(egui::Event::PointerMoved(from));
    h.step();
    pointer_button(h, from, egui::PointerButton::Primary, true);
    h.step();
    for k in 1..=4 {
        let pos = egui::pos2(
            from.x + (to.x - from.x) * k as f32 / 4.0,
            from.y + (to.y - from.y) * k as f32 / 4.0,
        );
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
        h.step();
    }
    pointer_button(h, to, egui::PointerButton::Primary, false);
    h.step();
    h.run_steps(2);
}

fn type_into<S>(h: &mut egui_kittest::Harness<'_, S>, s: &str) {
    h.input_mut().events.push(egui::Event::Text(s.to_string()));
    h.step();
    h.step();
}

/// 跑若干帧,收集期间所有写进剪贴板的文本
fn drain_copies<S>(h: &mut egui_kittest::Harness<'_, S>, steps: usize) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..steps {
        h.step();
        out.extend(
            h.output()
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                }),
        );
    }
    out
}

/// 带修饰键的单击(⇧ 点选 = 以原锚点扩展选区)
fn click_at_with<S>(h: &mut egui_kittest::Harness<'_, S>, pos: egui::Pos2, m: egui::Modifiers) {
    h.input_mut().events.push(egui::Event::PointerMoved(pos));
    h.step();
    h.input_mut().modifiers = m;
    pointer_button(h, pos, egui::PointerButton::Primary, true);
    h.step();
    pointer_button(h, pos, egui::PointerButton::Primary, false);
    h.step();
    h.input_mut().modifiers = Default::default();
    h.step();
}

/// 数据行下方的空白处(点它不应改变选区):第 6 行的位置,结果集得不足 6 行
fn blank_pos(h: &ResultHarness<'_>) -> egui::Pos2 {
    let (_, left, body_top) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    egui::pos2(left + 300.0, body_top + crate::theme::row_h() * 6.5)
}

fn cmd() -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        ..Default::default()
    }
}

fn shift() -> egui::Modifiers {
    egui::Modifiers {
        shift: true,
        ..Default::default()
    }
}

/// 结果网格此前连单元格都复制不了:点格没反应、⌘C 无效。
/// 现在:点选 → ⌘C 原样复制(NULL 写成 "NULL",徽章列复制原始值);点行号选整行
#[test]
fn result_grid_click_selects_and_copies_cell() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = result_grid_harness(1100.0, 700.0, false);
    assert_eq!(result_sel(&h), None, "没点过之前没有选区");
    assert_eq!(
        key_once(&mut h, egui::Key::C, cmd()),
        "",
        "没有选区时 ⌘C 不写剪贴板"
    );

    click_cell(&mut h, 0, 1);
    assert_eq!(result_sel(&h), Some(((0, 1), (0, 1))), "点选 role 列第一行");
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), "role_0");

    // NULL 格:复制成 "NULL" 文本(此前静默不复制)
    click_cell(&mut h, 0, 2);
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), "NULL");

    // 状态徽章列:复制的是原始值,不是徽章上的花体字
    click_cell(&mut h, 0, 3);
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), "1");

    // 点行号 = 选中整行,⌘C 得到整行 TSV
    {
        let pos = rowno_pos(&h, 1);
        click_at(&mut h, pos);
    }
    assert_eq!(result_sel(&h), Some(((1, 0), (1, 4))));
    assert_eq!(
        key_once(&mut h, egui::Key::C, cmd()),
        "2\trole_1\t角色 1\t2\t1731383330"
    );

    // 结果集换成 3 行(形状变了,旧选区作废);点表尾空白不动选区,⌘C 仍可用
    h.state().borrow_mut().as_mut().unwrap().results[0]
        .rows
        .truncate(3);
    h.run_steps(3);
    assert_eq!(result_sel(&h), None, "结果集变了旧选区作废");
    click_cell(&mut h, 1, 1);
    assert_eq!(result_sel(&h), Some(((1, 1), (1, 1))));
    {
        let pos = blank_pos(&h);
        click_at(&mut h, pos);
    }
    assert_eq!(result_sel(&h), Some(((1, 1), (1, 1))), "点空白不动选区");
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), "role_1");
}

/// 拖选矩形 / ⇧ 点选扩展 / ⌘A 全选:⌘C 得到 行×列 的 TSV。
/// 按住数据格拖动不再是横向平移
#[test]
fn result_grid_drag_shift_and_select_all_copy_tsv() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 窄窗口:列宽超出视口,拖动若还平移就会体现在横向偏移上
    let mut h = result_grid_harness(900.0, 700.0, false);
    assert_eq!(result_hscroll(&h), 0.0);

    // 从 role 第 1 行拖到 role_name 第 3 行
    drag_cells(&mut h, (0, 1), (2, 2));
    assert_eq!(result_sel(&h), Some(((0, 1), (2, 2))), "拖出矩形选区");
    assert_eq!(result_hscroll(&h), 0.0, "拖选不应平移网格");
    assert_eq!(
        key_once(&mut h, egui::Key::C, cmd()),
        "role_0\tNULL\nrole_1\t角色 1\nrole_2\t角色 2"
    );

    // ⇧ 点选:锚点不动,选区扩到新格
    click_cell(&mut h, 0, 1);
    click_cell_with(&mut h, 1, 3, shift());
    assert_eq!(result_sel(&h), Some(((0, 1), (1, 3))), "⇧ 点选以原锚点扩展");

    // ⌘A 全选整个结果集
    assert_eq!(key_once(&mut h, egui::Key::A, cmd()), "");
    assert_eq!(result_sel(&h), Some(((0, 0), (23, 4))));
    let all = key_once(&mut h, egui::Key::C, cmd());
    let lines: Vec<&str> = all.lines().collect();
    assert_eq!(lines.len(), 24, "{all}");
    assert!(lines.iter().all(|l| l.split('\t').count() == 5), "{all}");
    assert_eq!(lines[0], "1\trole_0\tNULL\t1\t1731383330");
    assert_eq!(lines[23], "24\trole_23\t角色 23\t2\t1731383330");
}

/// 键盘导航:方向键/Tab/Home/End/翻页/⌘+方向,Shift 扩展;选中格滚入视野(纵向 + 横向)
#[test]
fn result_grid_keyboard_moves_selection_and_scrolls_into_view() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let (cmd, shift) = (cmd(), shift());
    // 900 宽:5 列放不下,created 列在视口外
    let mut h = result_grid_harness(900.0, 700.0, false);
    use egui::Key::*;
    // 没点过:第一次按导航键落到左上角
    key_once(&mut h, ArrowDown, plain);
    assert_eq!(result_sel(&h), Some(((0, 0), (0, 0))));
    key_once(&mut h, ArrowRight, plain);
    assert_eq!(result_cur(&h), Some((0, 1)));
    key_once(&mut h, ArrowDown, plain);
    assert_eq!(result_cur(&h), Some((1, 1)));
    key_once(&mut h, ArrowUp, plain);
    assert_eq!(result_cur(&h), Some((0, 1)));
    key_once(&mut h, ArrowLeft, plain);
    key_once(&mut h, ArrowUp, plain);
    assert_eq!(result_sel(&h), Some(((0, 0), (0, 0))), "顶边/左边不越界");

    // Shift+方向:以原格为锚点扩展矩形选区
    key_once(&mut h, ArrowDown, shift);
    assert_eq!(result_sel(&h), Some(((0, 0), (1, 0))));
    key_once(&mut h, ArrowRight, plain);
    assert_eq!(
        result_sel(&h),
        Some(((1, 1), (1, 1))),
        "不带 Shift 收拢为单格"
    );

    // End 到最后一列:列宽超出视口,要横向滚进视野;Home 回到第一列滚回原点
    key_once(&mut h, End, plain);
    assert_eq!(result_cur(&h), Some((1, 4)));
    let hx = result_hscroll(&h);
    assert!(hx > 0.0, "End 到最右列后应横向滚入视野,实际偏移 {hx}");
    key_once(&mut h, Home, plain);
    assert_eq!(result_cur(&h), Some((1, 0)));
    assert_eq!(result_hscroll(&h), 0.0, "Home 回到第一列应滚回原点");

    // Tab 逐格右移;⌘+方向跳首尾;翻页越界夹取
    key_once(&mut h, Tab, plain);
    assert_eq!(result_cur(&h), Some((1, 1)));
    key_once(&mut h, ArrowRight, cmd);
    assert_eq!(result_cur(&h), Some((1, 4)));
    key_once(&mut h, ArrowLeft, cmd);
    assert_eq!(result_cur(&h), Some((1, 0)));
    key_once(&mut h, ArrowDown, cmd);
    assert_eq!(result_cur(&h), Some((23, 0)));
    key_once(&mut h, ArrowUp, cmd);
    assert_eq!(result_cur(&h), Some((0, 0)));

    // 翻到看不见的行:网格要跟着滚。先量一帧,翻两页后数据区应整片变化
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(40.0, 680.0)));
    h.step();
    let before = h.render().expect("render").into_raw();
    key_once(&mut h, PageDown, plain);
    key_once(&mut h, PageDown, plain);
    let row = result_sel(&h).unwrap().0 .0;
    assert!(row >= 12, "两次翻页后应到第 {row} 行之后");
    let after = h.render().expect("render").into_raw();
    // 数据区:避开侧栏/表头/滚动条,逻辑坐标 y 430..660 × 全宽(×2)
    let (w, _) = (1800usize, 1400usize);
    let moved = diff_pixels(&before, &after, w, (40, 860, 1760, 1320));
    assert!(moved > 100_000, "翻页后数据区应滚动了,差异像素 {moved}");
}

/// 点选结果格后网格持焦;Esc 把焦点交回 SQL 编辑器——接着打字进的是编辑器,
/// 方向键也归编辑器(选区不再动)
#[test]
fn result_grid_escape_hands_focus_back_to_editor() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let mut h = result_grid_harness(1100.0, 700.0, false);
    let sql0 = q_snap(&h).sql;

    click_cell(&mut h, 0, 1);
    key_once(&mut h, egui::Key::ArrowDown, plain);
    assert_eq!(
        result_sel(&h),
        Some(((1, 1), (1, 1))),
        "网格持焦时方向键移动选区"
    );

    key_once(&mut h, egui::Key::Escape, plain);
    assert_eq!(result_sel(&h), Some(((1, 1), (1, 1))), "Esc 不动选区");
    type_into(&mut h, "X");
    let sql1 = q_snap(&h).sql;
    assert!(
        sql1.contains('X') && sql1.len() == sql0.len() + 1,
        "打字应进 SQL 编辑器: {sql1:?}"
    );

    let before = result_sel(&h);
    key_once(&mut h, egui::Key::ArrowDown, plain);
    assert_eq!(result_sel(&h), before, "编辑器持焦后方向键归编辑器");
}

/// 双击 / 回车 = 查看完整内容(长文本被列宽截断时用);Esc 关闭后画面回到原样
#[test]
fn result_grid_double_click_and_enter_open_full_content() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let mut h = result_grid_harness(1100.0, 700.0, false);

    // 先点选一次(让"关掉弹窗后的画面"有选区可比),再隔开双击间隔
    let at = cell_pos(&h, 1, 2);
    click_at(&mut h, at);
    let _ = &at;
    assert_eq!(result_sel(&h), Some(((1, 2), (1, 2))));
    h.run_steps(40);
    let before = h.render().expect("render").into_raw();

    double_click_at(&mut h, at);
    let open = h.render().expect("render").into_raw();
    let dark = diff_pixels(&before, &open, 2200, (0, 0, 2200, 1400));
    assert!(dark > 300_000, "双击应打开弹窗(整幅压暗),差异像素 {dark}");

    key_once(&mut h, egui::Key::Escape, plain);
    let closed = h.render().expect("render").into_raw();
    assert_eq!(closed, before, "Esc 关闭弹窗后应回到原样");

    // 回车同样打开
    key_once(&mut h, egui::Key::Enter, plain);
    let open2 = h.render().expect("render").into_raw();
    assert!(
        diff_pixels(&closed, &open2, 2200, (0, 0, 2200, 1400)) > 300_000,
        "回车也应打开弹窗"
    );
    key_once(&mut h, egui::Key::Escape, plain);
    assert_eq!(h.render().expect("render").into_raw(), before);
}

/// 右键菜单:右键改选该格;复制整行 / 复制选区(带列名)/ 复制单元格。
/// 右键落在多格选区内时保留选区,菜单作用于整个选区
#[test]
fn result_grid_context_menu_copies_row_and_selection() {
    use egui_kittest::kittest::Queryable as _;
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = result_grid_harness(1100.0, 700.0, false);

    // 右键 role 列第 2 行 → 菜单 → 复制整行
    right_click_cell(&mut h, 1, 1);
    assert_eq!(result_sel(&h), Some(((1, 1), (1, 1))), "右键改选该格");
    assert!(
        h.query_by_label("复制整行 (TSV)").is_some(),
        "右键菜单应打开"
    );
    {
        let node = h.get_by_label("复制整行 (TSV)");
        node.simulate_click();
    }
    assert_eq!(
        drain_copies(&mut h, 6),
        vec!["2\trole_1\t角色 1\t2\t1731383330".to_string()]
    );

    // 拖出 2×2 选区后在其中右键:选区保留,菜单给"复制选区"
    drag_cells(&mut h, (0, 1), (1, 2));
    assert_eq!(result_sel(&h), Some(((0, 1), (1, 2))));
    right_click_cell(&mut h, 1, 2);
    assert_eq!(
        result_sel(&h),
        Some(((0, 1), (1, 2))),
        "右键落在选区内不改变选区"
    );
    assert!(
        h.query_by_label("复制选区(带列名)").is_some(),
        "多格选区的菜单应有带列名复制"
    );
    {
        let node = h.get_by_label("复制选区(带列名)");
        node.simulate_click();
    }
    assert_eq!(
        drain_copies(&mut h, 6),
        vec!["role\trole_name\nrole_0\tNULL\nrole_1\t角色 1".to_string()]
    );

    // 菜单用完焦点还回网格:方向键接着能用(此前菜单按钮把焦点拿走,键盘导航就此失效)
    assert!(h.query_by_label("复制选区").is_none(), "动作后菜单应关闭");
    // 选区当前格在第 2 行的 role_name:按 ↓ 收拢并移到第 3 行
    key_once(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
    assert_eq!(
        result_sel(&h),
        Some(((2, 2), (2, 2))),
        "菜单关了之后方向键应移动选区"
    );

    // Esc 关掉菜单(没点任何项)后,键盘同样可用
    right_click_cell(&mut h, 2, 1);
    assert!(h.query_by_label("复制单元格").is_some(), "菜单应再次打开");
    key_once(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(h.query_by_label("复制单元格").is_none(), "Esc 关闭菜单");
    key_once(&mut h, egui::Key::ArrowRight, egui::Modifiers::NONE);
    assert_eq!(
        result_sel(&h),
        Some(((2, 2), (2, 2))),
        "Esc 关掉菜单后方向键应移动选区"
    );
}

/// 可编辑结果集(单表带主键):回车编辑、提交后移到下一行、直接打字即编辑、Esc 取消、
/// Delete 置 NULL(暂存)、⌘V 整块粘贴。都只是暂存改动,保存前不碰数据库
#[test]
fn query_grid_edit_by_keyboard_and_paste() {
    use egui_kittest::kittest::Queryable as _;
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let mut h = result_grid_harness(1100.0, 700.0, true);
    assert!(q_snap(&h).changes.is_empty());

    // 点选后回车进入编辑,编辑框带着原值
    click_cell(&mut h, 0, 1);
    key_once(&mut h, egui::Key::Enter, plain);
    let s = q_snap(&h);
    assert_eq!(s.editing, Some((0, 1)));
    assert_eq!(s.draft, "role_0");
    type_into(&mut h, "!");
    assert_eq!(q_snap(&h).draft, "role_0!");
    // 再回车:提交,选中格移到下一行
    key_once(&mut h, egui::Key::Enter, plain);
    let s = q_snap(&h);
    assert_eq!(s.editing, None);
    assert_eq!(
        s.changes,
        vec![(0, "role".to_string(), Some("role_0!".to_string()))]
    );
    assert_eq!(result_sel(&h), Some(((1, 1), (1, 1))), "回车后移到下一行");

    // 直接打字 = 从选中格开始编辑,首字符就是打的内容;Esc 取消,不留改动
    type_into(&mut h, "Z");
    let s = q_snap(&h);
    assert_eq!(s.editing, Some((1, 1)));
    assert_eq!(s.draft, "Z");
    key_once(&mut h, egui::Key::Escape, plain);
    let s = q_snap(&h);
    assert_eq!(s.editing, None);
    assert_eq!(s.changes.len(), 1, "Esc 不提交");

    // Esc 之后网格继续接管键盘:Delete 把选中格置 NULL(暂存)。
    // 此前"改成 NULL"后网格里仍显示旧值,只有底色提示
    key_once(&mut h, egui::Key::Delete, plain);
    let s = q_snap(&h);
    assert!(
        s.changes.contains(&(1, "role".to_string(), None)),
        "{:?}",
        s.changes
    );
    // 显示值确实是 NULL:右键 → 复制单元格 得到 "NULL"
    right_click_cell(&mut h, 1, 1);
    {
        let node = h.get_by_label("复制单元格");
        node.simulate_click();
    }
    assert_eq!(drain_copies(&mut h, 6), vec!["NULL".to_string()]);

    // Tab 到右边一格;⌘V 把 2×2 的块贴到以它为左上角的位置
    key_once(&mut h, egui::Key::Tab, plain);
    assert_eq!(result_sel(&h), Some(((1, 2), (1, 2))));
    h.input_mut()
        .events
        .push(egui::Event::Paste("甲\t乙\n丙\t丁".to_string()));
    h.step();
    h.step();
    let s = q_snap(&h);
    for (r, col, v) in [
        (1, "role_name", "甲"),
        (1, "status", "乙"),
        (2, "role_name", "丙"),
        (2, "status", "丁"),
    ] {
        assert!(
            s.changes
                .contains(&(r, col.to_string(), Some(v.to_string()))),
            "({r}, {col}) = {v}:{:?}",
            s.changes
        );
    }
    assert_eq!(result_sel(&h), Some(((1, 2), (2, 3))), "贴完选中贴入的区域");
    assert_eq!(
        s.changes.len(),
        6,
        "贴入的 4 格都与原值不同:{:?}",
        s.changes
    );
    assert_eq!(s.editing, None);

    // 贴进与原值相同的值:不记改动(第 4 行的 status 原本就是 1)
    click_cell(&mut h, 3, 3);
    h.input_mut()
        .events
        .push(egui::Event::Paste("1".to_string()));
    h.step();
    h.step();
    assert_eq!(q_snap(&h).changes.len(), 6, "与原值相同的格不记改动");
}

/// 只读结果集(没有回写资格):打字/Delete/⌘V 都不产生改动,也不进编辑
#[test]
fn read_only_result_grid_ignores_edit_keys() {
    use egui_kittest::kittest::Queryable as _;
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let mut h = result_grid_harness(1100.0, 700.0, false);
    click_cell(&mut h, 0, 1);
    type_into(&mut h, "x");
    key_once(&mut h, egui::Key::Delete, plain);
    h.input_mut()
        .events
        .push(egui::Event::Paste("a\tb".to_string()));
    h.step();
    h.step();
    let s = q_snap(&h);
    assert!(
        s.changes.is_empty() && s.editing.is_none(),
        "只读结果集不应有改动"
    );
    // 右键菜单里没有"编辑此格"
    right_click_cell(&mut h, 0, 1);
    assert!(
        h.query_by_label("编辑此格").is_none(),
        "只读结果集的菜单不应有编辑项"
    );
}

/// 快速连点两个不同的格不算双击:egui 自带的双击判定只看间隔不看位置,
/// 照它的话"快点两格选一选"就会弹出"查看完整内容"
#[test]
fn result_grid_fast_clicks_on_different_cells_are_not_double_click() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use egui_kittest::kittest::Queryable as _;
    let mut h = result_grid_harness(1100.0, 700.0, false);

    click_cell(&mut h, 0, 1);
    click_cell(&mut h, 1, 2);
    click_cell(&mut h, 2, 3);
    assert_eq!(result_sel(&h), Some(((2, 3), (2, 3))), "每次点击都改选新格");
    // 弹窗的"关闭"按钮只在弹窗里出现
    assert!(
        h.query_by_label("关闭").is_none(),
        "连点三个不同的格不应弹窗"
    );

    // 拖选完马上点回起点也不算双击
    drag_cells(&mut h, (3, 1), (4, 2));
    click_cell(&mut h, 3, 1);
    assert_eq!(result_sel(&h), Some(((3, 1), (3, 1))));
    assert!(h.query_by_label("关闭").is_none(), "拖选后点回起点不应弹窗");

    // 可编辑结果集同理:连点两格不该进编辑
    let mut h = result_grid_harness(1100.0, 700.0, true);
    click_cell(&mut h, 0, 1);
    click_cell(&mut h, 1, 2);
    assert_eq!(q_snap(&h).editing, None, "连点两格不应进入编辑");
}

/// 双击要落在同一格上:egui 自带的双击判定只看间隔不看位置,照它的话"快速连点两格"
/// 的第二下就会直接进入编辑
#[test]
fn cell_double_click_needs_same_cell() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    // 先点 role 列第一行,紧接着点同列第二行
    click_grid(&mut h, egui::pos2(380.0, 182.0));
    assert_eq!(selection(&h).0, Some((0, 1)));
    click_grid(&mut h, egui::pos2(380.0, 212.0));
    assert_eq!(selection(&h).0, Some((1, 1)));
    assert_eq!(snap(&h).editing, None, "连点两个不同的格不应进入编辑");

    // 隔开双击间隔后,同一格连点两下:进入编辑
    h.run_steps(40);
    let at = egui::pos2(380.0, 182.0);
    click_grid(&mut h, at);
    click_grid(&mut h, at);
    assert_eq!(snap(&h).editing, Some((0, 1)), "同一格双击应进入编辑");
}

/// 横向滚轮:指针停在网格上,滚 `dx` 后移开
fn wheel_x(h: &mut ResultHarness<'_>, dx: f32) {
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(640.0, 620.0)));
    h.step();
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-dx, 0.0),
        modifiers: Default::default(),
    });
    h.step();
    h.step();
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(5.0, 5.0)));
    h.step();
    h.step();
}

/// 冻结列 + 横向滚动后,钉住的列盖在滚动区上面:点它选中的是钉住的那格(不是底下滚到
/// 同一位置的别的列),⌘C 复制它的值;点行号选整行;冻结区右侧仍是滚动区
#[test]
fn result_grid_frozen_overlay_keeps_selection_semantics() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let filler = "value-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let mut h = result_grid_harness(1280.0, 780.0, false);
    {
        let mut cell = h.state().borrow_mut();
        let q = cell.as_mut().unwrap();
        q.results[0] = shujing_core::ExecResult {
            columns: (0..6).map(|i| format!("col_{i}")).collect(),
            rows: (0..14)
                .map(|i| (0..6).map(|_| Some(format!("{filler}-{i}"))).collect())
                .collect(),
            affected: 0,
            truncated: false,
            elapsed_ms: 12,
            column_types: vec![],
        };
        q.q_frozen = 1;
    }
    h.run_steps(4);
    wheel_x(&mut h, 1000.0);
    // 滚轮后滚动还有一小段尾巴,等它停稳再量
    h.run_steps(30);
    assert!(result_hscroll(&h) > 0.5, "应已横向滚动,冻结覆盖层出现");

    let (ws, left, body_top) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    let row_y = |r: usize| body_top + crate::theme::row_h() * (r as f32 + 0.5);
    // 钉住的第 0 列第 2 行
    let pinned = egui::pos2(left + ws[0] + ws[1] / 2.0, row_y(1));
    click_at(&mut h, pinned);
    assert_eq!(result_sel(&h), Some(((1, 0), (1, 0))), "点到的是钉住的那格");
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), format!("{filler}-1"));

    // 钉住的行号:选中整行(6 列)
    let at = egui::pos2(left + crate::theme::ROWNO_W / 2.0, row_y(2));
    click_at(&mut h, at);
    assert_eq!(result_sel(&h), Some(((2, 0), (2, 5))));

    // 冻结区右侧:滚动区里的列(按点击当下的偏移反推是第几列)
    let off = result_hscroll(&h);
    let col = ((ws[1] + 40.0 + off) / ws[2]) as usize;
    assert!(col >= 1, "冻结区右侧不该再是第 0 列");
    let scrolled = egui::pos2(left + ws[0] + ws[1] + 40.0, row_y(3));
    click_at(&mut h, scrolled);
    assert_eq!(result_sel(&h), Some(((3, col), (3, col))));
    assert_eq!(key_once(&mut h, egui::Key::C, cmd()), format!("{filler}-3"));
}



// ---------------------------------------------------------------- 数据页:双击列缘自适应 + 列头拖动重排

fn data_geom(h: &GridHarness<'_>) -> (Vec<f32>, f32, f32) {
    let tab_id = {
        let app = h.state().borrow();
        app.tabs[2].id.clone()
    };
    let (ws, left, top) =
        table_view::debug_data_geom(&h.ctx, &tab_id).expect("数据页已渲染");
    (ws, left, top.expect("表头顶已知"))
}

/// 列缘中心 x(边界 b 左侧是可视列 b)
fn edge_x(h: &GridHarness<'_>, b: usize) -> f32 {
    let (ws, left, _) = data_geom(h);
    let mut x = left + if ws.first().is_some() { ws[0] } else { 0.0 };
    for k in 0..b {
        x += ws[1 + k];
    }
    x
}

/// 表头带中心 y
fn header_y(h: &GridHarness<'_>) -> f32 {
    let (_, _, top) = data_geom(h);
    top + crate::theme::data_header_h() / 2.0
}

/// 数据页单元格中心的逻辑坐标(按上一帧真实布局算)
fn data_cell(h: &GridHarness<'_>, r: usize, c: usize) -> egui::Pos2 {
    let (ws, left, top) = data_geom(h);
    let mut x = left + ws[0];
    for k in 0..c {
        x += ws[1 + k];
    }
    egui::pos2(x + ws[1 + c] / 2.0, top + crate::theme::data_header_h() + crate::theme::row_h() * (r as f32 + 0.5))
}

fn col_widths(h: &GridHarness<'_>) -> Vec<f32> {
    let app = h.state().borrow();
    let t = app.tabs[2].table_data().unwrap();
    t.base_widths.clone()
}

fn col_order(h: &GridHarness<'_>) -> Vec<String> {
    let app = h.state().borrow();
    let t = app.tabs[2].table_data().unwrap();
    t.col_order.clone()
}

/// 双击列缘:边界左侧那列(拖动改宽的也是它)按内容宽重画——先手动拖宽制造差异,
/// 再双击收回;别的列手拖的宽度不受影响。
/// 此前这条测试是空转通过的:拖宽的是左列、自适应的是右列,断言的那列前后根本没变
#[test]
fn data_grid_double_click_edge_fits_column_width() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    h.run_steps(4);
    let y = 300.0; // 表体中部(列缘手柄纵贯全表)
    // 拖 id|role 边界(边界 1)向右 60:边界左侧的 id 列被手动拖宽
    let before = data_geom(&h).0;
    let x0 = edge_x(&h, 1);
    drag_at(&mut h, egui::pos2(x0, y), egui::pos2(x0 + 60.0, y));
    let after_drag = data_geom(&h).0;
    assert_eq!(after_drag.len(), before.len(), "拖宽不改变列数");
    assert!(
        near(after_drag[1], before[1] + 60.0),
        "id 列应被拖宽 60: {before:?} → {after_drag:?}"
    );
    // 拖 role|role_name 边界(边界 2)向右 30:role 列拖宽
    let x2 = edge_x(&h, 2);
    drag_at(&mut h, egui::pos2(x2, y), egui::pos2(x2 + 30.0, y));
    let after_drag2 = data_geom(&h).0;
    assert!(
        near(after_drag2[2], after_drag[2] + 30.0),
        "role 列应被拖宽 30: {after_drag:?} → {after_drag2:?}"
    );

    // 双击 id|role 边界:id 列收回到内容宽
    h.run_steps(40); // 隔开与拖动的双击间隔
    let x1 = edge_x(&h, 1);
    click_grid(&mut h, egui::pos2(x1, y));
    click_grid(&mut h, egui::pos2(x1, y));
    h.run_steps(6);
    let (rows, columns) = {
        let app = h.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        (t.rows.clone(), t.columns.clone())
    };
    let fit = table_view::fit_width(&rows, &columns, 0);
    let ws = data_geom(&h).0;
    assert!(
        near(ws[1], fit),
        "双击后 id 列宽应回到内容宽 {fit}:实际 {}(拖宽后 {:?})",
        ws[1],
        after_drag
    );
    assert!(
        near(ws[2], after_drag2[2]),
        "自适应只动 id 列,role 列手拖的宽度保留: {after_drag2:?} → {ws:?}"
    );
}

/// 列缘是列的把手:在列缘上拖宽不该拉出矩形选区;双击列缘只做自适应,不进编辑
#[test]
fn data_grid_edge_is_a_column_handle_only() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    h.run_steps(4);
    let y = 300.0; // 表体中部(列缘手柄纵贯全表)
    let before = data_geom(&h).0;
    let x0 = edge_x(&h, 1);
    drag_at(&mut h, egui::pos2(x0, y), egui::pos2(x0 + 60.0, y));
    let after = data_geom(&h).0;
    assert!(
        near(after[1], before[1] + 60.0),
        "id 列宽应已改变: {before:?} → {after:?}"
    );
    assert_eq!(selection(&h).1, None, "拖宽列缘是列操作,不该拉出矩形选区");
    h.run_steps(40); // 隔开与拖动的双击间隔
    let x1 = edge_x(&h, 1);
    click_grid(&mut h, egui::pos2(x1, y));
    click_grid(&mut h, egui::pos2(x1, y));
    h.run_steps(3);
    assert_eq!(snap(&h).editing, None, "双击列缘只做自适应,不该进入编辑");
}

/// 列头拖动重排:role 列拖到最后,列序持久(col_order)、复制跟随新位置;
/// 拖动是列操作,不改变选区语义(落位后可视坐标失效,选区清空)
#[test]
fn data_grid_drag_header_reorders_columns() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let cmd = egui::Modifiers { command: true, ..Default::default() };
    let mut h = keyboard_grid_harness(8);
    h.run_steps(4);

    // 基线:role 列在第 2 位,复制得到 admin(该列第一行的值)
    let pos = data_cell(&h, 0, 1);
    click_grid(&mut h, pos);
    assert_eq!(selection(&h).0, Some((0, 1)));
    assert_eq!(key_once(&mut h, egui::Key::C, cmd), "admin");

    // 从 role 列头(可视位置 1)拖到边界 3 之后:role 落在可视位置 2
    let hy = header_y(&mut h);
    let ws_before = data_geom(&h).0;
    let src = edge_x(&mut h, 1) + 40.0; // role 列头中部
    let dst = edge_x(&mut h, 3) + 40.0;
    drag_at(&mut h, egui::pos2(src, hy), egui::pos2(dst, hy));

    let order = col_order(&h);
    assert!(!order.is_empty(), "拖动后应固化列顺序");
    // 列宽跟着列走:显示序 [id, role_name, role, …],不该整排跳回内容宽
    let ws_after = data_geom(&h).0;
    let want = [
        ws_before[0],
        ws_before[1],
        ws_before[3],
        ws_before[2],
        ws_before[4],
        ws_before[5],
    ];
    for (k, w) in want.into_iter().enumerate() {
        assert!(
            near(ws_after[k], w),
            "位置 {k} 的列宽应跟着列走: {ws_before:?} → {ws_after:?}(期望 {want:?})"
        );
    }
    assert_eq!(&order[0], "id", "其余列保持相对顺序: {order:?}");
    assert_eq!(&order[1], "role_name", "role 越过的列依次前移: {order:?}");
    assert_eq!(&order[2], "role", "role 落在可视位置 2: {order:?}");
    // 落位后可视坐标失效,选区清空
    assert_eq!(selection(&h), (None, None), "重排后选区清空");

    // 复制跟随新位置:role 现在是可视位置 2
    let pos = data_cell(&h, 0, 2);
    click_grid(&mut h, pos);
    assert_eq!(key_once(&mut h, egui::Key::C, cmd), "admin", "复制跟随重排后的列");

    // 无位移的列头点击仍走排序(拖拽状态原地清掉,不产生重排)
    let hy2 = header_y(&mut h);
    let before = col_order(&h);
    {
        let (ws, left, _) = data_geom(&h);
        let x = left + ws[0] + ws[1] + ws[2] / 2.0;
        click_at(&mut h, egui::pos2(x, hy2));
    }
    assert_eq!(col_order(&h), before, "原地点击列头不重排");
}


// ---------------------------------------------------------------- 结果网格:双击列缘自适应 + 列头拖动重排

/// 结果网格的边界 b 屏幕 x(边界 b 左侧是可视列 b)
fn rg_edge_x(h: &ResultHarness<'_>, b: usize) -> f32 {
    let (ws, left, _) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    let mut x = left + ws[0];
    for k in 0..b {
        x += ws[1 + k];
    }
    x
}

/// 结果网格表头底 y(表体首行顶)
fn hb_of(h: &ResultHarness<'_>) -> f32 {
    crate::grid::debug_geom(&h.ctx, RG_ID)
        .expect("网格已渲染")
        .2
}

/// 结果网格各数据列的实际宽度(去掉行号列与末尾占位列),按显示序
fn rg_widths(h: &ResultHarness<'_>) -> Vec<f32> {
    let (ws, ..) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    ws[1..ws.len().saturating_sub(1)].to_vec()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0
}

/// 双击列缘:先手动拖宽,双击收回内容宽;自适应只动这一列,别的列手拖的宽度保留
#[test]
fn result_grid_double_click_edge_fits_width() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = result_grid_harness(1100.0, 700.0, false);
    h.run_steps(4);
    let (ws, ..) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("网格已渲染");
    let y = hb_of(&h) + crate::theme::row_h() * 3.5; // 表体第 4 行中部
    // 拖 role|role_name 边界(边界 2)向右 60:边界左侧的 role 列被拖宽
    // (egui_extras 的把手属于左列;此前测试按"右列变宽"写,拖完断言的那列根本没动)
    let x_edge = rg_edge_x(&h, 2);
    drag_at(&mut h, egui::pos2(x_edge, y), egui::pos2(x_edge + 60.0, y));
    let ws2 = rg_widths(&h);
    assert!(
        near(ws2[1], ws[2] + 60.0),
        "role 列应被拖宽 60: {:?} → {:?}",
        ws,
        ws2
    );
    // 拖 id|role 边界(边界 1)向左 30:id 列收窄
    let x0b = rg_edge_x(&h, 1);
    drag_at(&mut h, egui::pos2(x0b, y), egui::pos2(x0b - 30.0, y));
    let ws2b = rg_widths(&h);
    assert!(
        near(ws2b[0], ws2[0] - 30.0) && near(ws2b[1], ws2[1]),
        "id 收窄、role 不变: {:?} → {:?}",
        ws2,
        ws2b
    );

    // 双击 role|role_name 边界:role 列收回到内容宽
    h.run_steps(40);
    let x1 = rg_edge_x(&h, 2);
    click_at(&mut h, egui::pos2(x1, y));
    click_at(&mut h, egui::pos2(x1, y));
    h.run_steps(6);
    let (rows, columns, types) = {
        let cell = h.state().borrow();
        let q = cell.as_ref().unwrap();
        let r = &q.results[q.active_set];
        (r.rows.clone(), r.columns.clone(), r.column_types.clone())
    };
    let fit = crate::grid::fit_width(&rows, &columns, &types, 1);
    let ws3 = rg_widths(&h);
    assert!(
        near(ws3[1], fit),
        "双击后 role 列应回到内容宽 {fit}:实际 {}(拖宽后 {:?})",
        ws3[1],
        ws2
    );
    assert!(
        near(ws3[0], ws2b[0]) && near(ws3[2], ws2b[2]) && near(ws3[3], ws2b[3]),
        "自适应只动 role 列,其余列宽原样(id 手动收窄的 {:?} 不该被重置): {:?} → {:?}",
        ws2b[0],
        ws2b,
        ws3
    );
}

/// 列缘是列的把手:在列缘上拖宽不该拉出单元格选区;双击列缘只做自适应——
/// 不该同时被当成"双击单元格"(可编辑网格进编辑、只读网格弹"完整内容")
#[test]
fn result_grid_edge_is_a_column_handle_only() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 只读:拖宽 + 双击
    let mut h = result_grid_harness(1100.0, 700.0, false);
    h.run_steps(4);
    let (ws, ..) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("已渲染");
    let y = hb_of(&h) + crate::theme::row_h() * 3.5;
    let x_edge = rg_edge_x(&h, 2);
    drag_at(&mut h, egui::pos2(x_edge, y), egui::pos2(x_edge + 60.0, y));
    assert!(near(rg_widths(&h)[1], ws[2] + 60.0), "列宽应已改变");
    assert_eq!(result_sel(&h), None, "拖宽列缘是列操作,不该拉出单元格选区");
    h.run_steps(40);
    let x_edge = rg_edge_x(&h, 2);
    click_at(&mut h, egui::pos2(x_edge, y));
    click_at(&mut h, egui::pos2(x_edge, y));
    h.run_steps(3);
    assert!(
        !crate::grid::debug_view_open(&h.ctx, RG_ID),
        "双击列缘只做自适应,不该弹出完整内容"
    );
    // 可编辑:双击列缘不进编辑
    let mut h = result_grid_harness(1100.0, 700.0, true);
    h.run_steps(4);
    let x_edge = rg_edge_x(&h, 2);
    let y = hb_of(&h) + crate::theme::row_h() * 3.5;
    click_at(&mut h, egui::pos2(x_edge, y));
    click_at(&mut h, egui::pos2(x_edge, y));
    h.run_steps(3);
    assert_eq!(q_snap(&h).editing, None, "双击列缘只做自适应,不该进入编辑");
}

/// 列头拖动重排:role 从可视位置 1 拖到 3 之后;落位后选区清空、列宽跟着列走;
/// 在新位置编辑该列 → 待保存改动记的是自然列名 role(回写 SQL 落对列)
#[test]
fn result_grid_drag_header_reorders_and_edits_right_column() {
    use egui_kittest::kittest::Queryable as _;
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let plain = egui::Modifiers::NONE;
    let mut h = result_grid_harness(1100.0, 700.0, true);
    h.run_steps(4);

    // 先把 role 列手动收窄 60:重排后这份宽度必须跟着列走
    let (ws, left, hb) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("已渲染");
    let y = hb + crate::theme::row_h() * 3.5;
    let x_edge = rg_edge_x(&mut h, 2);
    drag_at(&mut h, egui::pos2(x_edge, y), egui::pos2(x_edge - 60.0, y));
    let before = rg_widths(&h);
    assert!(before[1] < ws[2] - 50.0, "role 应已收窄: {:?} → {:?}", ws, before);
    let _ = left;

    // role 列头(可视位置 1)中部拖到边界 3 之后
    let hy = hb - crate::theme::data_header_h() / 2.0;
    let src = rg_edge_x(&mut h, 1) + 40.0;
    let dst = rg_edge_x(&mut h, 3) + 40.0;
    drag_at(&mut h, egui::pos2(src, hy), egui::pos2(dst, hy));

    let order = crate::grid::debug_cols(&h.ctx, RG_ID).order;
    assert_eq!(
        order,
        vec![
            "id".to_string(),
            "role_name".to_string(),
            "role".to_string(),
            "status".to_string(),
            "created".to_string()
        ],
        "role 应落在可视位置 2"
    );
    assert_eq!(result_sel(&h), None, "重排后可视坐标失效,选区清空");
    // 列宽跟着列走:显示序 [id, role_name, role, status, created]
    let after = rg_widths(&h);
    let want = [before[0], before[2], before[1], before[3], before[4]];
    for k in 0..want.len() {
        assert!(
            near(after[k], want[k]),
            "位置 {k} 的列宽应跟着列走: {:?} → {:?}(期望 {:?})",
            before,
            after,
            want
        );
    }

    // 新位置 (0,2) = role 列第一行:回车编辑加 !,提交后改动记自然列名 role
    click_cell(&mut h, 0, 2);
    assert_eq!(result_sel(&h), Some(((0, 2), (0, 2))), "点击应选中新位置的格");
    key_once(&mut h, egui::Key::Enter, plain);
    assert_eq!(q_snap(&h).editing, Some((0, 2)));
    type_into(&mut h, "!");
    key_once(&mut h, egui::Key::Enter, plain);
    let s = q_snap(&h);
    assert_eq!(s.editing, None);
    assert!(
        s.changes.contains(&(0, "role".to_string(), Some("role_0!".to_string()))),
        "改动应记自然列名 role: {:?}",
        s.changes
    );
    // 右键菜单也在:重排后复制跟随新位置
    let pos = cell_pos(&h, 0, 2);
    right_click_at(&mut h, pos);
    assert!(h.query_by_label("复制单元格").is_some(), "重排后右键菜单应打开");
    {
        let node = h.get_by_label("复制单元格");
        node.simulate_click();
    }
    assert_eq!(drain_copies(&mut h, 6), vec!["role_0!".to_string()]);
}

/// 结果网格连续两次拖动:第二次重排建立在第一次的结果上,列序始终是"全部列的一个排列"
#[test]
fn result_grid_second_drag_builds_on_first() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = result_grid_harness(1100.0, 700.0, false);
    h.run_steps(4);

    let drag_header = |h: &mut ResultHarness<'_>, from: usize, to: usize| {
        let hy = {
            let (_, _, hb) = crate::grid::debug_geom(&h.ctx, RG_ID).expect("已渲染");
            hb - crate::theme::data_header_h() / 2.0
        };
        let src = rg_edge_x(h, from) + 40.0;
        let dst = rg_edge_x(h, to) + 40.0;
        drag_at(h, egui::pos2(src, hy), egui::pos2(dst, hy));
    };
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // 第一次:role(可视位置 1)拖到边界 3 之后
    drag_header(&mut h, 1, 3);
    assert_eq!(
        crate::grid::debug_cols(&h.ctx, RG_ID).order,
        names(&["id", "role_name", "role", "status", "created"])
    );
    // 第二次:此刻可视位置 0 的列(id)拖到边界 3 之后
    drag_header(&mut h, 0, 3);
    assert_eq!(
        crate::grid::debug_cols(&h.ctx, RG_ID).order,
        names(&["role_name", "role", "id", "status", "created"]),
        "第二次重排必须建立在第一次的结果上"
    );
    // 第三次:往回拖——位置 4 的列(created)拖到首位
    drag_header(&mut h, 4, 0);
    assert_eq!(
        crate::grid::debug_cols(&h.ctx, RG_ID).order,
        names(&["created", "role_name", "role", "id", "status"])
    );
    // 复制跟随最终位置:可视位置 2 现在是 role 列
    click_cell(&mut h, 0, 2);
    let cmd = egui::Modifiers { command: true, ..Default::default() };
    assert_eq!(key_once(&mut h, egui::Key::C, cmd), "role_0");
}

/// 连续两次拖动:第二次重排必须建立在第一次的结果上,列序始终是"全部列的一个排列"。
/// 落位逻辑若把全序槽位当成列下标用,第一次(col_order 还是自然序)会侥幸正确,
/// 第二次起就错乱/重复——这里专门盯第二次
#[test]
fn data_grid_second_drag_builds_on_first() {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = keyboard_grid_harness(8);
    h.run_steps(4);
    let all: Vec<String> = {
        let app = h.state().borrow();
        let t = app.tabs[2].table_data().unwrap();
        t.columns.clone()
    };
    let is_permutation = |o: &[String]| {
        let mut a = o.to_vec();
        let mut b = all.clone();
        a.sort();
        b.sort();
        a == b
    };

    // 第一次:role(可视位置 1)拖到边界 3 之后
    let (from, to) = (
        egui::pos2(edge_x(&h, 1) + 40.0, header_y(&h)),
        egui::pos2(edge_x(&h, 3) + 40.0, header_y(&h)),
    );
    drag_at(&mut h, from, to);
    let first = col_order(&h);
    assert!(is_permutation(&first), "第一次后应是全排列: {first:?}");
    assert_eq!(&first[1], "role_name");
    assert_eq!(&first[2], "role");

    // 第二次:此刻可视位置 0 的列(id)拖到边界 3 之后
    let (from, to) = (
        egui::pos2(edge_x(&h, 0) + 40.0, header_y(&h)),
        egui::pos2(edge_x(&h, 3) + 40.0, header_y(&h)),
    );
    drag_at(&mut h, from, to);
    let second = col_order(&h);
    assert!(is_permutation(&second), "第二次后列序必须仍是全排列(不重复不丢列): {second:?}");
    // 第一次后可视序 [id, role_name, role, …];把位置 0 的 id 拖过两个列 → [role_name, role, id, …]
    assert_eq!(&second[0], "role_name", "role_name 前移到首位: {second:?}");
    assert_eq!(&second[1], "role", "{second:?}");
    assert_eq!(&second[2], "id", "id 落在位置 2: {second:?}");
}
