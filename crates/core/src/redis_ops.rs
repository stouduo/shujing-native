//! Redis 专用命令通道:键空间 / SCAN / 详情 / 删除 / SET / 原始命令
use crate::model::{es, Backend, ConnInfo, DbType};
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;

async fn get_con(backend: &mut Backend) -> Result<MultiplexedConnection, String> {
    match backend {
        Backend::Redis(c) => {
            // 多路复用连接;SELECT 会影响该连接,故每命令前都显式 SELECT
            c.get_multiplexed_async_connection().await.map_err(|e| format!("Redis 连接失败: {e}"))
        }
        _ => Err("非 Redis 连接".into()),
    }
}

/// 键空间概览:有数据的 DB 列表
pub async fn databases(backend: &mut Backend) -> Result<Vec<(u8, u64)>, String> {
    let mut con = get_con(backend).await?;
    let info: String = redis::cmd("INFO").arg("keyspace").query_async(&mut con).await.map_err(es)?;
    let mut out = Vec::new();
    for line in info.lines() {
        if let Some(rest) = line.strip_prefix("db") {
            let mut parts = rest.splitn(2, ':');
            let idx: u8 = parts.next().unwrap_or("0").parse().unwrap_or(0);
            let meta = parts.next().unwrap_or("");
            let keys = meta
                .split(',')
                .find_map(|p| p.strip_prefix("keys="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0u64);
            out.push((idx, keys));
        }
    }
    if out.is_empty() {
        out.push((0, 0));
    }
    Ok(out)
}

/// SCAN 收集 keys(每次最多 count 个,返回新游标)
pub async fn scan_keys(
    backend: &mut Backend,
    db: u8,
    pattern: &str,
    cursor: u64,
    count: usize,
) -> Result<(u64, Vec<String>), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
        .arg(cursor)
        .arg("MATCH")
        .arg(pattern)
        .arg("COUNT")
        .arg(count)
        .query_async(&mut con)
        .await
        .map_err(es)?;
    Ok((next, keys))
}

/// key 详情(类型/TTL/长度/内容预览)
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedisDetail {
    pub key_type: String,
    pub ttl: i64,
    pub len: u64,
    /// string 内容
    pub text: Option<String>,
    /// list/set/zset/hash 的成对内容
    pub pairs: Vec<(String, String)>,
}

pub async fn key_detail(
    backend: &mut Backend,
    db: u8,
    key: &str,
) -> Result<RedisDetail, String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    let kt: String = redis::cmd("TYPE").arg(key).query_async(&mut con).await.map_err(es)?;
    let ttl: i64 = redis::cmd("TTL").arg(key).query_async(&mut con).await.map_err(es)?;
    let mut d = RedisDetail { key_type: kt.clone(), ttl, len: 0, text: None, pairs: vec![] };
    match kt.as_str() {
        "string" => {
            let v: Option<String> = con.get(key).await.map_err(es)?;
            d.len = v.as_deref().map(|s| s.len()).unwrap_or(0) as u64;
            d.text = v.map(|s| s.chars().take(50_000).collect());
        }
        "list" => {
            let items: Vec<String> = con.lrange(key, 0, 199).await.map_err(es)?;
            d.len = redis::cmd("LLEN").arg(key).query_async(&mut con).await.map_err(es)?;
            d.pairs = items.into_iter().map(|v| (String::new(), v)).collect();
        }
        "set" => {
            let mut items: Vec<String> = con.smembers(key).await.map_err(es)?;
            items.sort();
            d.len = items.len() as u64;
            d.pairs = items.into_iter().map(|v| (String::new(), v)).collect();
        }
        "zset" => {
            let items: Vec<(String, f64)> = con.zrange_withscores(key, 0, 199).await.map_err(es)?;
            d.len = redis::cmd("ZCARD").arg(key).query_async(&mut con).await.map_err(es)?;
            d.pairs = items.into_iter().map(|(m, s)| (m, format!("{s}"))).collect();
        }
        "hash" => {
            let map: std::collections::HashMap<String, String> = con.hgetall(key).await.map_err(es)?;
            d.len = redis::cmd("HLEN").arg(key).query_async(&mut con).await.map_err(es)?;
            let mut pairs: Vec<(String, String)> = map.into_iter().collect();
            pairs.sort();
            d.pairs = pairs;
        }
        "stream" => {
            let entries: Vec<(String, Vec<(String, String)>)> =
                redis::cmd("XRANGE").arg(key).arg("-").arg("+").arg("COUNT").arg(100)
                    .query_async(&mut con).await.map_err(es)?;
            d.len = redis::cmd("XLEN").arg(key).query_async(&mut con).await.map_err(es)?;
            d.pairs = entries
                .into_iter()
                .flat_map(|(id, kvs)| {
                    let joined = kvs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
                    vec![(id, joined)]
                })
                .collect();
        }
        other => {
            return Err(format!("暂不支持展示类型: {other}"));
        }
    }
    Ok(d)
}

