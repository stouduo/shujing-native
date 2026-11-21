//! MongoDB 全链路 E2E(直调 Service):连接/库/集合/计数/文档增删改查/
//! JSON 导入导出/CSV 导入(类型推断)/唯一索引与冲突/删索引。
//! 运行:先起本地 MongoDB(默认 127.0.0.1:27017,无鉴权),然后
//! cargo run -p shujing-core --example e2e_mongo
//! 连接参数可用环境变量覆盖:E2E_MONGO_HOST/E2E_MONGO_PORT

use shujing_core::{ConnInfo, DbType, Service};

fn pass(name: &str, detail: &str) {
    println!("PASS  {name}  {detail}");
}

fn fail(name: &str, err: &str) -> ! {
    println!("FAIL  {name}  {err}");
    std::process::exit(1);
}

const DB: &str = "e2e_mongo";
const COLL: &str = "e2e_users";

#[tokio::main]
async fn main() {
    let dir = std::env::temp_dir().join("shujing_e2e_mongo_cfg");
    std::fs::create_dir_all(&dir).unwrap();
    let service = std::sync::Arc::new(Service::new(&dir));

    let host = std::env::var("E2E_MONGO_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("E2E_MONGO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(27017);

    // 0) 幂等重置:直接驱动删库(上一轮的文档/索引不残留)
    {
        let raw = mongodb::Client::with_uri_str(format!("mongodb://{host}:{port}"))
            .await
            .expect("raw client");
        raw.database(DB).drop().await.expect("reset drop db");
    }

    let info = ConnInfo {
        id: "e2e-mongo".into(),
        name: "e2e-mongo".into(),
        db_type: DbType::Mongo,
        host: Some(host.clone()),
        port: Some(port),
        user: None,
        password: None,
        database: Some(DB.into()),
        file_path: None,
        color: None,
        read_only: false,
        perm: None,
        ssh: None,
    };

    // 1) 连接
    match service.connect(info.clone()).await {
        Ok(c) => pass("connect", &c.version),
        Err(e) => fail("connect", &e),
    }

    // 3) 插入 2 个文档(中文/嵌套),返回 _id
    let id1 = match service
        .mongo_insert_doc(
            "e2e-mongo",
            Some(DB),
            COLL,
            r#"{"name":"张三","age":28,"score":91.5,"tags":["a","b"],"addr":{"city":"上海"}}"#,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => fail("insert_doc1", &e),
    };
    let id2 = match service
        .mongo_insert_doc(
            "e2e-mongo",
            Some(DB),
            COLL,
            r#"{"name":"李四","age":34,"score":88.25,"tags":[],"addr":{"city":"北京"}}"#,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => fail("insert_doc2", &e),
    };
    pass("insert_doc", &format!("2 docs, _id1={id1}"));

    // 2) 库列表含 e2e_mongo(MongoDB 空库不进 show dbs,须在首个插入后断言)
    match service.list_databases("e2e-mongo").await {
        Ok(dbs) if dbs.iter().any(|d| d == DB) => {
            pass("list_databases", &format!("{} dbs, 含 {DB}", dbs.len()))
        }
        Ok(dbs) => fail("list_databases", &format!("{DB} 不在 {dbs:?}")),
        Err(e) => fail("list_databases", &e),
    }

    // 4) 集合列表出现 e2e_users
    match service.list_tables("e2e-mongo", Some(DB.into())).await {
        Ok(ts) if ts.iter().any(|t| t.name == COLL) => {
            pass("list_tables", &format!("{} collections", ts.len()))
        }
        Ok(ts) => fail("list_tables", &format!("{COLL} 不在 {ts:?}")),
        Err(e) => fail("list_tables", &e),
    }

    // 5) 计数 = 2(两条路径:count_rows 通用 + mongo_count)
    for (name, r) in [
        ("count_rows", service.count_rows("e2e-mongo", COLL).await),
        (
            "mongo_count",
            service.mongo_count("e2e-mongo", Some(DB), COLL).await,
        ),
    ] {
        match r {
            Ok(2) => pass(name, "2"),
            Ok(n) => fail(name, &format!("expect 2 got {n}")),
            Err(e) => fail(name, &e),
        }
    }

    // 6) 采样浏览:全部 + 过滤(city=上海),中文保真
    match service
        .mongo_sample("e2e-mongo", Some(DB), COLL, None, 50, 0)
        .await
    {
        Ok((cols, rows)) => {
            if rows.len() != 2 || !cols.iter().any(|c| c == "name") {
                fail("sample_all", &format!("cols={cols:?} rows={}", rows.len()));
            }
            let zhang = rows.iter().find(|r| r.contains(&Some("张三".into())));
            if zhang.is_none() {
                fail("sample_all", "中文「张三」丢失");
            }
            pass("sample_all", &format!("{} rows, {} cols", rows.len(), cols.len()));
        }
        Err(e) => fail("sample_all", &e),
    }
    match service
        .mongo_sample(
            "e2e-mongo",
            Some(DB),
            COLL,
            Some(r#"{"addr.city":"上海"}"#),
            50,
            0,
        )
        .await
    {
        Ok((_, rows)) if rows.len() == 1 => pass("sample_filter", "嵌套路径过滤命中 1 条"),
        Ok((_, rows)) => fail("sample_filter", &format!("expect 1 got {}", rows.len())),
        Err(e) => fail("sample_filter", &e),
    }

    // 7) 读单文档 → 替换(改 age 并删掉 tags)→ 重读验证
    match service.mongo_get_doc("e2e-mongo", Some(DB), COLL, &id1).await {
        Ok(j) if j.contains("张三") => pass("get_doc", "中文保真"),
        Ok(j) => fail("get_doc", &format!("内容异常: {j}")),
        Err(e) => fail("get_doc", &e),
    }
    match service
        .mongo_replace_doc(
            "e2e-mongo",
            Some(DB),
            COLL,
            &id1,
            r#"{"name":"张三","age":29,"addr":{"city":"上海"}}"#,
        )
        .await
    {
        Ok(()) => match service.mongo_get_doc("e2e-mongo", Some(DB), COLL, &id1).await {
            Ok(j) if j.contains("\"age\": 29") && !j.contains("tags") && j.contains("张三") => {
                pass("replace_doc", "_id 保留、字段替换生效")
            }
            Ok(j) => fail("replace_doc", &format!("替换后异常: {j}")),
            Err(e) => fail("replace_doc:re-read", &e),
        },
        Err(e) => fail("replace_doc", &e),
    }

    // 8) 导出 JSON → 导入回新集合 → 计数一致
    let (exported, truncated) = match service
        .mongo_export_docs("e2e-mongo", Some(DB), COLL, 1000)
        .await
    {
        Ok(v) => v,
        Err(e) => fail("export_docs", &e),
    };
    if truncated || !exported.contains("张三") {
        fail("export_docs", &format!("truncated={truncated} 中文丢失?"));
    }
    match service
        .mongo_import_docs("e2e-mongo", Some(DB), "e2e_import", &exported)
        .await
    {
        Ok(2) => pass("import_docs", "导出→导入闭环 2 条"),
        Ok(n) => fail("import_docs", &format!("expect 2 got {n}")),
        Err(e) => fail("import_docs", &e),
    }

    // 9) CSV 导入:类型推断(空→NULL/true/false→Bool/整数/浮点/中文)
    let csv = "city,score,active,note\n上海,4.5,true,\n北京,3,false,备注\n深圳,5.75,true,你好";
    match service
        .mongo_import_docs_csv("e2e-mongo", Some(DB), "e2e_csv", csv)
        .await
    {
        Ok(3) => {
            match service
                .mongo_sample("e2e-mongo", Some(DB), "e2e_csv", None, 50, 0)
                .await
            {
                Ok((cols, rows)) => {
                    let city_i = cols.iter().position(|c| c == "city").unwrap_or(usize::MAX);
                    let score_i = cols.iter().position(|c| c == "score").unwrap_or(usize::MAX);
                    let active_i = cols.iter().position(|c| c == "active").unwrap_or(usize::MAX);
                    let note_i = cols.iter().position(|c| c == "note").unwrap_or(usize::MAX);
                    let row_of = |city: &str| {
                        rows.iter()
                            .find(|r| r[city_i].as_deref() == Some(city))
                            .unwrap_or_else(|| panic!("{city} 行缺失"))
                    };
                    let bj = row_of("北京");
                    if bj[score_i].as_deref() != Some("3") {
                        fail("import_csv:infer", &format!("score={:?}", bj[score_i]));
                    }
                    if bj[active_i].as_deref() != Some("false") {
                        fail("import_csv:infer", &format!("active={:?}", bj[active_i]));
                    }
                    if bj[note_i].as_deref() != Some("备注") {
                        fail("import_csv:infer", &format!("note={:?}", bj[note_i]));
                    }
                    let sh = row_of("上海");
                    if sh[score_i].as_deref() != Some("4.5") {
                        fail("import_csv:infer", &format!("score={:?}", sh[score_i]));
                    }
                    if sh[note_i].is_some() {
                        fail("import_csv:infer", "空列应推断为 NULL");
                    }
                    pass("import_csv", "3 条,类型推断/中文/NULL 正确");
                }
                Err(e) => fail("import_csv:sample", &e),
            }
        }
        Ok(n) => fail("import_csv", &format!("expect 3 got {n}")),
        Err(e) => fail("import_csv", &e),
    }

    // 10) 唯一索引:创建 → 冲突插入报错 → 删索引 → 同样插入成功
    let idx_name = match service
        .mongo_create_index(
            "e2e-mongo",
            Some(DB),
            "e2e_csv",
            &[("city".into(), false)],
            true,
            None,
            Some("e2e_unique_city"),
        )
        .await
    {
        Ok(n) => n,
        Err(e) => fail("create_index", &e),
    };
    match service
        .mongo_insert_doc("e2e-mongo", Some(DB), "e2e_csv", r#"{"city":"上海"}"#)
        .await
    {
        Err(e) if e.contains("duplicate") || e.contains("E11000") => {
            pass("unique_conflict", "唯一索引拦截重复键")
        }
        Ok(_) => fail("unique_conflict", "重复键未被拦截"),
        Err(e) => fail("unique_conflict", &e),
    }
    match service.mongo_list_indexes("e2e-mongo", Some(DB), "e2e_csv").await {
        Ok(ix) if ix.iter().any(|i| i.name == idx_name && i.unique) => {
            pass("list_indexes", &format!("含 {idx_name}(unique)"))
        }
        Ok(ix) => fail("list_indexes", &format!("{idx_name} 不在 {ix:?}")),
        Err(e) => fail("list_indexes", &e),
    }
    if let Err(e) = service
        .mongo_drop_index("e2e-mongo", Some(DB), "e2e_csv", "e2e_unique_city")
        .await
    {
        fail("drop_index", &e);
    }
    match service
        .mongo_insert_doc("e2e-mongo", Some(DB), "e2e_csv", r#"{"city":"上海"}"#)
        .await
    {
        Ok(_) => pass("drop_index", "删索引后重复键可插入"),
        Err(e) => fail("drop_index", &format!("删索引未生效: {e}")),
    }

    // 11) 删文档 → 计数回落
    match service.mongo_delete_doc("e2e-mongo", Some(DB), COLL, &id2).await {
        Ok(()) => match service.mongo_count("e2e-mongo", Some(DB), COLL).await {
            Ok(1) => pass("delete_doc", "删后计数=1"),
            Ok(n) => fail("delete_doc:count", &format!("expect 1 got {n}")),
            Err(e) => fail("delete_doc:count", &e),
        },
        Err(e) => fail("delete_doc", &e),
    }

    // 12) 清理临时集合
    let raw = mongodb::Client::with_uri_str(format!("mongodb://{host}:{port}"))
        .await
        .expect("raw client 2");
    for c in ["e2e_import", "e2e_csv"] {
        let _ = raw.database(DB).collection::<mongodb::bson::Document>(c).drop().await;
    }
    let _ = raw.database(DB).drop().await;

    println!("\n全部通过 ✓");
}
