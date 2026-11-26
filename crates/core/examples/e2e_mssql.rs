//! SQL Server 全链路 E2E(直调 Service):连接/库/表/视图/结构(主键、nvarchar
//! 字符数、多索引列不重复、索引列表、拼装 DDL 改名回建逐项比对)/计数/查询(各类型的值文本、列类型、NULL、空结果集
//! 列头、截断、多语句取首集、错误上报、写语句影响行数)/单元格回写(中文、置
//! NULL、新增、删除、出错回滚且不留悬挂事务)/跨库限定回写。
//! 运行:先起本机容器(见 crates/core/src/exec.rs 的 live_mssql 测试注释),然后
//! cargo run -p shujing-core --example e2e_mssql
//! 连接参数可用环境变量覆盖:E2E_MSSQL_HOST/E2E_MSSQL_PORT/E2E_MSSQL_USER/E2E_MSSQL_PASS

use shujing_core::edits::CellUpdate;
use shujing_core::{ConnInfo, DbType, Service};
use tokio_util::compat::TokioAsyncWriteCompatExt;

fn pass(name: &str, detail: &str) {
    println!("PASS  {name}  {detail}");
}

fn fail(name: &str, err: &str) -> ! {
    println!("FAIL  {name}  {err}");
    std::process::exit(1);
}

fn first(r: &shujing_core::ExecResult, row: usize, col: usize) -> Option<String> {
    r.rows.get(row)?.get(col)?.clone()
}

