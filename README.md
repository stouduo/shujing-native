# 数镜(shujing-native)

数镜的 Rust 原生版本:egui + eframe,无 WebView。目标:低内存(数据量增大时占用有上界)、原生渲染。

- `crates/core` — 核心库(自 dbclient/src-tauri 迁移):连接池(MySQL/PG/SQLite/Redis)、SQL 执行、编辑回写、结构查询、全局搜索
- `crates/app` — egui 界面

## 运行

```bash
cargo run --release -p shujing-app
# 压测:启动即连接并打开测试库全部表
SHUJING_AUTOOPEN=/tmp/shujing_big.db cargo run --release -p shujing-app
```

## 界面

v0.4.0 起界面按高保真原型重做,统一设计令牌(深浅双主题):

- 色板「石墨蓝 / 雾白」:背景 `#0C0E12` / 卡片 `#15181E` / 凸起 `#1F232A` / 分隔 `#2A2F38`,
  强调 `#5B8CFF`、成功 `#3DD68C`、危险 `#EF4444`;浅色对应 `#F4F6F9` / `#FFFFFF` / `#EAEDF2` / `#D9DEE6` / `#3B82F6`
- 字体:拉丁 SF Pro / 粗体 Helvetica Neue Bold / 中文冬青黑体 W3·W6 / 数据 Menlo
- 节奏:行高 32、工具条 44、标签栏 38、列头 36、状态栏 34;圆角 6/8/9/12
- 结构:侧栏连接树(搜索/展开收起/选中药丸)、标签栏、表工具条(筛选/刷新/更多/保存/丢弃)、
  虚拟化数据网格(排序、状态徽章、NULL 弱化、长文本省略、整行选中)、状态栏分页、弹窗统一 Modal

界面截图见 `docs/ui/`(由 `cargo test -p shujing-app render_` 离屏渲染生成),
其中 `compare_dark.png` / `compare_light.png` 是与原型的并排对比。

## 状态

- M0 ✓ workspace + core 抽取(12 测试)
- M1 ✓ 连接管理、库/表树、虚拟化数据网格 + 分页 + 排序、CSV 导入/导出、编辑回写
- M2 ✓ UI 全面对齐原型(设计令牌 / 深浅主题 / 侧栏 / 标签 / 工具条 / 状态栏 / 弹窗)
- v0.5–v0.9 ✓ ER 图、表设计器、Redis 面板、筛选栏、SQL 导入/导出、外观强调色等扩展页(沿用同一设计令牌)
- v0.9.3 ✓ 数镜应用图标、侧栏分隔/间距、标签栏滚动隔离、数据表常驻横向滚动条
- v0.10.3 ✓ 窄窗口响应式工具栏、查询结果网格滚动条、行详情浮层收窄、结构页 DDL 默认展开

## 验收

- `cargo test -p shujing-app` 覆盖离屏渲染 + 关键交互(翻页/拖拽/关闭标签/会话恢复等)
- 当前 v0.10.3 的界面截图与原型对比见 `docs/ui/compare_dark.png`、`compare_light.png`
