pub use crate::model::TableMeta;
use crate::model::{es, Backend, ColumnDef, ConnInfo, IndexDef, TableStructure};

/// 外键关系(ER 图数据源)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FkMeta {
    pub table: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

fn fk_sqlite(conn: &rusqlite::Connection) -> Result<Vec<FkMeta>, String> {
    let tables = schema_sqlite(conn)?;
    let mut out = Vec::new();
    for t in tables {
        if t.kind != "table" {
            continue;
        }
        let sql = format!("PRAGMA foreign_key_list({})", ident(&t.name, false));
        let Ok(mut stmt) = conn.prepare(&sql) else { continue };
        let Ok(mut rows) = stmt.query([]) else { continue };
        while let Ok(Some(row)) = rows.next() {
            // (id, seq, table, from, to, on_update, on_delete, match)
            let ref_table: String = row.get(2).map_err(es)?;
            let column: String = row.get(3).map_err(es)?;
            let ref_column: Option<String> = row.get(4).map_err(es)?;
            out.push(FkMeta {
                table: t.name.clone(),
                column,
                ref_table,
                ref_column: ref_column.unwrap_or_else(|| "id".into()),
            });
        }
    }
    Ok(out)
}

async fn fk_mysql(conn: &mut mysql_async::Conn, database: &str) -> Result<Vec<FkMeta>, String> {
    use mysql_async::prelude::*;
    let sval = |v: Option<&mysql_async::Value>| -> String {
        match v {
            Some(mysql_async::Value::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    };
    let mut result = conn
        .exec_iter(
            "SELECT TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
             FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL",
            (database,),
        )
        .await
        .map_err(es)?;
    let mut out = Vec::new();
    result
        .for_each(|row| {
            let v = row.unwrap();
            out.push(FkMeta {
                table: sval(v.get(0)),
                column: sval(v.get(1)),
                ref_table: sval(v.get(2)),
                ref_column: sval(v.get(3)),
            });
        })
        .await
        .map_err(es)?;
    Ok(out)
}

async fn fk_pg(client: &tokio_postgres::Client) -> Result<Vec<FkMeta>, String> {
    use tokio_postgres::SimpleQueryMessage;
    let messages = client
        .simple_query(
            "SELECT tc.table_name, kcu.column_name, ccu.table_name, ccu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema \
             JOIN information_schema.constraint_column_usage ccu \
               ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema \
             WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_schema = 'public'",
        )
        .await
        .map_err(es)?;
    let mut out = Vec::new();
    for msg in messages {
        if let SimpleQueryMessage::Row(row) = msg {
            if let (Some(t), Some(c), Some(rt), Some(rc)) =
                (row.get(0), row.get(1), row.get(2), row.get(3))
            {
                out.push(FkMeta {
                    table: t.to_string(),
                    column: c.to_string(),
                    ref_table: rt.to_string(),
                    ref_column: rc.to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// 表名来自我们查出的列表,仍做标识符转义防注入
/// 列出服务器上所有数据库
pub async fn list_databases(backend: &mut Backend, info: &ConnInfo) -> Result<Vec<String>, String> {
    match backend {
        Backend::Sqlite(_) => Ok(vec![]),
        Backend::MySql(mp) => {
            use mysql_async::prelude::*;
            let mut conn = Backend::mysql_conn(mp).await?;
            let mut result = conn
                .query_iter("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME NOT IN ('information_schema','mysql','performance_schema','sys') ORDER BY SCHEMA_NAME")
                .await
                .map_err(es)?;
            let mut out = Vec::new();
            result
                .for_each(|row| {
                    let v = row.unwrap();
                    if let Some(mysql_async::Value::Bytes(b)) = v.get(0) {
                        out.push(String::from_utf8_lossy(b).into_owned());
                    }
                })
                .await
                .map_err(es)?;
            Ok(out)
        }
        Backend::Pg(mp) => {
            use tokio_postgres::SimpleQueryMessage;
            let client = Backend::pg_client(mp).await?;
            let messages = client
                .simple_query("SELECT datname FROM pg_database WHERE datallowconn ORDER BY datname")
                .await
                .map_err(es)?;
            let mut out = Vec::new();
            for msg in messages {
                if let SimpleQueryMessage::Row(row) = msg {
                    if let Some(n) = row.get(0) {
                        out.push(n.to_string());
                    }
                }
            }
            Ok(out)
        }
        Backend::Redis(_) => Ok(vec![]),
    }
}

fn ident(name: &str, mysql: bool) -> String {
    if mysql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn schema_sqlite(conn: &rusqlite::Connection) -> Result<Vec<TableMeta>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM sqlite_master \
             WHERE type IN ('table','view','trigger') AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .map_err(es)?;
    let mut rows = stmt.query([]).map_err(es)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(es)? {
        let name: String = row.get(0).map_err(es)?;
        let kind: String = row.get(1).map_err(es)?;
        out.push(TableMeta { name, kind });
    }
    Ok(out)
}

async fn schema_mysql(conn: &mut mysql_async::Conn, database: &str) -> Result<Vec<TableMeta>, String> {
    use mysql_async::prelude::*;
    let mut result = conn
        .exec_iter(
            "SELECT TABLE_NAME, IF(TABLE_TYPE = 'VIEW', 'view', 'table') AS kind \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await
        .map_err(es)?;
    let mut out = Vec::new();
    result
        .for_each(|row| {
            let vals = row.unwrap();
            let get = |i: usize| {
                vals.get(i).and_then(|v| match v {
                    mysql_async::Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
                    _ => None,
                })
            };
            if let (Some(name), Some(kind)) = (get(0), get(1)) {
                out.push(TableMeta { name, kind });
            }
        })
        .await
        .map_err(es)?;

    // 触发器与函数/过程
    let mut result = conn
        .exec_iter(
            "SELECT TRIGGER_NAME, 'trigger' FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? \
             UNION ALL \
             SELECT ROUTINE_NAME, LOWER(ROUTINE_TYPE) FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ?",
            (database, database),
        )
        .await
        .map_err(es)?;
    result
        .for_each(|row| {
            let vals = row.unwrap();
            let get = |i: usize| {
                vals.get(i).and_then(|v| match v {
                    mysql_async::Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
                    _ => None,
                })
            };
            if let (Some(name), Some(kind)) = (get(0), get(1)) {
                out.push(TableMeta { name, kind });
            }
        })
        .await
        .map_err(es)?;
    Ok(out)
}

async fn schema_pg(client: &tokio_postgres::Client) -> Result<Vec<TableMeta>, String> {
    use tokio_postgres::SimpleQueryMessage;
    let messages = client
        .simple_query(
            "SELECT c.relname, CASE c.relkind WHEN 'r' THEN 'table' ELSE 'view' END AS kind \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r','v','m') \
             UNION ALL \
             SELECT t.tgname, 'trigger' FROM pg_trigger t \
             JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND NOT t.tgisinternal \
             UNION ALL \
             SELECT p.proname, 'function' FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'public' \
             ORDER BY 1",
        )
        .await
        .map_err(es)?;
    let mut out = Vec::new();
    for msg in messages {
        if let SimpleQueryMessage::Row(row) = msg {
            if let (Some(name), Some(kind)) = (row.get(0), row.get(1)) {
                out.push(TableMeta { name: name.to_string(), kind: kind.to_string() });
            }
        }
    }
    Ok(out)
}

// ── 表结构 ──────────────────────────────────────────────

fn struct_sqlite(conn: &rusqlite::Connection, table: &str) -> Result<TableStructure, String> {
    let mut out = TableStructure::default();

    // 字段
    let sql = format!("PRAGMA table_info({})", ident(table, false));
    let mut stmt = conn.prepare(&sql).map_err(es)?;
    let mut rows = stmt.query([]).map_err(es)?;
    while let Some(row) = rows.next().map_err(es)? {
        let name: String = row.get(1).map_err(es)?;
        let data_type: String = row.get(2).map_err(es)?;
        let not_null: i64 = row.get(3).map_err(es)?;
        let default: Option<String> = row.get(4).map_err(es)?;
        let pk: i64 = row.get(5).map_err(es)?;
        out.columns.push(ColumnDef {
            name,
            data_type,
            nullable: not_null == 0,
            key: if pk > 0 { "PRI".into() } else { String::new() },
            default,
            extra: if pk > 0 { "PRIMARY KEY".into() } else { String::new() },
            comment: String::new(),
        });
    }
    drop(rows);
    drop(stmt);

    // 索引
    let sql = format!("PRAGMA index_list({})", ident(table, false));
    if let Ok(mut stmt) = conn.prepare(&sql) {
        if let Ok(mut rows) = stmt.query([]) {
            while let Ok(Some(row)) = rows.next() {
                let name: String = row.get(1).map_err(es)?;
                let unique: i64 = row.get(2).map_err(es)?;
                let mut cols = Vec::new();
                let isql = format!("PRAGMA index_info({})", ident(&name, false));
                if let Ok(mut istmt) = conn.prepare(&isql) {
                    if let Ok(mut irows) = istmt.query([]) {
                        while let Ok(Some(irow)) = irows.next() {
                            if let Ok(Some(c)) = irow.get::<_, Option<String>>(2) {
                                cols.push(c);
                            }
                        }
                    }
                }
                out.indexes.push(IndexDef {
                    name,
                    columns: cols.join(", "),
                    unique: unique == 1,
                });
            }
        }
    }

    // DDL
    let ddl: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = ?1",
            [table],
            |r| r.get(0),
        )
        .map_err(es)?;
    out.ddl = ddl.unwrap_or_default();
    Ok(out)
}

async fn struct_mysql(
    conn: &mut mysql_async::Conn,
    database: &str,
    table: &str,
) -> Result<TableStructure, String> {
    use mysql_async::prelude::*;
    let mut out = TableStructure::default();

    let sval = |v: Option<&mysql_async::Value>| -> String {
        match v {
            Some(mysql_async::Value::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
            Some(mysql_async::Value::NULL) | None => String::new(),
            Some(other) => crate::exec::fmt_mysql_value(other).unwrap_or_default(),
        }
    };

    let mut result = conn
        .exec_iter(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await
        .map_err(es)?;
    result
        .for_each(|row| {
            let v = row.unwrap();
            out.columns.push(ColumnDef {
                name: sval(v.get(0)),
                data_type: sval(v.get(1)),
                nullable: sval(v.get(2)) == "YES",
                key: sval(v.get(3)),
                default: match v.get(4) {
                    Some(mysql_async::Value::NULL) | None => None,
                    Some(_) => Some(sval(v.get(4))),
                },
                extra: sval(v.get(5)),
                comment: sval(v.get(6)),
            });
        })
        .await
        .map_err(es)?;

    let mut result = conn
        .exec_iter(
            "SELECT INDEX_NAME, NON_UNIQUE, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ', ') \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? GROUP BY INDEX_NAME, NON_UNIQUE",
            (database, table),
        )
        .await
        .map_err(es)?;
    result
        .for_each(|row| {
            let v = row.unwrap();
            out.indexes.push(IndexDef {
                name: sval(v.get(0)),
                columns: sval(v.get(2)),
                unique: sval(v.get(1)) == "0",
            });
        })
        .await
        .map_err(es)?;

    let mut result = conn
        .query_iter(format!("SHOW CREATE TABLE {}", ident(table, true)))
        .await
        .map_err(es)?;
    result
        .for_each(|row| {
            if out.ddl.is_empty() {
                let v = row.unwrap();
                if let Some(d) = v.get(1) {
                    out.ddl = sval(Some(d));
                }
            }
        })
        .await
        .map_err(es)?;
    Ok(out)
}

async fn struct_pg(client: &tokio_postgres::Client, table: &str) -> Result<TableStructure, String> {
    use tokio_postgres::SimpleQueryMessage;
    let mut out = TableStructure::default();
    let qualified = format!("public.{}", ident(table, false));

    // 字段 + 注释
    let sql = format!(
        "SELECT c.column_name, c.data_type, c.is_nullable, c.column_default, \
                col_description(pgc.oid, c.ordinal_position) \
         FROM information_schema.columns c \
         JOIN pg_catalog.pg_class pgc ON pgc.relname = c.table_name \
         WHERE c.table_schema = 'public' AND c.table_name = {q} \
         ORDER BY c.ordinal_position",
        q = format!("'{}'", table.replace('\'', "''"))
    );
    for msg in client.simple_query(&sql).await.map_err(es)? {
        if let SimpleQueryMessage::Row(row) = msg {
            out.columns.push(ColumnDef {
                name: row.get(0).unwrap_or_default().to_string(),
                data_type: row.get(1).unwrap_or_default().to_string(),
                nullable: row.get(2) == Some("YES"),
                key: String::new(),
                default: row.get(3).map(str::to_string),
                extra: String::new(),
                comment: row.get(4).map(str::to_string).unwrap_or_default(),
            });
        }
    }

    // 主键
    let sql = format!(
        "SELECT a.attname FROM pg_index i \
         JOIN pg_class t ON t.oid = i.indrelid \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(i.indkey) \
         WHERE i.indisprimary AND t.relname = {q}",
        q = format!("'{}'", table.replace('\'', "''"))
    );
    for msg in client.simple_query(&sql).await.map_err(es)? {
        if let SimpleQueryMessage::Row(row) = msg {
            if let Some(name) = row.get(0) {
                if let Some(c) = out.columns.iter_mut().find(|c| c.name == name) {
                    c.key = "PRI".into();
                }
            }
        }
    }

    // 索引
    let sql = format!(
        "SELECT indexname, indexdef FROM pg_indexes WHERE schemaname = 'public' AND tablename = {q}",
        q = format!("'{}'", table.replace('\'', "''"))
    );
    for msg in client.simple_query(&sql).await.map_err(es)? {
        if let SimpleQueryMessage::Row(row) = msg {
            let def = row.get(1).unwrap_or_default().to_string();
            let cols_start = def.find('(').map(|i| i + 1).unwrap_or(def.len());
            let cols_end = def.rfind(')').unwrap_or(def.len());
            out.indexes.push(IndexDef {
                name: row.get(0).unwrap_or_default().to_string(),
                columns: def[cols_start..cols_end].to_string(),
                unique: def.contains("UNIQUE"),
            });
        }
    }

    // DDL(pg 无内置 SHOW CREATE,后续版本生成)
    let _ = qualified;
    Ok(out)
}

fn count_sqlite(conn: &rusqlite::Connection, table: &str) -> Result<u64, String> {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {}", ident(table, false)),
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as u64)
    .map_err(es)
}

async fn count_mysql(conn: &mut mysql_async::Conn, table: &str) -> Result<u64, String> {
    use mysql_async::prelude::*;
    let mut result = conn
        .query_iter(format!("SELECT COUNT(*) FROM {}", ident(table, true)))
        .await
        .map_err(es)?;
    let mut n: u64 = 0;
    result
        .for_each(|row| {
            if n == 0 {
                if let Some(v) = row.unwrap().first() {
                    n = crate::exec::fmt_mysql_value(v)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
            }
        })
        .await
        .map_err(es)?;
    Ok(n)
}

async fn count_pg(client: &tokio_postgres::Client, table: &str) -> Result<u64, String> {
    let row = client
        .query_one(
            &format!("SELECT COUNT(*) FROM public.{}", ident(table, false)),
            &[],
        )
        .await
        .map_err(|e| e.to_string())?;
    let n: i64 = row.try_get(0).map_err(|e| e.to_string())?;
    Ok(n as u64)
}

/// 提取触发器/函数/过程的 DDL
pub async fn object_ddl(
    backend: &mut Backend,
    info: &ConnInfo,
    kind: &str,
    name: &str,
) -> Result<String, String> {
    let mysql = info.db_type == crate::model::DbType::MySql;
    let sql = match (&info.db_type, kind) {
        (crate::model::DbType::Sqlite, _) => format!(
            "SELECT sql FROM sqlite_master WHERE name = {}",
            format!("'{}'", name.replace('\'', "''"))
        ),
        (crate::model::DbType::MySql, "trigger") => {
            format!("SHOW CREATE TRIGGER {}", ident(name, true))
        }
        (crate::model::DbType::MySql, "procedure") => {
            format!("SHOW CREATE PROCEDURE {}", ident(name, true))
        }
        (crate::model::DbType::MySql, _) => {
            format!("SHOW CREATE FUNCTION {}", ident(name, true))
        }
        (crate::model::DbType::Redis, _) => return Err("Redis 不支持该操作".into()),
        (crate::model::DbType::Postgres, "trigger") => format!(
            "SELECT pg_get_triggerdef(t.oid) FROM pg_trigger t \
             JOIN pg_class c ON c.oid = t.tgrelid WHERE t.tgname = {}",
            format!("'{}'", name.replace('\'', "''"))
        ),
        (crate::model::DbType::Postgres, _) => format!(
            "SELECT pg_get_functiondef(p.oid) FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'public' AND p.proname = {} LIMIT 1",
            format!("'{}'", name.replace('\'', "''"))
        ),
    };
    let _ = mysql;
    let r = backend.run_one(&sql, 2).await?;
    if r.rows.is_empty() {
        return Err(format!("{kind} {name} 不存在"));
    }
    // SHOW CREATE XXX 的定义列名各异(Create Trigger / Create Procedure / Create Function),
    // 统一取行中列名含 "create" 的第一列;sqlite 为 sql 列
    let idx = r
        .columns
        .iter()
        .position(|c| c.to_ascii_lowercase().contains("create") || c.eq_ignore_ascii_case("sql"))
        .unwrap_or(r.columns.len().saturating_sub(1));
    Ok(r.rows[0].get(idx).cloned().unwrap_or_default().unwrap_or_default())
}

impl Backend {
    pub async fn list_tables(&mut self, info: &crate::model::ConnInfo) -> Result<Vec<TableMeta>, String> {
        self.list_tables_in(info, info.database.as_deref()).await
    }

    pub async fn list_tables_in(&mut self, info: &crate::model::ConnInfo, database: Option<&str>) -> Result<Vec<TableMeta>, String> {
        match self {
            Backend::Sqlite(conn) => schema_sqlite(conn),
            Backend::MySql(mp) => {
                let mut conn = Backend::mysql_conn(mp).await?;
                let db = database.map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "未指定数据库".to_string())?;
                schema_mysql(&mut conn, db).await
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                schema_pg(&client).await
            }
            Backend::Redis(_) => crate::redis_ops::databases(self).await.map(|dbs| {
                dbs.into_iter()
                    .map(|(idx, keys)| TableMeta {
                        name: format!("db{idx}"),
                        kind: format!("redis-db:{idx}:{keys}"),
                    })
                    .collect()
            }),
        }
    }

/// 解析 MySQL 目标库:显式指定的 > 连接配置的 > 当前会话 USE 的(SELECT DATABASE())
async fn resolve_mysql_db(
    conn: &mut mysql_async::Conn,
    info: &ConnInfo,
    explicit: Option<&str>,
) -> Result<String, String> {
    use mysql_async::prelude::*;
    if let Some(d) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    if let Some(d) = info.database.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    let mut result = conn
        .query_iter("SELECT DATABASE()")
        .await
        .map_err(es)?;
    let mut cur: Option<String> = None;
    result
        .for_each(|row| {
            if cur.is_none() {
                if let Some(mysql_async::Value::Bytes(b)) = row.unwrap().first() {
                    let s = String::from_utf8_lossy(b).into_owned();
                    if !s.is_empty() {
                        cur = Some(s);
                    }
                }
            }
        })
        .await
        .map_err(es)?;
    cur.ok_or_else(|| "连接未指定数据库,请先在左侧选择要操作的库".to_string())
}

    pub async fn get_table_structure(
        &mut self,
        info: &ConnInfo,
        table: &str,
        database: Option<&str>,
    ) -> Result<TableStructure, String> {
        match self {
            Backend::Sqlite(conn) => struct_sqlite(conn, table),
            Backend::MySql(mp) => {
                let mut conn = Backend::mysql_conn(mp).await?;
                // 显式 database 最高优先:多库连接下表可能不在连接配置的默认库
                let db = Self::resolve_mysql_db(&mut conn, info, database).await?;
                struct_mysql(&mut conn, &db, table).await
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                struct_pg(&client, table).await
            }
            Backend::Redis(_) => Err("Redis 无表结构".into()),
        }
    }

    pub async fn count_rows(&mut self, table: &str) -> Result<u64, String> {
        match self {
            Backend::Sqlite(conn) => count_sqlite(conn, table),
            Backend::MySql(mp) => {
                let mut conn = Backend::mysql_conn(mp).await?;
                count_mysql(&mut conn, table).await
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                count_pg(&client, table).await
            }
            Backend::Redis(_) => Err("Redis 无表计数".into()),
        }
    }

    pub async fn list_foreign_keys(
        &mut self,
        info: &ConnInfo,
        database: Option<&str>,
    ) -> Result<Vec<FkMeta>, String> {
        match self {
            Backend::Sqlite(conn) => fk_sqlite(conn),
            Backend::MySql(mp) => {
                let mut conn = Backend::mysql_conn(mp).await?;
                let db = Self::resolve_mysql_db(&mut conn, info, database).await?;
                fk_mysql(&mut conn, &db).await
            }
            Backend::Pg(mp) => {
                let client = Backend::pg_client(mp).await?;
                fk_pg(&client).await
            }
            Backend::Redis(_) => Ok(vec![]),
        }
    }
}
