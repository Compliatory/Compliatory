#![forbid(unsafe_code)]

//! Governed domain model and deterministic primitives for Compliatory.

mod cursor;
mod digest;
mod error;
mod model;
mod tokenizer;

pub use cursor::{CursorCodec, CursorPayload};
pub use digest::{canonical_bytes, content_digest, derived_id};
pub use error::{DomainError, ErrorCode};
pub use model::*;
pub use tokenizer::{DEFAULT_TOKENIZER, EXACT_TOKENIZER, TokenEstimate, estimate_tokens};
