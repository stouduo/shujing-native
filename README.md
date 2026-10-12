# 数镜 Native(shujing-native)

数镜的 Rust 原生版本:egui + eframe,无 WebView。目标:低内存(数据量增大时占用有上界)、原生渲染。

- `crates/core` — 核心库(自 dbclient/src-tauri 迁移):连接池(MySQL/PG/SQLite/Redis)、SQL 执行、编辑回写、结构查询、全局搜索
- `crates/app` — egui 界面

## 运行

```bash
cargo run --release -p shujing-app
# 压测:启动即连接并打开测试库全部表
SHUJING_AUTOOPEN=/tmp/shujing_big.db cargo run --release -p shujing-app
```

## 状态

- M0 ✓ workspace + core 抽取(12 测试)
- M1 进行中:连接管理 ✓、库/表树 ✓、虚拟化数据网格+分页+排序 ✓、CSV 导出 ✓;编辑回写进行中
