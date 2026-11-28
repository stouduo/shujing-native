//! JWT(JSON Web Token)解码与验签。
//!
//! 解码 ≠ 可信:decode 只是把三段拆开看,verify 才是用密钥/公钥核对签名。
//! 验签永远按 header 里声明的 alg 分派,不接受调用方另行指定算法。
//!
//! 验签支持:HS256/384/512、RS256/384/512、PS256/384/512、ES256/384、EdDSA(Ed25519)。
//! 其余算法只解码,verify 返回 [`VerifyResult::Unsupported`]。

use base64::Engine as _;
use digest::{KeyInit, Mac};
use hmac::Hmac;
use ring::signature::UnparsedPublicKey;
use serde_json::Value;
use sha2::{Sha256, Sha384, Sha512};

/// 解码结果(未验签)
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedJwt {
    pub header: Value,
    pub payload: Value,
    /// payload 段本身是不是 JSON;不是时 `payload` 是原文字符串(`Value::String`)
    pub payload_is_json: bool,
    /// header 声明的算法,原样
    pub alg: String,
    /// 签名原始字节(alg=none 或两段式令牌时为空)
    pub signature: Vec<u8>,
    /// 签名段的原文(base64url,已去空白与填充;为空表示没有签名段)
    pub signature_b64: String,
    /// 待签名原文 = 原始 header 段 + "." + 原始 payload 段。验签必须喂它:
    /// 喂重新编码后的 JSON(键序/空白变了)签名就对不上
    pub signing_input: String,
}

/// 去掉尾部填充后按 base64url 解码(输入须已无空白)
fn b64url_decode(s: &str) -> Result<Vec<u8>, String> {
    let trimmed = s.trim_end_matches('=');
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed.as_bytes())
        .map_err(|e| format!("Base64URL 解码失败: {e}"))
}

/// 拆开一个 JWT(容忍粘贴带入的空白与尾部填充)。不验签
pub fn decode(token: &str) -> Result<DecodedJwt, String> {
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    let parts: Vec<&str> = compact.split('.').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(format!(
            "不是 JWT:应以 . 分成 2~3 段,实际 {} 段",
            parts.len()
        ));
    }
    let (h_b64, p_b64) = (parts[0], parts[1]);
    let sig_b64 = parts.get(2).copied().unwrap_or("");
    if h_b64.is_empty() || p_b64.is_empty() {
        return Err("不是 JWT:header 段或 payload 段为空".into());
    }
    let header: Value = serde_json::from_slice(&b64url_decode(h_b64)?)
        .map_err(|e| format!("header 不是合法 JSON: {e}"))?;
    if !header.is_object() {
        return Err("header 不是 JSON 对象".into());
    }
    let alg = header
        .get("alg")
        .and_then(Value::as_str)
        .ok_or("header 缺少 alg 字段(或不是字符串)")?
        .to_string();
    let payload_bytes = b64url_decode(p_b64)?;
    // payload 不要求是 JSON(JWS 允许任意字节):解不出就按文本展示
    let (payload, payload_is_json) = match serde_json::from_slice::<Value>(&payload_bytes) {
        Ok(v) => (v, true),
        Err(_) => (
            match String::from_utf8(payload_bytes) {
                Ok(s) => Value::String(s),
                Err(e) => Value::String(format!(
                    "(非 UTF-8 的二进制载荷,{} 字节)",
                    e.into_bytes().len()
                )),
            },
            false,
        ),
    };
    let signature = b64url_decode(sig_b64)?;
    Ok(DecodedJwt {
        header,
        payload,
        payload_is_json,
        alg,
        signature,
        signature_b64: sig_b64.trim_end_matches('=').to_string(),
        signing_input: format!("{h_b64}.{p_b64}"),
    })
}

// ---------------------------------------------------------------- 时间声明

/// payload 里的一个时间声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeClaim {
    /// iat / nbf / exp
    pub name: &'static str,
    /// 秒级时间戳(原值,小数向下取整)
    pub ts: i64,
    /// 本地时区的可读时间
    pub local: String,
}

