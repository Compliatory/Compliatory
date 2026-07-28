use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::DomainError;

pub const SHA256_PREFIX: &str = "sha256:";

pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, DomainError> {
    serde_jcs::to_vec(value)
        .map_err(|error| DomainError::invalid(format!("canonical serialization failed: {error}")))
}

pub fn content_digest<T: Serialize>(domain: &str, value: &T) -> Result<String, DomainError> {
    let bytes = canonical_bytes(value)?;
    let mut hasher = Sha256::new();
    hasher.update(b"compliatory\0");
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    hasher.update(bytes);
    Ok(format!("{SHA256_PREFIX}{:x}", hasher.finalize()))
}

pub fn derived_id(prefix: &str, digest: &str) -> Result<String, DomainError> {
    let hex = digest
        .strip_prefix(SHA256_PREFIX)
        .ok_or_else(|| DomainError::invalid("digest must use sha256: prefix"))?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DomainError::invalid(
            "digest must contain 64 hexadecimal digits",
        ));
    }
    Ok(format!("{prefix}{hex}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_digest_ignores_object_key_order() {
        let first = json!({"b": 2, "a": 1});
        let second = json!({"a": 1, "b": 2});
        assert_eq!(
            content_digest("test", &first).unwrap(),
            content_digest("test", &second).unwrap()
        );
    }

    #[test]
    fn domains_separate_equal_content() {
        let value = json!({"a": 1});
        assert_ne!(
            content_digest("fragment", &value).unwrap(),
            content_digest("packet", &value).unwrap()
        );
    }
}