/// key 统计(大 key / 热 key 分析)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStat {
    pub key: String,
    pub key_type: String,
    /// 字节(memory usage),-1 表示未知
    pub mem: i64,
    /// 元素数 / 字节长,,-1 表示未知
    pub len: i64,
    /// LFU 访问频率,-1 表示未知
    pub freq: i64,
}

/// 扫描分析:mode = "big"(内存+元素数)| "hot"(LFU 频率)
pub async fn analyze(
    backend: &mut Backend,
    db: u8,
    sample: usize,
    mode: &str,
) -> Result<Vec<KeyStat>, String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;

    if mode == "hot" {
        // OBJECT FREQ 依赖 LFU 策略
        let policy: Vec<String> = redis::cmd("CONFIG")
            .arg("GET")
            .arg("maxmemory-policy")
            .query_async(&mut con)
            .await
            .map_err(es)?;
        let p = policy.get(1).cloned().unwrap_or_default();
        if !p.contains("lfu") {
            return Err(format!("当前淘汰策略为 {p}(非 LFU),无法统计访问频率;可在服务端设置 maxmemory-policy=allkeys-lfu 后使用"));
        }
    }

    // SCAN 收集采样
    let mut keys: Vec<String> = Vec::new();
    let mut cursor: u64 = 0;
    'outer: loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("COUNT")
            .arg(500)
            .query_async(&mut con)
            .await
            .map_err(es)?;
        keys.extend(batch);
        if keys.len() >= sample || next == 0 {
            break 'outer;
        }
        cursor = next;
    }
    keys.truncate(sample);
    if keys.is_empty() {
        return Ok(vec![]);
    }

    // 分批 pipeline 统计
    let mut stats: Vec<KeyStat> = Vec::new();
    const BATCH: usize = 100;
    for chunk in keys.chunks(BATCH) {
        // 类型
        let mut pipe = redis::pipe();
        for k in chunk {
            pipe.cmd("TYPE").arg(k);
        }
        let types: Vec<String> = pipe.query_async(&mut con).await.map_err(es)?;

        if mode == "hot" {
            let mut pipe = redis::pipe();
            for k in chunk {
                pipe.cmd("OBJECT").arg("FREQ").arg(k);
            }
            let freqs: Vec<Option<i64>> = pipe.query_async(&mut con).await.map_err(es)?;
            for (i, k) in chunk.iter().enumerate() {
                stats.push(KeyStat {
                    key: k.clone(),
                    key_type: types[i].clone(),
                    mem: -1,
                    len: -1,
                    freq: freqs[i].unwrap_or(-1),
                });
            }
            continue;
        }

        // big:MEMORY USAGE
        let mut pipe = redis::pipe();
        for k in chunk {
            pipe.cmd("MEMORY").arg("USAGE").arg(k);
        }
        let mems: Vec<Option<i64>> = pipe.query_async(&mut con).await.map_err(es)?;
        // 元素数(按类型)
        let mut pipe = redis::pipe();
        for (i, k) in chunk.iter().enumerate() {
            match types[i].as_str() {
                "string" => pipe.cmd("STRLEN").arg(k),
                "list" => pipe.cmd("LLEN").arg(k),
                "hash" => pipe.cmd("HLEN").arg(k),
                "set" => pipe.cmd("SCARD").arg(k),
                "zset" => pipe.cmd("ZCARD").arg(k),
                _ => pipe.cmd("STRLEN").arg(k),
            };
        }
        let lens: Vec<Option<i64>> = pipe.query_async(&mut con).await.map_err(es)?;

        for (i, k) in chunk.iter().enumerate() {
            stats.push(KeyStat {
                key: k.clone(),
                key_type: types[i].clone(),
                mem: mems[i].unwrap_or(-1),
                len: lens[i].unwrap_or(-1),
                freq: -1,
            });
        }
    }

    // 排序:big 按内存(未知按 len),hot 按频率
    if mode == "hot" {
        stats.sort_by(|a, b| b.freq.cmp(&a.freq));
    } else {
        stats.sort_by(|a, b| b.mem.cmp(&a.mem).then(b.len.cmp(&a.len)));
    }
    Ok(stats)
}

