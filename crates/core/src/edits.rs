use crate::model::{Backend, ConnInfo, ExecResult};
use std::time::Instant;

/// 单行变更:主键定位 + 新值
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellUpdate {
    pub pk: Vec<(String, String)>,
    pub sets: Vec<(String, Option<String>)>,
}

/// 供组合模块引用的标识符引注(mysql=反引号,其余双引号)
pub fn quote_ident_pub(database: &Option<String>, name: &str, mysql: bool) -> String {
    match database {
        Some(db) => format!("{}.{}", quote(db, mysql), quote(name, mysql)),
        None => quote(name, mysql),
    }
}

/// 带库限定的表名(MySQL 多库连接必须 `db`.`table`,否则 No database selected)
pub fn qualified_table(table: &str, database: Option<&str>, mysql: bool) -> String {
    match database.map(str::trim).filter(|s| !s.is_empty()) {
        Some(db) => format!("{}.{}", quote(db, mysql), quote(&table, mysql)),
        None => quote(&table, mysql),
    }
}

fn quote(name: &str, mysql: bool) -> String {
    if mysql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// 字符串字面量(统一加引号,三库都能隐式接受数字/日期文本;
/// MySQL 默认把反斜杠当转义符,须同时双写,否则尾部反斜杠会吃掉收尾引号)
fn literal(v: &str, mysql: bool) -> String {
    let mut out = v.replace('\'', "''");
    if mysql {
        out = out.replace('\\', "\\\\");
    }
    format!("'{out}'")
}

fn where_of(pk: &[(String, String)], mysql: bool) -> Result<String, String> {
    if pk.is_empty() {
        return Err("该行没有主键,无法定位更新".into());
    }
    Ok(pk
        .iter()
        .map(|(c, v)| format!("{} = {}", quote(c, mysql), literal(v, mysql)))
        .collect::<Vec<_>>()
        .join(" AND "))
}

pub async fn apply_changes(
    backend: &mut Backend,
    info: &ConnInfo,
    table: &str,
    database: Option<&str>,
    updates: &[CellUpdate],
    deletes: &[Vec<(String, String)>],
    inserts: &[Vec<(String, String)>],
) -> Result<ExecResult, String> {
    let started = Instant::now();
    let mysql = info.db_type == crate::model::DbType::MySql;
    let table = qualified_table(table, database, mysql);

    // 先收集全部语句,再按后端包在单事务里执行:
    // 中途失败整体回滚,不会留下"前 k 条已落库"的半保存状态
    let mut stmts: Vec<String> = Vec::new();
    for u in updates {
        if u.sets.is_empty() {
            continue;
        }
        let sets = u
            .sets
            .iter()
            .map(|(c, v)| match v {
                Some(s) => format!("{} = {}", quote(c, mysql), literal(s, mysql)),
                None => format!("{} = NULL", quote(c, mysql)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        stmts.push(format!(
            "UPDATE {} SET {} WHERE {}",
            table,
            sets,
            where_of(&u.pk, mysql)?
        ));
    }
    for pk in deletes {
        stmts.push(format!("DELETE FROM {} WHERE {}", table, where_of(pk, mysql)?));
    }
    for ins in inserts {
        if ins.is_empty() {
            continue;
        }
        let cols = ins.iter().map(|(c, _)| quote(c, mysql)).collect::<Vec<_>>().join(", ");
        let vals = ins.iter().map(|(_, v)| literal(v, mysql)).collect::<Vec<_>>().join(", ");
        stmts.push(format!("INSERT INTO {} ({}) VALUES ({})", table, cols, vals));
    }

    if stmts.is_empty() {
        return Err("没有待保存的更改".into());
    }

    let affected: u64 = match backend {
        Backend::Sqlite(conn) => {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let mut affected = 0u64;
            for sql in &stmts {
                let n = tx.execute(sql, []).map_err(|e| e.to_string())?;
                affected += n as u64;
            }
            tx.commit().map_err(|e| e.to_string())?;
            affected
        }
        Backend::MySql(mp) => {
            use mysql_async::prelude::*;
            let mut conn = Backend::mysql_conn(mp).await.map_err(|e| e.to_string())?;
            conn.query_drop("START TRANSACTION").await.map_err(|e| e.to_string())?;
            let mut affected = 0u64;
            for sql in &stmts {
                if let Err(e) = conn.query_drop(sql.as_str()).await {
                    // 回滚必须落在同一会话上,否则池里留下悬挂事务
                    let _ = conn.query_drop("ROLLBACK").await;
                    return Err(e.to_string());
                }
                affected += conn.affected_rows();
            }
            conn.query_drop("COMMIT").await.map_err(|e| e.to_string())?;
            affected
        }
        Backend::Pg(mp) => {
            let client = Backend::pg_client(mp).await.map_err(|e| e.to_string())?;
            client.batch_execute("BEGIN").await.map_err(|e| e.to_string())?;
            let mut affected = 0u64;
            for sql in &stmts {
                match client.simple_query(sql).await {
                    Ok(msgs) => {
                        for m in msgs {
                            if let tokio_postgres::SimpleQueryMessage::CommandComplete(tag) = m {
                                affected += tag;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = client.batch_execute("ROLLBACK").await;
                        return Err(e.to_string());
                    }
                }
            }
            client.batch_execute("COMMIT").await.map_err(|e| e.to_string())?;
            affected
        }
        Backend::Redis(_) => return Err("Redis 请使用专门的命令通道".into()),
    };

    Ok(ExecResult {
        columns: vec![],
        rows: vec![],
        affected,
        truncated: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_table_limits_to_database() {
        assert_eq!(qualified_table("users", Some("shop"), true), "`shop`.`users`");
        assert_eq!(qualified_table("users", Some(" shop "), true), "`shop`.`users`");
        assert_eq!(qualified_table("users", None, true), "`users`");
        // 空串库视同未设置
        assert_eq!(qualified_table("users", Some(""), true), "`users`");
        // 非 MySQL 双引号
        assert_eq!(qualified_table("users", Some("pub"), false), "\"pub\".\"users\"");
    }
}

#[cfg(test)]
mod tx_tests {
    use super::*;
    use crate::model::ConnInfo;

    async fn test_backend(path: &str) -> (Backend, ConnInfo) {
        let info = ConnInfo {
            id: "t".into(),
            name: "t".into(),
            db_type: crate::model::DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(path.into()),
            color: None,
            read_only: false,
        };
        let b = crate::backend::connect(&info, None).await.unwrap();
        (b, info)
    }

    /// 按行合批:同一行多列改动合并为一条 UPDATE;事务:中途失败整体回滚
    #[tokio::test]
    async fn apply_changes_groups_rows_and_rolls_back() {
        let dir = std::env::temp_dir().join(format!("shujing_tx_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tx.db");
        let _ = std::fs::remove_file(&path);
        let (mut b, info) = test_backend(path.to_str().unwrap()).await;

        b.run_sql(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, age INT, UNIQUE(name))",
            100,
            None,
        )
        .await
        .unwrap();
        b.run_one("INSERT INTO t (id, name, age) VALUES (1, 'a', 1), (2, 'b', 2)", 100)
            .await
            .unwrap();

        // 同一行两列改动:合批后一条 UPDATE,两列都生效
        let updates = vec![CellUpdate {
            pk: vec![("id".into(), "1".into())],
            sets: vec![("name".into(), Some("a2".into())), ("age".into(), Some("11".into()))],
        }];
        let r = apply_changes(&mut b, &info, "t", None, &updates, &[], &[]).await.unwrap();
        assert_eq!(r.affected, 1, "合批后同行只产生一条 UPDATE");
        let row = b.run_one("SELECT name, age FROM t WHERE id = 1", 10).await.unwrap();
        assert_eq!(row.rows[0][0].as_deref(), Some("a2"));
        assert_eq!(row.rows[0][1].as_deref(), Some("11"));

        // 事务回滚:第一条成功、第二条撞唯一约束 → 全部不生效
        let updates = vec![
            CellUpdate { pk: vec![("id".into(), "1".into())], sets: vec![("age".into(), Some("99".into()))] },
            CellUpdate { pk: vec![("id".into(), "2".into())], sets: vec![("name".into(), Some("a2".into()))] },
        ];
        let err = apply_changes(&mut b, &info, "t", None, &updates, &[], &[]).await;
        assert!(err.is_err(), "撞唯一约束应报错");
        let row = b.run_one("SELECT age, name FROM t WHERE id = 1", 10).await.unwrap();
        assert_eq!(row.rows[0][0].as_deref(), Some("11"), "第一条 UPDATE 应回滚");
        let row = b.run_one("SELECT name FROM t WHERE id = 2", 10).await.unwrap();
        assert_eq!(row.rows[0][0].as_deref(), Some("b"), "第二条 UPDATE 应回滚");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
