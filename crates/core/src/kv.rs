//! 配置中心连接器(中间件对齐第四批):Nacos / Consul / etcd 三协议共用一套
//! KV 抽象——一个 Backend::Kv 变体承载三种 DbType(能力矩阵回报:接入成本
//! 从"全仓十余处臂"降为"此处一文件 + caps() 声明")。
//!
//! 键模型:Consul/etcd 原样 KV;Nacos 的 (group, dataId) 二元编码为
//! "{group}/{dataId}"(DEFAULT_GROUP 省略前缀),put 时反向拆解。
//! 全部走 HTTP:Consul 原生 / etcd v3 grpc-gateway(JSON) / Nacos Open API。

use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KvKind {
    Nacos,
    Consul,
    Etcd,
}

pub struct KvState {
    pub kind: KvKind,
    /// HTTP 根(Nacos 含 /nacos 前缀;Consul/etcd 裸根)
    pub base: String,
    pub auth: Option<(String, String)>,
    /// Nacos 命名空间(tenant;空=public)
    pub namespace: Option<String>,
    /// Nacos 登录态 accessToken
    token: std::sync::Mutex<Option<String>>,
    pub http: reqwest::Client,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KvEntry {
    pub key: String,
    pub value: String,
    pub binary: bool,
}

pub async fn connect(
    kind: KvKind,
    base: String,
    auth: Option<(String, String)>,
    namespace: Option<String>,
) -> Result<KvState, String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {e}"))?;
    let token = std::sync::Mutex::new(None);
    let st = KvState {
        kind,
        base,
        auth,
        namespace,
        token,
        http,
    };
    // 探活:list 空/有皆可,报错即不可达
    list(&st, "").await?;
    Ok(st)
}

fn b64enc(s: &str) -> String {
    use std::io::Write;
    let mut out = String::new();
    let b = s.as_bytes();
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (c.get(1).copied().unwrap_or(0) as u32) << 8
            | (c.get(2).copied().unwrap_or(0) as u32);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    let _ = std::io::stdout().flush();
    out
}