/// TTL 管理:seconds>0 EXPIRE,-1 PERSIST
pub async fn set_ttl(backend: &mut Backend, db: u8, key: &str, seconds: i64) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    if seconds < 0 {
        redis::cmd("PERSIST").arg(key).query_async::<()>(&mut con).await.map_err(es)?;
    } else {
        redis::cmd("EXPIRE").arg(key).arg(seconds).query_async::<()>(&mut con).await.map_err(es)?;
    }
    Ok(())
}

pub async fn rename(backend: &mut Backend, db: u8, key: &str, new_key: &str) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    redis::cmd("RENAME").arg(key).arg(new_key).query_async::<()>(&mut con).await.map_err(es)
}

/// pipeline 批量 TTL
pub async fn ttl_batch(backend: &mut Backend, db: u8, keys: &[String]) -> Result<Vec<i64>, String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    let mut pipe = redis::pipe();
    for k in keys {
        pipe.cmd("TTL").arg(k);
    }
    pipe.query_async(&mut con).await.map_err(es)
}

/// 新建 key:按类型写入(string/list/set/zset/hash)
pub async fn new_key(
    backend: &mut Backend,
    db: u8,
    key: &str,
    key_type: &str,
    text: &str,
    pairs: &[(String, String)],
) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    match key_type {
        "string" => {
            redis::cmd("SET").arg(key).arg(text).query_async::<()>(&mut con).await.map_err(es)?
        }
        "hash" => {
            if pairs.is_empty() {
                return Err("hash 需要至少一个字段".into());
            }
            let mut pipe = redis::pipe();
            for (f, v) in pairs {
                pipe.cmd("HSET").arg(key).arg(f).arg(v);
            }
            pipe.query_async::<()>(&mut con).await.map_err(es)?
        }
        "list" => {
            if pairs.is_empty() {
                return Err("list 需要至少一个元素".into());
            }
            let mut pipe = redis::pipe();
            for (_, v) in pairs {
                pipe.cmd("RPUSH").arg(key).arg(v);
            }
            pipe.query_async::<()>(&mut con).await.map_err(es)?
        }
        "set" => {
            if pairs.is_empty() {
                return Err("set 需要至少一个成员".into());
            }
            let mut pipe = redis::pipe();
            for (_, v) in pairs {
                pipe.cmd("SADD").arg(key).arg(v);
            }
            pipe.query_async::<()>(&mut con).await.map_err(es)?
        }
        "zset" => {
            if pairs.is_empty() {
                return Err("zset 需要至少一个成员".into());
            }
            let mut pipe = redis::pipe();
            for (m, score) in pairs {
                pipe.cmd("ZADD").arg(key).arg(score).arg(m);
            }
            pipe.query_async::<()>(&mut con).await.map_err(es)?
        }
        other => return Err(format!("不支持的类型: {other}")),
    }
    Ok(())
}

/// hash 字段:写(编辑或新增)
pub async fn hash_set(backend: &mut Backend, db: u8, key: &str, field: &str, value: &str) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    redis::cmd("HSET").arg(key).arg(field).arg(value).query_async::<()>(&mut con).await.map_err(es)
}

