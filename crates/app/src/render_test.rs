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
static RENDER_LOCK: Mutex<()> = Mutex::new(());

fn render(name: &str, w: f32, h: f32, dark: bool, mut draw: impl FnMut(&egui::Context) + 'static) {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    })
    .collect();
    let mut tables = tables;
    tables.push(shujing_core::TableMeta {
        name: "v_user_summary".into(),
        kind: "view".into(),
    });
    tables.push(shujing_core::TableMeta {
        name: "trg_touch_user".into(),
        kind: "trigger".into(),
    });
    tables.push(shujing_core::TableMeta {
        name: "fn_heat_score".into(),
        kind: "function".into(),
    });
    let archive: Vec<shujing_core::TableMeta> = (0..7)
        .map(|i| shujing_core::TableMeta {
            name: format!("archive_table_{i}"),
            kind: "table".into(),
        })
        .collect();
    let comic: Vec<shujing_core::TableMeta> = (0..4)
        .map(|i| shujing_core::TableMeta {
            name: format!("comic_table_{i}"),
            kind: "table".into(),
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
        confirm: None,
        snippets: vec![],
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
            let _ = structure::draw_structure_view(ctx, &tab_id, s);
        }
        TabKind::Query(q) => {
            let conns = app.conns.clone();
            let _ = query::draw_query_view(&conns, ctx, &tab_id, q, &[], &[], false, &[], &Default::default(), None);
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
        let _ = structure::draw_structure_view(ctx, "s1", s);
    });
}

#[test]
fn render_query_view() {
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
        q_editing: None,
        q_draft: String::new(),
        ac: None,
        q_frozen: 0,
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
    }];
    let cell = std::cell::RefCell::new(Some(q));
    let dbs = vec!["dramato".to_string(), "shop".to_string(), "stats".to_string()];
    render("query", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(&conns, ctx, "q1", q, &[], &dbs, true, &[], &Default::default(), None);
    });
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
        q_editing: None,
        q_draft: String::new(),
        ac: None,
        q_frozen: 0,
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
    }];
    let cell = std::cell::RefCell::new(Some(q));
    let dbs = vec!["dramato".to_string(), "shop".to_string()];
    render("query_narrow_db", 1024.0, 700.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(&conns, ctx, "q1", q, &[], &dbs, true, &[], &Default::default(), None);
    });
}

#[test]
fn render_conn_modal() {
    let mut m = crate::app::ConnModal::new();
    m.name = "dramato-prod".into();
    m.host = "10.0.0.8".into();
    m.port = "3306".into();
    m.user = "readonly".into();
    m.password = "secret".into();
    m.database = "dramato".into();
    m.test_result = Some(Ok("连接成功: MySQL 8.0.36".into()));
    let cell = std::cell::RefCell::new(Some(m));
    let app = std::cell::RefCell::new(full_app("role"));
    render("conn_modal", 1000.0, 720.0, true, move |ctx| {
        let mut b = app.borrow_mut();
        draw_app(&mut b, ctx);
        let mut mb = cell.borrow_mut();
        let _ = table_view::draw_conn_modal(ctx, mb.as_mut().unwrap());
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
    use crate::table_view::badge_value;
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
        q_editing: None,
        q_draft: String::new(),
        ac: None,
        q_frozen: 0,
    };
    q.sql = "SELECT id FROM backend_role_privilege".into();
    app.tabs.push(Tab {
        id: "q1".into(),
        title: "查询 1".into(),
        kind: TabKind::Query(q),
    });
    app.tabs.push(stub_tab("role", "backend_role_privilege"));
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
    let top_len = (crate::theme::TABBAR_H * 2.0) as usize * width * 4;
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
        let x_before_drag =
            table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
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
        let x_after_drag =
            table_view::LAST_HSCROLL_X.load(std::sync::atomic::Ordering::Relaxed);
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
    // 从截图核对的实际逻辑坐标:操作符下拉 ≈ (440, 149)
    click(&mut harness, egui::pos2(440.0, 149.0));
    harness.step();
    let out = harness.output().clone();
    let popup_rect = egui::Rect::from_min_size(egui::pos2(399.0, 170.0), egui::vec2(96.0, 250.0));
    let popup_shapes = out
        .shapes
        .iter()
        .filter(|cs| cs.clip_rect.intersects(popup_rect))
        .count();
    eprintln!("[popup-shapes] {popup_shapes}");
    // 弹层选项第一项(= )在按钮下方 ~30px;第二项(!=)≈ +28
    click(&mut harness, egui::pos2(440.0, 205.0));
    let b = harness.state();
    let conds_op = b.borrow().tabs[2]
        .table_data()
        .map(|t| t.conds[0].1.clone())
        .unwrap_or_default();
    assert_eq!(conds_op, "!=", "操作符下拉应可选,实际为 {conds_op:?}");
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
        q_editing: None,
        q_draft: String::new(),
        ac: Some(crate::app::AcState {
            word: "us".into(),
            selected: 0,
            forced: false,
        }),
        q_frozen: 0,
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
        vec!["id".to_string(), "user_name".to_string(), "status".to_string()],
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
    let crop_frozen_region = |h: &mut egui_kittest::Harness<std::cell::RefCell<ShujingApp>>| -> Vec<u8> {
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
    img.save("/tmp/shujing_ui/frozen_edit.png").expect("保存 PNG 失败");
}

#[test]
fn query_grid_frozen_columns_pin() {
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
        q_editing: None,
        q_draft: String::new(),
        ac: None,
        q_frozen: 1,
    };
    q.results = vec![shujing_core::ExecResult {
        columns: (0..6).map(|i| format!("col_{i}")).collect(),
        rows: (0..14)
            .map(|i| {
                (0..6)
                    .map(|_| Some(format!("{filler}-{i}")))
                    .collect()
            })
            .collect(),
        affected: 0,
        truncated: false,
        elapsed_ms: 12,
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
                let _ = query::draw_query_view(&conns, ctx, "q1", q, &[], &[], false, &[], &Default::default(), None);
            },
            cell,
        );
    harness.run_steps(6);

    let crop_frozen = |h: &mut egui_kittest::Harness<std::cell::RefCell<Option<QueryTab>>>| -> Vec<u8> {
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
    img.save("/tmp/shujing_ui/query_frozen.png").expect("保存 PNG 失败");
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
            h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
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
        assert!(
            !t.changes.is_empty() == false,
            "拖选本身不应产生改动"
        );
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
    img.save("/tmp/shujing_ui/range_fill.png").expect("保存 PNG 失败");
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
        assert_eq!(t.changes.len(), 40, "Delete 全选置 NULL 应暂存 40 处非主键改动");
        assert!(t.changes.iter().all(|(_, cn, _)| cn != "id"));
    }
}
