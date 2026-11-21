//! 数镜 MCP Server:stdio 上的 JSON-RPC 2.0(Model Context Protocol)。
//!
//! 让 AI 编码代理(Claude Code / Cursor / Windsurf…)直接查询数镜里已配置的连接:
//! 列连接/库/表/结构、执行 SQL(默认只读白名单,写模式需显式开启)。
//!
//! 配置(环境变量):
//! - SHUJING_MCP_MODE: read_only(默认)| safe_write(仅 DML)| high_risk_write(全放行;旧值 write 兼容映射)
//! - SHUJING_MCP_CONNS: 逗号分隔的连接 id 白名单(空=全部)
//! - SHUJING_CONFIG_DIR: 配置目录(默认 ~/.shujing-native,与桌面版一致)
//!
//! .mcp.json 示例:
//! { "mcpServers": { "shujing": { "command": "/Applications/数镜.app/Contents/MacOS/shujing-mcp" } } }

use serde_json::{json, Value};
use shujing_core::Service;
use std::io::{BufRead, Write};

struct Mcp {
    service: Service,
    mode: String,          // read_only | write
    allow_conns: Vec<String>, // 空=全部
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("SHUJING_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{home}/.shujing-native"));
    let _ = std::fs::create_dir_all(&config_dir);
    let mut mode = std::env::var("SHUJING_MCP_MODE").unwrap_or_else(|_| "read_only".into());
    // 旧配置兼容:write → high_risk_write
    if mode == "write" {
        mode = "high_risk_write".into();
    }
    if !matches!(mode.as_str(), "read_only" | "safe_write" | "high_risk_write") {
        eprintln!("[shujing-mcp] 未知的 SHUJING_MCP_MODE={mode},按 read_only 处理");
        mode = "read_only".into();
    }
    let allow_conns: Vec<String> = std::env::var("SHUJING_MCP_CONNS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mcp = Mcp {
        service: Service::new(&config_dir),
        mode,
        allow_conns,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        // 通知(id 为空)不回包
        let Some(id) = id else { continue };
        let result = rt.block_on(handle(&mcp, method, &params));
        let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let mut enc = serde_json::to_string(&resp).unwrap_or_default();
        enc.push('\n');
        if out.write_all(enc.as_bytes()).is_err() || out.flush().is_err() {
            break;
        }
    }
}

async fn handle(mcp: &Mcp, method: &str, params: &Value) -> Value {
    match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "shujing-mcp", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, is_err) = call_tool(mcp, name, &args).await;
            json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_err
            })
        }
        _ => json!({}), // 未知方法:按 MCP 惯例返回空结果(避免代理端硬错)
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "list_connections",
            "description": "列出数镜中已配置的数据库连接(id/名称/类型)。后续调用的 conn 参数用这里的 id。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_databases",
            "description": "列出指定连接下的数据库(MySQL 类)。",
            "inputSchema": { "type": "object", "required": ["conn"],
                "properties": { "conn": { "type": "string" } } }
        },
        {
            "name": "list_tables",
            "description": "列出库中的表/视图(name 与 kind)。",
            "inputSchema": { "type": "object", "required": ["conn"],
                "properties": {
                    "conn": { "type": "string" },
                    "db": { "type": "string", "description": "不填用连接默认库" }
                } }
        },
        {
            "name": "describe_table",
            "description": "查看表结构:列(类型/可空/默认/注释)/索引/建表 DDL。",
            "inputSchema": { "type": "object", "required": ["conn", "table"],
                "properties": {
                    "conn": { "type": "string" },
                    "db": { "type": "string" },
                    "table": { "type": "string" }
                } }
        },
        {
            "name": "query",
            "description": "执行 SQL 并返回结果(文本表格)。默认只读模式只放行 SELECT/SHOW/DESC/PRAGMA 查询;写语句需服务端以 SHUJING_MCP_MODE=write 启动并用 execute 工具。",
            "inputSchema": { "type": "object", "required": ["conn", "sql"],
                "properties": {
                    "conn": { "type": "string" },
                    "db": { "type": "string", "description": "会话库(多库连接必填)" },
                    "sql": { "type": "string" },
                    "max_rows": { "type": "integer", "description": "默认 50" }
                } }
        },
        {
            "name": "execute",
            "description": "执行写语句(INSERT/UPDATE/DELETE/DDL)。仅当服务端以 SHUJING_MCP_MODE=write 启动时可用。",
            "inputSchema": { "type": "object", "required": ["conn", "sql"],
                "properties": {
                    "conn": { "type": "string" },
                    "db": { "type": "string" },
                    "sql": { "type": "string" }
                } }
        }
    ])
}

fn text_result(v: Value) -> (String, bool) {
    (serde_json::to_string_pretty(&v).unwrap_or_default(), false)
}
fn text_err(msg: String) -> (String, bool) {
    (format!("错误: {msg}"), true)
}

fn s<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

