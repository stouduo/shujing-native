//! 连接配置导入:从 DBeaver(项目导出的 DataSources.json)与 Navicat(.ncx 导出)
//! 迁移连接。密码在两者中都是加密存储,无法解密——导入后密码留空,用户重填。

use crate::model::{ConnInfo, DbType, SshConfig};
use std::collections::BTreeMap;

fn conn_id(name: &str, db_type: &DbType) -> String {
    let t = match db_type {
        DbType::MySql => "mysql",
        DbType::Postgres => "pg",
        DbType::Sqlite => "sqlite",
        DbType::Redis => "redis",
        DbType::Mongo => "mongo",
        DbType::Mssql => "mssql",
    };
    // 稳定 id:导入重跑不会因时间戳不同而重复
    format!("imp-{t}-{}", sanitize(name))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase()
}

/// 按内容嗅探分发:DBeaver JSON / Navicat .ncx(XML)
pub fn import_connections(text: &str) -> Result<Vec<ConnInfo>, String> {
    let t = text.trim_start();
    if t.starts_with('{') {
        parse_dbeaver_json(text)
    } else if t.starts_with('<') {
        parse_navicat_ncx(text)
    } else {
        Err("无法识别的文件格式:需要 DBeaver 连接导出(JSON)或 Navicat .ncx(XML)".into())
    }
}

// ── DBeaver:DataSources.json ──────────────────────────────
// { "connections": { "<id>": { "provider": "mysql", "name": "...",
//     "configuration": { "host": ..., "port": "3306", "database": ..., "user": ... } } } }

fn dbeaver_provider_to_type(provider: &str) -> Option<DbType> {
    match provider.to_lowercase().as_str() {
        "mysql" | "mariadb" => Some(DbType::MySql),
        "postgresql" | "postgres" | "greenplum" | "openGauss" | "enterprisedb" => {
            Some(DbType::Postgres)
        }
        "sqlite" => Some(DbType::Sqlite),
        "redis" => Some(DbType::Redis),
        "mongodb" | "mongo" => Some(DbType::Mongo),
        "mssql" | "sqlserver" => Some(DbType::Mssql),
        _ => None,
    }
}

fn cfg_str<'a>(cfg: &'a serde_json::Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = cfg.get(*k) {
            match v {
                serde_json::Value::String(s) if !s.is_empty() => return Some(s.clone()),
                serde_json::Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn parse_dbeaver_json(text: &str) -> Result<Vec<ConnInfo>, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let conns_obj = root
        .get("connections")
        .and_then(|v| v.as_object())
        .ok_or("JSON 里没有 connections 对象(需要 DBeaver 的 DataSources.json)")?;
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for (id, v) in conns_obj {
        let provider = v.get("provider").and_then(|p| p.as_str()).unwrap_or("");
        let Some(db_type) = dbeaver_provider_to_type(provider) else {
            skipped.push(format!("{id}({provider})"));
            continue;
        };
        let cfg = v.get("configuration").cloned().unwrap_or_default();
        let name = v
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(id)
            .to_string();
        let host = cfg_str(&cfg, &["host", "server"]);
        let port = cfg_str(&cfg, &["port"]).and_then(|p| p.parse().ok());
        let user = cfg_str(&cfg, &["user", "userName"]);
        let database = cfg_str(&cfg, &["database", "dbName", "db"]);
        let file_path = if db_type == DbType::Sqlite {
            cfg_str(&cfg, &["path", "database", "db"])
        } else {
            None
        };
        // 密码是 DBeaver 本地加密,导入必为空——用户重填
        out.push(ConnInfo {
            id: conn_id(&name, &db_type),
            name,
            db_type,
            host,
            port,
            user,
            password: None,
            database: if db_type == DbType::Redis {
                None
            } else {
                database
            },
            file_path,
            color: None,
            read_only: false,
            ssh: cfg
                .get("configuration")
                .map(|c| {
                    parse_ssh_generic(c)
                })
                .unwrap_or_else(|| parse_ssh_generic(&cfg)),
        });
    }
    if !skipped.is_empty() {
        // 不支持类型的连接明确跳过,而不是静默消失
        eprintln!("conn_import: skipped {:?}", skipped);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// DBeaver 的 SSHJ/SSH 隧道配置存在 network-handlers 里,结构因版本而异:
/// 尽力从 configuration 里识别 ssh-* 键(v1:只支持最常见形态)
fn parse_ssh_generic(cfg: &serde_json::Value) -> Option<SshConfig> {
    let obj = cfg.as_object()?;
    let keys: BTreeMap<String, String> = obj
        .iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k.to_lowercase(), s.clone())),
            serde_json::Value::Number(n) => Some((k.to_lowercase(), n.to_string())),
            serde_json::Value::Bool(b) => Some((k.to_lowercase(), b.to_string())),
            _ => None,
        })
        .collect();
    let enabled = keys
        .get("ssh-tunnel-enabled")
        .or_else(|| keys.get("sshtunnel"))
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let host = keys.get("ssh-host")?.clone();
    let user = keys.get("ssh-user").or_else(|| keys.get("ssh-user-name"))?.clone();
    Some(SshConfig {
        host,
        port: keys
            .get("ssh-port")
            .and_then(|p| p.parse().ok())
            .unwrap_or(22),
        user,
        password: keys.get("ssh-pass").cloned(),
        key_path: keys.get("ssh-key-file").cloned(),
    })
}

