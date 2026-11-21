//! RabbitMQ 连接器(v1,中间件对齐第二批):走 Management HTTP API(默认 15672),
//! 不引入 AMQP 客户端——队列/交换机枚举、消息取回(requeue)、发布、清空全部覆盖。
//! 纯 reqwest(与 clickhouse.rs 同模式),零新依赖。
//! host 栏语义:management 地址;AMQP 应用端口(5672)仅作连接表单默认展示。

use serde_json::Value;
use std::time::Duration;

/// RabbitMQ 连接状态(management API)
pub struct RabbitState {
    /// 形如 http://127.0.0.1:15672
    pub base: String,
    pub auth: Option<(String, String)>,
    pub http: reqwest::Client,
}

fn api_url(base: &str, path: &str) -> String {
    // path 已含 /api 前缀(调用方传完整 API 路径)
    format!("{}{}", base.trim_end_matches('/'), path)
}

async fn get_json(st: &RabbitState, path: &str) -> Result<Value, String> {
    let mut req = st.http.get(api_url(&st.base, path));
    if let Some((u, p)) = &st.auth {
        req = req.basic_auth(u, Some(p));
    }
    let resp = req.send().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    if !status.is_success() {
        return Err(format!("RabbitMQ: HTTP {} {}", status, text.trim()));
    }
    serde_json::from_str(&text).map_err(|e| format!("RabbitMQ 响应解析失败: {e}"))
}

