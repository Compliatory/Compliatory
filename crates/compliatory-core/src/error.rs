use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    FullTextUnavailable,
    NotEntitled,
    NotApproved,
    UnknownReference,
    EditionConflict,
    InvalidCursor,
    AtomicFragmentTooLarge,
    TokenizerUnknown,
    CorpusSuperseded,
    InvalidInput,
    Internal,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullTextUnavailable => "full_text_unavailable",
            Self::NotEntitled => "not_entitled",
            Self::NotApproved => "not_approved",
            Self::UnknownReference => "unknown_reference",
            Self::EditionConflict => "edition_conflict",
            Self::InvalidCursor => "invalid_cursor",
            Self::AtomicFragmentTooLarge => "atomic_fragment_too_large",
            Self::TokenizerUnknown => "tokenizer_unknown",
            Self::CorpusSuperseded => "corpus_superseded",
            Self::InvalidInput => "invalid_input",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, JsonSchema)]
#[error("{code:?}: {message}")]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
}

impl DomainError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }
}
