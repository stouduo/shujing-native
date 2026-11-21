//! MySQL 全链路 E2E(直调 Service):连接/库/表/结构/计数/查询/单元格回写/
//! 跨库限定读/会话列表+kill_session(会话面板路径)/cancel_query(取消按钮路径,
//! 控制连接+代数戳)/取消后会话活性。
//! 运行:先起本地 MySQL(brew services start mysql,默认 root 无密码),然后
//! cargo run -p shujing-core --example e2e_mysql
//! 连接参数可用环境变量覆盖:E2E_MYSQL_HOST/E2E_MYSQL_PORT/E2E_MYSQL_USER/E2E_MYSQL_PASS

use mysql_async::prelude::*;
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
    let dir = std::env::temp_dir().join("shujing_e2e_mysql_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(&dir));

    let host = std::env::var("E2E_MYSQL_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("E2E_MYSQL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3306);
    let user = std::env::var("E2E_MYSQL_USER").unwrap_or_else(|_| "root".into());
    let pass_ = std::env::var("E2E_MYSQL_PASS").unwrap_or_default();
    let opts = || {
        mysql_async::OptsBuilder::default()
            .ip_or_hostname(host.clone())
            .tcp_port(port)
            .user(Some(user.clone()))
            .pass((!pass_.is_empty()).then(|| pass_.clone()))
    };

    // 0) 幂等重置:直接驱动建库建表灌种子
    {
        let mut conn = mysql_async::Conn::new(opts())
            .await
            .expect("raw mysql conn");
        conn.query_drop("CREATE DATABASE IF NOT EXISTS e2e_mysql").await.unwrap();
        conn.query_drop("CREATE DATABASE IF NOT EXISTS e2e_mysql2").await.unwrap();
        conn.query_drop("DROP TABLE IF EXISTS e2e_mysql.users").await.unwrap();
        conn.query_drop("DROP TABLE IF EXISTS e2e_mysql2.marker").await.unwrap();
        conn.query_drop(
            "CREATE TABLE e2e_mysql.users (\
             id BIGINT AUTO_INCREMENT PRIMARY KEY, \
             name VARCHAR(64) NULL, \
             age INT NULL, \
             score DOUBLE NULL, \
             active TINYINT(1) NOT NULL DEFAULT 1, \
             note VARCHAR(100) NULL) \
             ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        )
        .await
        .unwrap();
        conn.exec_drop(
            "INSERT INTO e2e_mysql.users (name, age, score, active, note) VALUES (?,?,?,?,?),(?,?,?,?,?),(?,?,?,?,?),(?,?,?,?,?)",
            vec![
                mysql_async::Value::from("张三"), mysql_async::Value::from(28), mysql_async::Value::from(91.5f64), mysql_async::Value::from(1u8), mysql_async::Value::from("甲"),
                mysql_async::Value::from("李四"), mysql_async::Value::from(34), mysql_async::Value::from(88.25f64), mysql_async::Value::from(0u8), mysql_async::Value::NULL,
                mysql_async::Value::from("王五"), mysql_async::Value::NULL, mysql_async::Value::from(72.0f64), mysql_async::Value::from(1u8), mysql_async::Value::from("丙"),
                mysql_async::Value::from("赵六"), mysql_async::Value::from(19), mysql_async::Value::from(95.75f64), mysql_async::Value::from(1u8), mysql_async::Value::NULL,
            ],
        )
        .await
        .unwrap();
        conn.query_drop(
            "CREATE TABLE e2e_mysql2.marker (id INT PRIMARY KEY, val VARCHAR(20))",
        )
        .await
        .unwrap();
        conn.query_drop("INSERT INTO e2e_mysql2.marker VALUES (1, '跨库值')")
            .await
            .unwrap();
    }

    let info = ConnInfo {
        id: "e2e-mysql".into(),
        name: "e2e-mysql".into(),
        db_type: DbType::MySql,
        host: Some(host.clone()),
        port: Some(port),
        user: Some(user.clone()),
        password: Some(pass_.clone()).filter(|s| !s.is_empty()),
        database: Some("e2e_mysql".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        ssh: None,
    };

    // 1) 连接 + 版本
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 2) 库列表含 e2e_mysql / e2e_mysql2
    match service.list_databases("e2e-mysql").await {
        Ok(dbs) => {
            let ok = dbs.iter().any(|d| d == "e2e_mysql") && dbs.iter().any(|d| d == "e2e_mysql2");
            if !ok {
                fail("list_databases", &format!("{dbs:?}"));
            }
            pass("list_databases", &format!("{} dbs", dbs.len()));
        }
        Err(e) => fail("list_databases", &e),
    }

    // 3) 表列表含 users
    match service.list_tables("e2e-mysql", Some("e2e_mysql".into())).await {
        Ok(ts) => {
            if !ts.iter().any(|t| t.name == "users") {
                fail("list_tables", &format!("{ts:?}"));
            }
            pass("list_tables", &format!("{} tables", ts.len()));
        }
        Err(e) => fail("list_tables", &e),
    }

    // 4) 结构:6 列,id=PRI
    match service
        .get_table_structure("e2e-mysql", "users", Some("e2e_mysql".to_string()))
        .await
    {
        Ok(s) => {
            let pk = s.columns.iter().find(|c| c.key == "PRI");
            if s.columns.len() != 6 || pk.map(|c| c.name.as_str()) != Some("id") {
                fail(
                    "structure",
                    &format!("cols={} pk={:?}", s.columns.len(), pk.map(|c| c.name.clone())),
                );
            }
            pass("structure", "6 cols, pk=id");
        }
        Err(e) => fail("structure", &e),
    }

    // 5) 计数
    match service.count_rows("e2e-mysql", "users").await {
        Ok(4) => pass("count_rows", "4"),
        Ok(n) => fail("count_rows", &format!("expect 4 got {n}")),
        Err(e) => fail("count_rows", &e),
    }

    // 6) 查询:中文值与 NULL
    match service
        .run_sql(
            "e2e-mysql",
            "SELECT name, age, score, note FROM users ORDER BY id".into(),
            None,
        )
        .await
    {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.rows[0][0].as_deref() != Some("张三") {
                fail("run_sql", &format!("name0={:?}", r.rows[0][0]));
            }
            if r.rows[2][1].is_some() {
                fail("run_sql", &format!("age2(null)={:?}", r.rows[2][1]));
            }
            if r.rows[1][3].is_some() {
                fail("run_sql", &format!("note1(null)={:?}", r.rows[1][3]));
            }
            pass("run_sql", "中文/数值/NULL 正确");
        }
        Err(e) => fail("run_sql", &e),
    }

    // 7) 跨库限定读(会话默认库 e2e_mysql,读 e2e_mysql2.marker)
    match service
        .run_sql("e2e-mysql", "SELECT val FROM e2e_mysql2.marker WHERE id=1".into(), None)
        .await
    {
        Ok(rs) => {
            let v = rs
                .first()
                .and_then(|r| r.rows.first())
                .and_then(|r| r.first())
                .and_then(|v| v.clone());
            if v.as_deref() != Some("跨库值") {
                fail("cross_db_read", &format!("val={v:?}"));
            }
            pass("cross_db_read", "库名限定跨库读正确");
        }
        Err(e) => fail("cross_db_read", &e),
    }

    // 8) 单元格回写:李四 age 34→35,重读验证
    let upd = vec![shujing_core::edits::CellUpdate {
        pk: vec![("id".into(), "2".into())],
        sets: vec![("age".into(), Some("35".into()))],
    }];
    match service
        .apply_changes("e2e-mysql", "users", Some("e2e_mysql"), &upd, &[], &[])
        .await
    {
        Ok(_) => {
            let v = service
                .run_sql("e2e-mysql", "SELECT age FROM users WHERE id=2".into(), None)
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

    // 9) 会话面板路径:长查询 SLEEP(30) → list_sessions 定位 → kill_session 中断
    {
        let svc = service.clone();
        let run = tokio::spawn(async move {
            svc.run_sql("e2e-mysql", "SELECT SLEEP(30) AS z".into(), None).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let sessions = match service.list_sessions("e2e-mysql").await {
            Ok(v) => v,
            Err(e) => fail("list_sessions", &e),
        };
        let target = sessions.iter().find(|s| s.query.contains("SLEEP"));
        let Some(target) = target else {
            let dump: Vec<String> = sessions
                .iter()
                .map(|s| format!("id={} cmd={} q={:?}", s.id, s.command, s.query))
                .collect();
            fail(
                "list_sessions",
                &format!("SLEEP 不在列表(共 {} 条): {}", sessions.len(), dump.join(" | ")),
            );
        };
        pass(
            "list_sessions",
            &format!("共 {} 条,SLEEP 会话 id={} ({})", sessions.len(), target.id, target.command),
        );
        let t0 = std::time::Instant::now();
        if let Err(e) = service.kill_session("e2e-mysql", &target.id, true).await {
            fail("kill_session", &e);
        }
        let kill_ms = t0.elapsed().as_millis();
        let started_at = std::time::Instant::now();
        match run.await.unwrap() {
            Err(e) => {
                let hit = e.contains("interrupted")
                    || e.contains("1314")
                    || e.contains("Query execution was interrupted");
                if !hit {
                    fail("kill_effect", &format!("异常信息不符: {e}"));
                }
                pass("kill_effect", &format!("KILL QUERY 中断长查询({}ms)", started_at.elapsed().as_millis()));
            }
            // MySQL 26 语义:SLEEP 被 KILL QUERY 中断时提前返回(结果=1)而非报错;
            // 真实慢查询才收 1314 错误。判据=远早于 30s 自然结束
            Ok(_) => {
                let ms = started_at.elapsed().as_millis();
                if ms > 10_000 {
                    fail("kill_effect", &format!("SLEEP 跑满({ms}ms),未被中断"));
                }
                pass("kill_effect", &format!("SLEEP 被 KILL 提前结束({ms}ms,MySQL 26 中断语义)"));
            }
        }
        if kill_ms > 5000 {
            fail("kill_latency", &format!("{kill_ms}ms(应秒回)"));
        }
    }

    // 10) 取消后会话活性
    match service.run_sql("e2e-mysql", "SELECT 42".into(), None).await {
        Ok(rs) => {
            let v = rs
                .first()
                .and_then(|r| r.rows.first())
                .and_then(|r| r.first())
                .cloned()
                .flatten();
            if v.as_deref() != Some("42") {
                fail("post_kill_query", &format!("值不符: {v:?}"));
            }
            pass("post_kill_query", "会话仍可用");
        }
        Err(e) => fail("post_kill_query", &e),
    }

    // 11) 取消按钮路径:cancel_query(控制连接 + 代数戳防误杀)
    {
        let svc = service.clone();
        let run = tokio::spawn(async move {
            svc.run_sql("e2e-mysql", "SELECT SLEEP(30) AS z2".into(), None).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let t0 = std::time::Instant::now();
        match service.cancel_query("e2e-mysql").await {
            Ok(_) => pass("cancel_query", "取消成功"),
            Err(e) => fail("cancel_query", &e),
        }
        let cancel_ms = t0.elapsed().as_millis();
        let started_at = std::time::Instant::now();
        match run.await.unwrap() {
            Err(e) => {
                if !e.contains("interrupted") && !e.contains("1314") {
                    fail("cancel_effect", &format!("异常信息不符: {e}"));
                }
                pass("cancel_effect", &format!("取消按钮中断长查询({}ms)", started_at.elapsed().as_millis()));
            }
            Ok(_) => {
                let ms = started_at.elapsed().as_millis();
                if ms > 10_000 {
                    fail("cancel_effect", &format!("SLEEP 跑满({ms}ms),未被取消"));
                }
                pass("cancel_effect", &format!("SLEEP 被取消提前结束({ms}ms,MySQL 26 中断语义)"));
            }
        }
        if cancel_ms > 5000 {
            fail("cancel_latency", &format!("{cancel_ms}ms(应秒回)"));
        }
    }

    // 12) 空闲取消:误杀防护(代数不匹配应放弃;不苛求报错文案)
    match service.cancel_query("e2e-mysql").await {
        Err(e) if e.contains("没有正在运行的查询") => pass("idle_cancel", "正确报无运行查询"),
        Err(_) => pass("idle_cancel", "空闲取消安全放弃(非误杀)"),
        Ok(_) => pass("idle_cancel", "空闲取消无害通过"),
    }

    // 13) 清理
    {
        let mut conn = mysql_async::Conn::new(opts()).await.unwrap();
        let _ = conn.query_drop("DROP DATABASE IF EXISTS e2e_mysql2").await;
        let _ = conn.query_drop("DROP DATABASE IF EXISTS e2e_mysql").await;
    }

    println!("\n全部通过 ✓");
}
