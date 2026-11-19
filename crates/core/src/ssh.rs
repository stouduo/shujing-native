//! SSH 隧道:本地随机端口 ←双向转发→ SSH 服务器 direct-tcpip → 目标数据库端口。
//! 任何 DB 驱动都透明走隧道(它们只看到 127.0.0.1:local_port)。

use crate::model::SshConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct SshClient;

impl russh::client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // v1 不做 known_hosts 校验(首次连接即信任);后续可加 TOFU 记忆
        Ok(true)
    }
}

/// 隧道句柄:Drop 即关闭(accept 循环退出 → SSH 会话断 → 各转发连接自然终结)
pub struct TunnelGuard {
    pub local_port: u16,
    pub cfg: SshConfig,
    alive: Arc<AtomicBool>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl TunnelGuard {
    /// SSH 会话是否仍存活(accept 循环还在跑)
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
    pub fn matches(&self, cfg: &SshConfig) -> bool {
        &self.cfg == cfg
    }
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

/// 建立隧道:连接 SSH 服务器并认证,在本地随机端口起监听,每个进来的 TCP
/// 连接开一条 direct-tcpip 通道双向拷贝到 (target_host, target_port)
pub async fn open_tunnel(
    cfg: &SshConfig,
    target_host: &str,
    target_port: u16,
) -> Result<TunnelGuard, String> {
    let es = |e: russh::Error| format!("SSH: {e}");
    let ssh_config = Arc::new(russh::client::Config::default());
    let mut handle = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        russh::client::connect(ssh_config, (cfg.host.as_str(), cfg.port), SshClient),
    )
    .await
    .map_err(|_| "SSH: 连接跳板机超时(10s)".to_string())?
    .map_err(es)?;

    let user = cfg.user.as_str();
    let authed = match (&cfg.key_path, &cfg.password) {
        (Some(path), passphrase) => {
            let key = russh::keys::load_secret_key(path, passphrase.as_deref())
                .map_err(|e| format!("SSH 私钥读取失败: {e}"))?;
            let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
            handle
                .authenticate_publickey(user, key_with_hash)
                .await
                .map_err(es)?
        }
        (None, Some(pw)) => handle.authenticate_password(user, pw).await.map_err(es)?,
        (None, None) => return Err("SSH: 需要密码或私钥".into()),
    };
    if !authed.success() {
        return Err("SSH: 认证失败(检查用户名/密码/私钥口令)".into());
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("SSH: 本地端口绑定失败: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("SSH: {e}"))?
        .port();

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let alive = Arc::new(AtomicBool::new(true));
    let alive_loop = alive.clone();
    let t_host = target_host.to_string();
    let sess = handle; // 会话句柄归转发循环:循环活着会话就活着
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                acc = listener.accept() => match acc {
                    Ok((mut tcp, _)) => {
                        let chan = sess
                            .channel_open_direct_tcpip(
                                t_host.clone(),
                                target_port as u32,
                                "127.0.0.1",
                                0u32,
                            )
                            .await;
                        match chan {
                            Ok(channel) => {
                                let mut stream = channel.into_stream();
                                tokio::spawn(async move {
                                    // 拷贝任一方向结束(会话断/DB 断)即回收
                                    let _ =
                                        tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                                });
                            }
                            Err(_) => {
                                // 目标不可达或会话已断:丢弃即关闭这条本地连接
                                drop(tcp);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        alive_loop.store(false, Ordering::Relaxed);
    });

    Ok(TunnelGuard {
        local_port,
        cfg: cfg.clone(),
        alive,
        shutdown: shutdown_tx,
    })
}
