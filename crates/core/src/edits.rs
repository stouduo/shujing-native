use crate::model::{Backend, ConnInfo, ExecResult};
use std::time::Instant;

/// 单行变更:主键定位 + 新值
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellUpdate {
    pub pk: Vec<(String, String)>,
    pub sets: Vec<(String, Option<String>)>,
}

/// SQL 文本方言:标识符引注、字符串字面量、分页写法的差异
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// MySQL / ClickHouse:反引号
    Mysql,
    /// SQL Server
    Mssql,
    /// 其余(SQLite / PG / 插件…):双引号 + 标准写法
    Standard,
}

impl Dialect {
    pub fn of(db_type: crate::model::DbType) -> Self {
        use crate::model::DbType;
        match db_type {
            DbType::MySql | DbType::ClickHouse => Self::Mysql,
            DbType::Mssql => Self::Mssql,
            _ => Self::Standard,
        }
    }
    fn mysql(self) -> bool {
        self == Self::Mysql
    }
    fn mssql(self) -> bool {
        self == Self::Mssql
    }
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

/// 数据页的表引用。SQL Server 的两段式名字是「架构.表」,限定库要写三段式
/// db..table(架构取默认)
pub fn page_table_ref(database: Option<&str>, table: &str, dialect: Dialect) -> String {
    let db = database.map(str::trim).filter(|s| !s.is_empty());
    match (dialect.mssql(), db) {
        (true, Some(db)) => format!("{}..{}", quote(db, false), quote(table, false)),
        (_, Some(db)) => qualified_table(table, Some(db), dialect.mysql()),
        (_, None) => quote(table, dialect.mysql()),
    }
}

/// 数据页分页子句(带前导空格)。SQL Server 没有 LIMIT/OFFSET:分页是
/// ORDER BY … OFFSET n ROWS FETCH NEXT m ROWS ONLY,而且必须带 ORDER BY——
/// 没有排序列时用 (SELECT NULL)(不改变自然顺序)
pub fn page_clause(order: Option<&str>, limit: usize, offset: usize, dialect: Dialect) -> String {
    match (dialect.mssql(), order) {
        (true, order) => format!(
            " ORDER BY {} OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY",
            order.unwrap_or("(SELECT NULL)")
        ),
        (false, Some(o)) => format!(" ORDER BY {o} LIMIT {limit} OFFSET {offset}"),
        (false, None) => format!(" LIMIT {limit} OFFSET {offset}"),
    }
}

/// 快速筛选:所有列 LIKE '%关键词%'(SQL Server 会把非字符串列隐式转成文本)
pub fn build_like_filter(columns: &[String], query: &str, dialect: Dialect) -> String {
    let pattern = literal(&format!("%{query}%"), dialect);
    // 整体加括号:与筛选栏条件 AND 拼接时,无括号的 OR 串会被 SQL 优先级改写语义
    format!(
        "({})",
        columns
            .iter()
            .map(|c| format!("{} LIKE {}", quote(c, dialect.mysql()), pattern))
            .collect::<Vec<_>>()
            .join(" OR ")
    )
}

/// 条件行的 WHERE:数值原样,其余按字符串字面量(LIKE 语义 = 包含,两侧补通配符,
/// 用户自带 % 则原样保留);自由 WHERE 原样并入
pub fn build_cond_where(
    conds: &[(String, String, String)],
    free: &str,
    dialect: Dialect,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for (col, op, val) in conds {
        let op = op.trim();
        if op.is_empty() {
            continue;
        }
        let ident = quote(col, dialect.mysql());
        if op == "IS NULL" || op == "IS NOT NULL" {
            parts.push(format!("{ident} {op}"));
            continue;
        }
        let v = val.trim();
        if v.is_empty() {
            continue;
        }
        let lit = if v.parse::<f64>().map(|f| f.is_finite()).unwrap_or(false) && op != "LIKE" {
            v.to_string()
        } else {
            let wrapped = if op == "LIKE" && !v.contains('%') {
                format!("%{v}%")
            } else {
                v.to_string()
            };
            literal(&wrapped, dialect)
        };
        parts.push(format!("{ident} {op} {lit}"));
    }
    let free_t = free.trim();
    if !free_t.is_empty() {
        parts.push(free_t.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

pub(crate) fn quote(name: &str, mysql: bool) -> String {
    if mysql {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// 字符串字面量(统一加引号,各库都能隐式接受数字/日期文本)。
/// MySQL 默认把反斜杠当转义符,须同时双写,否则尾部反斜杠会吃掉收尾引号;
/// SQL Server 必须 N'…':普通字面量按库的默认排序规则代码页转换,西文排序规则的
/// 实例上中文会变成 ?(写进 nvarchar 列、拿来做等值比较都一样)
pub fn literal(v: &str, dialect: Dialect) -> String {
    let mut out = v.replace('\'', "''");
    if dialect.mysql() {
        out = out.replace('\\', "\\\\");
    }
    if dialect.mssql() {
        return format!("N'{out}'");
    }
    format!("'{out}'")
}

fn where_of(pk: &[(String, String)], dialect: Dialect) -> Result<String, String> {
    if pk.is_empty() {
        return Err("该行没有主键,无法定位更新".into());
    }
    Ok(pk
        .iter()
        .map(|(c, v)| format!("{} = {}", quote(c, dialect.mysql()), literal(v, dialect)))
        .collect::<Vec<_>>()
        .join(" AND "))
}

/// 生成全部回写语句(纯函数:保存前预览与实际执行共用同一段构建逻辑)
pub fn build_change_stmts(
    info: &ConnInfo,
    table: &str,
    database: Option<&str>,
    updates: &[CellUpdate],
    deletes: &[Vec<(String, String)>],
    inserts: &[Vec<(String, String)>],
) -> Result<Vec<String>, String> {
    let dialect = Dialect::of(info.db_type);
    let mysql = dialect.mysql();
    // PG 的「库」= 连接级(dbname),SQL 不允许跨库引用("e2e_a"."users" 直接报
    // relation does not exist)——database 限定只对 MySQL/ClickHouse 生效
    let table = match info.db_type {
        // 插件驱动:库限定由 agent 会话解释,语句走 ANSI 双引号样式(契约文档约定)
        crate::model::DbType::Postgres | crate::model::DbType::Plugin => {
            qualified_table(table, None, false)
        }
        _ => page_table_ref(database, table, dialect),
    };
    let mut stmts: Vec<String> = Vec::new();
    for u in updates {
        if u.sets.is_empty() {
            continue;
        }
        let sets = u
            .sets
            .iter()
            .map(|(c, v)| match v {
                Some(s) => format!("{} = {}", quote(c, mysql), literal(s, dialect)),
                None => format!("{} = NULL", quote(c, mysql)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        stmts.push(format!(
            "UPDATE {} SET {} WHERE {}",
            table,
            sets,
            where_of(&u.pk, dialect)?
        ));
    }
    for pk in deletes {
        stmts.push(format!(
            "DELETE FROM {} WHERE {}",
            table,
            where_of(pk, dialect)?
        ));
    }
    for ins in inserts {
        if ins.is_empty() {
            continue;
        }
        let cols = ins.iter().map(|(c, _)| quote(c, mysql)).collect::<Vec<_>>().join(", ");
        let vals = ins
            .iter()
            .map(|(_, v)| literal(v, dialect))
            .collect::<Vec<_>>()
            .join(", ");
        stmts.push(format!("INSERT INTO {} ({}) VALUES ({})", table, cols, vals));
    }
    if stmts.is_empty() {
        return Err("没有待保存的更改".into());
    }
    // ClickHouse:UPDATE/DELETE 是 mutation,必须走 ALTER TABLE 语法
    if info.db_type == crate::model::DbType::ClickHouse {
        stmts = stmts
            .into_iter()
            .map(|s| {
                if let Some(rest) = s.strip_prefix("UPDATE ") {
                    // rest = "<表> SET <sets> WHERE <w>";CH mutation 要求保留 UPDATE 关键字
                    match rest.split_once(" SET ") {
                        Some((tbl, sets_where)) => format!("ALTER TABLE {tbl} UPDATE {sets_where}"),
                        None => s,
                    }
                } else if let Some(rest) = s.strip_prefix("DELETE FROM ") {
                    format!(
                        "ALTER TABLE {} DELETE WHERE {}",
                        table,
                        rest.split_once(" WHERE ")
                            .map(|(_, w)| w.to_string())
                            .unwrap_or_default()
                    )
                } else {
                    s
                }
            })
            .collect();
    }
    Ok(stmts)
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
    let stmts = build_change_stmts(info, table, database, updates, deletes, inserts)?;

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
            let client = Backend::pg_client(mp).await?;
            client
                .batch_execute("BEGIN")
                .await
                .map_err(crate::backend::pg_err)?;
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
                        return Err(crate::backend::pg_err(e));
                    }
                }
            }
            client
                .batch_execute("COMMIT")
                .await
                .map_err(crate::backend::pg_err)?;
            affected
        }
        Backend::Redis(_) => return Err("Redis 请使用专门的命令通道".into()),
        Backend::Plugin(s) => {
            // 契约 v1.1:execute_batch —— 语句打包一次提交,原子性由 agent 实现
            // (文档要求 agent 内部事务;MySQL 反引号不适用,语句走 ANSI 双引号)
            let stmts = build_change_stmts(info, table, database, updates, deletes, inserts)?;
            if stmts.is_empty() {
                return Ok(ExecResult {
                    columns: vec![],
                    rows: vec![],
                    affected: 0,
                    truncated: false,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                    column_types: vec![],
                });
            }
            let params = serde_json::json!({
                "database": s.database,
                "sqls": stmts,
            });
            let r = s.call("execute_batch", &params).await?;
            let affected = r.get("affected").and_then(|v| v.as_u64()).unwrap_or(0);
            return Ok(ExecResult {
                columns: vec![],
                rows: vec![],
                affected,
                truncated: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                column_types: vec![],
            });
        }
        #[cfg(feature = "heavy-mw")]
        Backend::Mongo(_) => return Err("MongoDB 不支持 SQL 回写".into()),
        Backend::Mssql(st) => {
            // 与 PG 同构:BEGIN/逐条/COMMIT,全程同一条连接
            let mut client = Backend::mssql_client(st).await?;
            crate::mssql::exec(&mut client, "BEGIN TRAN").await?;
            let mut affected = 0u64;
            for sql in &stmts {
                match crate::mssql::exec(&mut client, sql).await {
                    Ok(n) => affected += n,
                    Err(e) => {
                        let _ = crate::mssql::exec(&mut client, "ROLLBACK TRAN").await;
                        return Err(e);
                    }
                }
            }
            crate::mssql::exec(&mut client, "COMMIT TRAN").await?;
            affected
        }
        Backend::ClickHouse(st) => {
            // ClickHouse 无事务:逐条执行,任一条失败即中止(语句已过半不回滚)
            for sql in &stmts {
                crate::clickhouse::query(st, sql, None, 1).await.map_err(|e| e.to_string())?;
            }
            // mutation 异步生效,affected 不可得
            0u64
        }
        // 中间件族统一无 SQL 回写(此臂实际不可达:build_change_stmts 已拒;
        // 保留为穷举兜底,防止未来新类型漏检)
        Backend::Redis(_)
        | Backend::Rabbit(_)
        | Backend::Kv(_) => {
            let _e: String = "该数据源无 SQL 回写".into();
            0u64
        }
        #[cfg(feature = "heavy-mw")]
        Backend::Kafka(_) | Backend::Mqtt(_) => {
            let _e: String = "该数据源无 SQL 回写".into();
            0u64
        }
    };

    Ok(ExecResult {
        columns: vec![],
        rows: vec![],
        affected,
        truncated: false,
        elapsed_ms: started.elapsed().as_millis() as u64,
        column_types: vec![],
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

    #[test]
    fn pg_write_ignores_database_qualifier() {
        // E2E 实测回归:PG 带 database 的单元格回写曾拼出 "e2e_a"."users"
        // 直接报 relation does not exist(库在 PG 是连接级,不可跨库引用)
        let info = ConnInfo {
            id: "c".into(),
            name: "c".into(),
            db_type: crate::model::DbType::Postgres,
            host: None,
            port: None,
            user: None,
            password: None,
            database: Some("e2e_a".into()),
            file_path: None,
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        };
        let stmts = build_change_stmts(
            &info,
            "users",
            Some("e2e_a"),
            &[CellUpdate {
                pk: vec![("id".into(), "2".into())],
                sets: vec![("age".into(), Some("35".into()))],
            }],
            &[],
            &[],
        )
        .unwrap();
        assert!(
            stmts.iter().all(|s| !s.contains("\"e2e_a\"")),
            "{stmts:?}"
        );
        assert_eq!(
            stmts[0],
            "UPDATE \"users\" SET \"age\" = '35' WHERE \"id\" = '2'"
        );
    }

    fn conn(db_type: crate::model::DbType) -> ConnInfo {
        ConnInfo {
            id: "t".into(),
            name: "t".into(),
            db_type,
            host: None,
            port: None,
            user: None,
            password: None,
            database: Some("e2e".into()),
            file_path: None,
            color: None,
            read_only: false,
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        }
    }

    /// SQL Server 实连回归(E2E 实测):带库回写曾拼出 "e2e"."users"——两段式名字
    /// 在 T-SQL 里是「架构.表」,报 Invalid object name;字符串不带 N 前缀时
    /// 中文被按代码页转换,写进 nvarchar 列成了 ???
    #[test]
    fn mssql_write_uses_three_part_name_and_unicode_literal() {
        let updates = [CellUpdate {
            pk: vec![("id".into(), "2".into())],
            sets: vec![
                ("name".into(), Some("李小四".into())),
                ("note".into(), None),
            ],
        }];
        let stmts = build_change_stmts(
            &conn(crate::model::DbType::Mssql),
            "users",
            Some("e2e"),
            &updates,
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(
            stmts[0],
            "UPDATE \"e2e\"..\"users\" SET \"name\" = N'李小四', \"note\" = NULL WHERE \"id\" = N'2'"
        );
        // 不带库 = 连接当前库,只引注表名
        let stmts =
            build_change_stmts(&conn(crate::model::DbType::Mssql), "users", None, &updates, &[], &[])
                .unwrap();
        let first = &stmts[0];
        assert!(first.starts_with("UPDATE \"users\" SET"), "{first}");
        // 其余方言的字面量不变
        let std = build_change_stmts(
            &conn(crate::model::DbType::Sqlite),
            "users",
            None,
            &[CellUpdate {
                pk: vec![("id".into(), "1".into())],
                sets: vec![("name".into(), Some("it's".into()))],
            }],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(std[0], "UPDATE \"users\" SET \"name\" = 'it''s' WHERE \"id\" = '1'");
    }

    /// SQL Server 没有 LIMIT/OFFSET(实连实测:带上即语法错,数据页打不开);
    /// 分页是 ORDER BY … OFFSET n ROWS FETCH NEXT m ROWS ONLY,且必须带 ORDER BY
    #[test]
    fn page_clause_per_dialect() {
        use Dialect::{Mssql, Mysql, Standard};
        assert_eq!(page_clause(None, 200, 0, Standard), " LIMIT 200 OFFSET 0");
        assert_eq!(page_clause(None, 50, 100, Mysql), " LIMIT 50 OFFSET 100");
        assert_eq!(
            page_clause(Some("\"id\" DESC"), 50, 100, Standard),
            " ORDER BY \"id\" DESC LIMIT 50 OFFSET 100"
        );
        assert_eq!(
            page_clause(None, 200, 0, Mssql),
            " ORDER BY (SELECT NULL) OFFSET 0 ROWS FETCH NEXT 200 ROWS ONLY"
        );
        assert_eq!(
            page_clause(Some("\"id\" DESC"), 50, 100, Mssql),
            " ORDER BY \"id\" DESC OFFSET 100 ROWS FETCH NEXT 50 ROWS ONLY"
        );
    }

    #[test]
    fn page_table_ref_per_dialect() {
        use Dialect::{Mssql, Mysql, Standard};
        assert_eq!(page_table_ref(Some("shop"), "users", Mysql), "`shop`.`users`");
        assert_eq!(page_table_ref(None, "users", Mysql), "`users`");
        assert_eq!(page_table_ref(Some("pub"), "users", Standard), "\"pub\".\"users\"");
        assert_eq!(page_table_ref(None, "users", Standard), "\"users\"");
        // 三段式 db..table:架构取默认;空串库视同未设置
        assert_eq!(page_table_ref(Some("e2e"), "users", Mssql), "\"e2e\"..\"users\"");
        assert_eq!(page_table_ref(Some(""), "users", Mssql), "\"users\"");
    }

    #[test]
    fn like_filter_escapes_and_quotes() {
        use Dialect::{Mssql, Mysql, Standard};
        let cols = vec!["id".to_string(), "na`me".to_string()];
        // 值里的单引号翻倍;列名里的方言引号翻倍
        let mysql = build_like_filter(&cols, "o'brien", Mysql);
        assert!(mysql.contains("`id` LIKE '%o''brien%'"), "{mysql}");
        assert!(mysql.contains("`na``me` LIKE '%o''brien%'"), "{mysql}");
        let pg_cols = vec!["na\"me".to_string()];
        let pg = build_like_filter(&pg_cols, "x", Standard);
        assert!(pg.contains("\"na\"\"me\" LIKE '%x%'"), "{pg}");
        // SQL Server:字面量带 N 前缀,否则中文筛不到(实连实测 0 行)
        let mssql = build_like_filter(&pg_cols, "张", Mssql);
        assert!(mssql.contains("\"na\"\"me\" LIKE N'%张%'"), "{mssql}");
    }

    #[test]
    fn cond_where_builder() {
        use Dialect::{Mssql, Mysql, Standard};
        let conds = vec![
            ("age".to_string(), ">".to_string(), "18".to_string()),
            ("name".to_string(), "LIKE".to_string(), "张%".to_string()),
            ("del".to_string(), "IS NULL".to_string(), String::new()),
            ("empty".to_string(), "=".to_string(), String::new()),
        ];
        let w = build_cond_where(&conds, "", Mysql).unwrap();
        assert!(w.contains("`age` > 18"), "{w}");
        assert!(w.contains("`name` LIKE '张%'"), "{w}");
        assert!(w.contains("`del` IS NULL"), "{w}");
        assert!(!w.contains("empty"), "{w}");
        let w = build_cond_where(&conds, "", Mssql).unwrap();
        assert!(w.contains("\"age\" > 18"), "{w}");
        // 用户自带 % 的值原样保留,只多出 N 前缀
        assert!(w.contains("\"name\" LIKE N'张%'"), "{w}");
        let wrapped = build_cond_where(&[("n".into(), "LIKE".into(), "李".into())], "", Mssql).unwrap();
        assert_eq!(wrapped, "\"n\" LIKE N'%李%'");
        // 自由 WHERE 并入
        let w2 = build_cond_where(&[], "status = 1 AND amount > 0", Standard).unwrap();
        assert_eq!(w2, "status = 1 AND amount > 0");
        assert!(build_cond_where(&[], "", Mysql).is_none());
    }
}

#[cfg(test)]
/// 保存前 SQL 预览与实际执行共用构建逻辑(纯函数):语句内容/顺序/引注一致性
#[test]
fn build_change_stmts_shapes() {
    use crate::model::{ConnInfo, DbType};
    let info = ConnInfo {
        id: "t".into(),
        name: "t".into(),
        db_type: DbType::MySql,
        host: None,
        port: None,
        user: None,
        password: None,
        database: Some("db".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };
    let updates = vec![crate::edits::CellUpdate {
        pk: vec![("id".into(), "1".into())],
        sets: vec![("name".into(), Some("张三".into())), ("age".into(), None)],
    }];
    let stmts =
        build_change_stmts(&info, "users", Some("db"), &updates, &[], &[]).unwrap();
    assert_eq!(stmts.len(), 1);
    assert_eq!(
        stmts[0],
        "UPDATE `db`.`users` SET `name` = '张三', `age` = NULL WHERE `id` = '1'"
    );
    // 空集 = 错误(与执行端语义一致)
    assert!(build_change_stmts(&info, "users", None, &[], &[], &[]).is_err());

    // ClickHouse mutation 改写:UPDATE 保留 UPDATE 关键字(ALTER TABLE t UPDATE …),
    // DELETE 走 ALTER TABLE t DELETE WHERE
    let mut ch = info.clone();
    ch.db_type = DbType::ClickHouse;
    let deletes = vec![vec![("id".into(), "1".into())]];
    let ch_stmts =
        build_change_stmts(&ch, "users", Some("db"), &updates, &deletes, &[]).unwrap();
    assert_eq!(ch_stmts.len(), 2);
    assert_eq!(
        ch_stmts[0],
        "ALTER TABLE `db`.`users` UPDATE `name` = '张三', `age` = NULL WHERE `id` = '1'",
        "CH mutation 必须带 UPDATE 关键字"
    );
    assert_eq!(
        ch_stmts[1],
        "ALTER TABLE `db`.`users` DELETE WHERE `id` = '1'"
    );
}

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
            perm: None,
            group: None,
            plugin: None,
            ssh: None,
        };
        let b = crate::backend::connect(&info, None, None).await.unwrap();
        (b, info)
    }

    /// 按行合批:同一行多列改动合并为一条 UPDATE;事务:中途失败整体回滚
    #[tokio::test]
    async fn apply_changes_groups_rows_and_rolls_back() {
        let dir = std::env::temp_dir().join(format!("shujing_tx_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tx.db");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::write(&path, b"");
        let (mut b, info) = test_backend(path.to_str().unwrap()).await;

        b.run_sql(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, age INT, UNIQUE(name))",
            100,
            None,
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
