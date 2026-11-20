//! SQL Server(tiberius):通用查询/结构/计数。所有方法持有 mssql_client
//! 返回的互斥锁守卫(与 PG 同一并发纪律)。

use crate::model::Backend;
use tiberius::Row;

fn row_cell(row: &Row, i: usize) -> Option<String> {
    use tiberius::ColumnType as CT;
    let v = row.get::<&str, _>(i);
    if let Some(s) = v {
        return Some(s.to_string());
    }
    // 数值/时间等按 Display 兜底;tiberius Row::get 对每个目标类型独立尝试
    let null_type = matches!(row.columns().get(i).map(|c| c.column_type()), Some(CT::Null));
    if null_type {
        return None;
    }
    macro_rules! try_num {
        ($t:ty) => {
            if let Some(n) = row.get::<$t, _>(i) {
                return Some(n.to_string());
            }
        };
    }
    try_num!(i16);
    try_num!(i32);
    try_num!(i64);
    try_num!(u8);
    try_num!(f32);
    try_num!(f64);
    if let Some(b) = row.get::<bool, _>(i) {
        return Some(b.to_string());
    }
    if let Some(d) = row.get::<chrono::NaiveDateTime, _>(i) {
        return Some(d.format("%Y-%m-%d %H:%M:%S").to_string());
    }
    // 二进制/UUID/嵌套:十六进制/字符串兜底
    // 最后兜底:列声明类型可显示,值不可显示则置 NULL
    None
}

/// 通用查询:simple_query 渲染全部结果集(最后一张表为主)
pub async fn query(
    st: &mut crate::model::MssqlState,
    sql: &str,
    max_rows: usize,
) -> Result<crate::model::ExecResult, String> {
    let mut client = Backend::mssql_client(st).await?;
    let started = std::time::Instant::now();
    let mut rs = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    // 行名来自首行;行前拿列名的 api 是 async(Option<&[Column]>),用首行列数兜底
    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let batch = rs
        .into_first_result()
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    if let Some(first) = batch.first() {
        columns = (0..first.len())
            .enumerate()
            .map(|(i, _)| {
                first
                    .columns()
                    .get(i)
                    .map(|c| c.name().to_string())
                    .unwrap_or_else(|| format!("col{i}"))
            })
            .collect();
    }
    for row in &batch {
        if rows.len() >= max_rows {
            break;
        }
        rows.push((0..row.len()).map(|i| row_cell(row, i)).collect());
    }
    Ok(crate::model::ExecResult {
        columns,
        rows,
        affected: 0,
        truncated: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// 表结构(sys.columns;类型含长度)
pub async fn structure(
    st: &mut crate::model::MssqlState,
    database: Option<&str>,
    table: &str,
) -> Result<crate::model::TableStructure, String> {
    let mut client = Backend::mssql_client(st).await?;
    if let Some(d) = database.map(str::trim).filter(|s| !s.is_empty()) {
        let use_sql = format!("USE {}", crate::edits::quote(d, false));
        client
            .simple_query(&use_sql)
            .await
            .map_err(|e| format!("SQL Server: {e}"))?;
    }
    let qt = table.replace('\'', "''");
    let sql = format!(
        "SELECT c.name, CAST(t.name AS NVARCHAR(60)) + \
         CASE WHEN t.name IN ('varchar','nvarchar','char','nchar','varbinary') \
         THEN '(' + IIF(c.max_length = -1, 'MAX', CAST(c.max_length AS NVARCHAR(10))) + ')' \
         WHEN t.name IN ('decimal','numeric') \
         THEN '(' + CAST(c.precision AS NVARCHAR(10)) + ',' + CAST(c.scale AS NVARCHAR(10)) + ')' \
         ELSE '' END AS full_type, \
         c.is_nullable, \
         ISNULL(dc.definition, '') AS def, \
         CAST(ISNULL(ep.value, '') AS NVARCHAR(4000)) AS comment, \
         CAST(IIF(ic.column_id IS NOT NULL, 1, 0) AS bit) AS is_pk \
         FROM sys.columns c \
         JOIN sys.types t ON c.user_type_id = t.user_type_id \
         LEFT JOIN sys.default_constraints dc ON c.default_object_id = dc.object_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description' \
         LEFT JOIN sys.index_columns ic ON ic.object_id = c.object_id AND ic.column_id = c.column_id \
         LEFT JOIN sys.indexes i ON i.object_id = ic.object_id AND i.index_id = ic.index_id AND i.is_primary_key = 1 \
         WHERE c.object_id = OBJECT_ID('{qt}') \
         ORDER BY c.column_id"
    );
    let rows = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("SQL Server: {e}"))?
        .into_first_result()
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    let mut columns = Vec::new();
    for r in &rows {
        let name = r.get::<&str, _>(0).unwrap_or_default().to_string();
        let full_type = r.get::<&str, _>(1).unwrap_or_default().to_string();
        let nullable = r.get::<bool, _>(2).unwrap_or(true);
        let def = r.get::<&str, _>(3).unwrap_or_default().to_string();
        let comment = r.get::<&str, _>(4).unwrap_or_default().to_string();
        let is_pk = r.get::<bool, _>(5).unwrap_or(false);
        columns.push(crate::model::ColumnDef {
            name,
            data_type: full_type,
            nullable,
            key: if is_pk { "PRI".into() } else { String::new() },
            default: (!def.is_empty()).then(|| def),
            extra: String::new(),
            comment,
        });
    }
    Ok(crate::model::TableStructure {
        columns,
        indexes: vec![],
        ddl: String::new(),
    })
}

pub async fn count(
    st: &mut crate::model::MssqlState,
    table: &str,
) -> Result<u64, String> {
    let mut client = Backend::mssql_client(st).await?;
    let qt = table.replace('\'', "''");
    let sql = format!(
        "SELECT SUM(p.rows) FROM sys.partitions p WHERE p.object_id = OBJECT_ID('{qt}') AND p.index_id IN (0,1)"
    );
    let rows = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("SQL Server: {e}"))?
        .into_first_result()
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    Ok(rows
        .first()
        .and_then(|r| r.get::<i64, _>(0))
        .map(|n| n.max(0) as u64)
        .unwrap_or(0))
}
