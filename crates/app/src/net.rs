//! 异步桥:egui 帧循环(同步)+ tokio 后台任务(数据库 IO)。
//! 任务结果经 mpsc 投递,update() 每帧消费并 request_repaint。

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2) // 纯 DB IO,不需要核数个线程;线程栈与调度器都占内存
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

/// 在后台 tokio 线程上执行 future(不阻塞 UI 帧)。
/// catch_unwind:tokio 把任务 panic 捕进 JoinHandle,句柄丢弃即被静默吞掉,
/// 对应 tab 会永远停在"读取中…"且无任何报错——panic 至少要落日志文件
pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime().spawn(async move {
        if let Err(p) = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(future)).await {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "未知 panic".into());
            let line = format!(
                "[{}] 后台任务 panic: {msg}\n",
                std::backtrace::Backtrace::force_capture()
            );
            eprint!("{line}");
            let path = std::env::var("HOME").unwrap_or_default() + "/shujing_panic.log";
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(line.as_bytes());
            }
        }
    });
}

/// 阻塞型任务(大文件解析等):丢进 blocking 池,不占用仅有的 2 个 async worker
pub fn spawn_blocking<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    runtime().spawn_blocking(f);
}
