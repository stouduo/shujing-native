#![cfg(feature = "heavy-mw")]

use crate::backend_dispatch;
use crate::{model, Service};

impl Service {
    pub async fn mqtt_browse(
        &self,
        id: &str,
        secs: u64,
    ) -> Result<Vec<crate::mqtt::MqttMsg>, String> {
        backend_dispatch!(self, id, Mqtt(st) => {
        crate::mqtt::browse_snapshot(st, secs).await
                })
    }

    /// MQTT:发布一条(可 retain)

    pub async fn mqtt_publish(
        &self,
        id: &str,
        topic: &str,
        payload: &str,
        retain: bool,
    ) -> Result<usize, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_dml() {
            return Err("连接权限不足:发布消息需要非只读权限".into());
        }
        match &mut guard.backend {
            crate::model::Backend::Mqtt(st) => {
                crate::mqtt::publish(st, topic, payload, retain).await
            }
            _ => Err("非 MQTT 连接".into()),
        }
    }
}
