//! AI SQL 助手:OpenAI 兼容 /chat/completions(默认 GLM 端点,任何兼容服务可配)。
//! 三个能力:自然语言→SQL(带库结构上下文)/ 解释 SQL / 优化建议。
//! 生成结果只返回文本,调用方展示给用户审阅——绝不自动执行。

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiConfig {
    /// OpenAI 兼容 base_url(到 /v1 层;如 https://open.bigmodel.cn/api/paas/v4)
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            api_key: String::new(),
            model: "glm-4.7-flash".into(),
        }
    }
}

async fn chat(cfg: &AiConfig, system: &str, user: &str) -> Result<String, String> {
    if cfg.api_key.trim().is_empty() {
        return Err("未配置 AI API Key(设置 → AI 助手)".into());
    }
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "temperature": 0.2,
    });
    let client = reqwest::Client::new();
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client
            .post(&url)
            .bearer_auth(&cfg.api_key)
            .json(&body)
            .send(),
    )
    .await
    .map_err(|_| "AI 请求超时(60s)".to_string())?
    .map_err(|e| format!("AI 请求失败: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("AI 响应读取失败: {e}"))?;
    if !status.is_success() {
        // 截断长错误体(4xx 带 html 时很难看)
        let clip: String = text.chars().take(300).collect();
        return Err(format!("AI 服务返回 {status}: {clip}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("AI 响应非 JSON: {e}"))?;
    v.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("AI 响应缺 choices[0].message.content: {}", &text[..text.len().min(200)]))
}

/// 从回复中剥出 SQL(```sql ... ``` 围栏或裸语句)
pub fn extract_sql(reply: &str) -> String {
    // 优先 ```sql 围栏
    if let Some(i) = reply.find("```sql") {
        if let Some(rest) = reply.get(i + 6..) {
            if let Some(j) = rest.find("```") {
                return rest[..j].trim().to_string();
            }
        }
    }
    // 任意 ``` 围栏:首个换行前是语言标记(SELECT 这类内容本身不会被误剥)
    if let Some(i) = reply.find("```") {
        if let Some(rest) = reply.get(i + 3..) {
            let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or(rest);
            if let Some(j) = body.find("```") {
                return body[..j].trim().to_string();
            }
        }
    }
    reply.trim().to_string()
}

const SQL_SYSTEM: &str = "你是数据库 SQL 专家。根据用户的自然语言需求和提供的数据库结构,写出一条可直接执行的 SQL。\
只输出 SQL(用 ```sql 围栏),不要解释。写之前确认涉及的表和列都真实存在于给定结构中。";

/// 自然语言 → SQL;schema_ctx 形如 "表 users(id INT PK, name VARCHAR(64), ...)\n表 orders(...)"
pub async fn generate_sql(
    cfg: &AiConfig,
    dialect: &str,
    schema_ctx: &str,
    request: &str,
) -> Result<String, String> {
    let user = format!(
        "数据库方言: {dialect}\n\n数据库结构:\n{schema_ctx}\n\n需求: {request}"
    );
    let reply = chat(cfg, SQL_SYSTEM, &user).await?;
    Ok(extract_sql(&reply))
}

const EXPLAIN_SYSTEM: &str = "你是数据库 SQL 专家。用简体中文解释给定的 SQL:它做什么、\
用了哪些表和条件、执行顺序的直觉理解。控制在 200 字以内,要点式。";

pub async fn explain_sql(
    cfg: &AiConfig,
    dialect: &str,
    schema_ctx: &str,
    sql: &str,
) -> Result<String, String> {
    let user = format!(
        "数据库方言: {dialect}\n\n数据库结构(参考):\n{schema_ctx}\n\nSQL:\n{sql}"
    );
    chat(cfg, EXPLAIN_SYSTEM, &user).await
}

const OPTIMIZE_SYSTEM: &str = "你是数据库性能专家。分析给定的 SQL 和表结构,指出性能问题\
(缺索引/全表扫描风险/写法可优化处)并给出改进后的 SQL。\
如果已经足够好就直说。简体中文,300 字以内,改进 SQL 用 ```sql 围栏。";

pub async fn optimize_sql(
    cfg: &AiConfig,
    dialect: &str,
    schema_ctx: &str,
    sql: &str,
) -> Result<String, String> {
    let user = format!(
        "数据库方言: {dialect}\n\n数据库结构(参考):\n{schema_ctx}\n\nSQL:\n{sql}"
    );
    chat(cfg, OPTIMIZE_SYSTEM, &user).await
}

const FIX_SYSTEM: &str = "你是数据库 SQL 专家。用户会给出一条执行报错的 SQL 和数据库返回的\
错误信息,请定位原因(语法/字段不存在/类型不匹配/权限/方言差异等)并给出修正后的 SQL。\
先一句话说明原因,再给修正 SQL(用 ```sql 围栏)。如果信息不足以定位,列出最可能的 2-3 个原因。简体中文。";

pub async fn fix_sql(
    cfg: &AiConfig,
    dialect: &str,
    schema_ctx: &str,
    sql: &str,
    error: &str,
) -> Result<String, String> {
    let user = format!(
        "数据库方言: {dialect}\n\n数据库结构(参考):\n{schema_ctx}\n\nSQL:\n{sql}\n\n报错信息:\n{error}"
    );
    chat(cfg, FIX_SYSTEM, &user).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sql_fences() {
        assert_eq!(
            extract_sql("看这个:\n```sql\nSELECT 1;\n```\n完了"),
            "SELECT 1;"
        );
        assert_eq!(extract_sql("```SELECT 2```"), "SELECT 2");
        assert_eq!(extract_sql("SELECT 3;"), "SELECT 3;");
    }
}
