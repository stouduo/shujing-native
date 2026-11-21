#![cfg(feature = "heavy-mw")]

use crate::backend_dispatch;
use crate::{model, Service};

impl Service {
    pub async fn kafka_list_topics(
        &self,
        id: &str,
    ) -> Result<Vec<crate::kafka::TopicInfo>, String> {
        backend_dispatch!(self, id, Kafka(st) => {
        crate::kafka::list_topics(st).await
                })
    }

    /// Kafka:从头浏览消息

    pub async fn kafka_consume_recent(
        &self,
        id: &str,
        topic: &str,
        count: usize,
        timeout_secs: u64,
    ) -> Result<Vec<crate::kafka::StreamMsg>, String> {
        backend_dispatch!(self, id, Kafka(st) => {
                crate::kafka::consume_recent(&st.addr, topic, count, timeout_secs).await
        })
    }

    /// Kafka:生产消息

    pub async fn kafka_produce(
        &self,
        id: &str,
        topic: &str,
        key: Option<&str>,
        payload: &str,
    ) -> Result<(i32, i64), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_dml() {
            return Err("连接权限不足:发送消息需要非只读权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Kafka(st) => {
                crate::kafka::produce(st, topic, key, payload).await
            }
            _ => Err("非 Kafka 连接".into()),
        }
    }

    /// Kafka:新建 topic

    pub async fn kafka_create_topic(
        &self,
        id: &str,
        name: &str,
        partitions: i32,
        replication: i32,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_ddl() {
            return Err("连接权限不足:建 topic 是结构变更,需要完整权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Kafka(st) => {
                crate::kafka::create_topic(st, name, partitions, replication).await
            }
            _ => Err("非 Kafka 连接".into()),
        }
    }

    /// Kafka:删除 topic(破坏性)

    pub async fn kafka_delete_topic(&self, id: &str, name: &str) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_ddl() {
            return Err("连接权限不足:删 topic 是结构变更,需要完整权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Kafka(st) => crate::kafka::delete_topic(st, name).await,
            _ => Err("非 Kafka 连接".into()),
        }
    }

    /// Redis:服务器 INFO(分区解析)

    pub async fn kafka_groups(&self, id: &str) -> Result<Vec<crate::kafka::GroupInfo>, String> {
        backend_dispatch!(self, id, Kafka(st) => {
        crate::kafka::list_groups(st).await
                })
    }
}
