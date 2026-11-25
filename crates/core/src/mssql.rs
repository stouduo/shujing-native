//! SQL Server(tiberius):通用查询/结构/计数。所有方法持有 mssql_client
//! 返回的互斥锁守卫(与 PG 同一并发纪律)。

use crate::model::Backend;
use futures_util::TryStreamExt;
use tiberius::{ColumnData, FromSql, QueryItem};

/// 单元格 → 显示文本。按驱动的值变体逐类渲染:驱动的 `Row::get` 遇到类型不匹配
/// 会 panic(内部是 `try_get(..).unwrap()`),`cells()` 不做转换。
fn cell_text(data: &ColumnData<'static>) -> Option<String> {
    use ColumnData as D;
    Some(match data {
        D::U8(v) => v.as_ref()?.to_string(),
        D::I16(v) => v.as_ref()?.to_string(),
        D::I32(v) => v.as_ref()?.to_string(),
        D::I64(v) => v.as_ref()?.to_string(),
        // real 先 widening 成 f64 再打印会把 0.1 写成 0.10000000149011612
        D::F32(v) => {
            let v = *v.as_ref()?;
            format!("{v:?}")
        }
        D::F64(v) => crate::exec::fmt_f64(*v.as_ref()?),
        D::Bit(v) => u8::from(*v.as_ref()?).to_string(),
        D::String(s) => return s.as_ref().map(|s| s.to_string()),
        D::Guid(v) => v.as_ref()?.to_string(),
        D::Binary(b) => format!("<BLOB {} B>", b.as_ref()?.len()),
        D::Numeric(v) => v.as_ref()?.to_string(),
        D::Xml(x) => return x.as_ref().map(|x| x.as_ref().to_string()),
        D::DateTime(_) | D::SmallDateTime(_) | D::DateTime2(_) => {
            time_text::<chrono::NaiveDateTime>(data)?
        }
        D::Time(_) => time_text::<chrono::NaiveTime>(data)?,
        D::Date(_) => time_text::<chrono::NaiveDate>(data)?,
        D::DateTimeOffset(_) => time_text::<chrono::DateTime<chrono::FixedOffset>>(data)?,
    })
}

/// 驱动的时间类型 → chrono → 文本(`YYYY-MM-DD[ HH:MM:SS[.小数秒]]`,带时区的
/// 末尾追加时差)。越界的值驱动报错而不 panic,兜底显示原始值,不当作 NULL
fn time_text<'a, T: FromSql<'a> + std::fmt::Display>(
    data: &'a ColumnData<'static>,
) -> Option<String> {
    match T::from_sql(data) {
        Ok(v) => v.map(|v| v.to_string()),
        Err(_) => Some(format!("{data:?}")),
    }
}

/// 通用查询:取第一个结果集,最多 `max_rows` 行(多出的行读完丢弃并置 `truncated`)
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
    // 列名/类型取自结果集元数据(空结果集也有列头)
    let mut columns: Vec<String> = Vec::new();
    let mut column_types: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut truncated = false;
    while let Some(item) = rs.try_next().await.map_err(|e| format!("SQL Server: {e}"))? {
        match item {
            QueryItem::Metadata(meta) if meta.result_index() == 0 => {
                columns = meta.columns().iter().map(|c| c.name().to_string()).collect();
                column_types = meta
                    .columns()
                    .iter()
                    .map(|c| mssql_type_name(c.column_type()).to_string())
                    .collect();
            }
            QueryItem::Row(row) if row.result_index() == 0 => {
                if rows.len() >= max_rows {
                    truncated = true;
                    continue;
                }
                let cells: Vec<_> = row.cells().collect();
                if rows.is_empty() {
                    column_types = cells
                        .iter()
                        .map(|(c, d)| refine(c.column_type(), d).to_string())
                        .collect();
                    if columns.is_empty() {
                        columns = cells.iter().map(|(c, _)| c.name().to_string()).collect();
                    }
                }
                rows.push(cells.iter().map(|(_, d)| cell_text(d)).collect());
            }
            _ => {}
        }
    }
    // 流借着 client,读完先放手
    drop(rs);
    // 没有结果集 = 写语句:报最后一条语句的影响行数
    let affected = if columns.is_empty() { rowcount(&mut client).await? } else { 0 };
    Ok(crate::model::ExecResult {
        columns,
        rows,
        affected,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
        column_types,
    })
}

/// 执行非查询语句,返回影响行数。响应流读完:出错时驱动在取流那一步就报,
/// 但流里后续的错不读就不报;读完也免得驱动在下一条语句前替我们丢弃残留回包
pub(crate) async fn exec(client: &mut crate::model::MssqlClient, sql: &str) -> Result<u64, String> {
    let mut rs = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    while rs.try_next().await.map_err(|e| format!("SQL Server: {e}"))?.is_some() {}
    drop(rs);
    rowcount(client).await
}

/// 上一条语句的影响行数。simple_query 的流不暴露 DONE 计数,只能紧跟在同一个
/// 连接上问 `@@ROWCOUNT`:多语句批次只有最后一条语句的数,非写语句为 0
async fn rowcount(client: &mut crate::model::MssqlClient) -> Result<u64, String> {
    let row = client
        .simple_query("SELECT @@ROWCOUNT")
        .await
        .map_err(|e| format!("SQL Server: {e}"))?
        .into_row()
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    let n = row
        .as_ref()
        .and_then(|r| cell_text(r.get_column_data(0).ok()?))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(n)
}

