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
