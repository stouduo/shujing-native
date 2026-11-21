//! 配置中心 KV 方法(Nacos/Consul/Etcd 三类型共用)。

use crate::backend_dispatch;
use crate::Service;

impl Service {
    /// 列键(前缀过滤;空前缀=全部)
    pub async fn kv_list(&self, id: &str, prefix: &str) -> Result<Vec<crate::kv::KvEntry>, String> {
        backend_dispatch!(self, id, Kv(st) => {
            crate::kv::list(st, prefix).await
        })
    }

    /// 写键(只读连接拒绝)
    pub async fn kv_put(&self, id: &str, key: &str, value: &str) -> Result<(), String> {
        if self
            .list_saved_connections()
            .iter()
            .any(|c| c.id == id && c.read_only)
        {
            return Err("只读连接不允许写入".into());
        }
        backend_dispatch!(self, id, Kv(st) => {
            crate::kv::put(st, key, value).await
        })
    }

    /// 删键(只读连接拒绝)
    pub async fn kv_delete(&self, id: &str, key: &str) -> Result<(), String> {
        if self
            .list_saved_connections()
            .iter()
            .any(|c| c.id == id && c.read_only)
        {
            return Err("只读连接不允许删除".into());
        }
        backend_dispatch!(self, id, Kv(st) => {
            crate::kv::delete(st, key).await
        })
    }
}
