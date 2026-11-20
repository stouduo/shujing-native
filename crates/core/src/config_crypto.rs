//! 连接配置加密导出/导入:口令派生密钥(AES-256-GCM + PBKDF2-HMAC-SHA256)。
//! 格式:SJENC1:<base64(salt[16] | nonce[12] | ciphertext)>——无口令不可解。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::Aes256Gcm;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const ITERATIONS: u32 = 150_000;
const MAGIC: &str = "SJENC1:";

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(passphrase.as_bytes(), salt, ITERATIONS, &mut key);
    key
}

pub fn encrypt(plain: &str, passphrase: &str) -> Result<String, String> {
    if passphrase.chars().count() < 8 {
        return Err("口令至少 8 个字符".into());
    }
    let mut salt = [0u8; SALT_LEN];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
    let key = derive_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("加密初始化失败: {e}"))?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let ct = cipher
        .encrypt(&nonce.into(), plain.as_bytes())
        .map_err(|_| "加密失败".to_string())?;
    let mut packed = Vec::with_capacity(SALT_LEN + NONCE_LEN + ct.len());
    packed.extend_from_slice(&salt);
    packed.extend_from_slice(&nonce);
    packed.extend_from_slice(&ct);
    use base64::Engine;
    Ok(format!(
        "{}{}",
        MAGIC,
        base64::engine::general_purpose::STANDARD.encode(packed)
    ))
}

pub fn decrypt(payload: &str, passphrase: &str) -> Result<String, String> {
    let b64 = payload
        .trim()
        .strip_prefix(MAGIC)
        .ok_or("不是数镜加密备份格式")?;
    use base64::Engine;
    let packed = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| "备份内容不是合法 base64".to_string())?;
    if packed.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err("备份内容不完整".into());
    }
    let (salt, rest) = packed.split_at(SALT_LEN);
    let (nonce, ct) = rest.split_at(NONCE_LEN);
    let key = derive_key(passphrase, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("解密初始化失败: {e}"))?;
    let plain = cipher
        .decrypt(nonce.into(), ct)
        .map_err(|_| "解密失败:口令错误或文件已损坏".to_string())?;
    String::from_utf8(plain).map_err(|_| "解密结果不是文本".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_passphrase() {
        let secret = r#"[{"id":"c1","name":"x","dbType":"mysql","readOnly":false}]"#;
        let enc = encrypt(secret, "pass-1234").unwrap();
        assert!(enc.starts_with(MAGIC));
        let dec = decrypt(&enc, "pass-1234").unwrap();
        assert_eq!(dec, secret);
        // 错误口令
        assert!(decrypt(&enc, "wrong-pass").is_err());
        // 口令过短
        assert!(encrypt(secret, "short").is_err());
    }

    #[test]
    fn salt_randomized() {
        let a = encrypt("x", "pass-1234").unwrap();
        let b = encrypt("x", "pass-1234").unwrap();
        assert_ne!(a, b, "同口令两次加密产出必须不同(随机 salt/nonce)");
    }
}
