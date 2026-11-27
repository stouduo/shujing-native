//! MQTT 全链路 E2E(rumqttc 直连):浏览(订阅 # 采样,retained 可见)/发布(QoS1)/
//! 发布后再浏览(发布消息出现)。
//! 运行:本地 mosquitto(默认 127.0.0.1:1883,无鉴权),然后
//! cargo run -p shujing-core --example e2e_mqtt

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
    let dir = std::env::temp_dir().join("shujing_e2e_mqtt_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(dir));
    let info = ConnInfo {
        id: "e2e-mqtt".into(),
        name: "e2e-mqtt".into(),
        db_type: DbType::Mqtt,
        host: Some("127.0.0.1".into()),
        port: Some(1884),
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

    // 1) 连接(probe=1s 采样)
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 2) 发布两条(一条 retained,一条普通)
    match service
        .mqtt_publish("e2e-mqtt", "shujing/e2e/state", "你好 MQTT(retained)", true)
        .await
    {
        Ok(n) => pass("publish_retained", &format!("{n} B")),
        Err(e) => fail("publish_retained", &e),
    }
    match service
        .mqtt_publish("e2e-mqtt", "shujing/e2e/event", "普通消息", false)
        .await
    {
        Ok(n) => pass("publish_plain", &format!("{n} B")),
        Err(e) => fail("publish_plain", &e),
    }

    // 3) 浏览:retained 主题必现且中文保真;普通消息可能已消费完(无 retained 不保证在)
    match service.mqtt_browse("e2e-mqtt", 3).await {
        Ok(msgs) => {
            let st = msgs.iter().find(|m| m.topic == "shujing/e2e/state");
            match st {
                Some(m) if m.retained && m.payload.contains("你好 MQTT") => {
                    pass("browse_retained", "retained + 中文保真");
                }
                Some(m) => fail("browse_retained", &format!("{m:?}")),
                None => fail("browse_retained", "retained 主题未出现"),
            }
            pass("browse", &format!("{} topics seen", msgs.len()));
        }
        Err(e) => fail("browse", &e),
    }

    // 4) 无 SQL 执行面:界面侧已不给 MQTT 开 SQL 查询页,这里是后端的兜底——
    // 真要走到 run_sql,必须是明确的拒绝
    match service.run_sql("e2e-mqtt", "SELECT 1".into(), None).await {
        Err(e) if e.contains("无 SQL 执行面") => pass("no_sql_surface", &e),
        Err(e) => fail("no_sql_surface", &format!("拒绝文案不对: {e}")),
        Ok(_) => fail("no_sql_surface", "MQTT 连接竟然执行了 SQL"),
    }

    println!("\n全部通过 ✓");
}