/// payload 里的 iat / nbf / exp(数值型才认,字符串型的时间不猜),按这个顺序
pub fn time_claims(payload: &Value) -> Vec<TimeClaim> {
    use chrono::TimeZone as _;
    ["iat", "nbf", "exp"]
        .into_iter()
        .filter_map(|name| {
            // NumericDate 允许小数
            let ts = numeric_date(payload, name)?;
            let local = chrono::Local
                .timestamp_opt(ts, 0)
                .single()
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "超出可表示范围".into());
            Some(TimeClaim { name, ts, local })
        })
        .collect()
}

fn numeric_date(payload: &Value, key: &str) -> Option<i64> {
    payload
        .get(key)
        .and_then(Value::as_f64)
        .map(|v| v.floor() as i64)
}

/// 相对某一时刻的有效期状态,附带毫秒数(尚未生效多久 / 已过期多久 / 还剩多久)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validity {
    /// payload 没有 exp
    NoExpiry,
    /// nbf 在未来:尚未生效
    NotYetValid(i64),
    /// exp 已过
    Expired(i64),
    /// 在有效期内
    Valid(i64),
}

/// 只看 exp / nbf 两个声明,不代表令牌可信——签名没验过,内容随时可伪造
pub fn validity(payload: &Value, now_ms: i64) -> Validity {
    let ms = |key: &str| numeric_date(payload, key).map(|ts| ts.saturating_mul(1000));
    if let Some(nbf) = ms("nbf") {
        if now_ms < nbf {
            return Validity::NotYetValid(nbf - now_ms);
        }
    }
    match ms("exp") {
        Some(exp) if now_ms >= exp => Validity::Expired(now_ms - exp),
        Some(exp) => Validity::Valid(exp - now_ms),
        None => Validity::NoExpiry,
    }
}

// ---------------------------------------------------------------- 验签

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Valid,
    Invalid,
    /// 没法验(算法不支持、alg=none 未签名),附带原因
    Unsupported(String),
}

const EMPTY_KEY: &str = "请填写验签密钥:HS 系列填共享密钥,其余填 PEM 公钥";

/// 用密钥(HS 系列)或 PEM 公钥(其余)核对签名。按 header 声明的 alg 分派。
///
/// - `Ok(Valid/Invalid)`:公钥格式与算法对得上,签名核对的结果
/// - `Err`:密钥没法用于这个算法(格式不对、位数不足、类型不匹配)——
///   不报成"签名无效",验签工具不能把"没法验"说成"验不过"
pub fn verify(jwt: &DecodedJwt, key: &str) -> Result<VerifyResult, String> {
    let alg = jwt.alg.as_str();
    if alg == "none" {
        return Ok(VerifyResult::Unsupported(
            if jwt.signature.is_empty() {
                "alg=none:令牌没有签名,任何人都能伪造其内容".into()
            } else {
                "alg=none 却带着签名段,形态可疑".into()
            },
        ));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err(EMPTY_KEY.into());
    }
    let msg = jwt.signing_input.as_bytes();
    match alg {
        "HS256" | "HS384" | "HS512" => {
            // Mac::verify 是恒定时间比较
            Ok(if hmac_matches(alg, key.as_bytes(), msg, &jwt.signature) {
                VerifyResult::Valid
            } else {
                VerifyResult::Invalid
            })
        }
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" | "ES256" | "ES384" | "EdDSA" => {
            let (der, label) = pem_to_der(key)?;
            let key_bytes: &[u8] = if label.contains("RSA PUBLIC KEY") {
                // PKCS#1 RSAPublicKey:已经是 ring 要的格式
                if !alg.starts_with("RS") && !alg.starts_with("PS") {
                    return Err(format!("公钥是 RSA 公钥,与算法 {alg} 不匹配"));
                }
                &der
            } else {
                spki_bit_string(&der)?
            };
            check_key_shape(alg, key_bytes)?;
            let params: &dyn ring::signature::VerificationAlgorithm = match alg {
                "RS256" => &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                "RS384" => &ring::signature::RSA_PKCS1_2048_8192_SHA384,
                "RS512" => &ring::signature::RSA_PKCS1_2048_8192_SHA512,
                "PS256" => &ring::signature::RSA_PSS_2048_8192_SHA256,
                "PS384" => &ring::signature::RSA_PSS_2048_8192_SHA384,
                "PS512" => &ring::signature::RSA_PSS_2048_8192_SHA512,
                "ES256" => &ring::signature::ECDSA_P256_SHA256_FIXED,
                "ES384" => &ring::signature::ECDSA_P384_SHA384_FIXED,
                _ => &ring::signature::ED25519,
            };
            let ok = UnparsedPublicKey::new(params, key_bytes)
                .verify(msg, &jwt.signature)
                .is_ok();
            Ok(if ok {
                VerifyResult::Valid
            } else {
                VerifyResult::Invalid
            })
        }
        _ => Ok(VerifyResult::Unsupported(format!(
            "{alg} 暂不支持验签,仅解码"
        ))),
    }
}

