use crate::model::{Backend, ConnInfo, ExecResult};
use std::time::Instant;

/// 单行变更:主键定位 + 新值
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellUpdate {
    pub pk: Vec<(String, String)>,
    pub sets: Vec<(String, Option<String>)>,
}

fn quote(name: &str, mysql: bool) -> String {
    if mysql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// 字符串字面量(统一加引号,三库都能隐式接受数字/日期文本)
fn literal(v: &str) -> String {
    format!("'{}'", v.replace('\'', "''"))
}

fn where_of(pk: &[(String, String)], mysql: bool) -> Result<String, String> {
    if pk.is_empty() {
        return Err("该行没有主键,无法定位更新".into());
    }
    Ok(pk
        .iter()
        .map(|(c, v)| format!("{} = {}", quote(c, mysql), literal(v)))
        .collect::<Vec<_>>()
        .join(" AND "))
}

pub async fn apply_changes(
    backend: &mut Backend,
    info: &ConnInfo,
    table: &str,
    updates: &[CellUpdate],
    deletes: &[Vec<(String, String)>],
    inserts: &[Vec<(String, String)>],
) -> Result<ExecResult, String> {
    let started = Instant::now();
    let mysql = info.db_type == crate::model::DbType::MySql;
    let mut affected: u64 = 0;

    for u in updates {
        if u.sets.is_empty() {
            continue;
        }
        let sets = u
            .sets
            .iter()
            .map(|(c, v)| match v {
                Some(s) => format!("{} = {}", quote(c, mysql), literal(s)),
                None => format!("{} = NULL", quote(c, mysql)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "UPDATE {} SET {} WHERE {}",
            quote(table, mysql),
            sets,
            where_of(&u.pk, mysql)?
        );
        affected += backend.run_one(&sql, 1).await?.affected.max(0);
    }

    for pk in deletes {
        let sql = format!("DELETE FROM {} WHERE {}", quote(table, mysql), where_of(pk, mysql)?);
        affected += backend.run_one(&sql, 1).await?.affected.max(0);
    }

    for ins in inserts {
        if ins.is_empty() {
            continue;
        }
        let cols = ins.iter().map(|(c, _)| quote(c, mysql)).collect::<Vec<_>>().join(", ");
        let vals = ins.iter().map(|(_, v)| literal(v)).collect::<Vec<_>>().join(", ");
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote(table, mysql),
            cols,
            vals
        );
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
