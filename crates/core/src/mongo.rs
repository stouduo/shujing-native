//! MongoDB 浏览(v1 只读):集合列表/采样文档展平为网格/结构推断/计数。
//! 编辑留 v2(BSON 文档的 JSON 编辑器 + 替换写)。

use crate::model::{Backend, ColumnDef, TableStructure};
use mongodb::bson::{Bson, Document};

/// 元素类型 → 显示用类型名(bson 规范名,首字母小写)
fn type_name(v: &Bson) -> &'static str {
    use mongodb::bson::spec::ElementType;
    match v.element_type() {
        ElementType::Double => "double",
        ElementType::String => "string",
        ElementType::EmbeddedDocument => "object",
        ElementType::Array => "array",
        ElementType::Boolean => "bool",
        ElementType::DateTime => "date",
        ElementType::Timestamp => "timestamp",
        ElementType::Int32 => "int32",
        ElementType::Int64 => "int64",
        ElementType::ObjectId => "objectId",
        ElementType::Decimal128 => "decimal128",
        _ => "bson",
    }
}

/// Bson → 网格单元格文本;嵌套/无标量语义的走紧凑 JSON
fn bson_display(v: &Bson) -> Option<String> {
    match v {
        Bson::Null | Bson::Undefined => None,
        Bson::String(s) => Some(s.clone()),
        Bson::Int32(i) => Some(i.to_string()),
        Bson::Int64(i) => Some(i.to_string()),
        Bson::Double(d) => Some(d.to_string()),
        Bson::Boolean(b) => Some(b.to_string()),
        Bson::ObjectId(oid) => Some(oid.to_hex()),
        Bson::DateTime(dt) => Some(dt.try_to_rfc3339_string().unwrap_or_default()),
        Bson::Binary(b) => Some(format!("<{} bytes>", b.bytes.len())),
        Bson::RegularExpression(r) => Some(format!("/{}/{}", r.pattern, r.options)),
        Bson::JavaScriptCode(c) => Some(c.clone()),
        Bson::JavaScriptCodeWithScope(c) => Some(c.code.clone()),
        Bson::Document(_) | Bson::Array(_) => {
            // serde 序列化 = 标准 JSON(嵌套预览够用;大文档截断交给展示层)
            serde_json::to_string(v).ok()
        }
        _ => Some(format!("{v}")),
    }
}

fn default_db(
    st: &crate::model::MongoState,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(d) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    // try_lock 借不到(并发结构加载中)时用克隆值兜底
    let cur = st.cur_db.try_lock().ok().and_then(|g| g.clone());
    cur.filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "未指定数据库".to_string())
}

pub async fn list_databases(st: &crate::model::MongoState) -> Result<Vec<String>, String> {
    st.client
        .list_database_names()
        .await
        .map_err(|e| format!("MongoDB: {e}"))
}

pub async fn list_collections(
    st: &crate::model::MongoState,
    database: Option<&str>,
) -> Result<Vec<String>, String> {
    let db = default_db(st, database)?;
    let mut names = st
        .client
        .database(db.as_str())
        .list_collection_names()
        .await
        .map_err(|e| format!("MongoDB: {e}"))?;
    // 系统集合不进树
    names.retain(|n| !n.starts_with("system."));
    names.sort();
    Ok(names)
}

/// 采样文档 → 展平为网格(列=全部文档顶层键并集,首现顺序,_id 优先;上限 60 列)
pub async fn sample(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    filter_json: Option<&str>,
    limit: usize,
    offset: u64,
) -> Result<(Vec<String>, Vec<Vec<Option<String>>>), String> {
    let db = default_db(st, database)?;
    // 记住当前库(count_rows 无库参数,靠这里)
    if let Ok(mut g) = st.cur_db.try_lock() {
        *g = Some(db.to_string());
    }
    let filter: Document = match filter_json {
        Some(j) if !j.trim().is_empty() => serde_json::from_str(j)
            .map_err(|e| format!("过滤条件不是合法 JSON: {e}"))?,
        _ => Document::new(),
    };
    let opts = mongodb::options::FindOptions::builder()
        .skip(Some(offset))
        .limit(Some(limit as i64))
        .build();
    // mongodb 3.x:find 是 builder(Filter/Options 链式),cursor.stream() 才是
    // Stream(futures StreamExt::next 拉取)
    use futures_util::StreamExt;
    let mut cursor = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .find(filter)
        .with_options(opts)
        .await
        .map_err(|e| format!("MongoDB 查询失败: {e}"))?;

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    while let Some(Ok(doc)) = cursor.next().await {
        // 首个文档定列序;_id 置首;超出 60 列截断(schemaless 防爆)
        if columns.is_empty() {
            let mut keys: Vec<String> = doc.keys().cloned().collect();
            if keys.iter().any(|k| k == "_id") {
                keys.retain(|k| k != "_id");
                keys.insert(0, "_id".into());
            }
            keys.truncate(60);
            columns = keys;
        }
        let row: Vec<Option<String>> = columns
            .iter()
            .map(|k| doc.get(k).and_then(bson_display))
            .collect();
        rows.push(row);
    }
    Ok((columns, rows))
}

