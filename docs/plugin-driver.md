# 数镜驱动插件契约(v2)

数镜不再为"无 Rust 客户端"的数据库内置驱动;使用者可以**自己实现或安装驱动插件**
接入任意数据库(Oracle / 达梦 / RocketMQ 控制台 / 任何 agent 能解释的东西)。

## 安装(使用者)

把插件目录放进 `~/.shujing-native/plugins/`:

```
~/.shujing-native/plugins/
└── oracle-driver/
    ├── plugin.toml      # 清单
    └── agent            # 可执行文件(任何语言:Rust/Go/Python/脚本…自带运行时)
```

在数镜里新建连接:类型选 **Plugin**,「驱动插件」填清单里的 `name`,按插件的
说明填 host/port/user/password/database。

## 清单 plugin.toml

```toml
name = "oracle-driver"        # 插件名(连接里引用它)
kind = "driver"               # 驱动型插件
version = "0.1.0"
description = "Oracle 连接(oracle 实例)"
db_types = ["oracle"]         # 本插件提供的自定义类型名(可多个)
exec = "./agent"              # 可执行文件,相对插件目录
```

## 运行时契约

宿主 spawn `exec`(工作目录=插件目录),**stdin/stdout 上每行一个 JSON-RPC 2.0**
(与数镜 MCP Server 同构)。方法集:

| 方法 | 参数 | 返回 result |
|---|---|---|
| ping | `{}` | `{ok:true}` |
| connect | `{host,port,user,password,database}` | `{version:"..."}` |
| databases | `{}` | `{databases:["db1",...]}` |
| tables | `{database}` | `{tables:[{name,kind:"table"\|"view",comment}]}` |
| structure | `{database,table}` | `{columns:[{name,type,nullable,default,comment,pk}],pk:["col"]}` |
| query | `{database,sql,max}` | `{columns:[..],rows:[[字符串..]..],affected,truncated}` |
| count | `{database,table}` | `{count:n}` |

- 值一律字符串化(与数镜网格 text 协议对齐);NULL = JSON null
- 错误:`{"error":{"message":"..."}}`;数镜把 message 展示给用户
- 连接参数语义由 **agent 自己解释**(Oracle agent 拼 thin URL、RocketMQ agent 用
  addr……),数镜零方言知识
- 生命周期:每个数镜连接 = 一个 agent 进程;数镜断开即 kill

## 最小示例

参考 `crates/core/tests/plugins-sample-driver/`(python,内存假数据)——
`cargo run -p shujing-core --example e2e_plugin` 验证全链路(握手/库/表/结构/
查询/计数/重启恢复)。

## 安全与边界

- agent 以数镜同用户权限运行;它能访问什么由它自己的实现决定——**只安装可信插件**
- v1 契约不含表格回写/数据传输/备份(agent 未声明事务语义);SQL 直接执行可用
- v1 无签名/商店;第三方分发渠道请自行校验哈希
