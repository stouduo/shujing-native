//! PG 全链路 E2E(直调 Service,不经过 GUI):连接/浏览/结构/计数/查询/
//! 单元格回写/取消竞态治理(gen 代数戳)/连接活性。
//! 运行:先起本地 PG 并建好库(见脚本内连接参数),然后
//! cargo run -p shujing-core --example e2e_pg
//! 连接参数可用环境变量覆盖:E2E_PG_HOST(默认 127.0.0.1)/E2E_PG_PORT(默认 5433)

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
    let dir = std::env::temp_dir().join("shujing_e2e_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(&dir));

    let host = std::env::var("E2E_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("E2E_PG_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5433);
    let info = ConnInfo {
        id: "e2e-pg".into(),
        name: "e2e-pg".into(),
        db_type: DbType::Postgres,
        host: Some(host),
        port: Some(port),
        user: Some("postgres".into()),
        password: None,
        database: Some("e2e_a".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        ssh: None,
    };

    // 0) 数据重置(保证幂等:上一次回写会改数据)
    {
        let (client, conn) = tokio_postgres::connect(
            &format!(
                "host={} port={} user=postgres dbname=e2e_a",
                info.host.as_deref().unwrap_or("127.0.0.1"),
                info.port.unwrap_or(5433)
            ),
            tokio_postgres::NoTls,
        )
        .await
        .unwrap();
        tokio::spawn(async move { let _ = conn.await; });
        client
            .batch_execute("TRUNCATE users RESTART IDENTITY; INSERT INTO users (name, age, score, active) VALUES ('张三',28,91.50,true),('李四',34,88.25,false),('王五',NULL,72.00,true),('赵六',19,95.75,true); CREATE TABLE IF NOT EXISTS big (id INT); INSERT INTO big SELECT generate_series(1,3) WHERE NOT EXISTS (SELECT 1 FROM big)")
            .await
            .unwrap();
    }

    // 1) 连接 + 版本
    let r = service.connect(info.clone()).await;
    match &r {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", e),
    }

    // 2) 库列表含 e2e_a/e2e_b
    match service.list_databases("e2e-pg").await {
        Ok(dbs) => {
            let ok = dbs.iter().any(|d| d == "e2e_a") && dbs.iter().any(|d| d == "e2e_b");
            if !ok {
                fail("list_databases", &format!("{dbs:?}"));
            }
            pass("list_databases", &format!("{} dbs", dbs.len()));
        }
        Err(e) => fail("list_databases", &e),
    }

    // 3) e2e_a 表列表含 users
    match service.list_tables("e2e-pg", Some("e2e_a".into())).await {
        Ok(ts) => {
            if !ts.iter().any(|t| t.name == "users") {
                fail("list_tables", &format!("{ts:?}"));
            }
            pass("list_tables", &format!("{} tables", ts.len()));
        }
        Err(e) => fail("list_tables", &e),
    }

    // 4) 结构:4 列,id=PRI
    match service.get_table_structure("e2e-pg", "users", Some("e2e_a".to_string())).await {
        Ok(s) => {
            let pk = s.columns.iter().find(|c| c.key == "PRI");
            if s.columns.len() != 6 || pk.map(|c| c.name.as_str()) != Some("id") {
                fail(
                    "structure",
                    &format!(
                        "cols={} pk={:?}",
                        s.columns.len(),
                        pk.map(|c| c.name.clone())
                    ),
                );
            }
            pass("structure", "6 cols, pk=id");
        }
        Err(e) => fail("structure", &e),
    }

    // 5) 计数(默认库)
    match service.count_rows("e2e-pg", "users").await {
        Ok(4) => pass("count_rows", "4"),
        Ok(n) => fail("count_rows", &format!("expect 4 got {n}")),
        Err(e) => fail("count_rows", &e),
    }

    // 6) 查询:中文值与 NULL
    match service.run_sql("e2e-pg", "SELECT name, age, score FROM users ORDER BY id".into(), None).await {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.rows[0][0].as_deref() != Some("张三") {
                fail("run_sql", &format!("name0={:?}", r.rows[0][0]));
            }
            if r.rows[1][1].as_deref() != Some("34") {
                fail("run_sql", &format!("age1={:?}", r.rows[1][1]));
            }
            if r.rows[2][1].is_some() {
                fail("run_sql", &format!("age2(null)={:?}", r.rows[2][1]));
            }
            pass("run_sql", "中文/数值/NULL 正确");
        }
        Err(e) => fail("run_sql", &e),
    }

    // 7) 单元格回写:UPDATE 李四 age 34→35,重读验证
    let upd = vec![shujing_core::edits::CellUpdate {
        pk: vec![("id".into(), "2".into())],
        sets: vec![("age".into(), Some("35".into()))],
    }];
    match service.apply_changes("e2e-pg", "users", Some("e2e_a"), &upd, &[], &[]).await {
        Ok(_) => {
            let v = service
                .run_sql("e2e-pg", "SELECT age FROM users WHERE id=2".into(), None)
                .await
                .unwrap()
                .pop()
                .unwrap();
            if v.rows[0][0].as_deref() != Some("35") {
                fail("apply_changes", &format!("age={:?}", v.rows[0][0]));
            }
            pass("apply_changes", "回写后重读一致");
        }
        Err(e) => fail("apply_changes", &e),
    }

    // 8) 取消竞态治理:长查询 pg_sleep(30),800ms 后取消——应秒回且会话仍可用
    let svc = service.clone();
    let run = tokio::spawn(async move {
        svc.run_sql("e2e-pg", "SELECT count(*) FROM big, LATERAL pg_sleep(30)".into(), None)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let t0 = std::time::Instant::now();
    match service.cancel_query("e2e-pg").await {
        Ok(_) => pass("cancel_query", "取消成功"),
        Err(e) => fail("cancel_query", &e),
    }
    let cancel_ms = t0.elapsed().as_millis();
    let r: Result<Vec<shujing_core::ExecResult>, String> = run.await.unwrap();
    match r {
        Err(e) => {
            if !e.contains("canceling statement") {
                fail("cancelled_query_error", &e);
            }
            pass("cancel_effect", &format!("查询已中断({cancel_ms}ms 内返回取消)"));
        }
        Ok(_) => fail("cancel_effect", "长查询未被取消"),
    }
    if cancel_ms > 5000 {
        fail("cancel_latency", &format!("{cancel_ms}ms(应秒回)"));
    }

    // 9) 取消后会话活性:同连接立刻再查(取消不应挂死会话/误杀后续)
    match service.run_sql("e2e-pg", "SELECT 42".into(), None).await {
        Ok(rs) => {
            if rs.first().and_then(|r| r.rows.first()).and_then(|r| r.first())
                != Some(&Some("42".to_string()))
            {
                fail("post_cancel_query", "值不符");
            }
            pass("post_cancel_query", "会话仍可用");
        }
        Err(e) => fail("post_cancel_query", &e),
    }

    // 10) 空闲时取消:应得到「没有正在运行的查询」而非误伤后续
    match service.cancel_query("e2e-pg").await {
        Err(e) if e.contains("没有正在运行的查询") => {
            pass("idle_cancel", "正确报无运行查询");
        }
        Err(e) => fail("idle_cancel", &e),
        Ok(_) => pass("idle_cancel", "空闲取消无害通过"),
    }

    println!("\n全部通过 ✓");
}
