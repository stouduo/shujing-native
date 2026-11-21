use crate::backend_dispatch;
use crate::{model, redis_ops, Service};

impl Service {
    pub async fn redis_delete_by_pattern(
        &self,
        id: &str,
        db: u8,
        pattern: &str,
    ) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                crate::redis_ops::delete_by_pattern(&mut guard.backend, db, pattern).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// Kafka:topic 列表

    pub async fn redis_server_info(
        &self,
        id: &str,
    ) -> Result<Vec<crate::redis_ops::InfoSection>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                crate::redis_ops::server_info(&mut guard.backend).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// Redis:慢日志最近 n 条

    pub async fn redis_slowlog(
        &self,
        id: &str,
        n: i64,
    ) -> Result<Vec<crate::redis_ops::SlowEntry>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                crate::redis_ops::slowlog(&mut guard.backend, n).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// RabbitMQ:概览

    pub async fn redis_stream_xadd(
        &self,
        id: &str,
        db: u8,
        key: &str,
        fields: &[(String, String)],
    ) -> Result<String, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                if !guard.info.perm().can_dml() {
                    return Err("只读连接不允许写入".into());
                }
                crate::redis_ops::stream_xadd(&mut guard.backend, db, key, fields).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// Redis:stream 消费者组列表

    pub async fn redis_stream_groups(
        &self,
        id: &str,
        db: u8,
        key: &str,
    ) -> Result<Vec<crate::redis_ops::StreamGroup>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                crate::redis_ops::stream_groups(&mut guard.backend, db, key).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// Redis:按模式 SCAN 计数(不删)

    pub async fn redis_scan_count(&self, id: &str, db: u8, pattern: &str) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        match guard.backend {
            crate::model::Backend::Redis(_) => {
                crate::redis_ops::scan_count(&mut guard.backend, db, pattern).await
            }
            _ => Err("非 Redis 连接".into()),
        }
    }

    /// MongoDB:按 _id 取文档(pretty JSON,编辑器预填)

    pub async fn redis_scan(
        &self,
        id: &str,
        db: u8,
        pattern: &str,
        cursor: u64,
        count: usize,
    ) -> Result<(u64, Vec<String>), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::scan_keys(&mut guard.backend, db, pattern, cursor, count).await
    }

    pub async fn redis_key_detail(
        &self,
        id: &str,
        db: u8,
        key: &str,
    ) -> Result<redis_ops::RedisDetail, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::key_detail(&mut guard.backend, db, key).await
    }

    /// Redis 详情追加一页(list/zset 分页)

    pub async fn redis_key_detail_more(
        &self,
        id: &str,
        db: u8,
        key: &str,
        offset: usize,
    ) -> Result<redis_ops::RedisDetail, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::key_detail_page(&mut guard.backend, db, key, offset).await
    }

    pub async fn redis_key_types(
        &self,
        id: &str,
        db: u8,
        keys: &[String],
    ) -> Result<Vec<String>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::key_types(&mut guard.backend, db, keys).await
    }

    pub async fn redis_del(&self, id: &str, db: u8, key: &str) -> Result<u64, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if !guard.info.perm().can_dml() {
            return Err("只读连接不允许删除".into());
        }
        redis_ops::del_key(&mut guard.backend, db, key).await
    }

    pub async fn redis_set(&self, id: &str, db: u8, key: &str, value: &str) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许写入".into());
        }
        redis_ops::set_string(&mut guard.backend, db, key, value, true).await
    }

    pub async fn redis_set_ttl(
        &self,
        id: &str,
        db: u8,
        key: &str,
        seconds: i64,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许修改 TTL".into());
        }
        redis_ops::set_ttl(&mut guard.backend, db, key, seconds).await
    }

    pub async fn redis_rename(
        &self,
        id: &str,
        db: u8,
        key: &str,
        new_key: &str,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许重命名".into());
        }
        redis_ops::rename(&mut guard.backend, db, key, new_key).await
    }

    pub async fn redis_ttl_batch(
        &self,
        id: &str,
        db: u8,
        keys: &[String],
    ) -> Result<Vec<i64>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::ttl_batch(&mut guard.backend, db, keys).await
    }

    pub async fn redis_new_key(
        &self,
        id: &str,
        db: u8,
        key: &str,
        key_type: &str,
        text: &str,
        pairs: &[(String, String)],
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许新建".into());
        }
        redis_ops::new_key(&mut guard.backend, db, key, key_type, text, pairs).await
    }

    pub async fn redis_member_op(
        &self,
        id: &str,
        db: u8,
        key: &str,
        key_type: &str,
        op: &str,
        member: &str,
        extra: &str,
    ) -> Result<(), String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        if guard.info.read_only {
            return Err("只读连接不允许修改".into());
        }
        redis_ops::member_op(&mut guard.backend, db, key, key_type, op, member, extra).await
    }

    pub async fn redis_analyze(
        &self,
        id: &str,
        db: u8,
        sample: usize,
        mode: &str,
    ) -> Result<Vec<redis_ops::KeyStat>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        redis_ops::analyze(&mut guard.backend, db, sample.min(20000), mode).await
    }

    pub async fn redis_run(&self, id: &str, command: &str) -> Result<Vec<String>, String> {
        let live = self.state.live(id).ok_or("连接未建立或已断开")?;
        let mut guard = live.lock().await;
        let info = guard.info.clone();
        redis_ops::run_command(&mut guard.backend, &info, command).await
    }
}
