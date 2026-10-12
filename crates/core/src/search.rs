use crate::model::{Backend, ConnInfo};

/// 全局搜索命中:表 / 命中列 / 整行数据
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub table: String,
    pub column: String,
    pub columns: Vec<String>,
    pub row: Vec<Option<String>>,
}

fn quote(name: &str, mysql: bool) -> String {
    if mysql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// 在连接的全部表中搜索关键词(逐表 LIKE 扫描,文本匹配不分大小写)
pub async fn search_tables(
    backend: &mut Backend,
    info: &ConnInfo,
    keyword: &str,
    max_hits: usize,
) -> Result<Vec<SearchHit>, String> {
    let kw = keyword.trim();
    if kw.is_empty() {
        return Err("关键词为空".into());
    }
    let mysql = info.db_type == crate::model::DbType::MySql;
    let pg = info.db_type == crate::model::DbType::Postgres;
    let like_op = if pg { "ILIKE" } else { "LIKE" };
    // SQL 字面量转义
    let lit = format!("'%{}%'", kw.replace('\'', "''"));
    let kw_lower = kw.to_lowercase();

    let tables = backend.list_tables(info).await?;
    let mut hits: Vec<SearchHit> = Vec::new();

    for t in tables.iter().filter(|t| t.kind == "table") {
        if hits.len() >= max_hits {
            break;
        }
        // 拉列结构构建 OR 条件(每表最多取 5 条命中)
        let st = match backend.get_table_structure(info, &t.name, None).await {
            Ok(s) => s,
            Err(_) => continue,
        };
        if st.columns.is_empty() {
            continue;
        }
        let conds = st
            .columns
            .iter()
            .map(|c| format!("{} {} {}", quote(&c.name, mysql), like_op, lit))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!(
            "SELECT * FROM {} WHERE {} ",
            quote(&t.name, mysql),
            conds
        );
        let r = match backend.run_one(&sql, 5).await {
            Ok(r) => r,
            Err(_) => continue,
        };
        if r.columns.is_empty() {
            continue;
        }
        for row in r.rows {
            if hits.len() >= max_hits {
                break;
            }
            // 定位命中列(客户端大小写不敏感匹配)
            if let Some((ci, _)) = row
                .iter()
                .enumerate()
                .find(|(_, v)| v.as_deref().map(|s| s.to_lowercase().contains(&kw_lower)).unwrap_or(false))
            {
                hits.push(SearchHit {
                    table: t.name.clone(),
                    column: r.columns[ci].clone(),
                    columns: r.columns.clone(),
                    row,
                });
            }
        }
    }
    Ok(hits)
}
