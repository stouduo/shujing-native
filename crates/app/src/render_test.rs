//! 无头渲染自检:离屏渲染各视图存 PNG 到 /tmp/shujing_ui/,用于像素级自验收。
//! 运行:cargo test -p shujing-app --渲染即生成,不比对快照(纯生成供人工/代理查看)。

use crate::app::{ConnTree, QueryTab, ShujingApp, StructureTab, Tab, TabKind, TableTab};
use crate::query;
use crate::structure;
use crate::table_view;
use egui_kittest::Harness;
use std::collections::HashMap;
use std::sync::Arc;

fn render(name: &str, w: f32, h: f32, mut draw: impl FnMut(&egui::Context) + 'static) {
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
                crate::theme::apply(ctx);
            }
            draw(ctx);
        });
    harness.run_steps(3);
    let img = harness.render().expect("render 失败");
    img.save(out_dir.join(format!("{name}.png"))).expect("保存 PNG 失败");
}

fn fake_rows(n: usize) -> Vec<Vec<Option<String>>> {
    (0..n)
        .map(|i| {
            vec![
                Some((i + 1).to_string()),
                Some(format!("role_{i}")),
                if i % 4 == 0 { None } else { Some(format!("角色 {i}")) },
                Some(
                    "/admin/backend/menu_v2,/admin/backend/user_role_list,/admin/resource_assess,/admin/series_resource/search,/admin/episode_series/detail"
                        .to_string(),
                ),
                Some(if i % 3 == 0 { "1" } else { "2" }.to_string()),
                Some("1731383330".to_string()),
            ]
        })
        .collect()
}

fn fake_table_tab() -> TableTab {
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
        "created".into(),
        "updated".into(),
        "creator".into(),
        "department".into(),
        "level".into(),
        "remark".into(),
        "ext_config".into(),
    ];
    tab.rows = (0..24)
        .map(|i| {
            vec![
                Some((i + 1).to_string()),
                Some(format!("role_{i}")),
                if i % 4 == 0 { None } else { Some(format!("内容审核员 {i} 组")) },
                Some("/admin/backend/menu_v2,/admin/backend/user_role_list,/admin/resource_assess,/admin/series_resource/search,/admin/episode_series/detail".into()),
                Some(if i % 3 == 0 { "1" } else { "2" }.to_string()),
                Some("1731383330".into()),
                Some("1731470000".into()),
                if i % 5 == 0 { None } else { Some(format!("zhangwei{i}@corp.io")) },
                Some("内容运营部".into()),
                Some(format!("L{}", i % 6)),
                None,
                Some(r#"{"auto_review":true,"threshold":0.85,"tags":["首推","热搜"]}"#.into()),
            ]
        })
        .collect();
    tab.pk_cols = vec!["id".into()];
    tab.total = Some(312);
    tab.page = 1;
    tab.page_size = 100;
    tab.sort = Some(("created".into(), false));
    tab.changes.push((2, "role_name".into(), Some("已修改的名字".into())));
    tab.selected = Some((1, 2));
    tab.filter = "admin".into();
    tab.active_filter = Some("admin".into());
    tab.last_elapsed_ms = Some(37);
    tab
}

#[test]
fn render_data_table() {
    let cell = std::cell::RefCell::new(Some(fake_table_tab()));
    render("data_table", 1280.0, 780.0, move |ctx| {
        let mut b = cell.borrow_mut();
        let tab = b.as_mut().unwrap();
        let _actions = table_view::draw_table_view(ctx, "t1", tab);
    });
}

#[test]
fn render_sidebar_tabs_empty_state() {
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
        name: "本地 SQLite".into(),
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
        "abnormal_account",
        "abtest_strategy",
        "access_deny_info",
        "active_event_button",
        "activity_card",
        "actor_heat_score",
        "actor_info",
        "ad_task",
        "ads",
        "ads_drama_info",
        "backend_role_privilege",
        "campaign_report",
    ]
    .iter()
    .map(|n| shujing_core::TableMeta { name: n.to_string(), kind: "table".into() })
    .collect();
    let tree = ConnTree {
        online: true,
        version: "8.0.36".into(),
        databases: vec![
            "dramato".into(),
            "archive".into(),
            "comic".into(),
            "drama_book_stat".into(),
        ],
        expanded_db: Some("dramato".into()),
        tables,
        loading: false,
    };
    let mut qtab = QueryTab {
        conn_id: "c1".into(),
        sql: "SELECT * FROM users WHERE status = 1 LIMIT 50".into(),
        results: vec![],
        active_set: 0,
        running: false,
        error: None,
    };
    let _ = &mut qtab;
    let app = ShujingApp {
        service: Arc::new(shujing_core::Service::new(&dir)),
        conns: vec![conn, conn2],
        trees: HashMap::from([("c1".to_string(), tree)]),
        tabs: vec![Tab {
            id: "q1".into(),
            title: "查询 1".into(),
            kind: TabKind::Query(qtab),
        }],
        active_tab: 0,
        events: rx,
        event_tx: tx,
        conn_modal: None,
        error_toast: None,
        autoopen_done: true,
        query_seq: 1,
        table_filter: String::new(),
    };
    let cell = std::cell::RefCell::new(Some(app));
    render("sidebar_tabs", 1280.0, 780.0, move |ctx| {
        let mut b = cell.borrow_mut();
        let app = b.as_mut().unwrap();
        app.draw_sidebar_panel(ctx);
        table_view::draw_tabs(app, ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("点击左侧连接,展开库/表后点击表名开始").color(crate::theme::MUTED));
            });
        });
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
    render("structure", 1280.0, 780.0, move |ctx| {
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
                if i % 4 == 0 { None } else { Some(format!("角色 {i}")) },
                Some(if i % 3 == 0 { "1" } else { "2" }.to_string()),
                Some("1731383330".to_string()),
            ]
        })
        .collect();
    q.results = vec![shujing_core::ExecResult {
        columns: vec!["id".into(), "role".into(), "role_name".into(), "status".into(), "created".into()],
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
    render("query", 1280.0, 780.0, move |ctx| {
        let mut b = cell.borrow_mut();
        let q = b.as_mut().unwrap();
        let _ = query::draw_query_view(&conns, ctx, "q1", q);
    });
}
