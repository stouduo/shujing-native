//! Service 数据源方法分包:按数据源前缀拆分 impl 块(批次32 分包)。
//! 每文件 = `impl Service` 的一组同源方法;lib.rs 仅保留通用方法。

#[cfg(feature = "heavy-mw")]
mod kafka;
mod kv;
#[cfg(feature = "heavy-mw")]
mod mongo;
#[cfg(feature = "heavy-mw")]
mod mqtt;
mod rabbit;
mod redis;
