//! MQTT 连接器(中间件对齐第三批):rumqttc 纯 Rust 客户端。
//! MQTT 无"列出全部主题"的协议语义——浏览 = 订阅 # 通配符采样 N 秒收集
//! 出现过的主题(含 retained 载荷);发布 = 单条 QoS1(等 PubAck)。
//! 每次操作独立建连(MQTT connect 开销低;避免常驻 eventloop 任务)。

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use std::time::Duration;

/// 连接参数(Backend 常驻持有;客户端按操作临时建)
#[derive(Debug, Clone)]
pub struct MqttState {
    pub host: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
}

fn opts(state: &MqttState, client_id: &str) -> MqttOptions {
    let mut o = MqttOptions::new(client_id, &state.host, state.port);
    o.set_keep_alive(Duration::from_secs(15));
    if let Some((u, p)) = &state.auth {
        o.set_credentials(u, p);
    }
    o
}

/// 一条采样到的消息
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MqttMsg {
    pub topic: String,
    pub payload: String,
    /// 原始字节数(载荷非 UTF8 时显示 hex)
    pub bytes: usize,
    pub retained: bool,
    /// 非文本载荷标记(true 时 payload 是 hex 预览)
    pub binary: bool,
}

fn decode(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), false),
        Err(_) => {
            let take = bytes.len().min(24);
            (
                bytes[..take].iter().map(|b| format!("{b:02x}")).collect(),
                true,
            )
        }
    }
}

/// 浏览:订阅 # 采样 secs 秒,收集出现过的主题(每主题保留最近一条载荷)。
/// retained 消息会在订阅时立刻下发,所以空 broker 也能列出历史主题。
pub async fn browse_snapshot(
    state: &MqttState,
    secs: u64,
) -> Result<Vec<MqttMsg>, String> {
    let (client, mut evloop) = AsyncClient::new(opts(state, "shujing-browse"), 16);
    client
        .subscribe("#", QoS::AtMostOnce)
        .await
        .map_err(|e| format!("MQTT 订阅失败: {e}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs.max(1));
    // topic → (payload, bytes, retained);retained 优先保留
    let mut topics: std::collections::HashMap<String, (String, usize, bool, bool)> =
        std::collections::HashMap::new();
    loop {
        let left = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or(Duration::from_millis(1));
        match tokio::time::timeout(left, evloop.poll()).await {
            Err(_) => break, // 采样时间到
            Ok(Err(e)) => return Err(format!("MQTT: {e}")),
            Ok(Ok(Event::Incoming(Packet::Publish(p)))) => {
                let (payload, binary) = decode(&p.payload);
                let entry = topics
                    .entry(p.topic.to_string())
                    .or_insert((payload.clone(), p.payload.len(), p.retain, binary));
                if p.retain || entry.2 == false {
                    // retained 优先;非 retained 时保留最新
                    *entry = (payload, p.payload.len(), p.retain, binary);
                }
            }
            Ok(Ok(_)) => {}
        }
    }
    let _ = client.disconnect().await;
    let mut out: Vec<MqttMsg> = topics
        .into_iter()
        .map(|(topic, (payload, bytes, retained, binary))| MqttMsg {
            topic,
            payload,
            bytes,
            retained,
            binary,
        })
        .collect();
    out.sort_by(|a, b| a.topic.cmp(&b.topic));
    Ok(out)
}

/// 发布一条(QoS1,等确认);返回报文字节数
pub async fn publish(
    state: &MqttState,
    topic: &str,
    payload: &str,
    retain: bool,
) -> Result<usize, String> {
    if topic.trim().is_empty() {
        return Err("topic 不能为空".into());
    }
    let (client, mut evloop) = AsyncClient::new(opts(state, "shujing-publish"), 16);
    // 等 ConnAck 后发;QoS1 等 PubAck 确认
    let mut acked = false;
    let mut sent = false;
    client
        .publish(topic, QoS::AtLeastOnce, retain, payload.as_bytes().to_vec())
        .await
        .map_err(|e| format!("MQTT 发布失败: {e}"))?;
    sent = true;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if acked || !sent {
            break;
        }
        let left = deadline
            .checked_duration_since(tokio::time::Instant::now())
            .unwrap_or(Duration::from_millis(1));
        match tokio::time::timeout(left, evloop.poll()).await {
            Err(_) => break,
            Ok(Err(e)) => return Err(format!("MQTT: {e}")),
            Ok(Ok(ev)) => {
                if let Event::Incoming(Packet::PubAck(_)) = ev {
                    acked = true;
                    break;
                }
            }
        }
    }
    let _ = client.disconnect().await;
    if !acked {
        return Err("MQTT: 发布超时未确认".into());
    }
    Ok(payload.as_bytes().len())
}

/// 探活:短采样一次(1s);能建连不报错即视为可用
pub async fn probe(state: &MqttState) -> Result<(), String> {
    browse_snapshot(state, 1).await.map(|_| ())
}