/// 执行查询,取各行的第 0 列文本
async fn ids_of(service: &Service, sql: String) -> Result<Vec<String>, String> {
    service
        .run_sql("e2e-mssql", sql, None)
        .await?
        .pop()
        .ok_or_else(|| "无结果集".to_string())
        .map(|r| {
            r.rows
                .iter()
                .map(|row| row[0].clone().unwrap_or_default())
                .collect()
        })
}

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join("shujing_e2e_mssql_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(&dir));

    let host = std::env::var("E2E_MSSQL_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("E2E_MSSQL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1434);
    let user = std::env::var("E2E_MSSQL_USER").unwrap_or_else(|_| "sa".into());
    let pass_ = std::env::var("E2E_MSSQL_PASS").unwrap_or_else(|_| "Shujing#2026test".into());

    // 0) 幂等重置:直接驱动建库建表灌种子
    {
        let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await.expect("raw tcp");
        let mut cfg = tiberius::Config::new();
        cfg.host(host.as_str());
        cfg.port(port);
        cfg.authentication(tiberius::AuthMethod::sql_server(&user, &pass_));
        cfg.trust_cert();
        let mut conn = tiberius::Client::connect(cfg, tcp.compat_write()).await.expect("raw mssql conn");
        for sql in [
            "IF DB_ID('e2e_mssql') IS NOT NULL BEGIN ALTER DATABASE e2e_mssql SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE e2e_mssql END",
            "IF DB_ID('e2e_mssql2') IS NOT NULL BEGIN ALTER DATABASE e2e_mssql2 SET SINGLE_USER WITH ROLLBACK IMMEDIATE; DROP DATABASE e2e_mssql2 END",
            "CREATE DATABASE e2e_mssql",
            "CREATE DATABASE e2e_mssql2",
            "CREATE TABLE e2e_mssql.dbo.users (\
             id INT IDENTITY PRIMARY KEY, \
             name NVARCHAR(64) NULL, \
             age INT NULL, \
             score FLOAT NULL, \
             active BIT NOT NULL DEFAULT 1, \
             note VARCHAR(100) NULL, /* 西文排序规则的实例上 varchar 存不了中文,种子用西文 */ \
             created DATETIME2(3) NULL, \
             birthday DATE NULL, \
             amount DECIMAL(10,2) NULL, \
             uid UNIQUEIDENTIFIER NULL, \
             tiny_col TINYINT NULL, \
             big_col BIGINT NULL)",
            // id / age 各进多个索引:结构查询若按索引行展开,列会重复
            "CREATE INDEX ix_users_age ON e2e_mssql.dbo.users (age)",
            "CREATE INDEX ix_users_id_age ON e2e_mssql.dbo.users (id, age)",
            // CREATE VIEW 不允许库名前缀,借 sp_executesql 进到目标库上下文
            "EXEC e2e_mssql.dbo.sp_executesql N'CREATE VIEW dbo.v_users AS SELECT id, name FROM dbo.users'",
            "INSERT INTO e2e_mssql.dbo.users (name, age, score, active, note, created, birthday, amount, uid, tiny_col, big_col) VALUES \
             (N'张三', 28, 91.5, 1, 'A', '2026-11-24T12:34:56.789', '2026-11-24', 1234.50, NEWID(), 3, 9007199254740993), \
             (N'李四', 34, 88.25, 0, NULL, NULL, NULL, NULL, NULL, NULL, NULL), \
             (N'王五', NULL, 72.0, 1, 'C', NULL, NULL, NULL, NULL, NULL, NULL), \
             (N'赵六', 19, 95.75, 1, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
            "CREATE TABLE e2e_mssql.dbo.dept (id INT PRIMARY KEY, dname NVARCHAR(40) NULL)",
            // 被引用表先建:CREATE TABLE 里的外键不校验顺序,但引用不存在的表直接报错
            "CREATE TABLE e2e_mssql.dbo.badge2 (code VARCHAR(10) NOT NULL, dept INT NOT NULL, \
             CONSTRAINT PK_badge2 PRIMARY KEY CLUSTERED (code, dept))",
            "CREATE TABLE e2e_mssql.dbo.emp (\
             id INT PRIMARY KEY, \
             dept_id INT NOT NULL REFERENCES e2e_mssql.dbo.dept(id), \
             badge VARCHAR(10), \
             dept_code INT, \
             FOREIGN KEY (badge, dept_code) REFERENCES e2e_mssql.dbo.badge2(code, dept))",
            "CREATE TABLE e2e_mssql2.dbo.marker (id INT PRIMARY KEY, val NVARCHAR(20))",
            "INSERT INTO e2e_mssql2.dbo.marker VALUES (1, N'跨库值')",
        ] {
            let _ = conn.simple_query(sql).await.unwrap().into_results().await.unwrap();
        }
    }

    let info = ConnInfo {
        id: "e2e-mssql".into(),
        name: "e2e-mssql".into(),
        db_type: DbType::Mssql,
        host: Some(host.clone()),
        port: Some(port),
        user: Some(user.clone()),
        password: Some(pass_.clone()),
        database: Some("e2e_mssql".into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        group: None,
        plugin: None,
        ssh: None,
    };
    let db = Some("e2e_mssql".to_string());
    let run = |sql: &str| {
        let service = service.clone();
        let sql = sql.to_string();
        async move { service.run_sql("e2e-mssql", sql, None).await }
    };
    // 单元格文本:查询并取 [row][col]
    let cell = |sql: &'static str, row: usize, col: usize| {
        let service = service.clone();
        async move {
            let rs = service.run_sql("e2e-mssql", sql.to_string(), None).await;
            rs.and_then(|rs| rs.into_iter().next().map(|r| first(&r, row, col)).ok_or_else(|| "无结果集".to_string()))
        }
    };

    // 1) 连接 + 版本(单行)
    match service.connect(info.clone()).await {
        Ok(c) => {
            if c.version.contains('\n') {
                fail("connect", &format!("版本串应为一行: {:?}", c.version));
            }
            pass("connect", &c.version);
        }
        Err(e) => fail("connect", &e),
    }

    // 2) 库列表:含 e2e_mssql / e2e_mssql2,不含系统库
    match service.list_databases("e2e-mssql").await {
        Ok(dbs) => {
            let ok = dbs.iter().any(|d| d == "e2e_mssql")
                && dbs.iter().any(|d| d == "e2e_mssql2")
                && !dbs.iter().any(|d| d == "master" || d == "tempdb");
            if !ok {
                fail("list_databases", &format!("{dbs:?}"));
            }
            pass("list_databases", &format!("{} dbs", dbs.len()));
        }
        Err(e) => fail("list_databases", &e),
    }

    // 3) 表与视图
    match service.list_tables("e2e-mssql", Some("e2e_mssql".into())).await {
        Ok(ts) => {
            let kind = |n: &str| ts.iter().find(|t| t.name == n).map(|t| t.kind.clone());
            if kind("users").as_deref() != Some("table") || kind("v_users").as_deref() != Some("view") {
                fail("list_tables", &format!("{ts:?}"));
            }
            pass("list_tables", &format!("{} objects", ts.len()));
        }
        Err(e) => fail("list_tables", &e),
    }

    // 4) 结构:12 列各出现一次(主键列和 age 都进了多个索引)、只有 id=PRI、nvarchar 按字符数
    match service.get_table_structure("e2e-mssql", "users", db.clone()).await {
        Ok(s) => {
            let ty = |n: &str| {
                s.columns.iter().find(|c| c.name == n).map(|c| c.data_type.clone())
            };
            let problems: Vec<String> = [
                (s.columns.len() != 12, format!("cols={}", s.columns.len())),
                (s.columns.iter().filter(|c| c.name == "id").count() != 1, "id 列重复".into()),
                (s.columns.iter().find(|c| c.key == "PRI").map(|c| c.name.as_str()) != Some("id"), "主键不是 id".into()),
                (ty("name").as_deref() != Some("nvarchar(64)"), format!("name={:?}", ty("name"))),
                (ty("note").as_deref() != Some("varchar(100)"), format!("note={:?}", ty("note"))),
                (ty("amount").as_deref() != Some("decimal(10,2)"), format!("amount={:?}", ty("amount"))),
                (s.columns.iter().find(|c| c.name == "id").map(|c| c.nullable) != Some(false), "id 应为 NOT NULL".into()),
                (s.columns.iter().find(|c| c.name == "age").map(|c| c.nullable) != Some(true), "age 应为可空".into()),
                (s.columns.iter().find(|c| c.name == "active").and_then(|c| c.default.as_deref()).is_none(), "active 缺默认值".into()),
            ]
            .into_iter()
            .filter(|(bad, _)| *bad)
            .map(|(_, msg)| msg)
            .collect();
            if !problems.is_empty() {
                fail("structure", &format!("{};全部列: {:?}", problems.join("; "), s.columns));
            }
            pass("structure", "12 cols, pk=id, nvarchar 按字符数, 多索引列不重复");
        }
        Err(e) => fail("structure", &e),
    }

    // 4b) 索引列表:三个索引(PK + 两个普通),键列按序、唯一性正确
    // 4c) 建表语句:拼装 DDL 必须可执行——全文改名回建,再和原表逐项比对列与索引
    match service.get_table_structure("e2e-mssql", "users", db.clone()).await {
        Ok(orig) => {
            let find = |n: &str| orig.indexes.iter().find(|ix| ix.name == n).map(|ix| ix.columns.as_str());
            let pk = orig.indexes.iter().find(|ix| ix.name.starts_with("PK__users"));
            let problems: Vec<String> = [
                (orig.indexes.len() != 3, format!("indexes={:?}", orig.indexes)),
                (find("ix_users_age") != Some("age"), format!("ix_users_age={:?}", find("ix_users_age"))),
                (find("ix_users_id_age") != Some("id, age"), format!("ix_users_id_age={:?}", find("ix_users_id_age"))),
                (pk.map(|ix| (ix.unique, ix.columns.as_str())) != Some((true, "id")), "PK 索引应为 unique(id)".into()),
                (!orig.ddl.contains("CREATE TABLE [users] ("), "DDL 缺 CREATE TABLE".into()),
                (!orig.ddl.contains("[id] int NOT NULL,"), "DDL 缺 id 列".into()),
                (!orig.ddl.contains("[name] nvarchar(64) NULL,"), "DDL 缺 name 列".into()),
                (!orig.ddl.contains("[active] bit NOT NULL DEFAULT "), "DDL 缺默认值".into()),
                (!orig.ddl.contains("PRIMARY KEY CLUSTERED ([id])"), "DDL 缺主键约束".into()),
                (!orig.ddl.contains("CREATE INDEX [ix_users_age] ON [users] ([age]);"), "DDL 缺 ix_users_age".into()),
                (!orig.ddl.contains("CREATE INDEX [ix_users_id_age] ON [users] ([id], [age]);"), "DDL 缺 ix_users_id_age".into()),
            ]
            .into_iter()
            .filter(|(bad, _)| *bad)
            .map(|(_, msg)| msg)
            .collect();
            if !problems.is_empty() {
                fail("structure_indexes", &format!("{};DDL={}索引={:?}",
                    problems.join("; "), orig.ddl, orig.indexes));
            }
            pass("structure_indexes", "3 个索引(PK unique[id] + 2 普通),键列按序");

            // 改名回建:拼出来的 DDL 当真执行(users → users_ddl_copy,连主键约束名一起换)
            let copy = orig.ddl.replace("users", "users_ddl_copy");
            if let Err(e) = service.run_sql("e2e-mssql", copy, None).await {
                fail("ddl_executable", &format!("回建失败: {e}"));
            }
            match service.get_table_structure("e2e-mssql", "users_ddl_copy", db.clone()).await {
                Ok(c) => {
                    // 列逐项一致(拼装边界:IDENTITY 不进 DDL,extra 两边都是空)
                    let cols_same = orig.columns.len() == c.columns.len()
                        && orig.columns.iter().zip(&c.columns).all(|(a, b)| {
                            a.name == b.name
                                && a.data_type == b.data_type
                                && a.nullable == b.nullable
                                && a.default == b.default
                                && a.key == b.key
                        });
                    // 索引:PK 换了名,只比唯一性与键列;普通索引名与列都要一致
                    fn find_ix<'a>(
                        st: &'a shujing_core::TableStructure,
                        n: &str,
                    ) -> Option<&'a shujing_core::IndexDef> {
                        st.indexes.iter().find(|ix| ix.name == n)
                    }
                    // 索引名被全文改名(users → users_ddl_copy),按改名后的名字查
                    let ix_same = c.indexes.len() == orig.indexes.len()
                        && find_ix(&c, "ix_users_ddl_copy_age").map(|ix| (&ix.columns, ix.unique))
                            == Some((&"age".to_string(), false))
                        && find_ix(&c, "ix_users_ddl_copy_id_age").map(|ix| (&ix.columns, ix.unique))
                            == Some((&"id, age".to_string(), false))
                        && c.indexes.iter().filter(|ix| ix.unique).count()
                            == orig.indexes.iter().filter(|ix| ix.unique).count();
                    if !cols_same {
                        fail("ddl_executable", &format!("回建后列不一致:\n{:?}\n{:?}", orig.columns, c.columns));
                    }
                    if !ix_same {
                        fail("ddl_executable", &format!("回建后索引不一致:\n{:?}\n{:?}", orig.indexes, c.indexes));
                    }
                    pass("ddl_executable", "拼装 DDL 回建成功,列/索引逐项一致");
                }
                Err(e) => fail("ddl_executable", &format!("回建后查不到结构: {e}")),
            }
            if let Err(e) = service.run_sql("e2e-mssql", "DROP TABLE users_ddl_copy".into(), None).await {
                fail("ddl_cleanup", &e);
            }
        }
        Err(e) => fail("structure_indexes", &e),
    }

    // 5) 计数
    match service.count_rows("e2e-mssql", "users").await {
        Ok(4) => pass("count_rows", "4"),
        Ok(n) => fail("count_rows", &format!("expect 4 got {n}")),
        Err(e) => fail("count_rows", &e),
    }

    // 6) 查询:各类型的值文本 + 列类型 + NULL
    match run("SELECT * FROM users ORDER BY id").await {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            let expect_types = [
                "int", "nvarchar", "int", "float", "bit", "varchar", "datetime2", "date", "decimal",
                "uniqueidentifier", "tinyint", "bigint",
            ];
            let uid: Option<String> = r.rows[0][9].clone();
            let expect_row0: [Option<String>; 12] = [
                Some("1".into()), Some("张三".into()), Some("28".into()), Some("91.5".into()),
                Some("1".into()), Some("A".into()), Some("2026-11-24 12:34:56.789".into()),
                Some("2026-11-24".into()), Some("1234.50".into()), uid.clone(),
                Some("3".into()), Some("9007199254740993".into()),
            ];
            let problems: Vec<String> = [
                (r.columns.len() != 12, format!("cols={:?}", r.columns)),
                (r.column_types != expect_types, format!("types={:?}", r.column_types)),
                (r.rows.len() != 4, format!("rows={}", r.rows.len())),
                (r.rows[0] != expect_row0, format!("row0={:?}", r.rows[0])),
                (r.rows[1][4].as_deref() != Some("0"), format!("active1={:?}", r.rows[1][4])),
                (r.rows[1][5].is_some(), format!("note1(null)={:?}", r.rows[1][5])),
                (r.rows[2][2].is_some(), format!("age2(null)={:?}", r.rows[2][2])),
                (r.rows[2][3].as_deref() != Some("72.0"), format!("score2={:?}", r.rows[2][3])),
                (r.rows[1][9].is_some(), format!("uid1(null)={:?}", r.rows[1][9])),
                (
                    uid.as_deref().map(|u| u.len() == 36 && u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
                        != Some(true),
                    format!("uid 格式={uid:?}"),
                ),
            ]
            .into_iter()
            .filter(|(bad, _)| *bad)
            .map(|(_, msg)| msg)
            .collect();
            if !problems.is_empty() {
                fail("run_sql", &problems.join("; "));
            }
            pass("run_sql", "中文/整数/浮点/位/日期/小数/UUID/NULL 正确");
        }
        Err(e) => fail("run_sql", &e),
    }

    // 7) 空结果集也有列头
    match run("SELECT id, name FROM users WHERE 1=0").await {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.columns != ["id", "name"] || r.column_types != ["int", "nvarchar"] || !r.rows.is_empty() {
                fail("empty_result", &format!("cols={:?} types={:?} rows={}", r.columns, r.column_types, r.rows.len()));
            }
            pass("empty_result", "0 行但列头齐全");
        }
        Err(e) => fail("empty_result", &e),
    }

    // 8) 截断:4 行按 max=2 截断
    match service.run_sql_db("e2e-mssql", "SELECT id FROM users ORDER BY id".into(), Some(2), None).await {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.rows.len() != 2 || !r.truncated {
                fail("truncate", &format!("rows={} truncated={}", r.rows.len(), r.truncated));
            }
            pass("truncate", "4 行按 2 截断并如实标记");
        }
        Err(e) => fail("truncate", &e),
    }

    // 9) 多语句取第一个结果集
    match run("SELECT 1 AS a; SELECT 2 AS b").await {
        Ok(rs) => {
            let r = rs.first().expect("结果集");
            if r.columns != ["a"] || first(r, 0, 0).as_deref() != Some("1") {
                fail("multi_statement", &format!("cols={:?} row0={:?}", r.columns, r.rows.first()));
            }
            pass("multi_statement", "取首集");
        }
        Err(e) => fail("multi_statement", &e),
    }

    // 10) SQL 错误要上报
    match run("SELECT 1/0 AS x").await {
        Err(e) if e.contains("Divide by zero") || e.contains("除以零") => pass("sql_error", "错误上报"),
        Err(e) => pass("sql_error", &format!("错误上报: {e}")),
        Ok(_) => fail("sql_error", "除零错误被吞"),
    }

    // 11) 写语句的影响行数
    match run("UPDATE users SET age = 36 WHERE id = 2").await {
        Ok(rs) => match rs.first().and_then(|r| (r.columns.is_empty()).then_some(r.affected)) {
            Some(1) => pass("dml_affected", "UPDATE 影响 1 行"),
            other => fail("dml_affected", &format!("{other:?}")),
        },
        Err(e) => fail("dml_affected", &e),
    }
    match cell("SELECT age FROM users WHERE id = 2", 0, 0).await {
        Ok(v) if v.as_deref() == Some("36") => pass("dml_effect", "重读一致"),
        Ok(v) => fail("dml_effect", &format!("age={v:?}")),
        Err(e) => fail("dml_effect", &e),
    }

    // 12) 单元格回写:中文 + 置 NULL,重读一致
    let upd = vec![CellUpdate {
        pk: vec![("id".into(), "2".into())],
        sets: vec![("name".into(), Some("李小四".into())), ("note".into(), None), ("amount".into(), Some("-9.5".into()))],
    }];
    match service.apply_changes("e2e-mssql", "users", db.as_deref(), &upd, &[], &[]).await {
        Ok(r) => {
            if r.affected != 1 {
                fail("apply_changes", &format!("affected={}", r.affected));
            }
            match cell("SELECT name, note, amount FROM users WHERE id = 2", 0, 0).await {
                Ok(v) if v.as_deref() == Some("李小四") => pass("apply_changes_name", "中文原样写入"),
                Ok(v) => fail("apply_changes_name", &format!("name={v:?}")),
                Err(e) => fail("apply_changes_name", &e),
            }
            match cell("SELECT name, note, amount FROM users WHERE id = 2", 0, 1).await {
                Ok(None) => pass("apply_changes_null", "note 已置 NULL"),
                Ok(v) => fail("apply_changes_null", &format!("note={v:?}")),
                Err(e) => fail("apply_changes_null", &e),
            }
            match cell("SELECT name, note, amount FROM users WHERE id = 2", 0, 2).await {
                Ok(v) if v.as_deref() == Some("-9.50") => pass("apply_changes_decimal", "-9.50"),
                Ok(v) => fail("apply_changes_decimal", &format!("amount={v:?}")),
                Err(e) => fail("apply_changes_decimal", &e),
            }
        }
        Err(e) => fail("apply_changes", &e),
    }

    // 13) 回写出错必须报错、整批回滚、连接不留悬挂事务
    let bad = vec![
        CellUpdate {
            pk: vec![("id".into(), "3".into())],
            sets: vec![("note".into(), Some("不应留下".into()))],
        },
        CellUpdate {
            pk: vec![("id".into(), "3".into())],
            sets: vec![("age".into(), Some("不是数字".into()))],
        },
    ];
    match service.apply_changes("e2e-mssql", "users", db.as_deref(), &bad, &[], &[]).await {
        Err(e) => pass("apply_changes_error", &format!("已报错: {}", e.lines().next().unwrap_or(&e))),
        Ok(r) => fail("apply_changes_error", &format!("转换失败被吞,affected={}", r.affected)),
    }
    // id=3 的 note 种子是 'C':同批前一条改的值不能留下
    match cell("SELECT note FROM users WHERE id = 3", 0, 0).await {
        Ok(v) if v.as_deref() == Some("C") => pass("apply_changes_rollback", "同批前一条也回滚了"),
        Ok(v) => fail("apply_changes_rollback", &format!("note={v:?}")),
        Err(e) => fail("apply_changes_rollback", &e),
    }
    // 池里 3 条连接轮发:逐条问一遍(出错的那条不一定先被问到的)
    {
        let mut counts = Vec::new();
        for _ in 0..3 {
            match cell("SELECT @@TRANCOUNT", 0, 0).await {
                Ok(v) => counts.push(v),
                Err(e) => fail("apply_changes_no_dangling", &e),
            }
        }
        if counts.iter().any(|c| c.as_deref() != Some("0")) {
            fail("apply_changes_no_dangling", &format!("@@TRANCOUNT={counts:?}"));
        }
        pass("apply_changes_no_dangling", "3 条连接都无悬挂事务");
    }

    // 14) 新增(中文)与删除
    let ins = vec![vec![
        ("name".into(), "新同学".into()),
        ("age".into(), "20".into()),
        ("big_col".into(), "42".into()),
    ]];
    match service.apply_changes("e2e-mssql", "users", db.as_deref(), &[], &[], &ins).await {
        Ok(r) if r.affected == 1 => pass("apply_insert", "影响 1 行"),
        Ok(r) => fail("apply_insert", &format!("affected={}", r.affected)),
        Err(e) => fail("apply_insert", &e),
    }
    match cell("SELECT name, big_col FROM users WHERE id = 5", 0, 0).await {
        Ok(v) if v.as_deref() == Some("新同学") => pass("apply_insert_value", "中文原样写入"),
        Ok(v) => fail("apply_insert_value", &format!("name={v:?}")),
        Err(e) => fail("apply_insert_value", &e),
    }
    match cell("SELECT name, big_col FROM users WHERE id = 5", 0, 1).await {
        Ok(v) if v.as_deref() == Some("42") => pass("apply_insert_bigint", "42"),
        Ok(v) => fail("apply_insert_bigint", &format!("big_col={v:?}")),
        Err(e) => fail("apply_insert_bigint", &e),
    }
    let del = vec![vec![("id".into(), "5".into())]];
    match service.apply_changes("e2e-mssql", "users", db.as_deref(), &[], &del, &[]).await {
        Ok(r) if r.affected == 1 => pass("apply_delete", "影响 1 行"),
        Ok(r) => fail("apply_delete", &format!("affected={}", r.affected)),
        Err(e) => fail("apply_delete", &e),
    }
    match service.count_rows("e2e-mssql", "users").await {
        Ok(4) => pass("count_after_delete", "4"),
        other => fail("count_after_delete", &format!("{other:?}")),
    }

    // 14b) 外键列表:单列外键 + 复合外键都列出来,列序按约束定义
    match service.list_foreign_keys("e2e-mssql", db.clone()).await {
        Ok(fks) => {
            let find = |t: &str, c: &str| {
                fks.iter()
                    .find(|f| f.table == t && f.column == c)
                    .map(|f| (f.ref_table.as_str(), f.ref_column.as_str()))
            };
            let problems: Vec<String> = [
                (find("emp", "dept_id") != Some(("dept", "id")), format!("dept_id={:?}", find("emp", "dept_id"))),
                (find("emp", "badge") != Some(("badge2", "code")), format!("badge={:?}", find("emp", "badge"))),
                (find("emp", "dept_code") != Some(("badge2", "dept")), format!("dept_code={:?}", find("emp", "dept_code"))),
            ]
            .into_iter()
            .filter(|(bad, _)| *bad)
            .map(|(_, msg)| msg)
            .collect();
            if !problems.is_empty() {
                fail("foreign_keys", &format!("{};全部: {:?}", problems.join("; "), fks));
            }
            pass("foreign_keys", "单列与复合外键都列出,引用列正确");
        }
        Err(e) => fail("foreign_keys", &e),
    }

    // 15a) 另一个库里的表:结构走三段式名,不改连接的默认库
    match service.get_table_structure("e2e-mssql", "marker", Some("e2e_mssql2".into())).await {
        Ok(s) => {
            let names: Vec<&str> = s.columns.iter().map(|c| c.name.as_str()).collect();
            if names != ["id", "val"] || s.columns[0].key != "PRI" || s.columns[1].key != "" {
                fail("structure_other_db", &format!("{:?}", s.columns));
            }
            if s.columns[1].data_type != "nvarchar(20)" {
                fail("structure_other_db", &format!("val={:?}", s.columns[1].data_type));
            }
            if !s.ddl.contains("CREATE TABLE [marker] (") || !s.ddl.contains("PRIMARY KEY") {
                fail("structure_other_db", &format!("marker DDL={}", s.ddl));
            }
            pass("structure_other_db", "id=PRI, val=nvarchar(20), DDL 带主键");
        }
        Err(e) => fail("structure_other_db", &e),
    }
    match cell("SELECT DB_NAME()", 0, 0).await {
        Ok(v) if v.as_deref() == Some("e2e_mssql") => pass("default_db_unchanged", "连接默认库没被改"),
        Ok(v) => fail("default_db_unchanged", &format!("默认库变成了 {v:?}")),
        Err(e) => fail("default_db_unchanged", &e),
    }

    // 15b) 跨库限定:读写 e2e_mssql2.marker
    match run("SELECT val FROM e2e_mssql2.dbo.marker WHERE id=1").await {
        Ok(rs) => match rs.first().and_then(|r| first(r, 0, 0)) {
            Some(v) if v == "跨库值" => pass("cross_db_read", "库名限定跨库读正确"),
            other => fail("cross_db_read", &format!("val={other:?}")),
        },
        Err(e) => fail("cross_db_read", &e),
    }
    let upd2 = vec![CellUpdate {
        pk: vec![("id".into(), "1".into())],
        sets: vec![("val".into(), Some("跨库改".into()))],
    }];
    match service
        .apply_changes("e2e-mssql", "marker", Some("e2e_mssql2".into()), &upd2, &[], &[])
        .await
    {
        Ok(r) if r.affected == 1 => pass("cross_db_write", "影响 1 行"),
        Ok(r) => fail("cross_db_write", &format!("affected={}", r.affected)),
        Err(e) => fail("cross_db_write", &e),
    }
    match cell("SELECT val FROM e2e_mssql2.dbo.marker WHERE id=1", 0, 0).await {
        Ok(v) if v.as_deref() == Some("跨库改") => pass("cross_db_write_value", "跨库写后重读一致"),
        Ok(v) => fail("cross_db_write_value", &format!("val={v:?}")),
        Err(e) => fail("cross_db_write_value", &e),
    }

    // 16) 数据页:SQL 由 core 的方言函数生成(应用层原样调用),拿去打真库
    {
        use shujing_core::edits::{build_cond_where, build_like_filter, page_clause, page_table_ref, Dialect};
        let mssql = Dialect::Mssql;
        let table = page_table_ref(Some("e2e_mssql"), "users", mssql);
        // 不带排序列也要有 ORDER BY,否则 OFFSET … FETCH 是语法错
        let page = |limit: usize, offset: usize, order: Option<&'static str>| {
            let service = service.clone();
            let sql = format!("SELECT * FROM {table}{}", page_clause(order, limit, offset, mssql));
            async move { ids_of(&service, sql).await }
        };
        match page(2, 0, None).await {
            Ok(ids) if ids == ["1", "2"] => pass("page_first", "ids 1,2"),
            other => fail("page_first", &format!("{other:?}")),
        }
        match page(2, 2, None).await {
            Ok(ids) if ids == ["3", "4"] => pass("page_second", "ids 3,4"),
            other => fail("page_second", &format!("{other:?}")),
        }
        match page(2, 0, Some("\"id\" DESC")).await {
            Ok(ids) if ids == ["4", "3"] => pass("page_sorted", "id DESC → 4,3"),
            other => fail("page_sorted", &format!("{other:?}")),
        }
        let columns: Vec<String> = match run("SELECT * FROM users WHERE 1=0").await {
            Ok(rs) => rs[0].columns.clone(),
            Err(e) => fail("page_columns", &e),
        };
        let quick = format!("SELECT * FROM {table} WHERE {}{}", build_like_filter(&columns, "张", mssql), page_clause(None, 200, 0, mssql));
        match ids_of(&service, quick).await {
            Ok(ids) if ids == ["1"] => pass("page_quick_filter", "中文快速筛选命中张三"),
            other => fail("page_quick_filter", &format!("{other:?}")),
        }
        let cond = build_cond_where(&[("name".into(), "LIKE".into(), "小四".into())], "", mssql).unwrap();
        match ids_of(&service, format!("SELECT * FROM {table} WHERE {cond}{}", page_clause(None, 200, 0, mssql))).await {
            Ok(ids) if ids == ["2"] => pass("page_cond_filter", "中文条件筛选命中李小四"),
            other => fail("page_cond_filter", &format!("{other:?}")),
        }
        match ids_of(&service, format!("SELECT COUNT(*) FROM {table}")).await {
            Ok(n) if n == ["4"] => pass("page_count", "4"),
            other => fail("page_count", &format!("{other:?}")),
        }
        let marker = page_table_ref(Some("e2e_mssql2"), "marker", mssql);
        match ids_of(&service, format!("SELECT id FROM {marker}{}", page_clause(None, 200, 0, mssql))).await {
            Ok(n) if n == ["1"] => pass("page_other_db", "另一个库的表分页正确"),
            other => fail("page_other_db", &format!("{other:?}")),
        }
    }

    // 17) 会话库:界面选库 / 控制台 USE 之后,池里每条连接都落到选中的库
    let db_name = || {
        let service = service.clone();
        async move {
            service
                .run_sql("e2e-mssql", "SELECT DB_NAME()".into(), None)
                .await?
                .pop()
                .and_then(|r| first(&r, 0, 0))
                .ok_or_else(|| "无结果集".to_string())
        }
    };
    match service.run_sql_db("e2e-mssql", "SELECT 1".into(), None, Some("e2e_mssql2")).await {
        Ok(_) => pass("select_db", "会话库切到 e2e_mssql2"),
        Err(e) => fail("select_db", &e),
    }
    // 池里 3 条连接轮发:连问 5 次必须次次都是选中的库
    {
        let mut seen = Vec::new();
        for _ in 0..5 {
            match db_name().await {
                Ok(v) => seen.push(v),
                Err(e) => fail("session_db_aligned", &e),
            }
        }
        if seen.iter().any(|v| v != "e2e_mssql2") {
            fail("session_db_aligned", &format!("{seen:?}"));
        }
        pass("session_db_aligned", "3 条连接都对齐到 e2e_mssql2");
    }
    // 不带库名的语句(侧栏删除/清空模板就是这么拼的)落在会话库:marker 只存在于 e2e_mssql2
    match cell("SELECT val FROM marker WHERE id = 1", 0, 0).await {
        Ok(v) if v.as_deref() == Some("跨库改") => pass("bare_name_in_session_db", "不带库名读到了 e2e_mssql2.marker"),
        Ok(v) => fail("bare_name_in_session_db", &format!("val={v:?}")),
        Err(e) => fail("bare_name_in_session_db", &e),
    }
    // 控制台 USE:改的只是执行它的那条连接,其余连接取用时要跟上
    match run("USE e2e_mssql").await {
        Ok(_) => pass("console_use", "USE e2e_mssql"),
        Err(e) => fail("console_use", &e),
    }
    {
        let mut seen = Vec::new();
        for _ in 0..5 {
            match db_name().await {
                Ok(v) => seen.push(v),
                Err(e) => fail("console_use_aligned", &e),
            }
        }
        if seen.iter().any(|v| v != "e2e_mssql") {
            fail("console_use_aligned", &format!("{seen:?}"));
        }
        pass("console_use_aligned", "3 条连接都跟到 e2e_mssql");
    }
    match cell("SELECT name FROM users WHERE id = 1", 0, 0).await {
        Ok(v) if v.as_deref() == Some("张三") => pass("console_use_bare_read", "不带库名读到了 e2e_mssql.users"),
        Ok(v) => fail("console_use_bare_read", &format!("name={v:?}")),
        Err(e) => fail("console_use_bare_read", &e),
    }
    match run("USE [e2e_mssql2]").await {
        Ok(_) => {}
        Err(e) => fail("console_use_brackets", &e),
    }
    match db_name().await {
        Ok(v) if v == "e2e_mssql2" => pass("console_use_brackets", "[db] 引注也能识别"),
        Ok(v) => fail("console_use_brackets", &format!("db={v:?}")),
        Err(e) => fail("console_use_brackets", &e),
    }

    // 17b) 侧栏右键菜单的快捷模板(语句在 app/sidebar.rs 里拼,这里照抄验证)。
    // 都不带库名,靠会话库落库:此刻会话库还在 e2e_mssql2(那里没有 users),
    // 模拟界面选中 e2e_mssql 再操作——落错库会报 Invalid object name
    let in_db = |sql: &'static str| {
        let service = service.clone();
        async move {
            service
                .run_sql_db("e2e-mssql", sql.to_string(), None, Some("e2e_mssql"))
                .await
        }
    };
    match in_db("SELECT TOP 100 * FROM \"users\"").await {
        Ok(rs) if rs[0].rows.len() == 4 => pass("tpl_copy_select", "SELECT TOP 100 * 可执行"),
        Ok(rs) => fail("tpl_copy_select", &format!("rows={}", rs[0].rows.len())),
        Err(e) => fail("tpl_copy_select", &e),
    }
    match in_db("SELECT * INTO \"users_copy\" FROM \"users\"").await {
        Ok(_) => pass("tpl_copy_table", "SELECT … INTO 建出副本"),
        Err(e) => fail("tpl_copy_table", &e),
    }
    match in_db("UPDATE STATISTICS \"users_copy\"").await {
        Ok(_) => pass("tpl_analyze", "UPDATE STATISTICS 可执行"),
        Err(e) => fail("tpl_analyze", &e),
    }
    match in_db("DELETE FROM \"users_copy\"").await {
        Ok(rs) if rs[0].affected == 4 => pass("tpl_truncate", "清空表影响 4 行"),
        Ok(rs) => fail("tpl_truncate", &format!("affected={}", rs[0].affected)),
        Err(e) => fail("tpl_truncate", &e),
    }
    match in_db("DROP TABLE \"users_copy\"").await {
        Ok(_) => pass("tpl_drop", "DROP TABLE 可执行"),
        Err(e) => fail("tpl_drop", &e),
    }
    match service.count_rows("e2e-mssql", "users").await {
        Ok(4) => pass("tpl_target_db", "原表 4 行还在(操作落在选中的库)"),
        other => fail("tpl_target_db", &format!("{other:?}")),
    }

    // 18) 清理
    {
        let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await.expect("raw tcp");
        let mut cfg = tiberius::Config::new();
        cfg.host(host.as_str());
        cfg.port(port);
        cfg.authentication(tiberius::AuthMethod::sql_server(&user, &pass_));
        cfg.trust_cert();
        let mut conn = tiberius::Client::connect(cfg, tcp.compat_write()).await.expect("raw mssql conn");
        for sql in [
            "ALTER DATABASE e2e_mssql SET SINGLE_USER WITH ROLLBACK IMMEDIATE",
            "DROP DATABASE e2e_mssql",
            "ALTER DATABASE e2e_mssql2 SET SINGLE_USER WITH ROLLBACK IMMEDIATE",
            "DROP DATABASE e2e_mssql2",
        ] {
            let _ = conn.simple_query(sql).await.unwrap().into_results().await.unwrap();
        }
        service.disconnect("e2e-mssql").await;
    }

    println!("\n全部通过 ✓");
}