/// 结构推断:采样 50 个文档,列=键并集,类型=首见的 bson 类型名
pub async fn structure(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
) -> Result<TableStructure, String> {
    let db = default_db(st, database)?;
    if let Ok(mut g) = st.cur_db.try_lock() {
        *g = Some(db.to_string());
    }
    let opts = mongodb::options::FindOptions::builder().limit(Some(50)).build();
    use futures_util::StreamExt;
    let mut cursor = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .find(mongodb::bson::doc! {})
        .with_options(opts)
        .await
        .map_err(|e| format!("MongoDB: {e}"))?;

    let mut columns: Vec<ColumnDef> = Vec::new();
    let mut seen: std::collections::HashSet<String> = Default::default();
    while let Some(Ok(doc)) = cursor.next().await {
        // _id 置首
        let mut pairs: Vec<(&String, &Bson)> = doc.iter().collect();
        pairs.sort_by_key(|(k, _)| if k.as_str() == "_id" { 0 } else { 1 });
        for (k, v) in pairs {
            if seen.contains(k) || columns.len() >= 60 {
                continue;
            }
            seen.insert(k.clone());
            columns.push(ColumnDef {
                name: k.clone(),
                data_type: type_name(v).to_string(),
                nullable: true,
                key: if k == "_id" { "ID".into() } else { String::new() },
                default: None,
                extra: String::new(),
                comment: String::new(),
            });
        }
    }
    if columns.is_empty() {
        return Err(format!("集合 {collection} 为空或无法采样(无字段可展示)"));
    }
    Ok(TableStructure {
        columns,
        indexes: vec![],
        ddl: String::new(),
    })
}

pub async fn count(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
) -> Result<u64, String> {
    let db = default_db(st, database)?;
    st.client
        .database(db.as_str())
        .collection::<Document>(collection)
        .estimated_document_count()
        .await
        .map_err(|e| format!("MongoDB: {e}"))
}

pub async fn server_ping(st: &crate::model::MongoState) -> Result<(), String> {
    st.client
        .database("admin")
        .run_command(mongodb::bson::doc! { "ping": 1 })
        .await
        .map(|_| ())
        .map_err(|e| format!("MongoDB: {e}"))
}

/// Backend 快捷分发(list_databases/count 等无库上下文的调用从 State 记忆取)
pub async fn backend_list_databases(b: &mut Backend) -> Result<Vec<String>, String> {
    match b {
        Backend::Mongo(st) => list_databases(st).await,
        _ => Err("非 MongoDB 连接".into()),
    }
}

pub async fn backend_count(st: &crate::model::MongoState, collection: &str) -> Result<u64, String> {
    count(st, None, collection).await
}

// ── 文档编辑(v2)────────────────────────────────────────

/// _id 字符串 → 定位用的 Bson:24 位十六进制→ObjectId,整数→Int64,其余→字符串
pub fn parse_id(id: &str) -> Bson {
    if id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(id) {
            return Bson::ObjectId(oid);
        }
    }
    if let Ok(n) = id.parse::<i64>() {
        return Bson::Int64(n);
    }
    Bson::String(id.to_string())
}

fn id_filter(id: &str) -> Document {
    mongodb::bson::doc! { "_id": parse_id(id) }
}

fn json_to_document(json: &str) -> Result<Document, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("不是合法 JSON: {e}"))?;
        // serde_json::Value → Bson:bson 提供 Bson::try_from(serde_json::Value)
    let b = Bson::try_from(v).map_err(|e| format!("JSON→BSON 转换失败: {e}"))?;
    match b {
        Bson::Document(d) => Ok(d),
        _ => Err("顶层必须是 JSON 对象".into()),
    }
}