/// 成员操作:删除(SREM/ZREM/LREM)/ 添加(SADD/ZADD/RPUSH)/ list 按下标改(LSET)
pub async fn member_op(
    backend: &mut Backend,
    db: u8,
    key: &str,
    key_type: &str,
    op: &str, // "del" | "add" | "lset"
    member: &str,
    extra: &str, // zset score / lset index / hash value
) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    match (key_type, op) {
        ("hash", "del") => redis::cmd("HDEL").arg(key).arg(member).query_async::<()>(&mut con).await.map_err(es)?,
        ("hash", "add") | ("hash", "set") => {
            redis::cmd("HSET").arg(key).arg(member).arg(extra).query_async::<()>(&mut con).await.map_err(es)?
        }
        ("set", "del") => redis::cmd("SREM").arg(key).arg(member).query_async::<()>(&mut con).await.map_err(es)?,
        ("set", "add") => redis::cmd("SADD").arg(key).arg(member).query_async::<()>(&mut con).await.map_err(es)?,
        ("zset", "del") => redis::cmd("ZREM").arg(key).arg(member).query_async::<()>(&mut con).await.map_err(es)?,
        ("zset", "add") => {
            let score: f64 = extra.trim().parse().map_err(|_| "zset 分数无效")?;
            redis::cmd("ZADD").arg(key).arg(score).arg(member).query_async::<()>(&mut con).await.map_err(es)?
        }
        ("list", "del") => {
            redis::cmd("LREM").arg(key).arg(1).arg(member).query_async::<()>(&mut con).await.map_err(es)?
        }
        ("list", "add") => redis::cmd("RPUSH").arg(key).arg(member).query_async::<()>(&mut con).await.map_err(es)?,
        ("list", "lset") => {
            let idx: i64 = extra.trim().parse().map_err(|_| "list 下标无效")?;
            redis::cmd("LSET").arg(key).arg(idx).arg(member).query_async::<()>(&mut con).await.map_err(es)?
        }
        _ => return Err(format!("不支持的组合: {key_type}/{op}")),
    }
    Ok(())
}

/// pipeline 批量 TYPE
pub async fn key_types(backend: &mut Backend, db: u8, keys: &[String]) -> Result<Vec<String>, String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    let mut pipe = redis::pipe();
    for k in keys {
        pipe.cmd("TYPE").arg(k);
    }
    let types: Vec<String> = pipe.query_async(&mut con).await.map_err(es)?;
    Ok(types)
}

pub async fn del_key(backend: &mut Backend, db: u8, key: &str) -> Result<u64, String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    redis::cmd("DEL").arg(key).query_async(&mut con).await.map_err(es)
}

pub async fn set_string(
    backend: &mut Backend,
    db: u8,
    key: &str,
    value: &str,
    ttl_keep: bool,
) -> Result<(), String> {
    let mut con = get_con(backend).await?;
    redis::cmd("SELECT").arg(db).query_async::<()>(&mut con).await.map_err(es)?;
    let old_ttl: i64 = redis::cmd("TTL").arg(key).query_async(&mut con).await.map_err(es)?;
    redis::cmd("SET").arg(key).arg(value).query_async::<()>(&mut con).await.map_err(es)?;
    if ttl_keep && old_ttl > 0 {
        redis::cmd("EXPIRE").arg(key).arg(old_ttl).query_async::<()>(&mut con).await.map_err(es)?;
    }
    Ok(())
}

/// 原始命令执行(简单空格分词,支持引号),逐行返回结果
pub async fn run_command(
    backend: &mut Backend,
    info: &ConnInfo,
    cmd_text: &str,
) -> Result<Vec<String>, String> {
    if info.db_type == DbType::Redis && info.read_only && !crate::model::is_readonly_redis(cmd_text) {
        return Err("只读连接:该 Redis 命令被拒绝".into());
    }
    let mut con = get_con(backend).await?;
    // db 编号默认 SELECT(连接串已带,这里不强制)
    let parts = split_args(cmd_text)?;
    if parts.is_empty() {
        return Err("命令为空".into());
    }
    let mut cmd = redis::cmd(&parts[0]);
    for a in &parts[1..] {
        cmd.arg(a);
    }
    let val: redis::Value = cmd.query_async(&mut con).await.map_err(es)?;
    Ok(render_value(val, 0))
}