/// HS 系列的签名核对(Mac::verify 内部恒定时间比较)
fn hmac_matches(alg: &str, key: &[u8], data: &[u8], signature: &[u8]) -> bool {
    // RFC 2104 对密钥长度没有限制,new_from_slice 永不返回 Err
    macro_rules! check {
        ($d:ty) => {{
            let mut mac = <Hmac<$d> as KeyInit>::new_from_slice(key)
                .expect("HMAC 密钥长度不受限");
            Mac::update(&mut mac, data);
            // 长度不符直接判否(长度不是秘密);长度对了才交给库做恒定时间比较
            let want = <Hmac<$d> as digest::OutputSizeUser>::output_size();
            if signature.len() != want {
                false
            } else {
                Mac::verify(mac, digest::generic_array::GenericArray::from_slice(signature)).is_ok()
            }
        }};
    }
    match alg {
        "HS256" => check!(Sha256),
        "HS384" => check!(Sha384),
        _ => check!(Sha512),
    }
}

/// PEM → (DER, 围栏标签如 "PUBLIC KEY"/"RSA PUBLIC KEY")。
/// 没有围栏的裸 base64 也收(标签为空串)
fn pem_to_der(pem: &str) -> Result<(Vec<u8>, String), String> {
    let mut label = String::new();
    let mut body = String::new();
    for line in pem.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with("-----") {
            if label.is_empty() {
                label = l
                    .trim_matches('-')
                    .trim_start_matches("BEGIN ")
                    .trim_start_matches("END ")
                    .trim()
                    .to_string();
            }
            continue;
        }
        body.push_str(l);
    }
    if body.is_empty() {
        return Err("公钥为空或不是 PEM 格式".into());
    }
    let der = base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|_| "公钥内容不是合法 Base64".to_string())?;
    Ok((der, label))
}

/// 极简 DER 读取:取走一个 tag-length-value,返回 (tag, 内容, 剩余)
fn der_take(input: &[u8]) -> Result<(u8, &[u8], &[u8]), String> {
    let (&tag, rest) = input.split_first().ok_or("公钥 DER 为空")?;
    let (&first, mut rest) = rest.split_first().ok_or("公钥 DER 被截断")?;
    let len = if first & 0x80 == 0 {
        first as usize
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return Err("公钥 DER 长度字段非法".into());
        }
        let mut len = 0usize;
        for &b in &rest[..n] {
            len = (len << 8) | b as usize;
        }
        rest = &rest[n..];
        len
    };
    if rest.len() < len {
        return Err("公钥 DER 被截断".into());
    }
    Ok((tag, &rest[..len], &rest[len..]))
}

/// 从 SubjectPublicKeyInfo 里取出 BIT STRING 的内容(去掉首字节"未用位数")。
/// RSA 得到 PKCS#1 RSAPublicKey、EC 得到未压缩点、Ed25519 得到 32 字节原始公钥——
/// 正好是 ring 三类验签各自要的格式
fn spki_bit_string(spki: &[u8]) -> Result<&[u8], String> {
    let err = |m: &str| m.to_string();
    let (tag, seq, _) = der_take(spki)?;
    if tag != 0x30 {
        return Err("公钥不是 X.509 SubjectPublicKeyInfo(应为 -----BEGIN PUBLIC KEY----- 的 PEM)".into());
    }
    // SEQUENCE { AlgorithmIdentifier(SEQUENCE), subjectPublicKey BIT STRING }
    let (alg_tag, _, rest) = der_take(seq)?;
    if alg_tag != 0x30 {
        return Err(err("公钥结构异常:缺少 AlgorithmIdentifier"));
    }
    let (tag, bits, _) = der_take(rest)?;
    if tag != 0x03 || bits.is_empty() {
        return Err(err("公钥结构异常:缺少 BIT STRING"));
    }
    Ok(&bits[1..])
}