/// 这条连接当前的默认库
pub(crate) async fn current_database(
    client: &mut crate::model::MssqlClient,
) -> Result<String, String> {
    let row = client
        .simple_query("SELECT DB_NAME()")
        .await
        .map_err(|e| format!("SQL Server: {e}"))?
        .into_row()
        .await
        .map_err(|e| format!("SQL Server: {e}"))?;
    Ok(row
        .as_ref()
        .and_then(|r| cell_text(r.get_column_data(0).ok()?))
        .unwrap_or_default())
}

/// 切换连接的默认库
pub(crate) async fn switch_database(
    client: &mut crate::model::MssqlClient,
    db: &str,
) -> Result<(), String> {
    exec(client, &format!("USE {}", crate::edits::quote(db, false))).await.map(|_| ())
}

/// 列类型:类型码 → 类型名。datetime / smalldatetime 共用类型码 Datetimen
/// (宽度不体现在类型码里),按值的变体细分
fn refine(ct: tiberius::ColumnType, data: &ColumnData<'_>) -> &'static str {
    match (ct, data) {
        (tiberius::ColumnType::Datetimen, ColumnData::SmallDateTime(_)) => "smalldatetime",
        _ => mssql_type_name(ct),
    }
}

/// SQL Server 列类型(TDS 类型码)→ 类型名。驱动已按线上的字节宽度把 Intn/Floatn
/// 归到具体宽度(可空的 int/bigint/real/float…);仍分不出的只有可空 smallmoney
/// (与 money 同码,报成 money)。
pub(crate) fn mssql_type_name(ct: tiberius::ColumnType) -> &'static str {
    use tiberius::ColumnType as CT;
    match ct {
        CT::Null => "",
        CT::Bit | CT::Bitn => "bit",
        CT::Int1 => "tinyint",
        CT::Int2 => "smallint",
        CT::Int4 | CT::Intn => "int",
        CT::Int8 => "bigint",
        CT::Datetime4 => "smalldatetime",
        CT::Datetime | CT::Datetimen => "datetime",
        CT::Datetime2 => "datetime2",
        CT::DatetimeOffsetn => "datetimeoffset",
        CT::Daten => "date",
        CT::Timen => "time",
        CT::Float4 => "real",
        CT::Float8 | CT::Floatn => "float",
        CT::Money => "money",
        CT::Money4 => "smallmoney",
        CT::Decimaln | CT::Numericn => "decimal",
        CT::Guid => "uniqueidentifier",
        CT::BigVarBin => "varbinary",
        CT::BigBinary => "binary",
        CT::BigVarChar => "varchar",
        CT::BigChar => "char",
        CT::NVarchar => "nvarchar",
        CT::NChar => "nchar",
        CT::Xml => "xml",
        CT::Udt => "udt",
        CT::Text => "text",
        CT::Image => "image",
        CT::NText => "ntext",
        CT::SSVariant => "sql_variant",
    }
}

/// 表结构(sys.columns;类型含长度)。目标库用三段式名指过去,不发 USE:连接是
/// 池化轮发的,改了某条连接的默认库会串到之后的查询上
pub async fn structure(
    st: &mut crate::model::MssqlState,
    database: Option<&str>,
    table: &str,
) -> Result<crate::model::TableStructure, String> {
    let mut client = Backend::mssql_client(st).await?;
    let db = database
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| crate::edits::quote(d, false));
    // cat = "db".(视图前缀) / obj = OBJECT_ID 里的 "db".. ;没给库 = 连接当前库
    let cat = db.as_ref().map(|d| format!("{d}.")).unwrap_or_default();
    let obj = db.as_ref().map(|d| format!("{d}..")).unwrap_or_default();
    let qt = table.replace('\'', "''");
    // 列进多个索引也不重复、只有主键索引的列算主键:主键判定用 EXISTS,
    // 直接 JOIN sys.index_columns 会按索引行展开
    // 长度:nchar/nvarchar 的 max_length 是字节数,换成字符数
    let sql = format!(
        "SELECT c.name, CAST(t.name AS NVARCHAR(60)) + \
         CASE WHEN t.name IN ('nvarchar','nchar') \
         THEN '(' + IIF(c.max_length = -1, 'MAX', CAST(c.max_length / 2 AS NVARCHAR(10))) + ')' \
         WHEN t.name IN ('varchar','char','varbinary','binary') \
         THEN '(' + IIF(c.max_length = -1, 'MAX', CAST(c.max_length AS NVARCHAR(10))) + ')' \
         WHEN t.name IN ('decimal','numeric') \
         THEN '(' + CAST(c.precision AS NVARCHAR(10)) + ',' + CAST(c.scale AS NVARCHAR(10)) + ')' \
         ELSE '' END AS full_type, \
         c.is_nullable, \
         ISNULL(dc.definition, '') AS def, \
         CAST(ISNULL(ep.value, '') AS NVARCHAR(4000)) AS comment, \
         CAST(IIF(EXISTS (SELECT 1 FROM {cat}sys.index_columns ic \
         JOIN {cat}sys.indexes i ON i.object_id = ic.object_id AND i.index_id = ic.index_id \
         WHERE ic.object_id = c.object_id AND ic.column_id = c.column_id AND i.is_primary_key = 1), 1, 0) AS bit) AS is_pk \
         FROM {cat}sys.columns c \
         JOIN {cat}sys.types t ON c.user_type_id = t.user_type_id \
         LEFT JOIN {cat}sys.default_constraints dc ON c.default_object_id = dc.object_id \
         LEFT JOIN {cat}sys.extended_properties ep ON ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description' \
         WHERE c.object_id = OBJECT_ID('{obj}{qt}') \
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
