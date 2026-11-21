//! 数据对比:两张同构表按主键对齐,产出差异与目标侧同步 SQL(INSERT/UPDATE/DELETE)。

use crate::model::TableStructure;
use crate::Service;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowChange {
    /// 主键显示串(列=值 & 连接)
    pub pk: String,
    pub columns: Vec<(String, String, String)>, // 列, 源值, 目标值
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDiff {
    pub pk_cols: Vec<String>,
    pub only_src: Vec<Vec<Option<String>>>,
    pub only_dst: Vec<Vec<Option<String>>>,
    pub changed: Vec<RowChange>,
    pub src_rows: usize,
    pub dst_rows: usize,
    /// 目标侧同步 SQL(把目标变成与源一致;最多 max_rows 范围内)
    pub sync_sql: Vec<String>,
    pub truncated: bool,
}

fn pk_key(row: &[Option<String>], pk_idx: &[usize]) -> String {
    pk_idx
        .iter()
        .map(|&i| row.get(i).and_then(|v| v.clone()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\u{1}")
}

fn sync_literal(v: &str, mysql: bool) -> String {
    if mysql {
        format!("'{}'", v.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        format!("'{}'", v.replace('\'', "''"))
    }
}

async fn fetch_rows(
    service: &Service,
    conn: &str,
    db: Option<&str>,
    table: &str,
    pk_cols: &[String],
    max: usize,
    mysql: bool,
) -> Result<(Vec<String>, Vec<Vec<Option<String>>>), String> {
    let q = |c: &str| crate::edits::quote(c, mysql);
    let mut sql = format!("SELECT * FROM {}", crate::edits::quote(table, mysql));
    if let Some(d) = db {
        sql = format!(
            "SELECT * FROM {}.{}",
            crate::edits::quote(d, mysql),
            crate::edits::quote(table, mysql)
        );
    }
    if !pk_cols.is_empty() {
        sql.push_str(&format!(
            " ORDER BY {}",
            pk_cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ")
        ));
    }
    sql.push_str(&format!(" LIMIT {max}"));
    let r = service
        .run_sql_db(conn, sql, Some(max), db)
        .await?
        .pop()
        .ok_or("查询无结果")?;
    Ok((r.columns, r.rows))
}

/// 对比两张表(按主键;最多各拉 max 行)。同步 SQL 写给目标侧。
pub async fn diff_data(
    service: &Service,
    src: (&str, Option<&str>, &str),
    dst: (&str, Option<&str>, &str),
    max: usize,
) -> Result<DataDiff, String> {
    let (sc, sdb, st) = src;
    let (dc, ddb, dt) = dst;
    let infos = service.list_saved_connections();
    let src_info = infos.iter().find(|c| c.id == sc).ok_or("源连接不存在")?;
    let dst_info = infos.iter().find(|c| c.id == dc).ok_or("目标连接不存在")?;
    if src_info.db_type != dst_info.db_type {
        return Err("仅支持相同数据库类型之间的数据对比".into());
    }
    let mysql = dst_info.db_type == crate::model::DbType::MySql;
    service.connect_by_id(sc).await?;
    service.connect_by_id(dc).await?;
    let st_struct: TableStructure = service
        .get_table_structure(sc, st, sdb.map(str::to_string))
        .await?;
    let pk_cols: Vec<String> = st_struct
        .columns
        .iter()
        .filter(|c| c.key == "PRI")
        .map(|c| c.name.clone())
        .collect();
    if pk_cols.is_empty() {
        return Err("源表没有主键,无法逐行对齐".into());
    }
    let (src_cols, src_rows) =
        fetch_rows(service, sc, sdb, st, &pk_cols, max, mysql).await?;
    let (dst_cols, dst_rows) =
        fetch_rows(service, dc, ddb, dt, &pk_cols, max, mysql).await?;
    let pk_i_src: Vec<usize> = pk_cols
        .iter()
        .filter_map(|p| src_cols.iter().position(|c| c == p))
        .collect();
    let pk_i_dst: Vec<usize> = pk_cols
        .iter()
        .filter_map(|p| dst_cols.iter().position(|c| c == p))
        .collect();
    if pk_i_src.len() != pk_cols.len() || pk_i_dst.len() != pk_cols.len() {
        return Err("主键列不在查询结果中".into());
    }

    let mut map_dst: std::collections::HashMap<String, &Vec<Option<String>>> = dst_rows
        .iter()
        .map(|r| (pk_key(r, &pk_i_dst), r))
        .collect();
    let pk_for_out = pk_cols.clone();
    let mut out = DataDiff {
        pk_cols: pk_for_out,
        only_src: vec![],
        only_dst: vec![],
        changed: vec![],
        src_rows: src_rows.len(),
        dst_rows: dst_rows.len(),
        sync_sql: vec![],
        truncated: false,
    };
    let mut seen_dst: std::collections::HashSet<String> = Default::default();
    let dst_full = if let Some(d) = ddb.map(str::trim).filter(|s| !s.is_empty()) {
        format!("{}.{}", crate::edits::quote(d, mysql), crate::edits::quote(dt, mysql))
    } else {
        crate::edits::quote(dt, mysql)
    };
    let src_full = if let Some(d) = sdb.map(str::trim).filter(|s| !s.is_empty()) {
        format!("{}.{}", crate::edits::quote(d, mysql), crate::edits::quote(st, mysql))
    } else {
        crate::edits::quote(st, mysql)
    };
    for r in &src_rows {
        let key = pk_key(r, &pk_i_src);
        match map_dst.get(&key) {
            None => {
                // 目标缺行:INSERT(全列)
                let vals: Vec<String> = r
                    .iter()
                    .map(|v| match v {
                        None => "NULL".into(),
                        Some(s) => sync_literal(s, mysql),
                    })
                    .collect();
                let cols: Vec<String> = src_cols.iter().map(|c| crate::edits::quote(c, mysql)).collect();
                out.sync_sql.push(format!(
                    "INSERT INTO {dst_full} ({}) VALUES ({});",
                    cols.join(", "),
                    vals.join(", ")
                ));
                out.only_src.push(r.clone());
            }
            Some(dr) => {
                seen_dst.insert(key.clone());
                // 逐列比(源列序,目标列名映射)
                let mut cells = Vec::new();
                for (i, sc_name) in src_cols.iter().enumerate() {
                    let dci = match dst_cols.iter().position(|c| c == sc_name) {
                        Some(x) => x,
                        None => continue, // 结构差异由 schema diff 管
                    };
                    if r[i] != dr[dci] {
                        cells.push((
                            sc_name.clone(),
                            r[i].clone().unwrap_or_else(|| "NULL".into()),
                            dr[dci].clone().unwrap_or_else(|| "NULL".into()),
                        ));
                    }
                }
                if !cells.is_empty() {
                    let sets: Vec<String> = cells
                        .iter()
                        .map(|(c, s, _)| {
                            format!(
                                "{} = {}",
                                crate::edits::quote(c, mysql),
                                if s == "NULL" {
                                    "NULL".into()
                                } else {
                                    sync_literal(s, mysql)
                                }
                            )
                        })
                        .collect();
                    let wheres: Vec<String> = pk_cols
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let sv = r.get(pk_i_src[i]).and_then(|v| v.clone()).unwrap_or_default();
                            format!(
                                "{} = {}",
                                crate::edits::quote(p, mysql),
                                sync_literal(&sv, mysql)
                            )
                        })
                        .collect();
                    out.sync_sql.push(format!(
                        "UPDATE {dst_full} SET {} WHERE {};",
                        sets.join(", "),
                        wheres.join(" AND ")
                    ));
                    out.changed.push(RowChange { pk: key.replace('\u{1}', " & "), columns: cells });
                }
            }
        }
    }
    for r in &dst_rows {
        let key = pk_key(r, &pk_i_dst);
        if !seen_dst.contains(&key) {
            out.only_dst.push(r.clone());
            let wheres: Vec<String> = pk_cols
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let dv = r.get(pk_i_dst[i]).and_then(|v| v.clone()).unwrap_or_default();
                    format!(
                        "{} = {}",
                        crate::edits::quote(p, mysql),
                        sync_literal(&dv, mysql)
                    )
                })
                .collect();
            out.sync_sql.push(format!("DELETE FROM {dst_full} WHERE {};", wheres.join(" AND ")));
        }
    }
    out.truncated = src_rows.len() >= max || dst_rows.len() >= max;
    let _ = (src_full, dst_full, &map_dst);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Service;
    use std::sync::Arc;

    /// E2E:两个 SQLite 文件库,构造 源多一行/目标多一行/公共行改值,验证 diff 与同步 SQL
    #[tokio::test]
    async fn diff_two_sqlite_tables() {
        let dir = std::env::temp_dir().join(format!("sj-dd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let svc = Arc::new(Service::new(&dir));

        let path_a = dir.join("a.db");
        let path_b = dir.join("b.db");
        std::fs::write(&path_a, b"").unwrap();
        std::fs::write(&path_b, b"").unwrap();
        let mk = |id: &str, p: &std::path::PathBuf, name: &str| crate::model::ConnInfo {
            id: id.into(),
            name: name.into(),
            db_type: crate::model::DbType::Sqlite,
            host: None,
            port: None,
            user: None,
            password: None,
            database: None,
            file_path: Some(p.display().to_string()),
            color: None,
            read_only: false,
            perm: None,
            ssh: None,
        };
        svc.save_connection(mk("a", &path_a, "A")).unwrap();
        svc.save_connection(mk("b", &path_b, "B")).unwrap();

        let pa = path_a.display().to_string();
        let pb = path_b.display().to_string();
        svc.connect(crate::model::ConnInfo { id: "a".into(), name: "".into(), db_type: crate::model::DbType::Sqlite, host: None, port: None, user: None, password: None, database: None, file_path: Some(pa.clone()), color: None, read_only: false, perm: None, ssh: None }).await.unwrap();
        svc.connect(crate::model::ConnInfo { id: "b".into(), name: "".into(), db_type: crate::model::DbType::Sqlite, host: None, port: None, user: None, password: None, database: None, file_path: Some(pb.clone()), color: None, read_only: false, perm: None, ssh: None }).await.unwrap();

        svc.run_sql("a", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, age INT)".into(), None).await.unwrap();
        svc.run_sql("b", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, age INT)".into(), None).await.unwrap();
        svc.run_sql("a", "INSERT INTO t VALUES (1,'a',10),(2,'b',20),(3,'c',30)".into(), None).await.unwrap();
        svc.run_sql("b", "INSERT INTO t VALUES (1,'a',10),(2,'b',99),(9,'z',0)".into(), None).await.unwrap();

        let d = diff_data(&svc, ("a", None, "t"), ("b", None, "t"), 5000).await.unwrap();
        assert_eq!(d.src_rows, 3);
        assert_eq!(d.dst_rows, 3);
        assert_eq!(d.only_src.len(), 1);       // id=3
        assert_eq!(d.only_dst.len(), 1);       // id=9
        assert_eq!(d.changed.len(), 1);        // id=2 age 20→99
        assert_eq!(d.changed[0].columns[0].0, "age");
        assert!(d.sync_sql.iter().any(|s| s.starts_with("INSERT INTO")));
        assert!(d.sync_sql.iter().any(|s| s.starts_with("UPDATE")));
        assert!(d.sync_sql.iter().any(|s| s.starts_with("DELETE FROM")));
        // 同步 SQL 执行后目标应与源一致
        for sql in &d.sync_sql {
            svc.run_sql("b", sql.clone(), None).await.unwrap();
        }
        let d2 = diff_data(&svc, ("a", None, "t"), ("b", None, "t"), 5000).await.unwrap();
        assert!(d2.changed.is_empty() && d2.only_src.is_empty() && d2.only_dst.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