fn render_value(v: redis::Value, depth: usize) -> Vec<String> {
    match v {
        redis::Value::Nil => vec!["(nil)".into()],
        redis::Value::Int(i) => vec!["(integer) ".to_string() + &i.to_string()],
        redis::Value::BulkString(bytes) => {
            let s = String::from_utf8_lossy(&bytes);
            if depth == 0 { vec![format!("{s:?}")] } else { vec![s.to_string()] }
        }
        redis::Value::SimpleString(s) => vec![s],
        redis::Value::Okay => vec!["OK".into()],
        redis::Value::Array(items) => {
            let mut out = Vec::new();
            for (i, it) in items.iter().enumerate() {
                for line in render_value(it.clone(), depth + 1) {
                    out.push(format!("{}) {}", i + 1, line));
                }
            }
            if out.is_empty() { vec!["(empty array)".into()] } else { out }
        }
        redis::Value::Map(pairs) => pairs
            .into_iter()
            .flat_map(|(k, v)| {
                let ks = render_value(k, depth + 1).join(" ");
                let vs = render_value(v, depth + 1).join(" ");
                vec![format!("{ks} -> {vs}")]
            })
            .collect(),
        redis::Value::Set(items) => items.into_iter().flat_map(|v| render_value(v, depth + 1)).collect(),
        redis::Value::Double(d) => vec![format!("(double) {d}")],
        redis::Value::Boolean(b) => vec![format!("(bool) {b}")],
        other => vec![format!("{other:?}")],
        redis::Value::Array(items) => {
            let mut out = Vec::new();
            for (i, it) in items.iter().enumerate() {
                for line in render_value(it.clone(), depth + 1) {
                    out.push(format!("{}) {}", i + 1, line));
                }
            }
            if out.is_empty() { vec!["(empty array)".into()] } else { out }
        }
    }
}

/// 简易分词:空格分隔,支持双引号包裹(空引号产生空参数)
fn split_args(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has_token = false; // 当前 token 已由引号或字符开启
    let mut quote = false;
    for ch in s.chars() {
        if quote {
            if ch == '"' {
                quote = false;
            } else {
                cur.push(ch);
            }
        } else if ch == '"' {
            quote = true;
            has_token = true;
        } else if ch.is_whitespace() {
            if has_token || !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                has_token = false;
            }
        } else {
            cur.push(ch);
            has_token = true;
        }
    }
    if has_token || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_args_basic() {
        assert_eq!(split_args("GET key").unwrap(), vec!["GET", "key"]);
        assert_eq!(split_args("SET  k v").unwrap(), vec!["SET", "k", "v"]);
    }

    #[test]
    fn split_args_quotes() {
        assert_eq!(
            split_args("SET msg \"hello world\"").unwrap(),
            vec!["SET", "msg", "hello world"]
        );
        assert_eq!(split_args("SET k \"\"").unwrap(), vec!["SET", "k", ""]);
    }

    #[test]
    fn split_args_spaces_in_quotes() {
        assert_eq!(
            split_args("HSET h f \"a  b\"").unwrap(),
            vec!["HSET", "h", "f", "a  b"]
        );
    }

    #[test]
    fn split_args_chinese() {
        assert_eq!(split_args("SET 名字 张三").unwrap(), vec!["SET", "名字", "张三"]);
    }

    #[test]
    fn render_value_variants() {
        assert_eq!(render_value(redis::Value::Nil, 0), vec!["(nil)"]);
        assert_eq!(render_value(redis::Value::Int(42), 0), vec!["(integer) 42"]);
        assert_eq!(render_value(redis::Value::Okay, 0), vec!["OK"]);
        assert_eq!(
            render_value(redis::Value::BulkString(b"abc".to_vec()), 0),
            vec!["\"abc\""]
        );
        assert_eq!(
            render_value(
                redis::Value::Array(vec![redis::Value::Int(1), redis::Value::Int(2)]),
                0
            ),
            vec!["1) (integer) 1", "2) (integer) 2"]
        );
        assert_eq!(render_value(redis::Value::Array(vec![]), 0), vec!["(empty array)"]);
    }
}
