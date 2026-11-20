//! Kafka 连接器(v1,消息队列方向第一批):元数据(topic 列表)/消息浏览/
//! 生产消息。rdkafka(cmake-build 编译 librdkafka),无 SQL 语义,
//! 连接走 ConnInfo(host=bootstrap 地址,port=9092)。
//! 消息浏览语义:assign 全分区从 earliest 读,凑满 count 条或超时返回——
//! 只读消费,不提交位移(auto.commit 关闭)。

use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Message};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::topic_partition_list::{Offset, TopicPartitionList};
use std::time::Duration;

/// Kafka 连接状态:producer/admin 常驻(自带连接池),consumer 按需临时建
pub struct KafkaState {
    pub addr: String,
    pub producer: FutureProducer,
    pub admin: rdkafka::admin::AdminClient<rdkafka::client::DefaultClientContext>,
}

fn base_config(addr: &str) -> ClientConfig {
    let mut c = ClientConfig::new();
    c.set("bootstrap.servers", addr);
    // 消息体可能有任意字节,取字符串失败时按 hex 兜底在 UI 层处理
    c.set("client.id", "shujing");
    c
}

/// 建立连接:fetch_metadata 作探活(建 client 本身不握手)
pub async fn connect(addr: &str) -> Result<KafkaState, String> {
    let producer = base_config(addr)
        .create::<FutureProducer>()
        .map_err(|e| format!("Kafka producer 创建失败: {e}"))?;
    let admin = base_config(addr)
        .create::<rdkafka::admin::AdminClient<rdkafka::client::DefaultClientContext>>()
        .map_err(|e| format!("Kafka admin 创建失败: {e}"))?;
    let st = KafkaState {
        addr: addr.to_string(),
        producer,
        admin,
    };
    list_topics(&st).await?; // 探活
    Ok(st)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicInfo {
    pub name: String,
    pub partitions: usize,
    /// 单分区副本数(粗略健康度)
    pub replicas: i32,
}

pub async fn list_topics(st: &KafkaState) -> Result<Vec<TopicInfo>, String> {
    let md = st
        .admin
        .inner()
        .fetch_metadata(None, Duration::from_secs(8))
        .map_err(|e| format!("Kafka 元数据获取失败: {e}"))?;
    let mut out: Vec<TopicInfo> = md
        .topics()
        .iter()
        .filter_map(|t| {
            // error topic(权限/删除中)跳过
            t.partitions().first().map(|p| TopicInfo {
                name: t.name().to_string(),
                partitions: t.partitions().len(),
                replicas: p.replicas().len() as i32,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 一条消息的网格化形态
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMsg {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub key: Option<String>,
    pub payload: Option<String>,
    pub ts_millis: i64,
    /// 展示用时间(mm-dd HH:MM:SS),构造时格式化
    pub time: String,
}

fn msg_to_grid(m: &BorrowedMessage<'_>) -> StreamMsg {
    let dec = |b: Option<&[u8]>| -> Option<String> {
        b.map(|bytes| match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => format!("<hex {}>", hex_short(bytes)),
        })
    };
    let ts_millis = m.timestamp().to_millis().unwrap_or(0);
    let time = if ts_millis > 0 {
        chrono::DateTime::from_timestamp_millis(ts_millis)
            .map(|d| d.format("%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    StreamMsg {
        topic: m.topic().to_string(),
        partition: m.partition(),
        offset: m.offset(),
        key: dec(m.key()),
        payload: dec(m.payload()),
        ts_millis,
        time,
    }
}

fn hex_short(bytes: &[u8]) -> String {
    let take = bytes.len().min(24);
    bytes[..take].iter().map(|b| format!("{b:02x}")).collect::<String>()
}

/// 浏览:临时 consumer,assign 全分区从 earliest 读,凑满 count 或超时返回
pub async fn consume_recent(
    addr: &str,
    topic: &str,
    count: usize,
    timeout_secs: u64,
) -> Result<Vec<StreamMsg>, String> {
    let consumer: StreamConsumer = base_config(addr)
        .set("group.id", "shujing-browse")
        .set("enable.auto.commit", "false")
        .set("enable.partition.eof", "true")
        .set("auto.offset.reset", "earliest")
        .create::<StreamConsumer>()
        .map_err(|e| format!("Kafka consumer 创建失败: {e}"))?;
    // 元数据拿分区数,全部 assign 到 Beginning
    let mut cfg = ClientConfig::new();
    cfg.set("bootstrap.servers", addr);
    let admin = cfg
        .create::<rdkafka::admin::AdminClient<rdkafka::client::DefaultClientContext>>()
        .map_err(|e| format!("Kafka admin 创建失败: {e}"))?;
    let md = admin
        .inner()
        .fetch_metadata(Some(topic), Duration::from_secs(8))
        .map_err(|e| format!("Kafka 元数据获取失败: {e}"))?;
    let mut tpl = TopicPartitionList::new();
    for t in md.topics() {
        for p in t.partitions() {
            tpl.add_partition_offset(t.name(), p.id(), Offset::Beginning)
                .map_err(|e| format!("Kafka assign 失败: {e}"))?;
        }
    }
    consumer.assign(&tpl).map_err(|e| format!("Kafka assign 失败: {e}"))?;
    let mut out = Vec::new();
    let deadline = tokio::time::Duration::from_secs(timeout_secs);
    let started = tokio::time::Instant::now();
    while out.len() < count {
        let left = match deadline.checked_sub(started.elapsed()) {
            Some(l) => l,
            None => break,
        };
        match tokio::time::timeout(left, consumer.recv()).await {
            Ok(Ok(m)) => out.push(msg_to_grid(&m)),
            Ok(Err(e)) => {
                // 分区读到末尾等常见软错误:超时即返回已收到的
                if out.is_empty() {
                    return Err(format!("Kafka 消费失败: {e}"));
                }
                break;
            }
            Err(_) => break, // 超时:已收集的消息返回
        }
    }
    Ok(out)
}

/// 生产一条消息(返回 (partition, offset))
pub async fn produce(
    st: &KafkaState,
    topic: &str,
    key: Option<&str>,
    payload: &str,
) -> Result<(i32, i64), String> {
    let mut rec = FutureRecord::to(topic).payload(payload);
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        rec = rec.key(k);
    }
    match st.producer.send(rec, Duration::from_secs(8)).await {
        Ok(delivery) => Ok((delivery.partition, delivery.offset)),
        Err((e, _)) => Err(format!("Kafka 发送失败: {e}")),
    }
}

/// 新建 topic(分区/副本为服务端语义;副本受 broker 集群规模约束)
pub async fn create_topic(
    st: &KafkaState,
    name: &str,
    partitions: i32,
    replication: i32,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("topic 名不能为空".into());
    }
    use rdkafka::admin::{AdminOptions, NewTopic, TopicReplication};
    let spec = NewTopic::new(
        name,
        partitions.max(1),
        TopicReplication::Fixed(replication.max(1)),
    );
    let opts = AdminOptions::new().request_timeout(Some(Duration::from_secs(8)));
    match st.admin.create_topics(&[spec], &opts).await {
        Ok(mut v) => match v.pop() {
            Some(Ok(_)) => Ok(()),
            Some(Err((e, _))) => Err(format!("创建失败: {e}")),
            None => Ok(()),
        },
        Err(e) => Err(format!("创建失败: {e}")),
    }
}

/// 删除 topic(破坏性;调用方须二次确认)
pub async fn delete_topic(st: &KafkaState, name: &str) -> Result<(), String> {
    use rdkafka::admin::AdminOptions;
    let opts = AdminOptions::new().request_timeout(Some(Duration::from_secs(8)));
    match st
        .admin
        .delete_topics(&[name], &opts)
        .await
    {
        Ok(mut v) => match v.pop() {
            Some(Ok(_)) => Ok(()),
            Some(Err((e, _))) => Err(format!("删除失败: {e}")),
            None => Ok(()),
        },
        Err(e) => Err(format!("删除失败: {e}")),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub name: String,
    pub state: String,
    pub members: i32,
}

/// 消费组列表(v1 只列名/状态/成员数,拉偏移详情后续)
pub async fn list_groups(st: &KafkaState) -> Result<Vec<GroupInfo>, String> {
    let gl = st
        .admin
        .inner()
        .fetch_group_list(None, Duration::from_secs(8))
        .map_err(|e| format!("Kafka 组列表获取失败: {e}"))?;
    Ok(gl
        .groups()
        .iter()
        .map(|g| GroupInfo {
            name: g.name().to_string(),
            state: g.state().to_string(),
            members: g.members().len() as i32,
        })
        .collect())
}