/// 拉单个文档为 pretty JSON(编辑器预填)
pub async fn get_doc(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    id: &str,
) -> Result<String, String> {
    let db = default_db(st, database)?;
    let doc = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .find_one(id_filter(id))
        .await
        .map_err(|e| format!("MongoDB: {e}"))?
        .ok_or_else(|| format!("文档不存在(_id={id})"))?;
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

pub async fn insert_doc(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    json: &str,
) -> Result<String, String> {
    let db = default_db(st, database)?;
    let doc = json_to_document(json)?;
    if doc.contains_key("_id") {
        // 已带 _id:防重复(先查)
        let id_b = doc.get("_id").unwrap();
        let exists = st
            .client
            .database(db.as_str())
            .collection::<Document>(collection)
            .find_one(mongodb::bson::doc! { "_id": id_b.clone() })
            .await
            .map_err(|e| format!("MongoDB: {e}"))?
            .is_some();
        if exists {
            return Err("已存在相同 _id 的文档".into());
        }
    }
    let r = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .insert_one(doc)
        .await
        .map_err(|e| format!("插入失败: {e}"))?;
    Ok(r.inserted_id
        .as_object_id()
        .map(|o| o.to_hex())
        .unwrap_or_default())
}

/// 替换文档:_id 不可变,新 JSON 里的 _id 剥掉后按定位 id 替换
pub async fn replace_doc(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    id: &str,
    json: &str,
) -> Result<(), String> {
    let db = default_db(st, database)?;
    let mut doc = json_to_document(json)?;
    doc.remove("_id");
    let r = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .replace_one(id_filter(id), doc)
        .await
        .map_err(|e| format!("替换失败: {e}"))?;
    if r.matched_count == 0 {
        return Err(format!("未找到文档(_id={id})——可能已被删除"));
    }
    Ok(())
}

pub async fn delete_doc(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    id: &str,
) -> Result<(), String> {
    let db = default_db(st, database)?;
    let r = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .delete_one(id_filter(id))
        .await
        .map_err(|e| format!("删除失败: {e}"))?;
    if r.deleted_count == 0 {
        return Err(format!("未找到文档(_id={id})"));
    }
    Ok(())
}

// ── 索引管理(v2)────────────────────────────────────────

/// 索引信息(展示行)
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    /// 键展示,如 "name asc, age desc"
    pub keys: String,
    pub unique: bool,
    /// TTL 秒(None=非 TTL 索引)
    pub ttl: Option<i64>,
}

/// (字段, 降序?)列表 → 键 Document
pub fn index_keys_doc(fields: &[(String, bool)]) -> Document {
    let mut d = Document::new();
    for (f, desc) in fields {
        d.insert(
            f.clone(),
            if *desc {
                Bson::Int32(-1)
            } else {
                Bson::Int32(1)
            },
        );
    }
    d
}

/// 键 Document → "name asc, age desc" 展示串(非 ±1 方向原样显示值)
fn keys_display(keys: &Document) -> String {
    keys.iter()
        .map(|(k, v)| match v {
            Bson::Int32(1) | Bson::Int64(1) => format!("{k} asc"),
            Bson::Int32(-1) | Bson::Int64(-1) => format!("{k} desc"),
            other => format!("{k} {other}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub async fn list_indexes(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
) -> Result<Vec<IndexInfo>, String> {
    let db = default_db(st, database)?;
    use futures_util::StreamExt;
    let mut cur = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection)
        .list_indexes()
        .await
        .map_err(|e| format!("MongoDB: {e}"))?;
    let mut out = Vec::new();
    while let Some(m) = cur.next().await {
        let m = m.map_err(|e| format!("MongoDB: {e}"))?;
        let keys = keys_display(&m.keys);
        let (name, unique, ttl) = match m.options.as_ref() {
            Some(o) => (
                o.name.clone().unwrap_or_else(|| keys.clone()),
                o.unique.unwrap_or(false),
                o.expire_after.map(|d| d.as_secs() as i64),
            ),
            None => (keys.clone(), false, None),
        };
        out.push(IndexInfo {
            name,
            keys,
            unique,
            ttl,
        });
    }
    Ok(out)
}

/// 创建索引:字段列表 + 唯一/TTL/自定义名。
/// IndexOptions 未被 crate 公开导出,用 serde 反序列化组装 IndexModel
/// (camelCase + expireAfterSeconds 整数秒,与 crate 自身映射一致)。
pub async fn create_index(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    fields: &[(String, bool)],
    unique: bool,
    ttl_secs: Option<i64>,
    name: Option<&str>,
) -> Result<String, String> {
    let db = default_db(st, database)?;
    if fields.is_empty() {
        return Err("索引键不能为空".into());
    }
    let keys = index_keys_doc(fields);
    let mut spec = Document::new();
    spec.insert("key", Bson::Document(keys.clone()));
    if unique {
        spec.insert("unique", Bson::Boolean(true));
    }
    if let Some(t) = ttl_secs {
        spec.insert("expireAfterSeconds", Bson::Int64(t));
    }
    if let Some(n) = name.map(str::trim).filter(|s| !s.is_empty()) {
        spec.insert("name", Bson::String(n.to_string()));
    }
    let model: mongodb::IndexModel =
        mongodb::bson::de::from_document(spec).map_err(|e| format!("索引定义无效: {e}"))?;
    st.client
        .database(db.as_str())
        .collection::<Document>(collection)
        .create_index(model)
        .await
        .map_err(|e| format!("创建索引失败: {e}"))?;
    Ok(name
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| keys_display(&keys)))
}

pub async fn drop_index(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    name: &str,
) -> Result<(), String> {
    if name == "_id_" {
        return Err("_id 索引不可删除".into());
    }
    let db = default_db(st, database)?;
    st.client
        .database(db.as_str())
        .collection::<Document>(collection)
        .drop_index(name)
        .await
        .map_err(|e| format!("删除索引失败: {e}"))
}

// ── 批量导入(v2)────────────────────────────────────────

/// JSON → 文档数组:顶层 JSON 数组(每项必须是对象)或单个对象。
/// 纯函数,先于任何写入执行,语法/形态错误零副作用。
pub fn parse_import_docs(json: &str) -> Result<Vec<Document>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("不是合法 JSON: {e}"))?;
    let items: Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        obj @ serde_json::Value::Object(_) => vec![obj],
        _ => return Err("顶层必须是 JSON 数组(每项一个文档)".into()),
    };
    let mut docs = Vec::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        let b = Bson::try_from(it.clone())
            .map_err(|e| format!("第 {} 条转换失败: {e}", i + 1))?;
        match b {
            Bson::Document(d) => docs.push(d),
            _ => return Err(format!("第 {} 条顶层必须是 JSON 对象", i + 1)),
        }
    }
    Ok(docs)
}

