//! Redis 全链路 E2E(直调 Service):连接/SCAN/五类型写入与读取/TTL/重命名/
//! 批量删除(SCAN 游标,计数确认)/删除后计数归零。
//! 运行:本地起 redis-server(默认 127.0.0.1:6379,可用 E2E_REDIS_PORT 覆盖),然后
//! cargo run -p shujing-core --example e2e_redis

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
    let dir = std::env::temp_dir().join("shujing_e2e_redis_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(dir));
    let port: u16 = std::env::var("E2E_REDIS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6379);
    let info = ConnInfo {
        id: "e2e-redis".into(),
        name: "e2e-redis".into(),
        db_type: DbType::Redis,
        host: Some("127.0.0.1".into()),
        port: Some(port),
        user: None,
        password: None,
        database: Some("0".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        ssh: None,
    };

    // 1) 连接
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 1b) 清残留(幂等:上次运行的 e2e:* keys)
    let (_, old_keys) = service.redis_scan("e2e-redis", 0, "e2e:*", 0, 1000).await.unwrap();
    for k in &old_keys {
        let _ = service.redis_del("e2e-redis", 0, k).await;
    }

    // 2) 五类型写入(string/hash/list/set/zset),key 统一前缀
    service.redis_set("e2e-redis", 0, "e2e:str", "你好 Redis").await.unwrap();
    // hash: text 供 string 用;hash 走 pairs(键值对)
    service.redis_new_key("e2e-redis", 0, "e2e:h", "hash", "", &[("field1".into(), "值1".into()), ("field2".into(), "值2".into())]).await.unwrap();
    service
        .redis_new_key("e2e-redis", 0, "e2e:l", "list", "", &[("".into(), "a".into()), ("".into(), "b".into()), ("".into(), "c".into())])
        .await.unwrap();
    service
        .redis_new_key("e2e-redis", 0, "e2e:s", "set", "", &[("".into(), "x".into()), ("".into(), "y".into())])
        .await.unwrap();
    // zset: pairs=(score, member)
    service.redis_new_key("e2e-redis", 0, "e2e:z", "zset", "", &[("m1".into(), "100".into()), ("m2".into(), "200".into())]).await.unwrap();
    pass("seed_5_types", "string/hash/list/set/zset");

    // 3) SCAN 命中全部 5 个 + 中文值读回
    let (_, keys) = service.redis_scan("e2e-redis", 0, "e2e:*", 0, 100).await.unwrap();
    if keys.len() < 5 {
        fail("scan", &format!("expect >=5 got {}", keys.len()));
    }
    match service.redis_key_detail("e2e-redis", 0, "e2e:str").await {
        Ok(d) => {
            if d.text.as_deref() != Some("你好 Redis") {
                fail("key_detail", &format!("text={:?} type={}", d.text, d.key_type));
            }
            pass("key_detail", "中文值读回正确");
        }
        Err(e) => fail("key_detail", &e),
    }

    // 4) 类型分派
    let types = service
        .redis_key_types("e2e-redis", 0, &["e2e:str".into(), "e2e:h".into(), "e2e:l".into(), "e2e:s".into(), "e2e:z".into()])
        .await
        .unwrap();
    let expect = ["string", "hash", "list", "set", "zset"];
    if types != expect.to_vec() {
        fail("key_types", &format!("{types:?}"));
    }
    pass("key_types", "五类型分派正确");

    // 5) TTL 设置与批量读
    service.redis_set_ttl("e2e-redis", 0, "e2e:str", 3600).await.unwrap();
    let ttls = service.redis_ttl_batch("e2e-redis", 0, &["e2e:str".into(), "e2e:h".into()]).await.unwrap();
    if ttls[0] <= 0 || ttls[1] != -1 {
        fail("ttl", &format!("{ttls:?}"));
    }
    pass("ttl", "TTL=3600 / 永久=-1");

    // 6) 重命名
    service.redis_rename("e2e-redis", 0, "e2e:l", "e2e:l2").await.unwrap();
    let (_, keys2) = service.redis_scan("e2e-redis", 0, "e2e:l2", 0, 10).await.unwrap();
    if keys2.is_empty() {
        fail("rename", "新键不存在");
    }
    pass("rename", "e2e:l → e2e:l2");

    // 7) 批量删除(SCAN 游标):先计数,删,再计数归零
    let before = service.redis_scan_count("e2e-redis", 0, "e2e:*").await.unwrap();
    let deleted = service.redis_delete_by_pattern("e2e-redis", 0, "e2e:*").await.unwrap();
    let after = service.redis_scan_count("e2e-redis", 0, "e2e:*").await.unwrap();
    if before < 5 || deleted != before || after != 0 {
        fail(
            "batch_delete",
            &format!("before={before} deleted={deleted} after={after}"),
        );
    }
    pass("batch_delete", &format!("SCAN 计数 {before} → 删 {deleted} → 余 {after}"));

    println!("\n全部通过 ✓");
}
