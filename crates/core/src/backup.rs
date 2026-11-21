//! 数据库备份:连接右键「备份数据库…」→ 单文件 SQL dump(DDL + 数据 INSERT)。
//! v1 方言:MySQL / PostgreSQL / SQLite;逐表 500 行/批流式写盘,内存有界。
//! 一致性:LIMIT/OFFSET 分页(与数据传输同策略),不持锁——备份期间的并发
//! 写入可能出现弱一致,文件头时间戳如实标注备份时刻。

use crate::model::DbType;
use crate::transfer::{qualified, sql_literal};
use crate::Service;
use std::path::{Path, PathBuf};

pub struct BackupStats {
    pub tables: usize,
    pub rows: u64,
    pub bytes: u64,
    pub path: String,
    /// 跳过项说明(视图只写 DDL 不写数据;DDL 获取失败的表)
    pub skipped: Vec<String>,
}

/// 默认落盘路径:~/Documents/数镜备份/<连接名>-<库>-<时间戳>.sql(目录自动创建)
pub fn default_backup_path(conn_name: &str, db: &str) -> PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = PathBuf::from(format!("{home}/Documents/数镜备份"));
    let _ = std::fs::create_dir_all(&dir);
    let db_part = if db.trim().is_empty() { conn_name } else { db.trim() };
    let safe = |s: &str| {
        s.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>()
    };
    dir.join(format!("{}-{}-{}.sql", safe(conn_name), safe(db_part), ts))
}

/// 备份目录里的一个文件(管理面板用)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFileMeta {
    pub name: String,
    pub path: String,
    pub bytes: u64,
    /// modified epoch ms
    pub modified_ms: i64,
}

/// 列出备份目录最近的 .sql 文件(按修改时间倒序)
pub fn list_recent(dir: &Path, limit: usize) -> Vec<BackupFileMeta> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<BackupFileMeta> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("sql") {
                return None;
            }
            let md = e.metadata().ok()?;
            Some(BackupFileMeta {
                name: path.file_name()?.to_string_lossy().to_string(),
                path: path.to_string_lossy().to_string(),
                bytes: md.len(),
                modified_ms: md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0),
            })
        })
        .collect();
    sort_and_limit(&mut out, limit);
    out
}

/// 按 modified 倒序并截断(纯函数,便于确定性测试)
fn sort_and_limit(items: &mut Vec<BackupFileMeta>, limit: usize) {
    items.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    items.truncate(limit);
}

