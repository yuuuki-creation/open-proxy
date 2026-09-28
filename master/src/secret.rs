//! 随机凭据、Token 和哈希。只需要比对的秘密（会话 Token、Agent Token）存 SHA-256；
//! 要再次显示或下发的存明文（database.md「约定」）。

use anyhow::anyhow;
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// `n` 字节的随机数，base64url 编码（不带填充）。
pub fn random_token(n: usize) -> String {
    let mut buf = vec![0u8; n];
    OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// 会话、Agent 用的 Token：256 位随机数。
pub fn new_token() -> String {
    random_token(32)
}

/// 字符串的 SHA-256，小写十六进制。
pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// 字母和数字组成的随机密码（Hysteria2、AnyTLS、Mieru、SOCKS5 用）。
pub fn random_password(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut buf = vec![0u8; len];
    OsRng.fill_bytes(&mut buf);
    buf.iter()
        .map(|b| CHARS[*b as usize % CHARS.len()] as char)
        .collect()
}

/// Shadowsocks 2022（2022-blake3-aes-128-gcm）的密钥：16 字节随机数，标准 base64。
pub fn ss2022_key() -> String {
    let mut buf = [0u8; 16];
    OsRng.fill_bytes(&mut buf);
    STANDARD.encode(buf)
}

/// VLESS 用的 UUID。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// REALITY 的 X25519 密钥对，base64 raw url 编码：(私钥, 公钥)。
pub fn reality_keypair() -> (String, String) {
    let secret = x25519_dalek::StaticSecret::random_from_rng(OsRng);
    let public = x25519_dalek::PublicKey::from(&secret);
    (
        URL_SAFE_NO_PAD.encode(secret.to_bytes()),
        URL_SAFE_NO_PAD.encode(public.as_bytes()),
    )
}

/// REALITY 的 short ID：8 字节随机数的十六进制。
pub fn reality_short_id() -> String {
    let mut buf = [0u8; 8];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// 管理员密码的 Argon2id 哈希。
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("计算密码哈希: {e}"))
}

/// 校验管理员密码；哈希格式不对时当作不匹配。
pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}