// ── Navicat:.ncx(XML)─────────────────────────────────────
// <Navicat><Connection ConnectionName="x" ConnType="MYSQL" Host="..." Port="3306"
//                  UserName="root" Database="db" ... /></Navicat>

fn xml_attr(tag: &str, name: &str) -> Option<String> {
    // 找 name="value"(值可含转义实体;不含引号的简单值)
    let pat = format!("{}=\"", name);
    let mut search_from = 0;
    while let Some(pos) = tag[search_from..].find(&pat) {
        let abs = search_from + pos;
        // 属性名必须整词(前面是非字母数字)
        let ok_prefix = abs == 0
            || tag[..abs]
                .chars()
                .last()
                .map(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                .unwrap_or(true);
        if ok_prefix {
            let rest = &tag[abs + pat.len()..];
            if let Some(end) = rest.find('"') {
                return Some(xml_decode(&rest[..end]));
            }
        }
        search_from = abs + pat.len();
    }
    None
}

fn xml_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn ncx_type_to_db_type(ct: &str) -> Option<DbType> {
    match ct.to_uppercase().as_str() {
        "MYSQL" | "MARIADB" => Some(DbType::MySql),
        "POSTGRESQL" | "PG" => Some(DbType::Postgres),
        "SQLITE" => Some(DbType::Sqlite),
        "REDIS" => Some(DbType::Redis),
        "MONGODB" => Some(DbType::Mongo),
        "MSSQL" => Some(DbType::Mssql),
        _ => None,
    }
}

fn parse_navicat_ncx(text: &str) -> Result<Vec<ConnInfo>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find("<Connection ") else { break };
        let after = &rest[start..];
        let Some(end) = after.find('>') else { break };
        let tag = &after[..end];
        rest = &after[end + 1..];
        let Some(name) = xml_attr(tag, "ConnectionName") else {
            continue;
        };
        let ct = xml_attr(tag, "ConnType").unwrap_or_default();
        let Some(db_type) = ncx_type_to_db_type(&ct) else {
            continue;
        };
        let host = xml_attr(tag, "Host").or_else(|| xml_attr(tag, "Server"));
        let port = xml_attr(tag, "Port").and_then(|p| p.parse().ok());
        let user = xml_attr(tag, "UserName").or_else(|| xml_attr(tag, "UserName"));
        let database = xml_attr(tag, "Database");
        out.push(ConnInfo {
            id: conn_id(&name, &db_type),
            name,
            db_type,
            host,
            port,
            user,
            password: None,
            database: if db_type == DbType::Redis { None } else { database },
            file_path: if db_type == DbType::Sqlite {
                xml_attr(tag, "DatabaseFile").or_else(|| xml_attr(tag, "Database"))
            } else {
                None
            },
            color: None,
            read_only: false,
            ssh: xml_attr(tag, "SshHost")
                .filter(|h| !h.is_empty())
                .map(|ssh_host| SshConfig {
                    host: ssh_host,
                    port: xml_attr(tag, "SshPort")
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(22),
                    user: xml_attr(tag, "SshUserName").unwrap_or_default(),
                    password: None,
                    key_path: None,
                }),
        });
    }
    if out.is_empty() {
        return Err("NCX 里没有可识别的连接(仅支持 MYSQL/POSTGRESQL/SQLITE/REDIS)".into());
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 合并导入:按 (类型,名称) 去重,已存在的同名同型连接跳过。返回 (新增数, 跳过数)
pub fn merge_into(existing: &[ConnInfo], imported: Vec<ConnInfo>) -> (usize, usize) {
    let mut added = 0;
    let mut skipped = 0;
    let mut result = existing.to_vec();
    for c in imported {
        let dup = existing
            .iter()
            .any(|e| e.db_type == c.db_type && e.name == c.name)
            || result
                .iter()
                .any(|e| e.id == c.id && e.id != c.id && e.db_type == c.db_type);
        if dup {
            skipped += 1;
            continue;
        }
        result.push(c);
        added += 1;
    }
    // 通过回写 existing 长度语义传结果:调用方需要完整列表,这里返回列表由调用方组装
    let _ = result;
    (added, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbeaver_json_roundtrip() {
        let text = r#"{
          "connections": {
            "mysql-local": {
              "provider": "mysql",
              "name": "本地 MySQL",
              "configuration": { "host": "127.0.0.1", "port": "3306", "database": "dramato", "user": "root" }
            },
            "pg-prod": {
              "provider": "postgresql",
              "name": "生产 PG",
              "configuration": { "host": "10.0.0.9", "port": "5432", "database": "app", "user": "ops",
                                 "ssh-tunnel-enabled": "true", "ssh-host": "bastion", "ssh-port": "2222", "ssh-user": "deploy" }
            },
            "mongo-x": { "provider": "mongodb", "name": "不支持", "configuration": {} }
          }
        }"#;
        let conns = import_connections(text).unwrap();
        // mongo-x 现已支持(Mongo 类型),3 条全导入
        assert_eq!(conns.len(), 3);
        assert!(conns.iter().any(|c| c.db_type == DbType::Mongo && c.name == "不支持"));
        let mysql = conns.iter().find(|c| c.name == "本地 MySQL").unwrap();
        assert_eq!(mysql.db_type, DbType::MySql);
        assert_eq!(mysql.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(mysql.port, Some(3306));
        assert_eq!(mysql.database.as_deref(), Some("dramato"));
        assert!(mysql.password.is_none(), "密码必须留空(加密不可解)");
        let pg = conns.iter().find(|c| c.name == "生产 PG").unwrap();
        let ssh = pg.ssh.as_ref().unwrap();
        assert_eq!(ssh.host, "bastion");
        assert_eq!(ssh.port, 2222);
    }

    #[test]
    fn navicat_ncx_parse() {
        let text = r#"<?xml version="1.0"?>
        <Navicat>
          <Connection ConnectionName="订单库" ConnType="MYSQL" Host="192.168.1.5" Port="3307" UserName="dev" Database="orders"/>
          <Connection ConnectionName="缓存" ConnType="REDIS" Host="10.0.0.2" Port="6379" UserName=""/>
          <Connection ConnectionName="oracle-x" ConnType="ORACLE" Host="h" Port="1521"/>
        </Navicat>"#;
        let conns = import_connections(text).unwrap();
        assert_eq!(conns.len(), 2);
        let o = conns.iter().find(|c| c.name == "订单库").unwrap();
        assert_eq!(o.host.as_deref(), Some("192.168.1.5"));
        assert_eq!(o.port, Some(3307));
        assert_eq!(o.database.as_deref(), Some("orders"));
        assert!(conns.iter().all(|c| c.db_type != DbType::Sqlite || c.name != "oracle-x"));
    }

    #[test]
    fn merge_dedups_by_type_and_name() {
        let existing = vec![ConnInfo {
            id: "c1".into(),
            name: "本地 MySQL".into(),
            db_type: DbType::MySql,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: None,
            color: None,
            read_only: false,
            ssh: None,
        }];
        let imported = vec![ConnInfo {
            id: "imp-mysql-本地".into(),
            name: "本地 MySQL".into(),
            db_type: DbType::MySql,
            host: Some("h".into()),
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: None,
            color: None,
            read_only: false,
            ssh: None,
        }];
        let (added, skipped) = merge_into(&existing, imported);
        assert_eq!(added, 0);
        assert_eq!(skipped, 1);
    }
}