/// 全库备份。progress(已完成表数, 总表数, 当前表名) 在每个表开始时回调。
pub async fn backup_database(
    service: &Service,
    id: &str,
    database: Option<&str>,
    out_path: &Path,
    progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
) -> Result<BackupStats, String> {
    let infos = service.list_saved_connections();
    let info = infos
        .iter()
        .find(|c| c.id == id)
        .ok_or("连接不存在")?
        .clone();
    match info.db_type {
        DbType::MySql | DbType::Postgres | DbType::Sqlite => {}
        DbType::ClickHouse => {
            return Err("ClickHouse 备份暂不支持(建议用导出 CSV/JSON)".into())
        }
        DbType::Mssql => return Err("SQL Server 备份暂不支持(建议导出 SQL + 导入)".into()),
        DbType::Mongo => return Err("MongoDB 备份请用「导出集合 JSON」".into()),
        _ => return Err("中间件数据源没有 SQL 备份语义".into()),
    }
    service.connect_by_id(id).await?;

    let mysql = info.db_type == DbType::MySql;
    let sqlite = info.db_type == DbType::Sqlite;
    let tables = service.list_tables(id, database.map(str::to_string)).await?;
    if tables.is_empty() {
        return Err("该库没有可备份的表".into());
    }

    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(out_path)
        .await
        .map_err(|e| format!("备份文件创建失败: {e}"))?;

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let db_label = database.unwrap_or(if sqlite { "(单文件)" } else { "(默认库)" });
    let mut head = format!(
        "-- 数镜备份 (shujing-native) {ts}\n-- 连接: {}  类型: {:?}  数据库: {db_label}\n",
        info.name, info.db_type
    );
    if mysql {
        head.push_str("SET NAMES utf8mb4;\nSET FOREIGN_KEY_CHECKS=0;\n");
    }
    if sqlite {
        head.push_str("PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\n");
    }
    file.write_all(head.as_bytes())
        .await
        .map_err(|e| format!("备份写入失败: {e}"))?;

    let total = tables.len();
    let mut done: usize = 0;
    let mut rows_total: u64 = 0;
    let mut skipped: Vec<String> = Vec::new();

    for t in &tables {
        progress(done, total, &t.name);
        let is_view = t.kind.to_lowercase().contains("view");
        let name_quoted = crate::edits::quote(&t.name, mysql);
        let full = if sqlite {
            name_quoted.clone()
        } else {
            qualified(&t.name, database, mysql)
        };

        let mut section = format!("\n-- ─── {} ───\n", t.name);
        // DDL(视图也导 CREATE;数据只有表才有)
        match service
            .get_object_ddl(id, database.map(str::to_string), "table", &t.name)
            .await
        {
            Ok(ddl) if !ddl.trim().is_empty() => {
                let ddl = ddl.trim().trim_end_matches(';');
                section.push_str(ddl);
                section.push_str(";\n");
            }
            Ok(_) => {
                skipped.push(format!("{}(DDL 为空)", t.name));
                section.push_str(&format!("-- DDL 获取失败或为空: {}\n", t.name));
            }
            Err(e) => {
                skipped.push(format!("{}(DDL 失败: {e})", t.name));
                section.push_str(&format!("-- DDL 获取失败: {}\n", t.name));
            }
        }
        file.write_all(section.as_bytes())
            .await
            .map_err(|e| format!("备份写入失败: {e}"))?;

        if !is_view {
            let cols_list: Option<(Vec<String>, String)> = None; // 首批取列名
            let _ = cols_list;
            let mut offset: u64 = 0;
            let mut table_rows: u64 = 0;
            let mut cols_sql: Option<String> = None;
            loop {
                let sql = format!("SELECT * FROM {full} LIMIT 500 OFFSET {offset}");
                let rs = service.run_sql_db(id, sql, Some(500), database).await?;
                let Some(r) = rs.first() else { break };
                if r.columns.is_empty() || r.rows.is_empty() {
                    break;
                }
                if cols_sql.is_none() {
                    let cols: Vec<String> = r
                        .columns
                        .iter()
                        .map(|c| crate::edits::quote(c, mysql))
                        .collect();
                    cols_sql = Some(cols.join(", "));
                }
                let cols = cols_sql.clone().unwrap_or_default();
                let mut batch = String::new();
                for row in &r.rows {
                    let vals: Vec<String> = (0..r.columns.len())
                        .map(|i| match row.get(i) {
                            None | Some(None) => "NULL".to_string(),
                            Some(Some(s)) => sql_literal(s, mysql),
                        })
                        .collect();
                    batch.push_str(&format!(
                        "INSERT INTO {full} ({cols}) VALUES ({});\n",
                        vals.join(", ")
                    ));
                }
                file.write_all(batch.as_bytes())
                    .await
                    .map_err(|e| format!("备份写入失败: {e}"))?;
                table_rows += r.rows.len() as u64;
                offset += r.rows.len() as u64;
                if r.rows.len() < 500 {
                    break;
                }
            }
            rows_total += table_rows;
        } else {
            skipped.push(format!("{}(视图,只含 DDL)", t.name));
        }
        done += 1;
    }

    if sqlite {
        file.write_all(b"COMMIT;\n")
            .await
            .map_err(|e| format!("备份写入失败: {e}"))?;
    } else if mysql {
        file.write_all(b"SET FOREIGN_KEY_CHECKS=1;\n")
            .await
            .map_err(|e| format!("备份写入失败: {e}"))?;
    }
    file.flush()
        .await
        .map_err(|e| format!("备份写入失败: {e}"))?;
    let bytes = file
        .metadata()
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(BackupStats {
        tables: done,
        rows: rows_total,
        bytes,
        path: out_path.display().to_string(),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnInfo, DbType};

    fn sqlite_info(path: &str) -> ConnInfo {
        ConnInfo {
            id: "bak".into(),
            name: "bak".into(),
            db_type: DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(path.into()),
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        }
    }

    /// 备份闭环:建表灌中文数据 → 备份成文件 → 新空库执行该文件 → 行数与中文一致
    #[tokio::test]
    async fn backup_sqlite_roundtrip() {
        let dir = std::env::temp_dir().join(format!("shujing_backup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.db");
        std::fs::write(&src, b"").unwrap();
        let svc = Service::new(&dir);
        svc.save_connection(sqlite_info(src.to_str().unwrap())).unwrap();
        svc.connect(sqlite_info(src.to_str().unwrap())).await.unwrap();
        svc.run_sql(
            "bak",
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, score REAL); \
             INSERT INTO users (name, score) VALUES ('张三', 91.5), ('李四', 88.25), ('王五', NULL); \
             CREATE TABLE logs (id INTEGER PRIMARY KEY, note TEXT); \
             INSERT INTO logs (note) VALUES ('第一条'),('第二条');"
                .into(),
            None,
        )
        .await
        .unwrap();

        let out = dir.join("dump.sql");
        let stats = backup_database(
            &svc,
            "bak",
            None,
            &out,
            &|done, total, table| {
                println!("progress {done}/{total} {table}");
            },
        )
        .await
        .unwrap();
        assert_eq!(stats.tables, 2, "两张表");
        assert_eq!(stats.rows, 5, "3+2 行");
        assert!(stats.bytes > 0);
        let dump = std::fs::read_to_string(&out).unwrap();
        assert!(dump.contains("CREATE TABLE"), "含 DDL");
        assert!(dump.contains("张三"), "中文保真");
        assert!(dump.contains("INSERT INTO"), "含数据");

        // 回导:新空库执行备份文件 → 行数与中文一致
        let dst = dir.join("dst.db");
        std::fs::write(&dst, b"").unwrap();
        svc.save_connection(ConnInfo {
            id: "restore".into(),
            name: "restore".into(),
            db_type: DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(dst.to_str().unwrap().into()),
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        })
        .unwrap();
        svc.connect(ConnInfo {
            id: "restore".into(),
            name: "restore".into(),
            db_type: DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(dst.to_str().unwrap().into()),
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        })
        .await
        .unwrap();
        svc.run_sql("restore", dump, None).await.unwrap();
        let rs = svc
            .run_sql("restore", "SELECT name, score FROM users ORDER BY id".into(), None)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(rs.rows.len(), 3);
        assert_eq!(rs.rows[0][0].as_deref(), Some("张三"));
        assert_eq!(rs.rows[0][1].as_deref(), Some("91.5"));
        assert_eq!(rs.rows[2][1], None, "NULL 保真");
        let logs = svc
            .run_sql("restore", "SELECT count(*) FROM logs".into(), None)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(logs.rows[0][0].as_deref(), Some("2"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod recent_tests {
    use super::*;

    #[test]
    fn list_recent_filters_and_limits() {
        let dir = std::env::temp_dir().join(format!("shujing_bk_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.sql", "b.sql", "c.txt"] {
            std::fs::write(dir.join(name), b"-- x").unwrap();
        }
        let list = list_recent(&dir, 5);
        assert_eq!(list.len(), 2, "txt 被过滤: {list:?}");
        assert_eq!(list_recent(&dir, 1).len(), 1, "limit 生效");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sort_and_limit_deterministic() {
        let mut items = vec![
            BackupFileMeta { name: "old".into(), path: String::new(), bytes: 1, modified_ms: 1000 },
            BackupFileMeta { name: "new".into(), path: String::new(), bytes: 2, modified_ms: 3000 },
            BackupFileMeta { name: "mid".into(), path: String::new(), bytes: 3, modified_ms: 2000 },
        ];
        sort_and_limit(&mut items, 2);
        assert_eq!(items[0].name, "new");
        assert_eq!(items[1].name, "mid");
        assert_eq!(items.len(), 2);
    }
}
