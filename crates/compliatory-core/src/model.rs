use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DomainError, ErrorCode, content_digest, derived_id};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ContentLayer {
    Catalog,
    Guidance,
    Normative,
    Tenant,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    FullTextUnavailable,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LocatorKind {
    Clause,
    Paragraph,
    Annex,
    Table,
    Figure,
    Definition,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct Locator {
    pub kind: LocatorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clause: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub figure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
}

impl Locator {
    #[must_use]
    pub fn clause(value: impl Into<String>) -> Self {
        Self {
            kind: LocatorKind::Clause,
            clause: Some(value.into()),
            paragraph: None,
            annex: None,
            table: None,
            figure: None,
            definition: None,
        }
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        let present = [
            self.clause.is_some(),
            self.annex.is_some(),
            self.table.is_some(),
            self.figure.is_some(),
            self.definition.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        if present != 1 {
            return Err(DomainError::invalid(
                "locator must identify exactly one structural unit",
            ));
        }
        if self.paragraph.is_some()
            && !matches!(self.kind, LocatorKind::Clause | LocatorKind::Paragraph)
        {
            return Err(DomainError::invalid(
                "paragraph can only refine a clause or paragraph locator",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn display_key(&self) -> String {
        let base = self
            .clause
            .as_ref()
            .or(self.annex.as_ref())
            .or(self.table.as_ref())
            .or(self.figure.as_ref())
            .or(self.definition.as_ref())
            .cloned()
            .unwrap_or_default();
        self.paragraph.map_or(base.clone(), |paragraph| {
            format!("{base}#paragraph-{paragraph}")
        })
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct NormativeRef {
    pub standard_id: String,
    pub edition: String,
    #[serde(default)]
    pub amendments: Vec<String>,
    pub language: String,
    pub locator: Locator,
    pub source_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_page: Option<u32>,
}

impl NormativeRef {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.standard_id.trim().is_empty()
            || self.edition.trim().is_empty()
            || self.language.trim().is_empty()
        {
            return Err(DomainError::invalid(
                "standard, edition and language are required",
            ));
        }
        self.locator.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Approval {
    pub corpus_version: String,
    pub approved_at: DateTime<Utc>,
    pub approved_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentFragment {
    pub fragment_id: String,
    pub layer: ContentLayer,
    pub reference: NormativeRef,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanatory_text: Option<String>,
    pub content_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Serialize)]
struct FragmentDigestInput<'a> {
    layer: ContentLayer,
    reference: &'a NormativeRef,
    availability: Availability,
    exact_text: &'a Option<String>,
    explanatory_text: &'a Option<String>,
    approval_corpus: Option<&'a str>,
    title: &'a Option<String>,
}

impl DocumentFragment {
    pub fn new(
        layer: ContentLayer,
        reference: NormativeRef,
        availability: Availability,
        exact_text: Option<String>,
        explanatory_text: Option<String>,
        approval: Option<Approval>,
        title: Option<String>,
    ) -> Result<Self, DomainError> {
        reference.validate()?;
        match layer {
            ContentLayer::Normative => {
                if explanatory_text.is_some() {
                    return Err(DomainError::invalid(
                        "normative fragments cannot carry explanatory text",
                    ));
                }
                if availability == Availability::Available
                    && (exact_text.as_deref().is_none_or(str::is_empty) || approval.is_none())
                {
                    return Err(DomainError::new(
                        ErrorCode::NotApproved,
                        "available normative fragments require exact text and approval",
                    ));
                }
            }
            ContentLayer::Catalog | ContentLayer::Guidance | ContentLayer::Tenant => {
                if exact_text.is_some() || approval.is_some() {
                    return Err(DomainError::invalid(
                        "non-normative layers cannot carry exact text or normative approval",
                    ));
                }
            }
        }
        let input = FragmentDigestInput {
            layer,
            reference: &reference,
            availability,
            exact_text: &exact_text,
            explanatory_text: &explanatory_text,
            approval_corpus: approval.as_ref().map(|value| value.corpus_version.as_str()),
            title: &title,
        };
        let content_digest = content_digest("fragment-v1", &input)?;
        let fragment_id = derived_id("frag_", &content_digest)?;
        Ok(Self {
            fragment_id,
            layer,
            reference,
            availability,
            exact_text,
            explanatory_text,
            content_digest,
            approval,
            title,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Citation {
    pub reference: NormativeRef,
    pub fragment_id: String,
    pub quoted_text: String,
    pub content_digest: String,
    pub corpus_version: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Scoping,
    ApplicabilityMapping,
    EvidenceCollection,
    PrimaryEvaluation,
    IndependentChallenge,
    Reconciliation,
    DeterministicValidation,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Scoper,
    Collector,
    Evaluator,
    SecurityReviewer,
    IndependentReviewer,
    Reconciler,
    Validator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileRef {
    pub profile_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Definition {
    pub term: String,
    pub fragment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Relation {
    pub relation_type: String,
    pub source_fragment_id: String,
    pub target_fragment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PacketBudget {
    pub requested_tokens: u32,
    pub estimated_tokens: u32,
    pub tokenizer_id: String,
    pub estimator: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OmissionReason {
    BudgetExhausted,
    NotEntitled,
    NotApproved,
    OutsideFocus,
    AtomicFragmentTooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OmittedItem {
    pub reference: NormativeRef,
    pub reason: OmissionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkPacket {
    pub packet_id: String,
    pub content_digest: String,
    pub corpus_versions: Vec<String>,
    pub profile: ProfileRef,
    pub phase: Phase,
    pub role: Role,
    pub objective: String,
    pub normative_fragments: Vec<DocumentFragment>,
    pub guidance_fragments: Vec<DocumentFragment>,
    pub tenant_fragments: Vec<DocumentFragment>,
    pub definitions: Vec<Definition>,
    pub relations: Vec<Relation>,
    pub budget: PacketBudget,
    pub omitted: Vec<OmittedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_packet_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct PacketDigestInput<'a> {
    pub corpus_versions: &'a [String],
    pub profile: &'a ProfileRef,
    pub phase: Phase,
    pub role: Role,
    pub objective: &'a str,
    pub fragment_digests: Vec<&'a str>,
    pub definitions: &'a [Definition],
    pub relations: &'a [Relation],
    pub budget: &'a PacketBudget,
    pub omitted: &'a [OmittedItem],
    pub parent_packet_id: &'a Option<String>,
    pub effective_rights: &'a BTreeSet<String>,
}

impl WorkPacket {
    pub fn assign_digest(
        &mut self,
        effective_rights: &BTreeSet<String>,
    ) -> Result<(), DomainError> {
        let fragment_digests = self
            .normative_fragments
            .iter()
            .chain(&self.guidance_fragments)
            .chain(&self.tenant_fragments)
            .map(|fragment| fragment.content_digest.as_str())
            .collect();
        let input = PacketDigestInput {
            corpus_versions: &self.corpus_versions,
            profile: &self.profile,
            phase: self.phase,
            role: self.role,
            objective: &self.objective,
            fragment_digests,
            definitions: &self.definitions,
            relations: &self.relations,
            budget: &self.budget,
            omitted: &self.omitted,
            parent_packet_id: &self.parent_packet_id,
            effective_rights,
        };
        self.content_digest = content_digest("work-packet-v1", &input)?;
        self.packet_id = derived_id("pkt_", &self.content_digest)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileCriterion {
    pub criterion_id: String,
    pub references: Vec<NormativeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NormativeProfile {
    pub profile: ProfileRef,
    pub title: String,
    pub criteria: Vec<ProfileCriterion>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn reference() -> NormativeRef {
        NormativeRef {
            standard_id: "SYN-62304".to_owned(),
            edition: "1".to_owned(),
            amendments: vec![],
            language: "en".to_owned(),
            locator: Locator::clause("5.5.3"),
            source_digest: format!("sha256:{}", "0".repeat(64)),
            source_page: Some(2),
        }
    }

    #[test]
    fn guidance_cannot_carry_exact_text() {
        let error = DocumentFragment::new(
            ContentLayer::Guidance,
            reference(),
            Availability::Available,
            Some("not allowed".to_owned()),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn normative_fragment_requires_approval() {
        let error = DocumentFragment::new(
            ContentLayer::Normative,
            reference(),
            Availability::Available,
            Some("synthetic requirement".to_owned()),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotApproved);
    }

    #[test]
    fn fragment_digest_ignores_approval_timestamp_and_subject() {
        let first = Approval {
            corpus_version: "corpus_1".to_owned(),
            approved_at: Utc.timestamp_opt(1, 0).unwrap(),
            approved_by: "alice".to_owned(),
        };
        let second = Approval {
            corpus_version: "corpus_1".to_owned(),
            approved_at: Utc.timestamp_opt(2, 0).unwrap(),
            approved_by: "bob".to_owned(),
        };
        let first = DocumentFragment::new(
            ContentLayer::Normative,
            reference(),
            Availability::Available,
            Some("synthetic requirement".to_owned()),
            None,
            Some(first),
            None,
        )
        .unwrap();
        let second = DocumentFragment::new(
            ContentLayer::Normative,
            reference(),
            Availability::Available,
            Some("synthetic requirement".to_owned()),
            None,
            Some(second),
            None,
        )
        .unwrap();
        assert_eq!(first.content_digest, second.content_digest);
    }
}
