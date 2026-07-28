use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DomainError, ErrorCode};

pub const DEFAULT_TOKENIZER: &str = "conservative:utf8-bytes-v1";
pub const EXACT_TOKENIZER: &str = "registry:o200k_base";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TokenEstimate {
    pub tokenizer_id: String,
    pub estimated_tokens: u32,
    pub estimator: String,
}

pub fn estimate_tokens(
    tokenizer_id: Option<&str>,
    text: &str,
) -> Result<TokenEstimate, DomainError> {
    let tokenizer_id = tokenizer_id.unwrap_or(DEFAULT_TOKENIZER);
    match tokenizer_id {
        DEFAULT_TOKENIZER => Ok(TokenEstimate {
            tokenizer_id: tokenizer_id.to_owned(),
            estimated_tokens: u32::try_from(text.len()).unwrap_or(u32::MAX),
            estimator: "conservative".to_owned(),
        }),
        EXACT_TOKENIZER => Ok(TokenEstimate {
            tokenizer_id: tokenizer_id.to_owned(),
            estimated_tokens: u32::try_from(
                tiktoken_rs::o200k_base_singleton()
                    .encode_ordinary(text)
                    .len(),
            )
            .unwrap_or(u32::MAX),
            estimator: "exact".to_owned(),
        }),
        _ => Err(DomainError::new(
            ErrorCode::TokenizerUnknown,
            format!("unsupported tokenizer: {tokenizer_id}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_estimator_counts_utf8_bytes() {
        assert_eq!(estimate_tokens(None, "é").unwrap().estimated_tokens, 2);
    }

    #[test]
    fn unknown_tokenizer_is_explicit() {
        assert_eq!(
            estimate_tokens(Some("unknown"), "text").unwrap_err().code,
            ErrorCode::TokenizerUnknown
        );
    }

    #[test]
    fn o200k_uses_exact_tokenizer() {
        let estimate = estimate_tokens(Some(EXACT_TOKENIZER), "hello world").unwrap();
        assert_eq!(estimate.estimated_tokens, 2);
        assert_eq!(estimate.estimator, "exact");
    }
}
