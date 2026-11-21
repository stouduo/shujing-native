use crate::backend_dispatch;
use crate::{model, Service};

impl Service {
    pub async fn mongo_get_doc(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        doc_id: &str,
    ) -> Result<String, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::get_doc(st, database, collection, doc_id).await
        })
    }

    /// MongoDB:插入文档(返回 inserted _id 文本)

    pub async fn mongo_insert_doc(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        json: &str,
    ) -> Result<String, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::insert_doc(st, database, collection, json).await
        })
    }

    /// MongoDB:替换文档(_id 不可变)

    pub async fn mongo_replace_doc(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        doc_id: &str,
        json: &str,
    ) -> Result<(), String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::replace_doc(st, database, collection, doc_id, json).await
        })
    }

    /// MongoDB:删除文档

    pub async fn mongo_delete_doc(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        doc_id: &str,
    ) -> Result<(), String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::delete_doc(st, database, collection, doc_id).await
        })
    }

    /// MongoDB:列出集合索引

    pub async fn mongo_list_indexes(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
    ) -> Result<Vec<crate::mongo::IndexInfo>, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::list_indexes(st, database, collection).await
        })
    }

    /// MongoDB:创建索引(返回索引名)
    #[allow(clippy::too_many_arguments)]

    pub async fn mongo_create_index(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        fields: &[(String, bool)],
        unique: bool,
        ttl_secs: Option<i64>,
        name: Option<&str>,
    ) -> Result<String, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::create_index(st, database, collection, fields, unique, ttl_secs, name)
                    .await
        })
    }

    /// MongoDB:删除索引

    pub async fn mongo_drop_index(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        name: &str,
    ) -> Result<(), String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::drop_index(st, database, collection, name).await
        })
    }

    /// MongoDB:批量导入文档(返回成功条数)

    pub async fn mongo_import_docs(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        json: &str,
    ) -> Result<usize, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::import_docs(st, database, collection, json).await
        })
    }

    /// 服务器会话列表(MySQL/PG)

    pub async fn mongo_export_docs(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        max: usize,
    ) -> Result<(String, bool), String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::export_docs(st, database, collection, max).await
        })
    }

    /// MongoDB:批量导入 CSV(首行列名,类型自动推断)

    pub async fn mongo_import_docs_csv(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        csv_raw: &str,
    ) -> Result<usize, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::import_docs_csv(st, database, collection, csv_raw).await
        })
    }

    /// MongoDB 集合计数(带库上下文)

    pub async fn mongo_count(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
    ) -> Result<u64, String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::count(st, database, collection).await
        })
    }

    /// MongoDB 采样浏览:返回(列,行)展平网格

    pub async fn mongo_sample(
        &self,
        id: &str,
        database: Option<&str>,
        collection: &str,
        filter_json: Option<&str>,
        limit: usize,
        offset: u64,
    ) -> Result<(Vec<String>, Vec<Vec<Option<String>>>), String> {
        backend_dispatch!(self, id, Mongo(st) => {
                crate::mongo::sample(st, database, collection, filter_json, limit, offset)
                    .await
        })
    }
}
