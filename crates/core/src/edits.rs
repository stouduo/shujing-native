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
    let mut affected: u64 = 0;

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
        let sql = format!("UPDATE {} SET {} WHERE {}", table, sets, where_of(&u.pk, mysql)?);
        affected += backend.run_one(&sql, 1).await?.affected.max(0);
    }

    for pk in deletes {
        let sql = format!("DELETE FROM {} WHERE {}", table, where_of(pk, mysql)?);
        affected += backend.run_one(&sql, 1).await?.affected.max(0);
    }

    for ins in inserts {
        if ins.is_empty() {
            continue;
        }
        let cols = ins.iter().map(|(c, _)| quote(c, mysql)).collect::<Vec<_>>().join(", ");
        let vals = ins.iter().map(|(_, v)| literal(v, mysql)).collect::<Vec<_>>().join(", ");
        let sql = format!("INSERT INTO {} ({}) VALUES ({})", table, cols, vals);
        affected += backend.run_one(&sql, 1).await?.affected.max(0);
    }

    if updates.is_empty() && deletes.is_empty() && inserts.is_empty() {
        return Err("没有待保存的更改".into());
    }

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
