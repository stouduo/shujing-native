//! 数镜 CLI:命令行访问桌面端已配置的数据库连接(共用 ~/.shujing-native)。
//!
//! 子命令:
//!   connections                     列出已保存连接
//!   tables <连接> [库]              列出表
//!   query <连接> <SQL> [--db 库]    执行 SQL(表格输出;--json 机器可读)
//!
//! 全局: SHUJING_CONFIG_DIR 覆盖配置目录(默认 ~/.shujing-native)

use shujing_core::{ConnInfo, Service};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("SHUJING_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{home}/.shujing-native"));
    let service = Service::new(&config_dir);
    let conns = service.list_saved_connections();

    let Some(cmd) = args.first() else {
        print_help(&conns.len());
        std::process::exit(2);
    };
    let rest = &args[1..];
    let code = match cmd.as_str() {
        "connections" | "ls" => cmd_connections(&conns),
        "tables" => cmd_tables(&service, &conns, rest),
        "query" | "q" => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(cmd_query(&service, &conns, rest))
        }
        "-h" | "--help" | "help" => {
            print_help(&conns.len());
            0
        }
        other => {
            eprintln!("未知子命令: {other}(help 查看用法)");
            2
        }
    };
    std::process::exit(code);
}

fn print_help(n: &usize) {
    println!(
        "数镜 CLI(与桌面端共用配置:~/.shujing-native,当前 {n} 条连接)\n\n\
         USAGE:\n  \
         shujing-cli connections                列出连接\n  \
         shujing-cli tables <连接> [库]         列出表\n  \
         shujing-cli query <连接> <SQL> [--db 庐] [--json] [--max N]\n\n\
         <连接> 按 id 或名称匹配(名称唯一时可用);只读连接自动受限。"
    );
}

fn resolve<'a>(conns: &'a [ConnInfo], key: &str) -> Option<&'a ConnInfo> {
    conns
        .iter()
        .find(|c| c.id == key)
        .or_else(|| {
            let hits: Vec<&ConnInfo> = conns.iter().filter(|c| c.name == key).collect();
            if hits.len() == 1 {
                hits.into_iter().next()
            } else {
                None
            }
        })
        .or_else(|| {
            // 前缀匹配兜底
            let hits: Vec<&ConnInfo> = conns
                .iter()
                .filter(|c| c.name.starts_with(key) || c.id.starts_with(key))
                .collect();
            if hits.len() == 1 {
                hits.into_iter().next()
            } else {
                None
            }
        })
}

fn cmd_connections(conns: &[ConnInfo]) -> i32 {
    if conns.is_empty() {
        println!("(无已保存连接)");
        return 0;
    }
    println!("{:<20} {:<12} {:<28} {:<16} {}", "NAME", "TYPE", "HOST", "PORT", "DB");
    for c in conns {
        println!(
            "{:<20} {:<12} {:<28} {:<16} {}",
            c.name,
            match c.db_type {
                shujing_core::DbType::MySql => "mysql",
                shujing_core::DbType::Postgres => "postgres",
                shujing_core::DbType::Sqlite => "sqlite",
                shujing_core::DbType::Redis => "redis",
                shujing_core::DbType::Mongo => "mongodb",
                shujing_core::DbType::Mssql => "sqlserver",
                shujing_core::DbType::ClickHouse => "clickhouse",
                shujing_core::DbType::Kafka => "kafka",
                shujing_core::DbType::Rabbit => "rabbitmq",
                shujing_core::DbType::Mqtt => "mqtt",
                shujing_core::DbType::Nacos => "nacos",
                shujing_core::DbType::Consul => "consul",
                shujing_core::DbType::Etcd => "etcd",
                shujing_core::DbType::Plugin => "plugin",
            },
            c.host.as_deref().or(c.file_path.as_deref()).unwrap_or("-"),
            c.port.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            c.database.as_deref().unwrap_or("-")
        );
    }
    0
}

