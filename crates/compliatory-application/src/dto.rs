use compliatory_core::{
    Citation, ContentLayer, DocumentFragment, ErrorCode, LocatorKind, NormativeRef, Phase,
    ProfileRef, Role, WorkPacket,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchReferencesInput {
    pub query: String,
    #[serde(default)]
    pub standard_ids: Vec<String>,
    #[serde(default)]
    pub editions: Vec<String>,
    #[serde(default)]
    pub layers: Vec<ContentLayer>,
    #[serde(default)]
    pub locator_kinds: Vec<LocatorKind>,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
    #[serde(default)]
    pub cursor: Option<String>,
}

const fn default_search_limit() -> u32 {
    20
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHit {
    pub reference: NormativeRef,
    pub fragment_id: String,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub layer: ContentLayer,
    pub availability: compliatory_core::Availability,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchReferencesOutput {
    pub results: Vec<SearchHit>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BuildWorkPacketInput {
    pub profile: ProfileRef,
    pub phase: Phase,
    pub role: Role,
    pub objective: String,
    #[serde(default)]
    pub focus_refs: Vec<NormativeRef>,
    pub evidence_scope_digest: String,
    #[serde(default = "default_packet_budget")]
    pub token_budget: u32,
    #[serde(default)]
    pub tokenizer_id: Option<String>,
    #[serde(default)]
    pub continuation_cursor: Option<String>,
}

const fn default_packet_budget() -> u32 {
    4096
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExpandWorkPacketInput {
    pub packet_id: String,
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<NormativeRef>,
    #[serde(default = "default_expand_budget")]
    pub token_budget: u32,
    #[serde(default)]
    pub tokenizer_id: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
}

const fn default_expand_budget() -> u32 {
    2048
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidateCitationsInput {
    pub citations: Vec<Citation>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CitationError {
    UnknownReference,
    EditionMismatch,
    AmendmentMismatch,
    LanguageMismatch,
    TextMismatch,
    DigestMismatch,
    CorpusSuperseded,
    NotEntitled,
    NotApproved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CitationValidation {
    pub valid: bool,
    pub errors: Vec<CitationError>,
    pub canonical_reference: Option<NormativeRef>,
    pub current_content_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidateCitationsOutput {
    pub results: Vec<CitationValidation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FindingRef {
    pub criterion_id: String,
    #[serde(default)]
    pub citation_digests: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckProfileCoverageInput {
    pub profile: ProfileRef,
    #[serde(default)]
    pub finding_refs: Vec<FindingRef>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Covered,
    Missing,
    NotApplicablePendingApproval,
    EditionConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CriterionCoverage {
    pub criterion_id: String,
    pub status: CoverageStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckProfileCoverageOutput {
    pub profile: ProfileRef,
    pub criteria: Vec<CriterionCoverage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceDocument {
    pub uri: String,
    pub mime_type: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
}

impl From<compliatory_core::DomainError> for ToolError {
    fn from(value: compliatory_core::DomainError) -> Self {
        Self {
            code: value.code,
            message: value.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ResourceValue {
    Fragment(DocumentFragment),
    Packet(WorkPacket),
    Profile(compliatory_core::NormativeProfile),
}
