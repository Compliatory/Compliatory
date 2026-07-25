use std::{cmp::Ordering, collections::BTreeMap, sync::Arc};

use chrono::Utc;
use compliatory_core::{
    ContentLayer, CursorCodec, CursorPayload, DocumentFragment, DomainError, ErrorCode,
    OmissionReason, OmittedItem, PacketBudget, WorkPacket, content_digest, estimate_tokens,
};

use crate::{
    AuthContext, BuildWorkPacketInput, CheckProfileCoverageInput, CheckProfileCoverageOutput,
    CitationError, CitationValidation, CoverageStatus, CriterionCoverage, ExpandWorkPacketInput,
    Permission, RegulatoryRepository, RepositorySearch, ResourceDocument, SearchHit,
    SearchReferencesInput, SearchReferencesOutput, ValidateCitationsInput, ValidateCitationsOutput,
};

pub const MAX_ORDINARY_TOKEN_BUDGET: u32 = 8192;
const MAX_SEARCH_LIMIT: u32 = 100;

#[derive(Clone)]
pub struct RegulatoryService {
    repository: Arc<dyn RegulatoryRepository>,
    cursor_codec: CursorCodec,
}

impl RegulatoryService {
    pub fn new(
        repository: Arc<dyn RegulatoryRepository>,
        cursor_key: impl AsRef<[u8]>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            repository,
            cursor_codec: CursorCodec::new(cursor_key)?,
        })
    }

    pub fn search_references(
        &self,
        auth: &AuthContext,
        input: &SearchReferencesInput,
    ) -> Result<SearchReferencesOutput, DomainError> {
        auth.require(Permission::CatalogRead)?;
        if input.query.trim().is_empty() {
            return Err(DomainError::invalid("search query cannot be empty"));
        }
        let limit = input.limit.clamp(1, MAX_SEARCH_LIMIT);
        let digest = Self::search_input_digest(input)?;
        let offset = if let Some(cursor) = &input.cursor {
            let payload = self.cursor_codec.decode(cursor)?;
            self.validate_cursor(auth, &payload, "search_references", &digest)?;
            payload.next_index
        } else {
            0
        };
        let query = RepositorySearch {
            query: input.query.clone(),
            standard_ids: input.standard_ids.clone(),
            editions: input.editions.clone(),
            layers: input.layers.clone(),
            locator_kinds: input.locator_kinds.clone(),
            offset,
            limit: limit.saturating_add(1),
        };
        let mut fragments = self.repository.search(auth, &query)?;
        fragments.sort_by(fragment_order);
        let has_more = fragments.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        fragments.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        let results = fragments
            .into_iter()
            .map(|fragment| SearchHit {
                preview: preview_for(auth, &fragment),
                reference: fragment.reference,
                fragment_id: fragment.fragment_id,
                title: fragment.title,
                layer: fragment.layer,
                availability: fragment.availability,
                score: 1.0,
            })
            .collect();
        let next_cursor = if has_more {
            Some(self.cursor_codec.encode(&CursorPayload {
                tenant_id: auth.tenant_id.clone(),
                operation: "search_references".to_owned(),
                input_digest: digest,
                corpus_versions: self.repository.current_corpus_versions(auth)?,
                next_index: offset.saturating_add(limit),
            })?)
        } else {
            None
        };
        Ok(SearchReferencesOutput {
            results,
            next_cursor,
        })
    }

    pub fn build_work_packet(
        &self,
        auth: &AuthContext,
        input: &BuildWorkPacketInput,
    ) -> Result<WorkPacket, DomainError> {
        auth.require(Permission::PacketBuild)?;
        validate_budget(input.token_budget)?;
        if input.objective.trim().is_empty() {
            return Err(DomainError::invalid("objective cannot be empty"));
        }
        let digest = Self::packet_input_digest(input)?;
        let offset = if let Some(cursor) = &input.continuation_cursor {
            if input.focus_refs.is_empty() {
                return Err(DomainError::new(
                    ErrorCode::InvalidCursor,
                    "focus_refs are required for continuation",
                ));
            }
            let payload = self.cursor_codec.decode(cursor)?;
            self.validate_cursor(auth, &payload, "build_work_packet", &digest)?;
            payload.next_index
        } else {
            0
        };
        let mut candidates =
            self.repository
                .packet_candidates(auth, &input.profile, &input.focus_refs)?;
        candidates.sort_by(fragment_order);
        self.create_packet(
            auth,
            &input.profile,
            input.phase,
            input.role,
            &input.objective,
            input.token_budget,
            input.tokenizer_id.as_deref(),
            &candidates,
            vec![],
            offset,
            digest,
            None,
        )
    }

    pub fn expand_work_packet(
        &self,
        auth: &AuthContext,
        input: &ExpandWorkPacketInput,
    ) -> Result<WorkPacket, DomainError> {
        auth.require(Permission::PacketBuild)?;
        validate_budget(input.token_budget)?;
        let source = self
            .repository
            .packet(auth, &input.packet_id)?
            .ok_or_else(|| DomainError::new(ErrorCode::UnknownReference, "packet not found"))?;
        let digest = content_digest(
            "expand-input-v1",
            &(
                &input.packet_id,
                &input.relation_types,
                &input.source_refs,
                input.token_budget,
                &input.tokenizer_id,
            ),
        )?;
        let offset = if let Some(cursor) = &input.cursor {
            let payload = self.cursor_codec.decode(cursor)?;
            self.validate_cursor(auth, &payload, "expand_work_packet", &digest)?;
            payload.next_index
        } else {
            0
        };
        let mut related =
            self.repository
                .related_fragments(auth, &input.source_refs, &input.relation_types)?;
        related.sort_by(|left, right| fragment_order(&left.fragment, &right.fragment));
        let candidates: Vec<_> = related
            .iter()
            .map(|related| related.fragment.clone())
            .collect();
        let relations = related
            .into_iter()
            .map(|related| related.relation)
            .collect();
        self.create_packet(
            auth,
            &source.profile,
            source.phase,
            source.role,
            &source.objective,
            input.token_budget,
            input.tokenizer_id.as_deref(),
            &candidates,
            relations,
            offset,
            digest,
            Some(source.packet_id),
        )
    }

    pub fn validate_citations(
        &self,
        auth: &AuthContext,
        input: &ValidateCitationsInput,
    ) -> Result<ValidateCitationsOutput, DomainError> {
        auth.require(Permission::CitationValidate)?;
        let mut results = Vec::with_capacity(input.citations.len());
        for citation in &input.citations {
            let fragment = self
                .repository
                .fragment_by_id(auth, &citation.fragment_id)?;
            let Some(fragment) = fragment else {
                results.push(CitationValidation {
                    valid: false,
                    errors: vec![CitationError::UnknownReference],
                    canonical_reference: None,
                    current_content_digest: None,
                });
                continue;
            };
            let mut errors = Vec::new();
            if citation.reference.standard_id != fragment.reference.standard_id {
                errors.push(CitationError::UnknownReference);
            }
            if citation.reference.edition != fragment.reference.edition {
                errors.push(CitationError::EditionMismatch);
            }
            if citation.reference.amendments != fragment.reference.amendments {
                errors.push(CitationError::AmendmentMismatch);
            }
            if citation.reference.language != fragment.reference.language {
                errors.push(CitationError::LanguageMismatch);
            }
            if citation.reference.locator != fragment.reference.locator {
                errors.push(CitationError::UnknownReference);
            }
            if citation.content_digest != fragment.content_digest {
                errors.push(CitationError::DigestMismatch);
            }
            if citation.quoted_text != fragment.exact_text.as_deref().unwrap_or_default() {
                errors.push(CitationError::TextMismatch);
            }
            if fragment.approval.is_none() {
                errors.push(CitationError::NotApproved);
            } else if fragment
                .approval
                .as_ref()
                .is_some_and(|approval| approval.corpus_version != citation.corpus_version)
            {
                errors.push(CitationError::CorpusSuperseded);
            }
            errors.sort();
            errors.dedup();
            results.push(CitationValidation {
                valid: errors.is_empty(),
                errors,
                canonical_reference: Some(fragment.reference),
                current_content_digest: Some(fragment.content_digest),
            });
        }
        Ok(ValidateCitationsOutput { results })
    }

    pub fn check_profile_coverage(
        &self,
        auth: &AuthContext,
        input: &CheckProfileCoverageInput,
    ) -> Result<CheckProfileCoverageOutput, DomainError> {
        auth.require(Permission::CoverageCheck)?;
        let profile = self
            .repository
            .profile(auth, &input.profile)?
            .ok_or_else(|| DomainError::new(ErrorCode::UnknownReference, "profile not found"))?;
        let findings: BTreeMap<_, _> = input
            .finding_refs
            .iter()
            .map(|finding| (finding.criterion_id.as_str(), finding))
            .collect();
        let criteria = profile
            .criteria
            .iter()
            .map(|criterion| {
                let status = match findings.get(criterion.criterion_id.as_str()) {
                    None => CoverageStatus::Missing,
                    Some(finding) if finding.status == "not_applicable" => {
                        CoverageStatus::NotApplicablePendingApproval
                    }
                    Some(finding) if finding.citation_digests.is_empty() => CoverageStatus::Missing,
                    Some(finding) => {
                        let mut resolved = Vec::new();
                        for digest in &finding.citation_digests {
                            if let Some(fragment) =
                                self.repository.fragment_by_digest(auth, digest)?
                            {
                                resolved.push(fragment);
                            }
                        }
                        if resolved.len() != finding.citation_digests.len() {
                            CoverageStatus::Missing
                        } else if resolved.iter().any(|fragment| {
                            criterion.references.iter().any(|reference| {
                                fragment.reference.standard_id == reference.standard_id
                                    && (fragment.reference.edition != reference.edition
                                        || fragment.reference.amendments != reference.amendments)
                            })
                        }) {
                            CoverageStatus::EditionConflict
                        } else if resolved.iter().any(|fragment| {
                            criterion.references.iter().any(|reference| {
                                fragment.reference.standard_id == reference.standard_id
                                    && fragment.reference.edition == reference.edition
                                    && fragment.reference.amendments == reference.amendments
                                    && fragment.reference.language == reference.language
                                    && fragment.reference.locator == reference.locator
                            })
                        }) {
                            CoverageStatus::Covered
                        } else {
                            CoverageStatus::Missing
                        }
                    }
                };
                Ok(CriterionCoverage {
                    criterion_id: criterion.criterion_id.clone(),
                    status,
                })
            })
            .collect::<Result<Vec<_>, DomainError>>()?;
        Ok(CheckProfileCoverageOutput {
            profile: profile.profile,
            criteria,
        })
    }

    pub fn read_resource(
        &self,
        auth: &AuthContext,
        uri: &str,
    ) -> Result<ResourceDocument, DomainError> {
        if let Some(packet_id) = uri.strip_prefix("reg://packets/") {
            auth.require(Permission::PacketBuild)?;
            let packet = self.repository.packet(auth, packet_id)?.ok_or_else(|| {
                DomainError::new(ErrorCode::UnknownReference, "resource not found")
            })?;
            return Ok(ResourceDocument {
                uri: uri.to_owned(),
                mime_type: "application/json".to_owned(),
                value: serde_json::to_value(packet)
                    .map_err(|error| DomainError::invalid(error.to_string()))?,
            });
        }
        if let Some(path) = uri.strip_prefix("reg://profiles/") {
            auth.require(Permission::CatalogRead)?;
            let (profile_id, version) = path
                .split_once("/versions/")
                .ok_or_else(|| DomainError::invalid("invalid profile resource URI"))?;
            let profile_ref = compliatory_core::ProfileRef {
                profile_id: profile_id.to_owned(),
                version: version.to_owned(),
            };
            let profile = self
                .repository
                .profile(auth, &profile_ref)?
                .ok_or_else(|| {
                    DomainError::new(ErrorCode::UnknownReference, "resource not found")
                })?;
            return Ok(ResourceDocument {
                uri: uri.to_owned(),
                mime_type: "application/json".to_owned(),
                value: serde_json::to_value(profile)
                    .map_err(|error| DomainError::invalid(error.to_string()))?,
            });
        }
        if let Some(path) = uri.strip_prefix("reg://standards/") {
            auth.require(Permission::CatalogRead)?;
            let parts: Vec<_> = path.split('/').collect();
            if parts.len() != 5 || parts[3] != "clauses" {
                return Err(DomainError::invalid("invalid standard resource URI"));
            }
            let reference = compliatory_core::NormativeRef {
                standard_id: parts[0].to_owned(),
                edition: parts[1].to_owned(),
                amendments: vec![],
                language: parts[2].to_owned(),
                locator: compliatory_core::Locator::clause(parts[4]),
                source_digest: String::new(),
                source_page: None,
            };
            let fragment = self
                .repository
                .fragment_by_reference(auth, &reference)?
                .ok_or_else(|| {
                    DomainError::new(ErrorCode::UnknownReference, "resource not found")
                })?;
            return Ok(ResourceDocument {
                uri: uri.to_owned(),
                mime_type: "application/json".to_owned(),
                value: serde_json::to_value(fragment)
                    .map_err(|error| DomainError::invalid(error.to_string()))?,
            });
        }
        Err(DomainError::new(
            ErrorCode::UnknownReference,
            "resource not found",
        ))
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    fn create_packet(
        &self,
        auth: &AuthContext,
        profile: &compliatory_core::ProfileRef,
        phase: compliatory_core::Phase,
        role: compliatory_core::Role,
        objective: &str,
        token_budget: u32,
        tokenizer_id: Option<&str>,
        candidates: &[DocumentFragment],
        relations: Vec<compliatory_core::Relation>,
        offset: u32,
        input_digest: String,
        parent_packet_id: Option<String>,
    ) -> Result<WorkPacket, DomainError> {
        let corpus_versions = self.repository.current_corpus_versions(auth)?;
        let mut selected = Vec::new();
        let mut omitted = Vec::new();
        let mut estimated_tokens = 0_u32;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let remaining = candidates.iter().skip(start);
        let mut next_index = None;
        let mut estimator_name = None;
        let mut effective_tokenizer = None;
        for (relative_index, fragment) in remaining.enumerate() {
            let canonical = compliatory_core::canonical_bytes(fragment)?;
            let text = String::from_utf8(canonical)
                .map_err(|error| DomainError::invalid(error.to_string()))?;
            let estimate = estimate_tokens(tokenizer_id, &text)?;
            estimator_name.get_or_insert(estimate.estimator.clone());
            effective_tokenizer.get_or_insert(estimate.tokenizer_id.clone());
            if estimate.estimated_tokens > token_budget {
                if selected.is_empty() {
                    return Err(DomainError::new(
                        ErrorCode::AtomicFragmentTooLarge,
                        format!(
                            "fragment {} exceeds the packet budget",
                            fragment.fragment_id
                        ),
                    ));
                }
                omitted.push(OmittedItem {
                    reference: fragment.reference.clone(),
                    reason: OmissionReason::AtomicFragmentTooLarge,
                });
                continue;
            }
            if estimated_tokens.saturating_add(estimate.estimated_tokens) > token_budget {
                next_index =
                    Some(offset.saturating_add(u32::try_from(relative_index).unwrap_or(u32::MAX)));
                omitted.extend(
                    candidates
                        .iter()
                        .skip(start + relative_index)
                        .map(|candidate| OmittedItem {
                            reference: candidate.reference.clone(),
                            reason: OmissionReason::BudgetExhausted,
                        }),
                );
                break;
            }
            estimated_tokens = estimated_tokens.saturating_add(estimate.estimated_tokens);
            selected.push(fragment.clone());
        }
        let continuation_cursor = next_index
            .map(|next_index| {
                self.cursor_codec.encode(&CursorPayload {
                    tenant_id: auth.tenant_id.clone(),
                    operation: if parent_packet_id.is_some() {
                        "expand_work_packet".to_owned()
                    } else {
                        "build_work_packet".to_owned()
                    },
                    input_digest,
                    corpus_versions: corpus_versions.clone(),
                    next_index,
                })
            })
            .transpose()?;
        let mut normative_fragments = Vec::new();
        let mut guidance_fragments = Vec::new();
        let mut tenant_fragments = Vec::new();
        for fragment in selected {
            match fragment.layer {
                ContentLayer::Normative => normative_fragments.push(fragment),
                ContentLayer::Guidance | ContentLayer::Catalog => guidance_fragments.push(fragment),
                ContentLayer::Tenant => tenant_fragments.push(fragment),
            }
        }
        let mut packet = WorkPacket {
            packet_id: String::new(),
            content_digest: String::new(),
            corpus_versions,
            profile: profile.clone(),
            phase,
            role,
            objective: objective.to_owned(),
            normative_fragments,
            guidance_fragments,
            tenant_fragments,
            definitions: vec![],
            relations,
            budget: PacketBudget {
                requested_tokens: token_budget,
                estimated_tokens,
                tokenizer_id: effective_tokenizer.unwrap_or_else(|| {
                    tokenizer_id
                        .unwrap_or(compliatory_core::DEFAULT_TOKENIZER)
                        .to_owned()
                }),
                estimator: estimator_name.unwrap_or_else(|| "conservative".to_owned()),
            },
            omitted,
            continuation_cursor,
            parent_packet_id,
            created_at: Utc::now(),
        };
        packet.assign_digest(&auth.rights_digest_input())?;
        self.repository.save_packet(auth, &packet)?;
        Ok(packet)
    }

    fn validate_cursor(
        &self,
        auth: &AuthContext,
        payload: &CursorPayload,
        operation: &str,
        input_digest: &str,
    ) -> Result<(), DomainError> {
        let current_versions = self.repository.current_corpus_versions(auth)?;
        if payload.tenant_id != auth.tenant_id
            || payload.operation != operation
            || payload.input_digest != input_digest
            || payload.corpus_versions != current_versions
        {
            return Err(DomainError::new(
                ErrorCode::InvalidCursor,
                "cursor does not match the authenticated request",
            ));
        }
        Ok(())
    }

    fn search_input_digest(input: &SearchReferencesInput) -> Result<String, DomainError> {
        content_digest(
            "search-input-v1",
            &(
                &input.query,
                &input.standard_ids,
                &input.editions,
                &input.layers,
                &input.locator_kinds,
                input.limit,
            ),
        )
    }

    fn packet_input_digest(input: &BuildWorkPacketInput) -> Result<String, DomainError> {
        content_digest(
            "packet-input-v1",
            &(
                &input.profile,
                input.phase,
                input.role,
                &input.objective,
                &input.focus_refs,
                &input.evidence_scope_digest,
                input.token_budget,
                &input.tokenizer_id,
            ),
        )
    }
}

