use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{DomainError, ErrorCode, canonical_bytes};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CursorPayload {
    pub tenant_id: String,
    pub operation: String,
    pub input_digest: String,
    pub corpus_versions: Vec<String>,
    pub next_index: u32,
}

#[derive(Clone)]
pub struct CursorCodec {
    key: Vec<u8>,
}

impl CursorCodec {
    pub fn new(key: impl AsRef<[u8]>) -> Result<Self, DomainError> {
        let key = key.as_ref();
        if key.len() < 32 {
            return Err(DomainError::invalid(
                "cursor signing key must contain at least 32 bytes",
            ));
        }
        Ok(Self { key: key.to_vec() })
    }

    pub fn encode(&self, payload: &CursorPayload) -> Result<String, DomainError> {
        let body = canonical_bytes(payload)?;
        let mut mac = HmacSha256::new_from_slice(&self.key)
            .map_err(|_| DomainError::invalid("invalid cursor key"))?;
        mac.update(&body);
        let signature = mac.finalize().into_bytes();
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(body),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    pub fn decode(&self, encoded: &str) -> Result<CursorPayload, DomainError> {
        let (body, signature) = encoded
            .split_once('.')
            .ok_or_else(|| DomainError::new(ErrorCode::InvalidCursor, "malformed cursor"))?;
        let body = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| DomainError::new(ErrorCode::InvalidCursor, "malformed cursor body"))?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| {
            DomainError::new(ErrorCode::InvalidCursor, "malformed cursor signature")
        })?;
        let mut mac = HmacSha256::new_from_slice(&self.key)
            .map_err(|_| DomainError::new(ErrorCode::InvalidCursor, "invalid cursor key"))?;
        mac.update(&body);
        mac.verify_slice(&signature)
            .map_err(|_| DomainError::new(ErrorCode::InvalidCursor, "invalid cursor signature"))?;
        serde_json::from_slice(&body)
            .map_err(|_| DomainError::new(ErrorCode::InvalidCursor, "invalid cursor payload"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codec() -> CursorCodec {
        CursorCodec::new([7_u8; 32]).unwrap()
    }

    #[test]
    fn cursor_round_trips() {
        let payload = CursorPayload {
            tenant_id: "tenant-a".to_owned(),
            operation: "build_work_packet".to_owned(),
            input_digest: "sha256:00".to_owned(),
            corpus_versions: vec!["corpus-a".to_owned()],
            next_index: 3,
        };
        assert_eq!(
            codec().decode(&codec().encode(&payload).unwrap()).unwrap(),
            payload
        );
    }

    #[test]
    fn cursor_tampering_fails() {
        let payload = CursorPayload {
            tenant_id: "tenant-a".to_owned(),
            operation: "search".to_owned(),
            input_digest: "sha256:00".to_owned(),
            corpus_versions: vec![],
            next_index: 1,
        };
        let mut encoded = codec().encode(&payload).unwrap();
        encoded.replace_range(0..1, "x");
        assert_eq!(
            codec().decode(&encoded).unwrap_err().code,
            ErrorCode::InvalidCursor
        );
    }
}
