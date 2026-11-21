use crate::backend_dispatch;
use crate::{model, Service};

impl Service {
    pub async fn rabbit_overview(&self, id: &str) -> Result<crate::rabbit::RabbitOverview, String> {
        backend_dispatch!(self, id, Rabbit(st) => {
        crate::rabbit::overview(st).await
                })
    }

    /// RabbitMQ:单队列详情(统计准)

    pub async fn rabbit_queue_detail(
        &self,
        id: &str,
        vhost: &str,
        queue: &str,
    ) -> Result<crate::rabbit::RabbitQueue, String> {
        backend_dispatch!(self, id, Rabbit(st) => {
                crate::rabbit::queue_detail(st, vhost, queue).await
        })
    }

    /// RabbitMQ:队列列表

    pub async fn rabbit_list_queues(
        &self,
        id: &str,
    ) -> Result<Vec<crate::rabbit::RabbitQueue>, String> {
        backend_dispatch!(self, id, Rabbit(st) => {
        crate::rabbit::list_queues(st).await
                })
    }

    /// RabbitMQ:从队列取回消息(requeue)

    pub async fn rabbit_get_messages(
        &self,
        id: &str,
        vhost: &str,
        queue: &str,
        count: usize,
    ) -> Result<Vec<crate::rabbit::StreamMsg>, String> {
        backend_dispatch!(self, id, Rabbit(st) => {
                crate::rabbit::get_messages(st, vhost, queue, count).await
        })
    }

    /// RabbitMQ:发布消息到 exchange(默认交换机+routing_key=队列名)

    pub async fn rabbit_publish(
        &self,
        id: &str,
        vhost: &str,
        queue: &str,
        payload: &str,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_dml() {
            return Err("连接权限不足:发送消息需要非只读权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Rabbit(st) => {
                crate::rabbit::publish(st, vhost, "", queue, payload).await
            }
            _ => Err("非 RabbitMQ 连接".into()),
        }
    }

    /// MQTT:主题采样浏览(订阅 # secs 秒)

    pub async fn rabbit_purge(&self, id: &str, vhost: &str, queue: &str) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_dml() {
            return Err("连接权限不足:清空队列需要非只读权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Rabbit(st) => crate::rabbit::purge_queue(st, vhost, queue).await,
            _ => Err("非 RabbitMQ 连接".into()),
        }
    }
}
