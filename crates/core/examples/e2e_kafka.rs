//! Kafka 全链路 E2E(直调 Service):连接/topic 建-列-删/发消息/从头读取/消费组。
//! 运行:本地 broker(默认 127.0.0.1:9092,E2E_KAFKA_ADDR 可覆盖),然后
//! cargo run -p shujing-core --example e2e_kafka

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
    let dir = std::env::temp_dir().join("shujing_e2e_kafka_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(dir));
    let addr = std::env::var("E2E_KAFKA_ADDR").unwrap_or_else(|_| "127.0.0.1:9092".into());
    let info = ConnInfo {
        id: "e2e-kafka".into(),
        name: "e2e-kafka".into(),
        db_type: DbType::Kafka,
        host: Some(addr.clone()),
        port: None,
        user: None,
        password: None,
        database: None,
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        ssh: None,
    };

    // 1) 连接(探活=元数据) + 版本信息
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 2) 新建 topic(1 分区 1 副本)
    match service
        .kafka_create_topic("e2e-kafka", "shujing-e2e", 1, 1)
        .await
    {
        Ok(_) => pass("create_topic", "shujing-e2e p1 r1"),
        Err(e) => fail("create_topic", &e),
    }

    // 3) topic 列表含新 topic
    match service.kafka_list_topics("e2e-kafka").await {
        Ok(ts) => {
            if !ts.iter().any(|t| t.name == "shujing-e2e") {
                fail("list_topics", &format!("{:?}", ts.iter().map(|t| &t.name).collect::<Vec<_>>()));
            }
            pass("list_topics", &format!("{} topics", ts.len()));
        }
        Err(e) => fail("list_topics", &e),
    }

    // 4) 发送两条(key/payload 中文)
    for (k, v) in [("k1", "你好 Kafka"), ("k2", "消息体-2")] {
        match service
            .kafka_produce("e2e-kafka", "shujing-e2e", Some(k), v)
            .await
        {
            Ok((p, o)) => pass("produce", &format!("{k} → p{p} o{o}")),
            Err(e) => fail("produce", &e),
        }
    }

    // 5) 从头读取:应收到 2 条且中文保真
    match service
        .kafka_consume_recent("e2e-kafka", "shujing-e2e", 200, 8)
        .await
    {
        Ok(msgs) => {
            if msgs.len() < 2 {
                fail("consume", &format!("expect >=2 got {}", msgs.len()));
            }
            let has_zh = msgs.iter().any(|m| m.payload.as_deref() == Some("你好 Kafka"));
            let has_key = msgs.iter().any(|m| m.key.as_deref() == Some("k2"));
            if !has_zh || !has_key {
                fail("consume", &format!("zh={has_zh} key={has_key}"));
            }
            pass("consume", &format!("{} msgs, 中文/键保真", msgs.len()));
        }
        Err(e) => fail("consume", &e),
    }

    // 6) 消费组列表(空也可)
    match service.kafka_groups("e2e-kafka").await {
        Ok(g) => pass("list_groups", &format!("{} groups", g.len())),
        Err(e) => fail("list_groups", &e),
    }

    // 7) 删除 topic(清理)
    match service.kafka_delete_topic("e2e-kafka", "shujing-e2e").await {
        Ok(_) => pass("delete_topic", "清理完成"),
        Err(e) => fail("delete_topic", &e),
    }

    println!("\n全部通过 ✓");
}
