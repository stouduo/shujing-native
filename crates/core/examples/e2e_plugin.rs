//! 驱动插件全链路 E2E:fixture agent(python,tests/plugins-sample-driver)作为
//! sidecar 子进程,验证 契约握手/连接/库/表/结构/查询/计数/回写拒绝/agent 重启恢复。
//! 运行:cargo run -p shujing-core --example e2e_plugin

use shujing_core::{ConnInfo, DbType, Service};

fn pass(name: &str, detail: &str) {
    println!("PASS  {name}  {detail}");
}

fn fail(name: &str, err: &str) -> ! {
    println!("FAIL  {name}  {err}");
    std::process::exit(1);
}

fn sample_conn(dir: &std::path::Path) -> ConnInfo {
    ConnInfo {
        id: "e2e-plugin".into(),
        name: "e2e-plugin".into(),
        db_type: DbType::Plugin,
        host: Some("127.0.0.1".into()),
        port: Some(9999),
        user: Some("demo".into()),
        password: Some("secret".into()),
        database: Some("main".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: Some("sample".into()),
        ssh: None,
    }
}

#[tokio::main]
async fn main() {
    // 装备:把 fixture 插件复制进临时 config dir 的 plugins 目录
    let dir = std::env::temp_dir().join(format!("shujing_e2e_plugin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    let manifest_dir = env!("CARGO_MANIFEST_DIR").to_string();
    let src_dir = std::path::Path::new(&manifest_dir).join("tests/plugins-sample-driver");
    let dst = dir.join("plugins/sample-driver");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::copy(src_dir.join("plugin.toml"), dst.join("plugin.toml")).unwrap();
    std::fs::copy(src_dir.join("agent.py"), dst.join("agent.py")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dst.join("agent.py"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    let service = std::sync::Arc::new(Service::new(&dir));
    let info = sample_conn(&dir);

    // 0) 注册表可见
    match service.list_saved_connections().len() {
        _ => {}
    }
    let drivers = shujing_core::plugins::scan_drivers(&dir.join("plugins"));
    match drivers.iter().find(|d| d.name == "sample") {
        Some(d) if d.db_types.contains(&"sampledb".to_string()) => {
            pass("scan_drivers", &format!("sample(db_types={:?})", d.db_types));
        }
        _ => fail("scan_drivers", "fixture 插件未被发现"),
    }

    // 1) 连接(agent 握手 + connect)
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 2) 库列表(agent databases)
    match service.list_databases("e2e-plugin").await {
        Ok(dbs) if dbs.contains(&"main".to_string()) => {
            pass("list_databases", &format!("dbs={dbs:?}"))
        }
        Ok(dbs) => fail("list_databases", &format!("{dbs:?}")),
        Err(e) => fail("list_databases", &e),
    }

    // 3) 表列表
    match service.list_tables("e2e-plugin", Some("main".into())).await {
        Ok(ts) if ts.iter().any(|t| t.name == "users") => {
            pass("list_tables", &format!("{} tables, 含 users", ts.len()))
        }
        Ok(ts) => fail("list_tables", &format!("{ts:?}")),
        Err(e) => fail("list_tables", &e),
    }

    // 4) 结构:id=PRI + 中文注释透传
    match service
        .get_table_structure("e2e-plugin", "users", Some("main".into()))
        .await
    {
        Ok(st) => {
            let pk = st.columns.iter().find(|c| c.key == "PRI");
            let zh = st.columns.iter().any(|c| c.comment == "姓名");
            if pk.map(|c| c.name.as_str()) != Some("id") || !zh {
                fail(
                    "structure",
                    &format!("pk={:?} 中文注释={zh}", pk.map(|c| c.name.clone())),
                );
            }
            pass("structure", "pk=id,中文注释透传");
        }
        Err(e) => fail("structure", &e),
    }

    // 5) 计数(agent count)
    match service.count_rows("e2e-plugin", "users").await {
        Ok(2) => pass("count_rows", "2"),
        Ok(n) => fail("count_rows", &format!("expect 2 got {n}")),
        Err(e) => fail("count_rows", &e),
    }

    // 6) 查询(agent query,中文保真)
    match service
        .run_sql_db(
            "e2e-plugin",
            "SELECT id, name FROM users".into(),
            None,
            Some("main".into()),
        )
        .await
    {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.rows.len() != 2 || r.rows[0][1].as_deref() != Some("张三") {
                fail("run_sql", &format!("{:?}", r.rows));
            }
            pass("run_sql", "查询 2 行,中文保真");
        }
        Err(e) => fail("run_sql", &e),
    }

    // 7) 表格回写(execute_batch 契约):fixture 返回 affected=语句数
    let upd = vec![shujing_core::edits::CellUpdate {
        pk: vec![("id".into(), "1".into())],
        sets: vec![("name".into(), Some("改".into()))],
    }];
    match service
        .apply_changes("e2e-plugin", "users", Some("main"), &upd, &[], &[])
        .await
    {
        Ok(r) if r.affected == 1 => pass("apply_changes", "回写 execute_batch,affected=1"),
        Ok(r) => fail("apply_changes", &format!("affected={}", r.affected)),
        Err(e) => fail("apply_changes", &e),
    }

    // 8) agent 重启恢复:断开→重连(agent 进程全新,连接语义完好)
    let _ = service.disconnect("e2e-plugin").await;
    match service.connect(info.clone()).await {
        Ok(c) => pass("reconnect", &c.version),
        Err(e) => fail("reconnect", &e),
    }
    match service.count_rows("e2e-plugin", "users").await {
        Ok(2) => pass("post_reconnect_count", "重连后计数一致"),
        Ok(n) => fail("post_reconnect_count", &format!("{n}")),
        Err(e) => fail("post_reconnect_count", &e),
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("\n全部通过 ✓");
}