async fn put_json(st: &RabbitState, path: &str, body: &Value) -> Result<(), String> {
    let mut req = st.http.put(api_url(&st.base, path));
    if let Some((u, p)) = &st.auth {
        req = req.basic_auth(u, Some(p));
    }
    let resp = req
        .json(body)
        .send()
        .await
        .map_err(|e| format!("RabbitMQ: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    if !status.is_success() {
        return Err(format!("RabbitMQ: HTTP {} {}", status, text.trim()));
    }
    Ok(())
}

async fn post_json(st: &RabbitState, path: &str, body: &Value) -> Result<(), String> {
    let mut req = st.http.post(api_url(&st.base, path));
    if let Some((u, p)) = &st.auth {
        req = req.basic_auth(u, Some(p));
    }
    let resp = req
        .json(body)
        .send()
        .await
        .map_err(|e| format!("RabbitMQ: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    if !status.is_success() {
        return Err(format!("RabbitMQ: HTTP {} {}", status, text.trim()));
    }
    Ok(())
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 建立连接:overview 作探活
pub async fn connect(
    base: String,
    auth: Option<(String, String)>,
) -> Result<RabbitState, String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {e}"))?;
    let st = RabbitState { base, auth, http };
    overview(&st).await?;
    Ok(st)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RabbitOverview {
    pub version: String,
    pub queue_count: u64,
    pub msg_total: u64,
    pub connections: u64,
}

/// 服务器概览(版本/队列数/消息总数/连接数)
pub async fn overview(st: &RabbitState) -> Result<RabbitOverview, String> {
    let v = get_json(&st, "/api/overview").await?;
    let q = v["queues"].as_u64().unwrap_or(0);
    let msgs = v["messages"].as_u64().unwrap_or(0);
    let conns = v["object_totals"]["connections"].as_u64().unwrap_or(0);
    Ok(RabbitOverview {
        version: v["rabbitmq_version"]
            .as_str()
            .or_else(|| v["management_version"].as_str())
            .unwrap_or("unknown")
            .to_string(),
        queue_count: q,
        msg_total: msgs,
        connections: conns,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RabbitQueue {
    pub name: String,
    /// vhost(浏览/取消息路径参数)
    pub vhost: String,
    pub messages: u64,
    pub messages_ready: u64,
    pub messages_unacked: u64,
    pub consumers: u64,
    /// 声明类型(队列参数 user.who?用 rabbitmq 3.x 的 arguments.type 直取;quorum 等)
    pub queue_type: String,
}

/// 队列列表(全部 vhost;含消息数/消费者数/类型)
pub async fn list_queues(st: &RabbitState) -> Result<Vec<RabbitQueue>, String> {
    let v = get_json(&st, "/api/queues").await?;
    let arr = v.as_array().cloned().unwrap_or_default();
    let mut out: Vec<RabbitQueue> = arr
        .iter()
        .map(|q| RabbitQueue {
            name: q["name"].as_str().unwrap_or("").to_string(),
            vhost: q["vhost"].as_str().unwrap_or("/").to_string(),
            messages: q["messages"].as_u64().unwrap_or(0),
            messages_ready: q["messages_ready"].as_u64().unwrap_or(0),
            messages_unacked: q["messages_unacknowledged"].as_u64().unwrap_or(0),
            consumers: q["consumers"].as_u64().unwrap_or(0),
            queue_type: q["arguments"]["x-queue-type"]
                .as_str()
                .unwrap_or("classic")
                .to_string(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn vhost_enc(vhost: &str) -> String {
    urlenc(vhost)
}

/// 单队列详情(GET /api/queues/{vhost}/{name};统计比列表端点准)
pub async fn queue_detail(
    st: &RabbitState,
    vhost: &str,
    queue: &str,
) -> Result<RabbitQueue, String> {
    let v = get_json(
        st,
        &format!(
            "/api/queues/{}/{}",
            vhost_enc(vhost),
            urlenc(queue)
        ),
    )
    .await?;
    Ok(RabbitQueue {
        name: v["name"].as_str().unwrap_or(queue).to_string(),
        vhost: v["vhost"].as_str().unwrap_or(vhost).to_string(),
        messages: v["messages"].as_u64().unwrap_or(0),
        messages_ready: v["messages_ready"].as_u64().unwrap_or(0),
        messages_unacked: v["messages_unacknowledged"].as_u64().unwrap_or(0),
        consumers: v["consumers"].as_u64().unwrap_or(0),
        queue_type: v["arguments"]["x-queue-type"]
            .as_str()
            .unwrap_or("classic")
            .to_string(),
    })
}

/// 从队列取回消息(requeue=true 仅浏览不移除;ackmode 2=reject+requeue)
pub async fn get_messages(
    st: &RabbitState,
    vhost: &str,
    queue: &str,
    count: usize,
) -> Result<Vec<StreamMsg>, String> {
    let body = serde_json::json!({
        "count": count,
        "ackmode": "ack_requeue_true",
        "encoding": "auto",
        "truncate": 50_000
    });
    let v = get_json_post(
        st,
        &format!(
            "/api/queues/{}/{}/get",
            vhost_enc(vhost),
            urlenc(queue)
        ),
        &body,
    )
    .await?;
    let arr = v.as_array().cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .map(|m| {
            let payload = match &m["payload"] {
                Value::String(s) => Some(s.clone()),
                Value::Null => None,
                other => serde_json::to_string(other).ok(),
            };
            StreamMsg {
                exchange: m["exchange"].as_str().unwrap_or("").to_string(),
                routing_key: m["routing_key"].as_str().unwrap_or("").to_string(),
                redelivered: m["redelivered"].as_bool().unwrap_or(false),
                payload,
                payload_bytes: m["payload_bytes"].as_u64().unwrap_or(0),
                ts: m["properties"]["timestamp"].as_i64().unwrap_or(0),
            }
        })
        .collect())
}

async fn get_json_post(st: &RabbitState, path: &str, body: &Value) -> Result<Value, String> {
    let mut req = st.http.post(api_url(&st.base, path));
    if let Some((u, p)) = &st.auth {
        req = req.basic_auth(u, Some(p));
    }
    let resp = req
        .json(body)
        .send()
        .await
        .map_err(|e| format!("RabbitMQ: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    if !status.is_success() {
        return Err(format!("RabbitMQ: HTTP {} {}", status, text.trim()));
    }
    serde_json::from_str(&text).map_err(|e| format!("RabbitMQ 响应解析失败: {e}"))
}

/// 一条浏览到的消息
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMsg {
    pub exchange: String,
    pub routing_key: String,
    pub redelivered: bool,
    pub payload: Option<String>,
    pub payload_bytes: u64,
    pub ts: i64,
}

/// 发布消息到 exchange(routing_key 路由;默认交换机 "" 时 routing_key=队列名)
pub async fn publish(
    st: &RabbitState,
    vhost: &str,
    exchange: &str,
    routing_key: &str,
    payload: &str,
) -> Result<(), String> {
    // 默认交换机语义:routing_key=队列名;队列不存在时消息被静默丢弃——
    // 先幂等声明(durable)再发
    if exchange.is_empty() {
        let body = serde_json::json!({"durable": true, "auto_delete": false});
        put_json(
            st,
            &format!("/api/queues/{}/{}", vhost_enc(vhost), urlenc(routing_key)),
            &body,
        )
        .await?;
    }
    let body = serde_json::json!({
        "properties": {"content_type": "text/plain", "delivery_mode": 2},
        "routing_key": routing_key,
        "payload": payload,
        "payload_encoding": "string"
    });
    post_json(
        st,
        &format!(
            "/api/exchanges/{}/{}/publish",
            vhost_enc(vhost),
            urlenc(exchange)
        ),
        &body,
    )
    .await
}

/// 清空队列(purge;破坏性,调用方二次确认)
pub async fn purge_queue(st: &RabbitState, vhost: &str, queue: &str) -> Result<(), String> {
    // rabbitmq 4.4:清空走 DELETE /api/queues/{vhost}/{name}/contents
    // (旧的 POST .../purge 端点已移除,4.4.1 实测 405)
    let mut req = st
        .http
        .delete(api_url(
            &st.base,
            &format!(
                "/api/queues/{}/{}/contents",
                vhost_enc(vhost),
                urlenc(queue)
            ),
        ));
    if let Some((u, p)) = &st.auth {
        req = req.basic_auth(u, Some(p));
    }
    let resp = req.send().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("RabbitMQ: {e}"))?;
    if !status.is_success() {
        return Err(format!("RabbitMQ: HTTP {} {}", status, text.trim()));
    }
    Ok(())
}