/// 批量导入:解析后分 200 条一批 insert_many。
/// 某批失败即中止并报告(已导入条数 + 失败批次范围)——不回放重试,
/// 避免无 _id 文档在部分成功后被重复插入。
pub async fn import_docs(
    st: &crate::model::MongoState,
    database: Option<&str>,
    collection: &str,
    json: &str,
) -> Result<usize, String> {
    let db = default_db(st, database)?;
    let docs = parse_import_docs(json)?;
    let coll = st
        .client
        .database(db.as_str())
        .collection::<Document>(collection);
    let mut ok = 0usize;
    for (ci, chunk) in docs.chunks(200).enumerate() {
        if let Err(e) = coll.insert_many(chunk.to_vec()).await {
            let start = ci * 200 + 1;
            let end = start + chunk.len() - 1;
            return Err(format!("已导入 {ok} 条;第 {start}-{end} 条导入失败: {e}"));
        }
        ok += chunk.len();
    }
    Ok(ok)
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn parse_array_of_objects() {
        let docs = parse_import_docs(r#"[{"a":1},{"b":"x"}]"#).unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].get_i32("a").ok(), Some(1));
    }

    #[test]
    fn parse_single_object() {
        let docs = parse_import_docs(r#"{"a":1}"#).unwrap();
        assert_eq!(docs.len(), 1);
    }

    #[test]
    fn reject_bad_json() {
        assert!(parse_import_docs("{oops").is_err());
    }

    #[test]
    fn reject_scalar_top() {
        assert!(parse_import_docs("42").is_err());
    }

    #[test]
    fn reject_array_of_scalars() {
        assert!(parse_import_docs("[1,2]").is_err());
    }

    #[test]
    fn keys_doc_and_display() {
        let d = index_keys_doc(&[("name".into(), false), ("age".into(), true)]);
        assert_eq!(d.get_i32("name").ok(), Some(1));
        assert_eq!(d.get_i32("age").ok(), Some(-1));
        assert_eq!(keys_display(&d), "name asc, age desc");
    }

    #[test]
    fn index_model_from_spec() {
        // 与 create_index 相同的 serde 组装路径:unique + TTL + 自定义名
        let mut spec = Document::new();
        spec.insert("key", Bson::Document(index_keys_doc(&[("u".into(), true)])));
        spec.insert("unique", Bson::Boolean(true));
        spec.insert("expireAfterSeconds", Bson::Int64(3600));
        spec.insert("name", Bson::String("u_ttl".into()));
        let m: mongodb::IndexModel =
            mongodb::bson::de::from_document(spec).expect("spec 反序列化失败");
        assert_eq!(m.keys.get_i32("u").ok(), Some(-1));
        let o = m.options.as_ref().expect("options 缺失");
        assert_eq!(o.unique, Some(true));
        assert_eq!(o.expire_after, Some(std::time::Duration::from_secs(3600)));
        assert_eq!(o.name.as_deref(), Some("u_ttl"));
    }
}
