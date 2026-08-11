#![forbid(unsafe_code)]

//! Administrative helpers. None of these operations are exposed over MCP.

use chrono::{TimeZone, Utc};
use compliatory_core::{
    Approval, Availability, ContentLayer, DocumentFragment, DomainError, Locator, NormativeProfile,
    NormativeRef, ProfileCriterion, ProfileRef, content_digest, derived_id,
};
use compliatory_sqlite::SqliteRepository;

const SYNTHETIC_TEXT_PREFIX: &str =
    "SYNTHETIC TEST MATERIAL — NON-NORMATIVE AND NOT FROM ANY STANDARD.";

struct Family<'a> {
    standard_id: &'a str,
    edition: &'a str,
    amendments: &'a [&'a str],
    profile_id: &'a str,
    clauses: &'a [(&'a str, &'a str)],
}

pub fn seed_synthetic_fixtures(
    repository: &SqliteRepository,
    tenant_id: &str,
) -> Result<(), DomainError> {
    let families = [
        Family {
            standard_id: "SYN-IEC-62304",
            edition: "2006",
            amendments: &["A1:2015"],
            profile_id: "synthetic-iec62304-class-c",
            clauses: &[
                ("5.5.3", "Unit verification"),
                ("5.7.4", "Regression verification"),
            ],
        },
        Family {
            standard_id: "SYN-IEC-81001-5-1",
            edition: "2021",
            amendments: &[],
            profile_id: "synthetic-iec81001-security",
            clauses: &[("7.1", "Security activity"), ("7.2", "Security evidence")],
        },
        Family {
            standard_id: "SYN-DO-178",
            edition: "C",
            amendments: &["ED-12C"],
            profile_id: "synthetic-do178c",
            clauses: &[("6.3", "Software verification"), ("6.4", "Test coverage")],
        },
        Family {
            standard_id: "SYN-DO-178",
            edition: "B",
            amendments: &["ED-12B"],
            profile_id: "synthetic-do178b",
            clauses: &[
                ("6.3", "Legacy software verification"),
                ("6.4", "Legacy test coverage"),
            ],
        },
    ];
    for family in families {
        seed_family(repository, tenant_id, &family)?;
    }
    Ok(())
}

