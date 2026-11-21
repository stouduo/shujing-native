//! RabbitMQ 全链路 E2E(直调 Service,走 Management HTTP API):
//! 连接/概览/队列列表/发布(默认交换机)/取回消息(requeue,中文保真)/清空队列。
//! 运行:本地 rabbitmq + management 插件(默认 127.0.0.1:15672,guest/guest),然后
//! cargo run -p shujing-core --example e2e_rabbit

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
    let dir = std::env::temp_dir().join("shujing_e2e_rabbit_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(dir));
    let addr = std::env::var("E2E_RABBIT_ADDR").unwrap_or_else(|_| "http://127.0.0.1:15672".into());
    let info = ConnInfo {
        id: "e2e-rabbit".into(),
        name: "e2e-rabbit".into(),
        db_type: DbType::Rabbit,
        host: Some(addr.clone()),
        port: None,
        user: Some("guest".into()),
        password: Some("guest".into()),
        database: None,
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        ssh: None,
    };

    // 1) 连接(overview 探活)
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 1b) 幂等:清掉上轮残留消息
    let _ = service.rabbit_purge("e2e-rabbit", "/", "shujing-e2e-queue").await;

    // 2) 发布两条到默认交换机(路由键=shujing-e2e-queue)
    for payload in ["你好 RabbitMQ", "消息体-2"] {
        match service
            .rabbit_publish("e2e-rabbit", "/", "shujing-e2e-queue", payload)
            .await
        {
            Ok(_) => pass("publish", payload),
            Err(e) => fail("publish", &e),
        }
    }

    // 3) 队列列表:应出现 shujing-e2e-queue(自动声明)且消息数 2
    match service.rabbit_list_queues("e2e-rabbit").await {
        Ok(_) => {
            // 详情端点统计也滞后(5s 采样):轮询到 >=2,10s 超时判实际深度
            let mut got = 0u64;
            let mut last = 0u64;
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Ok(q) = service
                    .rabbit_queue_detail("e2e-rabbit", "/", "shujing-e2e-queue")
                    .await
                {
                    last = q.messages;
                    if q.messages >= 2 {
                        got = q.messages;
                        break;
                    }
                }
            }
            if got >= 2 {
                pass("list_queues", &format!("detail {got} msgs"));
            } else {
                fail("list_queues", &format!("stats lag, last={last}"));
            }
        }
        Err(e) => fail("list_queues", &e),
    }

    // 4) 取回消息(requeue,中文保真)
    match service
        .rabbit_get_messages("e2e-rabbit", "/", "shujing-e2e-queue", 20)
        .await
    {
        Ok(msgs) => {
            if msgs.len() < 2 {
                fail("get_messages", &format!("expect >=2 got {}", msgs.len()));
            }
            let has_zh = msgs
                .iter()
                .any(|m| m.payload.as_deref() == Some("你好 RabbitMQ"));
            if !has_zh {
                fail("get_messages", "中文载荷丢失");
            }
            let requeued = msgs.iter().any(|m| m.redelivered);
            pass(
                "get_messages",
                &format!("{} msgs, 中文保真(requeue 标记 {})", msgs.len(), requeued),
            );
        }
        Err(e) => fail("get_messages", &e),
    }

    // 5) 清空队列
    match service.rabbit_purge("e2e-rabbit", "/", "shujing-e2e-queue").await {
        Ok(_) => pass("purge", "已清空"),
        Err(e) => fail("purge", &e),
    }
    // 管理统计刷新有滞后(默认 5s 采样):轮询等待归零,8s 超时按实际深度判定
    let mut m = -1i64;
    for _ in 0..16 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if let Ok(q) = service
            .rabbit_queue_detail("e2e-rabbit", "/", "shujing-e2e-queue")
            .await
        {
            if q.messages == 0 {
                m = 0;
                break;
            }
            m = q.messages as i64;
        }
    }
    if m == 0 {
        pass("purge_verify", "消息数归零(统计同步后)");
    } else {
        fail("purge_verify", &format!("msgs={m}"));
    }

    println!("\n全部通过 ✓");
}