fn cmd_tables(service: &Service, conns: &[ConnInfo], rest: &[String]) -> i32 {
    let Some(key) = rest.first() else {
        eprintln!("用法: shujing-cli tables <连接> [库]");
        return 2;
    };
    let Some(c) = resolve(conns, key) else {
        eprintln!("连接不存在: {key}(connections 列出全部)");
        return 1;
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async {
        match service.connect(c.clone()).await {
            Err(e) => {
                eprintln!("连接失败: {e}");
                1
            }
            Ok(_) => match service
                .list_tables(&c.id, rest.get(1).cloned())
                .await
            {
                Ok(ts) => {
                    for t in ts {
                        println!("{}", t.name);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("列取失败: {e}");
                    1
                }
            },
        }
    })
}

async fn cmd_query(service: &Service, conns: &[ConnInfo], rest: &[String]) -> i32 {
    // 解析 flags
    let mut db: Option<String> = None;
    let mut json_out = false;
    let mut max: usize = 100;
    let mut pos: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--db" if i + 1 < rest.len() => {
                db = Some(rest[i + 1].clone());
                i += 2;
            }
            "--json" => {
                json_out = true;
                i += 1;
            }
            "--max" if i + 1 < rest.len() => {
                max = rest[i + 1].parse().unwrap_or(100);
                i += 2;
            }
            _ => {
                pos.push(rest[i].clone());
                i += 1;
            }
        }
    }
    let Some(key) = pos.first() else {
        eprintln!("用法: shujing-cli query <连接> <SQL> [--db 库] [--json] [--max N]");
        return 2;
    };
    let Some(c) = resolve(conns, key) else {
        eprintln!("连接不存在: {key}");
        return 1;
    };
    let Some(sql) = pos.get(1) else {
        eprintln!("缺少 SQL");
        return 2;
    };
    if let Err(e) = service.connect_by_id(&c.id).await {
        eprintln!("连接失败: {e}");
        return 1;
    }
    let started = Instant::now();
    let sql = sql.trim().trim_end_matches(';').to_string();
    let is_read = shujing_core::is_readonly_sql(&sql);
    let result = if is_read {
        service.run_sql_db(&c.id, sql.clone(), Some(max), db.as_deref()).await
    } else {
        // 写语句:仍走 run_sql_db(执行臂按方言处理);CLI 无确认交互,由用户自担
        service.run_sql_db(&c.id, sql.clone(), Some(1), db.as_deref()).await
    };
    match result {
        Err(e) => {
            eprintln!("执行失败: {e}");
            1
        }
        Ok(rs) => {
            let elapsed = started.elapsed().as_millis();
            if json_out {
                let out: Vec<serde_json::Value> = rs
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "columns": r.columns,
                            "rows": r.rows,
                            "affected": r.affected,
                            "truncated": r.truncated,
                            "elapsedMs": r.elapsed_ms,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
                return 0;
            }
            for r in &rs {
                if !r.columns.is_empty() {
                    let widths: Vec<usize> = r
                        .columns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let data_w = r
                                .rows
                                .iter()
                                .map(|row| {
                                    row.get(i)
                                        .and_then(|v| v.as_deref())
                                        .map(|s| display_width(s))
                                        .unwrap_or(4)
                                })
                                .max()
                                .unwrap_or(4);
                            display_width(c).max(data_w).min(40)
                        })
                        .collect();
                    let header: Vec<String> = r
                        .columns
                        .iter()
                        .enumerate()
                        .map(|(i, c)| pad(c, widths[i]))
                        .collect();
                    println!("{}", header.join("  "));
                    println!("{}", widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("  "));
                    for row in &r.rows {
                        let line: Vec<String> = r
                            .columns
                            .iter()
                            .enumerate()
                            .map(|(i, _)| {
                                let cell = row.get(i).and_then(|v| v.as_deref()).unwrap_or("NULL");
                                pad(cell, widths[i])
                            })
                            .collect();
                        println!("{}", line.join("  "));
                    }
                    println!(
                        "({} rows{})",
                        r.rows.len(),
                        if r.truncated { ", truncated" } else { "" }
                    );
                } else {
                    println!("OK affected={}", r.affected);
                }
            }
            eprintln!("-- {elapsed} ms --");
            0
        }
    }
}

/// 显示宽度(CJK 记 2)
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    let cp = c as u32;
    (0x1100..=0x115F).contains(&cp)
        || (0x2E80..=0xA4CF).contains(&cp)
        || (0xAC00..=0xD7A3).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE30..=0xFE4F).contains(&cp)
        || (0xFF00..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x20000..=0x3FFFD).contains(&cp)
}

fn pad(s: &str, w: usize) -> String {
    let dw = display_width(s);
    if dw >= w {
        s.chars().take(w).collect()
    } else {
        format!("{s}{}", " ".repeat(w - dw))
    }
}
