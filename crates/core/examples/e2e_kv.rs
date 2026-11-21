//! 配置中心三协议 E2E(直调 Service):连接/列键/发布(写)/读回(中文保真)/删除。
//! 运行:本地 consul -dev(8500)与 etcd(2379),然后
//! cargo run -p shujing-core --example e2e_kv

use shujing_core::{ConnInfo, DbType, Service};

fn pass(name: &str, detail: &str) {
    println!("PASS  {name}  {detail}");
}

fn fail(name: &str, err: &str) -> ! {
    println!("FAIL  {name}  {err}");
    std::process::exit(1);
}

async fn run_one(service: &Service, id: &str, db_type: DbType, port: u16, host: &str) {
    let info = ConnInfo {
        id: id.into(),
        name: id.into(),
        db_type,
        host: Some(host.into()),
        port: Some(port),
        user: None,
        password: None,
        database: None,
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };
    let tag = format!("{db_type:?}");

    // 连接(探活=list)
    match service.connect(info.clone()).await {
        Ok(c) => pass(&format!("connect/{tag}"), &c.version),
        Err(e) => fail(&format!("connect/{tag}"), &e),
    }

    // 幂等:清掉上轮残留(前缀键)
    if let Ok(entries) = service.kv_list(id, "shujing-e2e/").await {
        for e in &entries {
            let _ = service.kv_delete(id, &e.key).await;
        }
    }

    // 写两条(中文 + 普通)
    for (key, val) in [
        ("shujing-e2e/config.yaml", "你好 配置中心"),
        ("shujing-e2e/plain", "plain-value"),
    ] {
        if let Err(e) = service.kv_put(id, key, val).await {
            fail(&format!("put/{tag}"), &e);
        }
        pass(&format!("put/{tag}"), key);
    }

    // 列键:含前缀键
    match service.kv_list(id, "").await {
        Ok(entries) => {
            if !entries.iter().any(|e| e.key.starts_with("shujing-e2e/")) {
                fail(&format!("list/{tag}"), "前缀键未出现");
            }
            pass(&format!("list/{tag}"), &format!("{} keys", entries.len()));
        }
        Err(e) => fail(&format!("list/{tag}"), &e),
    }

    // 读回中文保真
    match service.kv_list(id, "shujing-e2e/").await {
        Ok(entries) => {
            let hit = entries
                .iter()
                .find(|e| e.key == "shujing-e2e/config.yaml");
            match hit {
                Some(e) if e.value == "你好 配置中心" => pass(&format!("get/{tag}"), "中文保真"),
                Some(e) => fail(&format!("get/{tag}"), &format!("value={:?}", e.value)),
                None => fail(&format!("get/{tag}"), "键缺失"),
            }
        }
        Err(e) => fail(&format!("get/{tag}"), &e),
    }

    // 删除两条(清理)
    for key in ["shujing-e2e/config.yaml", "shujing-e2e/plain"] {
        if let Err(e) = service.kv_delete(id, key).await {
            fail(&format!("delete/{tag}"), &e);
        }
    }
    match service.kv_list(id, "shujing-e2e/").await {
        Ok(entries) => {
            if entries.iter().any(|e| e.key.starts_with("shujing-e2e/")) {
                fail(&format!("delete/{tag}"), "清理后仍有残留");
            }
            pass(&format!("delete/{tag}"), "清理完成");
        }
        Err(e) => fail(&format!("delete/{tag}"), &e),
    }
}

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join("shujing_e2e_kv_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(dir));

    run_one(&service, "e2e-consul", DbType::Consul, 8500, "127.0.0.1").await;
    run_one(&service, "e2e-etcd", DbType::Etcd, 2379, "127.0.0.1").await;

    println!("\n全部通过 ✓");
}
