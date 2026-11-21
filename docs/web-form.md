# Web 形态可行性结论(批次42,2026-11-21)

## 验证结果

1. **渲染层可切换 ✅**:app 代码零 glow 直接引用;eframe 切 `wgpu` 后 `cargo check` 通过(22.9s)。
   已固化为 `renderer-glow`(默认)/ `renderer-wgpu` 双 feature。
2. **数据层不可直连 ❌**:`cargo check -p shujing-core --target wasm32-unknown-unknown` 失败于
   mio(tokio 网络底层)——浏览器没有 TCP/Unix socket,mysql_async / tokio-postgres /
   mongodb / redis / rdkafka 等 native 驱动**无法在 wasm 里编译**,更谈不上运行。

## 结论

「Web 形态」= 浏览器前端 + **Rust 后端服务**(复用 core)。这是架构级立项,不是渲染层切换:

```
浏览器(egui wasm,renderer-wgpu)
   │  HTTP/WebSocket(JSON)
   axum 服务端(复用 crates/core 全部逻辑,Docker 打包)
   │  native TCP
   MySQL / PG / Redis / Mongo / ...
```

工作量评估(供立项参考):
- axum API 层:连接 CRUD / 元数据 / SQL 执行 / 事件流 —— 约 2-3 周
- 前端远程模式(net.rs wasm 分支:fetch/WebSocket 替换 tokio)—— 约 2 周
- 认证/多用户/部署(Docker 镜像)—— 约 1 周
- 交付形态:`shujing-server` 二进制 + wasm 静态资源,Docker 一键部署

可行但属独立立项;桌面端不受影响。
