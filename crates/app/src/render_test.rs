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
        tables_by_db: HashMap::from([
            ("dramato".to_string(), tables),
            ("archive".to_string(), archive),
            ("comic".to_string(), comic),
        ]),
        loading: false,
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
        auto_connect_done: true,
        quick_open: None,
        stats: None,
        import: None,
        new_table: None,
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
            let _ = table_view::draw_table_view(ctx, &tab_id, t);
        }
        TabKind::Structure(s) => {
            let _ = structure::draw_structure_view(ctx, &tab_id, s);
        }
        TabKind::Query(q) => {
            let conns = app.conns.clone();
            let _ = query::draw_query_view(&conns, ctx, &tab_id, q);
        }
        TabKind::Designer(d) => {
            crate::designer::stash_dialect(ctx, shujing_core::DbType::MySql);
            let _ = crate::designer::draw_designer_view(ctx, &tab_id, d);
        }
        TabKind::Redis(rt) => {
            let _ = crate::redis::draw_redis_view(ctx, &tab_id, rt, false);
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
    render("data_table", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _actions = table_view::draw_table_view(ctx, "t1", tab);
    });
}

#[test]
fn render_row_detail() {
    let mut tab = role_tab();
    tab.detail = true;
    let cell = std::cell::RefCell::new(Some(tab));
    render("row_detail", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _ = table_view::draw_table_view(ctx, "t1", tab);
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
        sql: "SELECT id, role, role_name, status\nFROM backend_role_privilege\nWHERE status = 1\nORDER BY created DESC\nLIMIT 50;".into(),
        history: vec!["SELECT * FROM backend_role_privilege LIMIT 10;".into()],
        results: vec![],
        active_set: 0,
        running: false,
        error: None,
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
    render("query", 1280.0, 780.0, true, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(&conns, ctx, "q1", q);
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
    app.conns.push(shujing_core::ConnInfo {
        id: "c1".into(),
        name: "dramato-dev".into(),
        db_type: shujing_core::DbType::MySql,
        host: Some("127.0.0.1".into()),
        port: Some(3306),
        user: Some("root".into()),
        password: None,
        database: None,
        file_path: None,
        color: None,
        read_only: false,
    });
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