fn seed_family(
    repository: &SqliteRepository,
    tenant_id: &str,
    family: &Family<'_>,
) -> Result<(), DomainError> {
    let source_digest = content_digest(
        "synthetic-source-v1",
        &(family.standard_id, family.edition, family.amendments),
    )?;
    let corpus_digest =
        content_digest("synthetic-corpus-v1", &(family.standard_id, family.edition))?;
    let corpus_version = derived_id("corpus_", &corpus_digest)?;
    repository.publish_corpus(
        tenant_id,
        &corpus_version,
        &source_digest,
        "subject:synthetic-fixture-generator",
    )?;
    let approved_at = Utc.timestamp_opt(0, 0).single().ok_or_else(|| {
        DomainError::invalid("failed to construct deterministic fixture timestamp")
    })?;
    let mut criteria = Vec::new();
    let mut normative_ids = Vec::new();
    for (index, (clause, title)) in family.clauses.iter().enumerate() {
        let reference = NormativeRef {
            standard_id: family.standard_id.to_owned(),
            edition: family.edition.to_owned(),
            amendments: family
                .amendments
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            language: "en".to_owned(),
            locator: Locator::clause(*clause),
            source_digest: source_digest.clone(),
            source_page: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
        };
        let catalog = DocumentFragment::new(
            ContentLayer::Catalog,
            reference.clone(),
            Availability::FullTextUnavailable,
            None,
            None,
            None,
            Some(format!("SYNTHETIC — {title}")),
        )?;
        repository.insert_fragment(tenant_id, &catalog)?;
        let guidance = DocumentFragment::new(
            ContentLayer::Guidance,
            reference.clone(),
            Availability::Available,
            None,
            Some(format!(
                "{SYNTHETIC_TEXT_PREFIX} Original guidance for testing discovery of {title}."
            )),
            None,
            Some(format!("SYNTHETIC guidance — {title}")),
        )?;
        repository.insert_fragment(tenant_id, &guidance)?;
        let normative = DocumentFragment::new(
            ContentLayer::Normative,
            reference.clone(),
            Availability::Available,
            Some(format!(
                "{SYNTHETIC_TEXT_PREFIX} Test requirement {index} for {title}."
            )),
            None,
            Some(Approval {
                corpus_version: corpus_version.clone(),
                approved_at,
                approved_by: "subject:synthetic-fixture-generator".to_owned(),
            }),
            Some(format!("SYNTHETIC exact fragment — {title}")),
        )?;
        normative_ids.push(normative.fragment_id.clone());
        repository.insert_fragment(tenant_id, &normative)?;
        criteria.push(ProfileCriterion {
            criterion_id: format!("{}-criterion-{}", family.profile_id, index + 1),
            references: vec![reference],
        });
    }
    if let [first, second, ..] = normative_ids.as_slice() {
        repository.insert_relation(tenant_id, "normative_reference", first, second)?;
    }
    repository.insert_profile(
        tenant_id,
        &NormativeProfile {
            profile: ProfileRef {
                profile_id: family.profile_id.to_owned(),
                version: "1".to_owned(),
            },
            title: format!(
                "SYNTHETIC profile for {} {}",
                family.standard_id, family.edition
            ),
            criteria,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use compliatory_application::{
        AuthContext, BuildWorkPacketInput, CheckProfileCoverageInput, ExpandWorkPacketInput,
        FindingRef, RegulatoryRepository, RegulatoryService, SearchReferencesInput,
        ValidateCitationsInput,
    };
    use compliatory_core::{Citation, ErrorCode, Phase, Role};

    use super::*;

    #[test]
    fn synthetic_fixture_seeding_is_idempotent() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
    }

    #[test]
    fn fixtures_cover_do178_editions_without_substitution() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let b = repository
            .profile(
                &auth,
                &ProfileRef {
                    profile_id: "synthetic-do178b".to_owned(),
                    version: "1".to_owned(),
                },
            )
            .unwrap()
            .unwrap();
        let c = repository
            .profile(
                &auth,
                &ProfileRef {
                    profile_id: "synthetic-do178c".to_owned(),
                    version: "1".to_owned(),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(b.criteria[0].references[0].edition, "B");
        assert_eq!(c.criteria[0].references[0].edition, "C");
    }

    #[test]
    fn search_finds_a_structured_synthetic_reference() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let service = RegulatoryService::new(repository, [4_u8; 32]).unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let output = service
            .search_references(
                &auth,
                &SearchReferencesInput {
                    query: "Unit verification".to_owned(),
                    standard_ids: vec!["SYN-IEC-62304".to_owned()],
                    editions: vec!["2006".to_owned()],
                    layers: vec![ContentLayer::Guidance],
                    locator_kinds: vec![],
                    limit: 20,
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(output.results.len(), 1);
        assert_eq!(
            output.results[0].reference.locator,
            Locator::clause("5.5.3")
        );
        assert_eq!(output.results[0].layer, ContentLayer::Guidance);
    }

    #[test]
    fn vertical_slice_builds_a_deterministic_packet() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let service = RegulatoryService::new(repository, [4_u8; 32]).unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let input = BuildWorkPacketInput {
            profile: ProfileRef {
                profile_id: "synthetic-iec62304-class-c".to_owned(),
                version: "1".to_owned(),
            },
            phase: Phase::PrimaryEvaluation,
            role: Role::Evaluator,
            objective: "Evaluate synthetic unit-verification evidence".to_owned(),
            focus_refs: vec![],
            evidence_scope_digest: format!("sha256:{}", "a".repeat(64)),
            token_budget: 4096,
            tokenizer_id: None,
            continuation_cursor: None,
        };
        let first = service.build_work_packet(&auth, &input).unwrap();
        let second = service.build_work_packet(&auth, &input).unwrap();
        assert_eq!(first.packet_id, second.packet_id);
        assert_eq!(first.content_digest, second.content_digest);
        assert!(!first.normative_fragments.is_empty());
    }

    #[test]
    fn a_single_character_citation_change_is_detected() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let service = RegulatoryService::new(repository, [4_u8; 32]).unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let packet = service
            .build_work_packet(
                &auth,
                &BuildWorkPacketInput {
                    profile: ProfileRef {
                        profile_id: "synthetic-iec62304-class-c".to_owned(),
                        version: "1".to_owned(),
                    },
                    phase: Phase::PrimaryEvaluation,
                    role: Role::Evaluator,
                    objective: "Validate a synthetic citation".to_owned(),
                    focus_refs: vec![],
                    evidence_scope_digest: format!("sha256:{}", "b".repeat(64)),
                    token_budget: 4096,
                    tokenizer_id: Some("registry:o200k_base".to_owned()),
                    continuation_cursor: None,
                },
            )
            .unwrap();
        let fragment = packet.normative_fragments.first().unwrap();
        let mut quoted_text = fragment.exact_text.clone().unwrap();
        quoted_text.push('!');
        let output = service
            .validate_citations(
                &auth,
                &ValidateCitationsInput {
                    citations: vec![Citation {
                        reference: fragment.reference.clone(),
                        fragment_id: fragment.fragment_id.clone(),
                        quoted_text,
                        content_digest: fragment.content_digest.clone(),
                        corpus_version: fragment.approval.as_ref().unwrap().corpus_version.clone(),
                    }],
                },
            )
            .unwrap();
        assert!(!output.results[0].valid);
        assert!(
            output.results[0]
                .errors
                .contains(&compliatory_application::CitationError::TextMismatch)
        );
    }

    #[test]
    fn coverage_reports_missing_criteria_without_deciding_compliance() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let profile_ref = ProfileRef {
            profile_id: "synthetic-do178c".to_owned(),
            version: "1".to_owned(),
        };
        let profile = repository.profile(&auth, &profile_ref).unwrap().unwrap();
        let fragment = repository
            .fragment_by_reference(&auth, &profile.criteria[0].references[0])
            .unwrap()
            .unwrap();
        let service = RegulatoryService::new(repository, [4_u8; 32]).unwrap();
        let output = service
            .check_profile_coverage(
                &auth,
                &CheckProfileCoverageInput {
                    profile: profile_ref,
                    finding_refs: vec![FindingRef {
                        criterion_id: "synthetic-do178c-criterion-1".to_owned(),
                        citation_digests: vec![fragment.content_digest],
                        status: "preuve_insuffisante".to_owned(),
                    }],
                },
            )
            .unwrap();
        assert_eq!(
            output.criteria[0].status,
            compliatory_application::CoverageStatus::Covered
        );
        assert_eq!(
            output.criteria[1].status,
            compliatory_application::CoverageStatus::Missing
        );
    }

    #[test]
    fn continuation_cursor_cannot_cross_tenants() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        seed_synthetic_fixtures(&repository, "tenant-b").unwrap();
        let service = RegulatoryService::new(repository.clone(), [4_u8; 32]).unwrap();
        let auth_a = AuthContext::local_service("tenant-a", "test");
        let auth_b = AuthContext::local_service("tenant-b", "test");
        let profile = repository
            .profile(
                &auth_a,
                &ProfileRef {
                    profile_id: "synthetic-iec62304-class-c".to_owned(),
                    version: "1".to_owned(),
                },
            )
            .unwrap()
            .unwrap();
        let mut input = BuildWorkPacketInput {
            profile: profile.profile,
            phase: Phase::PrimaryEvaluation,
            role: Role::Evaluator,
            objective: "Continue a bounded synthetic packet".to_owned(),
            focus_refs: profile.criteria[0].references.clone(),
            evidence_scope_digest: format!("sha256:{}", "d".repeat(64)),
            token_budget: 1600,
            tokenizer_id: None,
            continuation_cursor: None,
        };
        let first = service.build_work_packet(&auth_a, &input).unwrap();
        input.continuation_cursor = first.continuation_cursor;
        let error = service.build_work_packet(&auth_b, &input).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidCursor);
    }

    #[test]
    fn expansion_records_the_relation_and_parent_packet() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        seed_synthetic_fixtures(&repository, "tenant-a").unwrap();
        let auth = AuthContext::local_service("tenant-a", "test");
        let profile = repository
            .profile(
                &auth,
                &ProfileRef {
                    profile_id: "synthetic-iec62304-class-c".to_owned(),
                    version: "1".to_owned(),
                },
            )
            .unwrap()
            .unwrap();
        let service = RegulatoryService::new(repository, [4_u8; 32]).unwrap();
        let packet = service
            .build_work_packet(
                &auth,
                &BuildWorkPacketInput {
                    profile: profile.profile,
                    phase: Phase::PrimaryEvaluation,
                    role: Role::Evaluator,
                    objective: "Expand a synthetic normative relation".to_owned(),
                    focus_refs: profile.criteria[0].references.clone(),
                    evidence_scope_digest: format!("sha256:{}", "e".repeat(64)),
                    token_budget: 4096,
                    tokenizer_id: Some("registry:o200k_base".to_owned()),
                    continuation_cursor: None,
                },
            )
            .unwrap();
        let expanded = service
            .expand_work_packet(
                &auth,
                &ExpandWorkPacketInput {
                    packet_id: packet.packet_id.clone(),
                    relation_types: vec!["normative_reference".to_owned()],
                    source_refs: profile.criteria[0].references.clone(),
                    token_budget: 2048,
                    tokenizer_id: Some("registry:o200k_base".to_owned()),
                    cursor: None,
                },
            )
            .unwrap();
        assert_eq!(expanded.parent_packet_id, Some(packet.packet_id));
        assert_eq!(expanded.relations.len(), 1);
        assert_eq!(expanded.relations[0].relation_type, "normative_reference");
    }
}