fn b64dec(s: &str) -> Option<Vec<u8>> {
    let T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|t| *t == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Nacos (group, dataId) ↔ 字符串键编码
fn nacos_key(group: &str, data_id: &str) -> String {
    if group == "DEFAULT_GROUP" {
        data_id.to_string()
    } else {
        format!("{group}/{data_id}")
    }
}

fn nacos_split(key: &str) -> (String, String) {
    match key.split_once('/') {
        Some((g, d)) if !g.is_empty() && !d.is_empty() => (g.to_string(), d.to_string()),
        _ => ("DEFAULT_GROUP".to_string(), key.to_string()),
    }
}

async fn nacos_token(st: &KvState) -> Result<Option<String>, String> {
    let Some((u, p)) = &st.auth else {
        return Ok(None);
    };
    if let Ok(g) = st.token.lock() {
        if let Some(t) = g.as_ref() {
            return Ok(Some(t.clone()));
        }
    }
    let resp = st
        .http
        .post(format!("{}/nacos/v1/auth/login", st.base))
        .form(&[("username", u.as_str()), ("password", p.as_str())])
        .send()
        .await
        .map_err(|e| format!("Nacos 登录: {e}"))?;
    let text = resp.text().await.map_err(|e| format!("Nacos: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("Nacos 登录响应: {e}"))?;
    let tk = v["accessToken"]
        .as_str()
        .ok_or("Nacos 登录失败(无 accessToken)")?
        .to_string();
    if let Ok(mut g) = st.token.lock() {
        *g = Some(tk.clone());
    }
    Ok(Some(tk))
}

async fn http_get(st: &KvState, url: &str) -> Result<String, String> {
    let resp = st.http.get(url).send().await.map_err(|e| format!("HTTP: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("HTTP: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {} {}", status, text.trim()));
    }
    Ok(text)
}

/// 列出键(前缀过滤;空前缀=全部)。每键带当前值。
pub async fn list(st: &KvState, prefix: &str) -> Result<Vec<KvEntry>, String> {
    let mut out: Vec<KvEntry> = match st.kind {
        KvKind::Consul => {
            // consul 2.x:空前缀+recurse 返回 404(防全库拖垮),根列取走 ?keys
            // 再逐键取值(配置中心键数有限,上限 500)
            let keys_raw = http_get(st, &format!("{}/v1/kv/?keys", st.base)).await?;
            let key_names: Vec<String> =
                serde_json::from_str(&keys_raw).map_err(|e| format!("Consul: {e}"))?;
            let mut out = Vec::new();
            for key in key_names.into_iter().take(500) {
                if let Ok(raw) = http_get(st, &format!("{}/v1/kv/{}", st.base, urlenc(&key))).await
                {
                    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                        let bytes = v[0]["Value"].as_str().and_then(b64dec).unwrap_or_default();
                        let (payload, binary) = match std::str::from_utf8(&bytes) {
                            Ok(s) => (s.to_string(), false),
                            Err(_) => (
                                bytes[..bytes.len().min(24)]
                                    .iter()
                                    .map(|b| format!("{b:02x}"))
                                    .collect(),
                                true,
                            ),
                        };
                        out.push(KvEntry {
                            key,
                            value: payload,
                            binary,
                        });
                    }
                }
            }
            return Ok(out);
        }
        KvKind::Etcd => {
            // etcd 前缀扫描:range_end = 末字节+1 进位('\0' 会变成 >=key 的全量语义)
            let mut key_b = String::new();
            let mut range_b = String::new();
            if !prefix.is_empty() {
                key_b = b64enc(prefix);
                let mut last = prefix.as_bytes().to_vec();
                let mut i = last.len();
                loop {
                    if i == 0 {
                        // 全 0xFF 前缀:range_end 留空 = 到末尾
                        range_b = String::new();
                        break;
                    }
                    i -= 1;
                    if last[i] < 0xFF {
                        last[i] += 1;
                        range_b = b64enc(&String::from_utf8_lossy(&last[..i + 1]));
                        break;
                    }
                }
            }
            // etcd 扫描:全量= key=\0 → range_end=\xff\xff\xff(\0..\x01 不含普通键,
            // gateway 空串会被当精确查);前缀= 末字节+1 进位
            let mut body = serde_json::json!({
                "key": if prefix.is_empty() { b64enc("\u{0}") } else { key_b },
                "range_end": if prefix.is_empty() {
                    serde_json::Value::String(b64enc("\u{ff}\u{ff}\u{ff}"))
                } else {
                    serde_json::Value::String(range_b)
                },
                "keys_only": false,
            });
            let v: Value = get_json_post(st, "/v3/kv/range", &body).await?;
            v["kvs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|kv| {
                    let key_b64 = kv["key"].as_str()?;
                    let key = String::from_utf8(b64dec(key_b64)?).ok()?;
                    let bytes = kv["value"].as_str().and_then(b64dec).unwrap_or_default();
                    let (payload, binary) = match std::str::from_utf8(&bytes) {
                        Ok(s) => (s.to_string(), false),
                        Err(_) => (
                            bytes[..bytes.len().min(24)]
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect(),
                            true,
                        ),
                    };
                    Some(KvEntry {
                        key,
                        value: payload,
                        binary,
                    })
                })
                .collect()
        }
        KvKind::Nacos => {
            let tk = nacos_token(st).await?;
            let tenant = st.namespace.clone().unwrap_or_default();
            let mut url = format!(
                "{}/nacos/v1/cs/configs?search=accurate&pageNo=1&pageSize=200&tenant={tenant}",
                st.base
            );
            if let Some(t) = &tk {
                url.push_str(&format!("&accessToken={t}"));
            }
            let raw = http_get(st, &url).await?;
            let v: Value = serde_json::from_str(&raw).map_err(|e| format!("Nacos: {e}"))?;
            let mut out = Vec::new();
            if let Some(items) = v["pageItems"].as_array() {
                for it in items {
                    let data_id = it["dataId"].as_str().unwrap_or("").to_string();
                    let group = it["group"].as_str().unwrap_or("DEFAULT_GROUP").to_string();
                    let content = it["content"].as_str().unwrap_or("").to_string();
                    out.push(KvEntry {
                        key: nacos_key(&group, &data_id),
                        value: content,
                        binary: false,
                    });
                }
            }
            out
        }
    };
    if !prefix.is_empty() {
        out.retain(|e| e.key.starts_with(prefix));
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

async fn get_json_post(st: &KvState, path: &str, body: &Value) -> Result<Value, String> {
    let resp = st
        .http
        .post(format!("{}{}", st.base, path))
        .json(body)
        .send()
        .await
        .map_err(|e| format!("etcd: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("etcd: {e}"))?;
    if !status.is_success() {
        return Err(format!("etcd: HTTP {} {}", status, text.trim()));
    }
    serde_json::from_str(&text).map_err(|e| format!("etcd 响应解析失败: {e}"))
}

/// 读单个键(不存在返回 Err)
pub async fn get(st: &KvState, key: &str) -> Result<String, String> {
    match st.kind {
        KvKind::Consul => {
            let raw = http_get(st, &format!("{}/v1/kv/{}", st.base, urlenc(key))).await?;
            let v: Value = serde_json::from_str(&raw).map_err(|e| format!("Consul: {e}"))?;
            v[0]["Value"]
                .as_str()
                .and_then(b64dec)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .ok_or_else(|| format!("键 {key} 值为空或不存在"))
        }
        KvKind::Etcd => {
            let body = serde_json::json!({"key": b64enc(key)});
            let v = get_json_post(st, "/v3/kv/range", &body).await?;
            let b64 = v["kvs"][0]["value"]
                .as_str()
                .ok_or_else(|| format!("键 {key} 不存在"))?;
            String::from_utf8(b64dec(b64).unwrap_or_default())
                .map_err(|e| format!("值非 UTF8: {e}"))
        }
        KvKind::Nacos => {
            let (group, data_id) = nacos_split(key);
            let tk = nacos_token(st).await?;
            let tenant = st.namespace.clone().unwrap_or_default();
            let mut url = format!(
                "{}/nacos/v1/cs/configs?dataId={}&group={}&tenant={tenant}",
                st.base,
                urlenc(&data_id),
                urlenc(&group)
            );
            if let Some(t) = &tk {
                url.push_str(&format!("&accessToken={t}"));
            }
            http_get(st, &url).await
        }
    }
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 写键(Consul/etcd 原样;Nacos 经 group/dataId 拆解)
pub async fn put(st: &KvState, key: &str, value: &str) -> Result<(), String> {
    match st.kind {
        KvKind::Consul => {
            let resp = st
                .http
                .put(format!("{}/v1/kv/{}", st.base, urlenc(key)))
                .body(value.to_string())
                .send()
                .await
                .map_err(|e| format!("Consul: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Consul: HTTP {}", resp.status()));
            }
            Ok(())
        }
        KvKind::Etcd => {
            let body = serde_json::json!({"key": b64enc(key), "value": b64enc(value)});
            get_json_post(st, "/v3/kv/put", &body).await.map(|_| ())
        }
        KvKind::Nacos => {
            let (group, data_id) = nacos_split(key);
            let tk = nacos_token(st).await?;
            let tenant = st.namespace.clone().unwrap_or_default();
            let mut form: Vec<(&str, String)> = vec![
                ("dataId", data_id),
                ("group", group),
                ("content", value.to_string()),
                ("type", "text".into()),
                ("tenant", tenant),
            ];
            if let Some(t) = &tk {
                form.push(("accessToken", t.clone()));
            }
            let resp = st
                .http
                .post(format!("{}/nacos/v1/cs/configs", st.base))
                .form(&form)
                .send()
                .await
                .map_err(|e| format!("Nacos: {e}"))?;
            let status = resp.status();
            let text = resp.text().await.map_err(|e| format!("Nacos: {e}"))?;
            if !status.is_success() || text.trim() != "true" {
                return Err(format!("Nacos 写入失败: HTTP {} {}", status, text.trim()));
            }
            Ok(())
        }
    }
}

/// 删键
pub async fn delete(st: &KvState, key: &str) -> Result<(), String> {
    match st.kind {
        KvKind::Consul => {
            let resp = st
                .http
                .delete(format!("{}/v1/kv/{}", st.base, urlenc(key)))
                .send()
                .await
                .map_err(|e| format!("Consul: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Consul: HTTP {}", resp.status()));
            }
            Ok(())
        }
        KvKind::Etcd => {
            let body = serde_json::json!({"key": b64enc(key)});
            get_json_post(st, "/v3/kv/deleterange", &body).await.map(|_| ())
        }
        KvKind::Nacos => {
            let (group, data_id) = nacos_split(key);
            let tk = nacos_token(st).await?;
            let tenant = st.namespace.clone().unwrap_or_default();
            let mut url = format!(
                "{}/nacos/v1/cs/configs?dataId={}&group={}&tenant={tenant}",
                st.base,
                urlenc(&data_id),
                urlenc(&group)
            );
            if let Some(t) = &tk {
                url.push_str(&format!("&accessToken={t}"));
            }
            let resp = st.http.delete(url).send().await.map_err(|e| format!("Nacos: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("Nacos 删除失败: HTTP {}", resp.status()));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod kv_tests {
    use super::*;

    #[test]
    fn nacos_key_roundtrip() {
        assert_eq!(nacos_split("my-app.yaml"), ("DEFAULT_GROUP".into(), "my-app.yaml".into()));
        assert_eq!(nacos_split("CUSTOM/db.properties"), ("CUSTOM".into(), "db.properties".into()));
        assert_eq!(nacos_key("DEFAULT_GROUP", "a.yml"), "a.yml");
        assert_eq!(nacos_key("G1", "a.yml"), "G1/a.yml");
        // 无 group 段的斜杠键回默认组
        assert_eq!(nacos_split("/x"), ("DEFAULT_GROUP".into(), "/x".into()));
    }

    #[test]
    fn b64_roundtrip() {
        for s in ["", "abc", "hello world", "中文值", "ab"] {
            assert_eq!(b64dec(&b64enc(s)).as_deref(), Some(s.as_bytes()));
        }
    }

    #[test]
    fn urlenc_keeps_safe() {
        assert_eq!(urlenc("a/b c"), "a%2Fb%20c");
        assert_eq!(urlenc("abc.1_2-3"), "abc.1_2-3");
    }
}
