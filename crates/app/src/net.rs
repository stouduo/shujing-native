//! 异步桥:egui 帧循环(同步)+ tokio 后台任务(数据库 IO)。
//! 任务结果经 mpsc 投递,update() 每帧消费并 request_repaint。

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("tokio runtime"))
}

/// 在后台 tokio 线程上执行 future(不阻塞 UI 帧)
pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime().spawn(future);
}