fn validate_budget(budget: u32) -> Result<(), DomainError> {
    if budget == 0 || budget > MAX_ORDINARY_TOKEN_BUDGET {
        return Err(DomainError::invalid(format!(
            "token budget must be between 1 and {MAX_ORDINARY_TOKEN_BUDGET}"
        )));
    }
    Ok(())
}

fn fragment_order(left: &DocumentFragment, right: &DocumentFragment) -> Ordering {
    (
        &left.reference.standard_id,
        &left.reference.edition,
        &left.reference.language,
        &left.reference.locator,
        left.layer,
        &left.content_digest,
    )
        .cmp(&(
            &right.reference.standard_id,
            &right.reference.edition,
            &right.reference.language,
            &right.reference.locator,
            right.layer,
            &right.content_digest,
        ))
}

fn preview_for(auth: &AuthContext, fragment: &DocumentFragment) -> Option<String> {
    match fragment.layer {
        ContentLayer::Normative if auth.permissions.contains(&Permission::NormativeRead) => {
            fragment.exact_text.as_ref().map(|text| {
                let mut preview: String = text.chars().take(160).collect();
                if text.chars().count() > 160 {
                    preview.push('…');
                }
                preview
            })
        }
        ContentLayer::Guidance if auth.permissions.contains(&Permission::GuidanceRead) => fragment
            .explanatory_text
            .as_ref()
            .or(fragment.title.as_ref())
            .map(|text| text.chars().take(160).collect()),
        ContentLayer::Catalog => fragment.title.clone(),
        _ => None,
    }
}