async fn call_tool(mcp: &Mcp, name: &str, args: &Value) -> (String, bool) {
    match name {
        "list_connections" => {
            let conns: Vec<Value> = mcp
                .service
                .list_saved_connections()
                .iter()
                .filter(|c| mcp.allow_conns.is_empty() || mcp.allow_conns.contains(&c.id))
                .map(|c| {
                    json!({
                        "id": c.id, "name": c.name,
                        "type": format!("{:?}", c.db_type).to_lowercase(),
                        "host": c.host, "database": c.database,
                    })
                })
                .collect();
            text_result(json!({ "connections": conns }))
        }
        _ => {
            // 其余工具都需要 conn
            let Some(conn) = s(args, "conn") else {
                return text_err("缺少 conn 参数(先 list_connections 取 id)".into());
            };
            if !mcp.allow_conns.is_empty() && !mcp.allow_conns.iter().any(|c| c == conn) {
                return text_err(format!("连接 {conn} 不在允许清单(SHUJING_MCP_CONNS)"));
            }
            let db = s(args, "db");
            match name {
                "list_databases" => match mcp.service.connect_by_id(conn).await {
                    Ok(()) => match mcp.service.list_databases(conn).await {
                        Ok(dbs) => text_result(json!({ "databases": dbs })),
                        Err(e) => text_err(e),
                    },
                    Err(e) => text_err(e),
                },
                "list_tables" => match mcp.service.connect_by_id(conn).await {
                    Ok(()) => match mcp.service.list_tables(conn, db.map(str::to_string)).await {
                        Ok(tables) => text_result(json!({ "tables": tables
                            .into_iter().map(|t| json!({"name": t.name, "kind": t.kind})).collect::<Vec<_>>() })),
                        Err(e) => text_err(e),
                    },
                    Err(e) => text_err(e),
                },
                "describe_table" => {
                    let Some(table) = s(args, "table") else {
                        return text_err("缺少 table 参数".into());
                    };
                    match mcp.service.connect_by_id(conn).await {
                        Ok(()) => match mcp
                            .service
                            .get_table_structure(conn, table, db.map(str::to_string))
                            .await
                        {
                            Ok(st) => text_result(serde_json::to_value(&st).unwrap_or_default()),
                            Err(e) => text_err(e),
                        },
                        Err(e) => text_err(e),
                    }
                }
                "query" | "execute" => {
                    let Some(sql) = s(args, "sql") else {
                        return text_err("缺少 sql 参数".into());
                    };
                    // 三档权限:read_only(只读) / safe_write(仅 DML) / high_risk_write(全放行)
                    if name == "execute" && mcp.mode == "read_only" {
                        return text_err(
                            "服务端为只读模式(SHUJING_MCP_MODE=read_only),写语句被拒绝;\
                             需要 DML 以 safe_write 启动,需要 DDL 以 high_risk_write 启动"
                                .into(),
                        );
                    }
                    if name == "execute"
                        && mcp.mode == "safe_write"
                        && shujing_core::sql_write_kind(sql) != Some("dml")
                    {
                        return text_err(
                            "safe_write 模式仅放行 DML(INSERT/UPDATE/DELETE/REPLACE);\
                             DDL(CREATE/DROP/ALTER/TRUNCATE 等)需要 SHUJING_MCP_MODE=high_risk_write"
                                .into(),
                        );
                    }
                    if name == "query"
                        && mcp.mode == "read_only"
                        && !shujing_core::is_readonly_sql(sql)
                    {
                        return text_err(
                            "只读模式仅放行查询语句(SELECT/SHOW/DESC/PRAGMA 查询);写操作需要服务端以 SHUJING_MCP_MODE=safe_write(DML)或 high_risk_write(全部)启动并改用 execute 工具".into(),
                        );
                    }
                    let max_rows = args.get("max_rows").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                    match mcp.service.connect_by_id(conn).await {
                        Ok(()) => {
                            match mcp
                                .service
                                .run_sql_db(conn, sql.to_string(), Some(max_rows), db)
                                .await
                            {
                                Ok(results) => {
                                    // 多结果集逐个渲染为文本表格
                                    let mut out = String::new();
                                    for (i, r) in results.iter().enumerate() {
                                        if !r.columns.is_empty() {
                                            render_table(&r.columns, &r.rows, &mut out);
                                        }
                                        if results.len() > 1 {
                                            out.push_str(&format!(
                                                "\n[结果集 {}] {} 行, 耗时 {}ms\n",
                                                i + 1,
                                                r.rows.len(),
                                                r.elapsed_ms
                                            ));
                                        } else {
                                            out.push_str(&format!(
                                                "\n[{} 行, 耗时 {}ms]\n",
                                                r.rows.len(),
                                                r.elapsed_ms
                                            ));
                                        }
                                    }
                                    (out.trim().to_string(), false)
                                }
                                Err(e) => text_err(e),
                            }
                        }
                        Err(e) => text_err(e),
                    }
                }
                _ => text_err(format!("未知工具: {name}")),
            }
        }
    }
}

fn render_table(columns: &[String], rows: &[Vec<Option<String>>], out: &mut String) {
    // 紧凑文本表:列宽按内容截断到 24
    let clip = |s: &str| -> String {
        let t: String = s.chars().take(24).collect();
        if s.chars().count() > 24 { format!("{t}…") } else { t }
    };
    let heads: Vec<String> = columns.iter().map(|c| clip(c)).collect();
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| clip(c.as_deref().unwrap_or("NULL"))).collect())
        .collect();
    let mut widths = heads.iter().map(|h| h.chars().count()).collect::<Vec<_>>();
    for row in &body {
        for (i, c) in row.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let line = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<w$}", c, w = widths[i]))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    out.push_str(&line(&heads));
    out.push('\n');
    out.push_str(&widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("-+-"));
    out.push('\n');
    for row in &body {
        out.push_str(&line(row));
        out.push('\n');
    }
}
