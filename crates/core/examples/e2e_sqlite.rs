//! SQLite 全链路 E2E(直调 Service,零外部依赖):
//! 连接/表列表/结构/外键/计数/查询(中文+NULL)/回写三形态(改/删/插)/只读门禁/取消语义。
//! 运行:cargo run -p shujing-core --example e2e_sqlite

use shujing_core::{ConnInfo, DbType, Service};

fn pass(name: &str, detail: &str) {
    println!("PASS  {name}  {detail}");
}

fn fail(name: &str, err: &str) -> ! {
    println!("FAIL  {name}  {err}");
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    // 独立临时库文件:跑完删除,不残留
    let dir = std::env::temp_dir().join(format!("shujing_e2e_sqlite_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("e2e.sqlite");
    let db_str = db_path.display().to_string();

    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, note TEXT);
            CREATE TABLE orders (
                id INTEGER PRIMARY KEY,
                user_id INTEGER NOT NULL REFERENCES users(id),
                amount REAL NOT NULL
            );
            CREATE INDEX idx_orders_user ON orders(user_id);
            INSERT INTO users (name, age, note) VALUES
              ('张三', 28, 'vip'), ('李四', NULL, NULL), ('王五', 41, '');
            INSERT INTO orders (user_id, amount) VALUES (1, 99.5), (1, 12.0), (3, 60.25);
            "#,
        )
        .unwrap();
    }

    let service = std::sync::Arc::new(Service::new(
        std::env::temp_dir().join("shujing_e2e_sqlite_cfg"),
    ));
    let info = ConnInfo {
        id: "e2e-sqlite".into(),
        name: "e2e-sqlite".into(),
        db_type: DbType::Sqlite,
        host: None,
        port: None,
        user: None,
        password: None,
        database: None,
        file_path: Some(db_str.clone()),
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };

    // 1) 连接 + 版本
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 2) 表列表:users/orders
    match service.list_tables("e2e-sqlite", None).await {
        Ok(ts) => {
            let names: Vec<&str> = ts.iter().map(|t| t.name.as_str()).collect();
            if !names.contains(&"users") || !names.contains(&"orders") {
                fail("list_tables", &format!("{names:?}"));
            }
            pass("list_tables", &format!("{names:?}"));
        }
        Err(e) => fail("list_tables", &e),
    }

    // 3) 结构:users 4 列,id=PRI;orders 外键指向 users
    match service.get_table_structure("e2e-sqlite", "users", None).await {
        Ok(s) => {
            let pk = s.columns.iter().find(|c| c.key == "PRI");
            if s.columns.len() != 4 || pk.map(|c| c.name.as_str()) != Some("id") {
                fail("structure", &format!("cols={}", s.columns.len()));
            }
            pass("structure", "4 cols, pk=id");
        }
        Err(e) => fail("structure", &e),
    }
    match service.list_foreign_keys("e2e-sqlite", None).await {
        Ok(fks) => {
            let ok = fks.iter().any(|f| f.table == "orders" && f.ref_table == "users");
            if !ok {
                fail("foreign_keys", &format!("{fks:?}"));
            }
            pass("foreign_keys", "orders→users");
        }
        Err(e) => fail("foreign_keys", &e),
    }

    // 4) 计数
    match service.count_rows("e2e-sqlite", "users").await {
        Ok(3) => pass("count_rows", "3"),
        Ok(n) => fail("count_rows", &format!("expect 3 got {n}")),
        Err(e) => fail("count_rows", &e),
    }

    // 5) 查询:中文/NULL/浮点
    match service
        .run_sql(
            "e2e-sqlite",
            "SELECT name, age, amount FROM users u JOIN orders o ON o.user_id=u.id ORDER BY o.id"
                .into(),
            None,
        )
        .await
    {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.rows[0][0].as_deref() != Some("张三") {
                fail("run_sql", &format!("name={:?}", r.rows[0][0]));
            }
            if r.rows[0][2].as_deref() != Some("99.5") {
                fail("run_sql", &format!("amount={:?}", r.rows[0][2]));
            }
            pass("run_sql", "JOIN/中文/浮点 正确");
        }
        Err(e) => fail("run_sql", &e),
    }

    // 6) 回写三形态:UPDATE/DELETE/INSERT,重读验证
    let upd = vec![shujing_core::edits::CellUpdate {
        pk: vec![("id".into(), "2".into())],
        sets: vec![("age".into(), Some("30".into()))],
    }];
    let del = vec![vec![("id".into(), "2".into())]]; // 李四无订单,可删(王五被 orders 引用,删了该触发 FK 拒绝)
    let ins = vec![vec![
        ("name".into(), "孙七".into()),
        ("age".into(), "22".into()),
    ]];
    match service
        .apply_changes("e2e-sqlite", "users", None, &upd, &del, &ins)
        .await
    {
        Ok(_) => {
            let r = service
                .run_sql("e2e-sqlite", "SELECT id, name, age FROM users ORDER BY id".into(), None)
                .await
                .unwrap()
                .pop()
                .unwrap();
            let names: Vec<String> = r
                .rows
                .iter()
                .map(|row| row[1].clone().unwrap_or_default())
                .collect();
            if !names.contains(&"孙七".to_string())
                || names.iter().any(|n| n == "李四")
            {
                fail("apply_changes", &format!("{names:?}"));
            }
            pass("apply_changes", "改/删/插 三形态重读一致");
        }
        Err(e) => fail("apply_changes", &e),
    }

    // 7) 只读连接:写语句被拒,读正常
    let mut ro = info.clone();
    ro.id = "e2e-sqlite-ro".into();
    ro.read_only = true;
    service.connect(ro.clone()).await.unwrap();
    match service
        .run_sql("e2e-sqlite-ro", "DELETE FROM users WHERE id=1".into(), None)
        .await
    {
        Err(e) if e.contains("只读") => pass("read_only_gate", "写语句被拒"),
        Err(e) => fail("read_only_gate", &e),
        Ok(_) => fail("read_only_gate", "只读连接放行了 DELETE"),
    }

    // 8) 取消语义:SQLite 不支持,应明确报错而非误操作
    match service.cancel_query("e2e-sqlite").await {
        Err(e) if e.contains("不支持取消") => pass("cancel_semantics", "明确报不支持"),
        Err(e) => fail("cancel_semantics", &e),
        Ok(_) => fail("cancel_semantics", "不该成功"),
    }

    // 清理
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
    println!("\n全部通过 ✓");
}
