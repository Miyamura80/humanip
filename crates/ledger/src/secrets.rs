//! Id and secret generation, secret hashing, constant-time comparison.

use humanip_core::accounts::StoreError;
use sha2::{Digest, Sha256};

/// Every issued secret starts with this, so leaked keys are easy to scan for.
pub const SECRET_PREFIX: &str = "hip_live_";
const SECRET_CHARS: usize = 32;
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// SHA-256 of a presented secret, as stored in `keys.secret_sha256`.
///
/// No salt and no slow KDF: secrets are 32 random base62 characters (about
/// 190 bits), so there is no dictionary or rainbow table to defend against,
/// and a plain hash lets `authenticate` look the key up by index.
pub fn hash_secret(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn fill(buf: &mut [u8]) -> Result<(), StoreError> {
    getrandom::fill(buf).map_err(|e| StoreError::Storage(format!("rng: {e}")))
}

fn random_hex_id(prefix: &str) -> Result<String, StoreError> {
    let mut b = [0u8; 6];
    fill(&mut b)?;
    let mut s = String::with_capacity(prefix.len() + 12);
    s.push_str(prefix);
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    Ok(s)
}

/// `acct_` plus 12 hex characters.
pub fn new_account_id() -> Result<String, StoreError> {
    random_hex_id("acct_")
}

/// `key_` plus 12 hex characters. Public; safe to log.
pub fn new_key_id() -> Result<String, StoreError> {
    random_hex_id("key_")
}

/// `hip_live_` plus 32 uniformly random base62 characters.
pub fn new_secret() -> Result<String, StoreError> {
    let mut out = String::with_capacity(SECRET_PREFIX.len() + SECRET_CHARS);
    out.push_str(SECRET_PREFIX);
    let mut n = 0;
    let mut buf = [0u8; 64];
    while n < SECRET_CHARS {
        fill(&mut buf)?;
        // Rejection sampling: 248 = 4 * 62, so `b % 62` stays uniform.
        for &b in buf.iter().filter(|&&b| b < 248) {
            if n == SECRET_CHARS {
                break;
            }
            out.push(BASE62[(b % 62) as usize] as char);
            n += 1;
        }
    }
    Ok(out)
}

/// Constant-time equality for equal-length digests.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_secrets_have_the_documented_shape() {
        let a = new_account_id().unwrap();
        assert!(a.starts_with("acct_") && a.len() == 17);
        let k = new_key_id().unwrap();
        assert!(k.starts_with("key_") && k.len() == 16);
        assert!(k[4..].chars().all(|c| c.is_ascii_hexdigit()));
        let s = new_secret().unwrap();
        assert_eq!(s.len(), SECRET_PREFIX.len() + 32);
        assert!(s[SECRET_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(s, new_secret().unwrap());
    }

    #[test]
    fn ct_eq_matches_plain_eq() {
        assert!(ct_eq(&hash_secret("x"), &hash_secret("x")));
        assert!(!ct_eq(&hash_secret("x"), &hash_secret("y")));
        assert!(!ct_eq(b"ab", b"abc"));
    }
}