/// 公钥形态与算法是否对得上、签名库是否支持
fn check_key_shape(alg: &str, key: &[u8]) -> Result<(), String> {
    match alg {
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" => {
            let bits = rsa_modulus_bits(key)
                .ok_or_else(|| "公钥不是 RSA 公钥,与算法不匹配".to_string())?;
            // ring 拒绝 2048 位以下的 RSA:报成"不支持"而不是"签名无效"
            if bits < 2048 {
                return Err(format!(
                    "RSA 密钥只有 {bits} 位,低于 2048 位:签名库拒绝验签"
                ));
            }
            Ok(())
        }
        // 未压缩点:0x04 || X || Y
        "ES256" if key.len() == 65 && key[0] == 0x04 => Ok(()),
        "ES384" if key.len() == 97 && key[0] == 0x04 => Ok(()),
        "EdDSA" if key.len() == 32 => Ok(()),
        _ => Err(format!("公钥与算法 {alg} 不匹配")),
    }
}

/// PKCS#1 RSAPublicKey ::= SEQUENCE { modulus INTEGER, exponent INTEGER } 的模数位数
fn rsa_modulus_bits(pkcs1: &[u8]) -> Option<usize> {
    let (tag, seq, _) = der_take(pkcs1).ok()?;
    if tag != 0x30 {
        return None;
    }
    let (tag, mut n, _) = der_take(seq).ok()?;
    if tag != 0x02 {
        return None;
    }
    while n.first() == Some(&0) {
        n = &n[1..];
    }
    let leading = n.first().map_or(0, |b| b.leading_zeros()) as usize;
    Some(n.len() * 8 - leading)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 基准向量(见 tests/data/jwt_vectors.json 的说明):python hmac + 系统 openssl 签出,
    /// 与本实现无共同代码
    fn vectors() -> Value {
        serde_json::from_str(include_str!("../tests/data/jwt_vectors.json")).unwrap()
    }

    /// 还原完整 token:payload 段共用
    fn token(v: &Value, alg: &str) -> String {
        let (h, s) = v["tokens"][alg].as_str().unwrap().split_once('.').unwrap();
        format!("{}.{}.{}", h, v["payload_b64"].as_str().unwrap(), s)
    }

    /// 该算法对应的正确密钥/公钥
    fn right_key(v: &Value, alg: &str) -> String {
        match alg {
            "HS256" | "HS384" | "HS512" => v["secret"].as_str().unwrap().to_string(),
            _ => v["pub"][alg].as_str().unwrap().to_string(),
        }
    }

    /// 改掉签名段的第一个字符(整 6 bit,必然改变首个签名字节)
    fn tampered(token: &str) -> String {
        let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
        let sig = &mut parts[2];
        let first = sig.chars().next().unwrap();
        sig.replace_range(..1, if first == 'A' { "B" } else { "A" });
        parts.join(".")
    }

    /// 改掉 payload 段的第一个字符(内容变了,签名自然对不上)
    fn tampered_payload(token: &str) -> String {
        let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
        let p = &mut parts[1];
        let first = p.chars().next().unwrap();
        p.replace_range(..1, if first == 'A' { "B" } else { "A" });
        parts.join(".")
    }

    fn b64url(s: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
    }

    #[test]
    fn decode_reads_three_parts() {
        let v = vectors();
        let jwt = decode(&token(&v, "HS256")).unwrap();
        assert_eq!(jwt.alg, "HS256");
        assert_eq!(jwt.header["typ"], "JWT");
        assert_eq!(jwt.payload["sub"], "1234567890");
        assert_eq!(jwt.payload["name"], "数镜 测试");
        assert_eq!(jwt.payload["exp"], 1790003600);
        assert_eq!(jwt.payload["admin"], true);
        // 签名原始字节 = python hmac 的结果
        assert_eq!(
            jwt.signature
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "50eca82ebf3a9837c656715ec943cca05c9f8b4ba92bea8637af56cc76c082e9"
        );
        // 待签名原文就是原始两段;签名段原文原样保留(页面直接展示)
        let (h, s) = v["tokens"]["HS256"].as_str().unwrap().split_once('.').unwrap();
        assert_eq!(jwt.signing_input, format!("{h}.{}", v["payload_b64"].as_str().unwrap()));
        assert_eq!(jwt.signature_b64, s);
        // 带填充的输入:原文里的填充也去掉
        let padded = format!("{h}.{}.{}==", v["payload_b64"].as_str().unwrap(), s);
        assert_eq!(decode(&padded).unwrap().signature_b64, s);
    }

    #[test]
    fn decode_tolerates_whitespace_and_padding() {
        let v = vectors();
        let clean = decode(&token(&v, "HS256")).unwrap();
        let t = token(&v, "HS256");
        // 粘贴带入的换行/空格、尾部填充都不能让解码失败
        let wrapped = format!("  {}\n {}\t{}  == ", &t[..20], &t[20..60], &t[60..]);
        assert_eq!(decode(&wrapped).unwrap(), clean);
        assert_eq!(decode(&format!("\n{t}\n")).unwrap(), clean);
    }

    #[test]
    fn decode_rejects_malformed() {
        let v = vectors();
        let t = token(&v, "HS256");
        for bad in ["", "abc", "a.b.c.d", "只有一段", ".x.y", "x..y"] {
            assert!(decode(bad).is_err(), "{bad:?} 不该被当成 JWT");
        }
        // header 不是 JSON / 不是对象 / 缺 alg
        let (h, s) = v["tokens"]["HS256"].as_str().unwrap().split_once('.').unwrap();
        let p = v["payload_b64"].as_str().unwrap();
        let with_header = |header: &str| format!("{}.{}.{}", b64url(header), p, s);
        assert!(decode(&with_header("not json")).is_err());
        assert!(decode(&with_header("[1,2]")).is_err());
        assert!(decode(&with_header(r#"{"typ":"JWT"}"#)).is_err());
        assert!(decode(&with_header(r#"{"alg":5}"#)).is_err());
        // 签名段不是合法 base64url
        assert!(decode(&format!("{h}.{p}.***")).is_err());
    }

    #[test]
    fn unsigned_token_has_no_signature() {
        let v = vectors();
        let p = v["payload_b64"].as_str().unwrap();
        let h = b64url(r#"{"alg":"none"}"#);
        let jwt = decode(&format!("{h}.{p}")).unwrap();
        assert_eq!(jwt.alg, "none");
        assert!(jwt.signature.is_empty());
        assert_eq!(decode(&format!("{h}.{p}.")).unwrap(), jwt);
    }

    #[test]
    fn non_json_payload_is_shown_as_text() {
        let h = b64url(r#"{"alg":"none"}"#);
        let jwt = decode(&format!("{h}.{}", b64url("hello"))).unwrap();
        assert_eq!(jwt.payload, Value::String("hello".into()));
        assert!(!jwt.payload_is_json);
        let jwt = decode(&format!("{h}.{}", b64url("{not json"))).unwrap();
        assert_eq!(jwt.payload, Value::String("{not json".into()));
        assert!(!jwt.payload_is_json);
        // 本身就是 JSON 字符串的 payload:带引号的那种,与上面的"非 JSON 文本"要能分开
        let jwt = decode(&format!("{h}.{}", b64url(r#""hello""#))).unwrap();
        assert_eq!(jwt.payload, Value::String("hello".into()));
        assert!(jwt.payload_is_json);
        let jwt = decode(&format!("{h}.{}", b64url("{not json"))).unwrap();
        assert_eq!(jwt.payload, Value::String("{not json".into()));
        // 非 UTF-8 的二进制载荷
        let bin = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xffu8, 0xfe, 0x80]);
        let jwt = decode(&format!("{h}.{bin}")).unwrap();
        assert_eq!(
            jwt.payload,
            Value::String("(非 UTF-8 的二进制载荷,3 字节)".into())
        );
    }

    #[test]
    fn time_claims_follow_numeric_date() {
        let v = vectors();
        let jwt = decode(&token(&v, "HS256")).unwrap();
        let claims = time_claims(&jwt.payload);
        let by: Vec<(&str, i64)> = claims.iter().map(|c| (c.name, c.ts)).collect();
        assert_eq!(by, [("iat", 1790000000), ("exp", 1790003600)]);
        // 本地时间串能原路解析回同一个时刻(不依赖机器时区)
        for c in &claims {
            let naive = chrono::NaiveDateTime::parse_from_str(&c.local, "%Y-%m-%d %H:%M:%S").unwrap();
            let back = naive.and_local_timezone(chrono::Local).single().unwrap().timestamp();
            assert_eq!(back, c.ts, "{} 的本地时间换算有误", c.name);
        }
        // 小数 NumericDate 向下取整;字符串型的时间不猜
        let p: Value = serde_json::from_str(r#"{"nbf":1.5e9,"exp":1790003600.9,"iat":"昨天"}"#).unwrap();
        let by: Vec<(&str, i64)> = time_claims(&p).iter().map(|c| (c.name, c.ts)).collect();
        assert_eq!(by, [("nbf", 1_500_000_000), ("exp", 1790003600)]);
        assert!(time_claims(&Value::Null).is_empty());
    }

    #[test]
    fn validity_boundaries() {
        const EXP_MS: i64 = 1790003600 * 1000;
        let exp: Value = serde_json::from_str(r#"{"exp":1790003600}"#).unwrap();
        assert_eq!(validity(&exp, EXP_MS - 1), Validity::Valid(1));
        assert_eq!(validity(&exp, EXP_MS), Validity::Expired(0));
        assert_eq!(validity(&exp, EXP_MS + 60_000), Validity::Expired(60_000));
        assert_eq!(validity(&Value::Null, 0), Validity::NoExpiry);
        // nbf 未到优先于 exp;到了就不拦
        let both: Value = serde_json::from_str(r#"{"nbf":2000000000,"exp":1790003600}"#).unwrap();
        assert_eq!(validity(&both, 0), Validity::NotYetValid(2000000000 * 1000));
        assert_eq!(validity(&both, 2000000000 * 1000), Validity::Expired(2000000000 * 1000 - EXP_MS));
        // 字符串型的 exp 不算数
        let s: Value = serde_json::from_str(r#"{"exp":"1"}"#).unwrap();
        assert_eq!(validity(&s, i64::MAX), Validity::NoExpiry);
    }

    #[test]
    fn verify_accepts_all_reference_algorithms() {
        let v = vectors();
        for alg in ["HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "ES256", "ES384", "EdDSA"] {
            let jwt = decode(&token(&v, alg)).unwrap();
            assert_eq!(jwt.alg, alg);
            assert_eq!(
                verify(&jwt, &right_key(&v, alg)).unwrap(),
                VerifyResult::Valid,
                "{alg} 的 openssl/python 基准签名应验签通过"
            );
        }
    }

    #[test]
    fn verify_rejects_wrong_key_and_tampered_token() {
        let v = vectors();
        for alg in ["HS256", "HS384", "HS512", "RS256", "RS384", "RS512", "PS256", "ES256", "ES384", "EdDSA"] {
            let t = token(&v, alg);
            assert_eq!(
                verify(&decode(&tampered(&t)).unwrap(), &right_key(&v, alg)).unwrap(),
                VerifyResult::Invalid,
                "{alg}:签名被改应判无效"
            );
            assert_eq!(
                verify(&decode(&tampered_payload(&t)).unwrap(), &right_key(&v, alg)).unwrap(),
                VerifyResult::Invalid,
                "{alg}:内容被改应判无效"
            );
        }
        // HS 换密钥
        let jwt = decode(&token(&v, "HS256")).unwrap();
        assert_eq!(verify(&jwt, &(v["secret"].as_str().unwrap().to_string() + "x")).unwrap(), VerifyResult::Invalid);
        // RSA 换成另一把(位数合规的)公钥:是"无效",不是报错
        let jwt = decode(&token(&v, "RS256")).unwrap();
        assert_eq!(verify(&jwt, v["other_rsa_pub"].as_str().unwrap()).unwrap(), VerifyResult::Invalid);
        // 空密钥是输入错误
        assert_eq!(verify(&jwt, "  ").unwrap_err(), EMPTY_KEY);
    }

    #[test]
    fn verify_reports_unsupported_honestly() {
        let v = vectors();
        let p = v["payload_b64"].as_str().unwrap();
        // alg=none:两段式 / 带空签名段 / 带着签名段
        for t in [
            format!("{}.{}", b64url(r#"{"alg":"none"}"#), p),
            format!("{}.{}.", b64url(r#"{"alg":"none"}"#), p),
        ] {
            let jwt = decode(&t).unwrap();
            assert!(matches!(verify(&jwt, "").unwrap(), VerifyResult::Unsupported(_)));
        }
        let t = format!("{}.{}.{}", b64url(r#"{"alg":"none"}"#), p, b64url("x"));
        let r = verify(&decode(&t).unwrap(), "").unwrap();
        assert!(matches!(&r, VerifyResult::Unsupported(m) if m.contains("可疑")), "{r:?}");
        // 未知算法 / 暂不支持的算法
        let t = format!("{}.{}.{}", b64url(r#"{"alg":"ES256K"}"#), p, b64url("x"));
        let r = verify(&decode(&t).unwrap(), v["pub"]["ES256"].as_str().unwrap()).unwrap();
        assert!(matches!(&r, VerifyResult::Unsupported(m) if m.contains("ES256K")), "{r:?}");
        // 1024 位 RSA:签名本身合法,但签名库拒绝——必须报"不支持",不能报"无效"
        let jwt = decode(&token(&v, "RSA1024")).unwrap();
        let err = verify(&jwt, v["pub"]["RSA1024"].as_str().unwrap()).unwrap_err();
        assert!(err.contains("1024"), "{err}");
    }

    #[test]
    fn verify_accepts_key_format_variants() {
        let v = vectors();
        let jwt = decode(&token(&v, "RS256")).unwrap();
        let pem = v["pub"]["RS256"].as_str().unwrap();
        assert_eq!(verify(&jwt, pem).unwrap(), VerifyResult::Valid);
        // CRLF 换行、首尾空白
        assert_eq!(verify(&jwt, &format!("\n{}\n", pem.replace('\n', "\r\n"))).unwrap(), VerifyResult::Valid);
        // 裸 base64 的 DER(没有 PEM 围栏)
        assert_eq!(verify(&jwt, v["rsa_spki_b64"].as_str().unwrap()).unwrap(), VerifyResult::Valid);
        // PKCS#1 格式的 RSA 公钥
        assert_eq!(verify(&jwt, v["rsa_pkcs1_pem"].as_str().unwrap()).unwrap(), VerifyResult::Valid);
    }

    #[test]
    fn verify_key_algorithm_mismatch_is_an_error() {
        let v = vectors();
        // 公钥类型与算法不符:是输入错误,不是"签名无效"
        let cases = [
            ("RS256", v["pub"]["ES256"].as_str().unwrap(), "不是 RSA"),
            ("ES256", v["pub"]["RS256"].as_str().unwrap(), "不匹配"),
            ("EdDSA", v["pub"]["RS256"].as_str().unwrap(), "不匹配"),
            ("ES256", v["rsa_pkcs1_pem"].as_str().unwrap(), "RSA 公钥"),
        ];
        for (alg, key, needle) in cases {
            let jwt = decode(&token(&v, alg)).unwrap();
            let err = verify(&jwt, key).unwrap_err();
            assert!(err.contains(needle), "{alg}: {err}");
        }
        // 不是 base64 / 不是 DER 结构
        for bad in ["hello world", "AAAA", "-----BEGIN PUBLIC KEY-----\n???\n-----END PUBLIC KEY-----"] {
            assert!(verify(&jwt_of(&v, "RS256"), bad).is_err(), "{bad:?}");
        }
        assert!(verify(&jwt_of(&v, "ES256"), &b64url("not a key")).is_err());
    }

    fn jwt_of(v: &Value, alg: &str) -> DecodedJwt {
        decode(&token(v, alg)).unwrap()
    }
}
