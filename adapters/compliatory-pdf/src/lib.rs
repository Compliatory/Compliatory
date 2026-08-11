#![forbid(unsafe_code)]

//! Administrative, human-controlled PDF ingestion.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use chrono::Utc;
use compliatory_core::{
    Approval, Availability, DocumentFragment, DomainError, ErrorCode, Locator, content_digest,
    derived_id,
};
use compliatory_sqlite::SqliteRepository;
use rusqlite::{OptionalExtension, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExpectedUnit {
    pub locator: Locator,
    pub heading: String,
    #[serde(default)]
    pub source_page: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IngestionManifest {
    pub standard_id: String,
    pub edition: String,
    #[serde(default)]
    pub amendments: Vec<String>,
    pub language: String,
    pub usage_right: String,
    #[serde(default)]
    pub expected_units: Vec<ExpectedUnit>,
}

impl IngestionManifest {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.standard_id.trim().is_empty()
            || self.edition.trim().is_empty()
            || self.language.trim().is_empty()
            || self.usage_right.trim().is_empty()
        {
            return Err(DomainError::invalid(
                "standard, edition, language and usage right are required",
            ));
        }
        if self.expected_units.is_empty() {
            return Err(DomainError::invalid(
                "at least one expected structural unit is required",
            ));
        }
        for unit in &self.expected_units {
            unit.locator.validate()?;
            if unit.heading.trim().is_empty() {
                return Err(DomainError::invalid("unit headings cannot be empty"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewedUnit {
    pub locator: Locator,
    pub heading: String,
    pub source_page: Option<u32>,
    pub exact_text: String,
    pub confidence_basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewBundle {
    pub ingestion_id: String,
    pub source_digest: String,
    pub manifest: IngestionManifest,
    pub units: Vec<ReviewedUnit>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IngestionRecord {
    pub ingestion_id: String,
    pub state: String,
    pub source_digest: String,
    pub review_digest: Option<String>,
    pub failure_reason: Option<String>,
    pub corpus_version: Option<String>,
}

pub trait SecurityScanner {
    fn scan(&self, path: &Path) -> Result<(), DomainError>;
    fn scanner_id(&self) -> &str;
}

pub struct CommandScanner {
    command: PathBuf,
}

impl CommandScanner {
    #[must_use]
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
        }
    }
}

impl SecurityScanner for CommandScanner {
    fn scan(&self, path: &Path) -> Result<(), DomainError> {
        let status = Command::new(&self.command)
            .arg(path)
            .status()
            .map_err(|error| {
                DomainError::new(
                    ErrorCode::NotApproved,
                    format!("security scanner failed to start: {error}"),
                )
            })?;
        if !status.success() {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                "security scanner rejected the source",
            ));
        }
        Ok(())
    }

    fn scanner_id(&self) -> &str {
        self.command.to_str().unwrap_or("configured-command")
    }
}

/// Scanner reserved for original synthetic test documents.
pub struct SyntheticFixtureScanner;

impl SecurityScanner for SyntheticFixtureScanner {
    fn scan(&self, _path: &Path) -> Result<(), DomainError> {
        Ok(())
    }

    fn scanner_id(&self) -> &'static str {
        "synthetic-fixture-scanner-v1"
    }
}

pub struct IngestionService<'a> {
    repository: &'a SqliteRepository,
}

impl<'a> IngestionService<'a> {
    #[must_use]
    pub const fn new(repository: &'a SqliteRepository) -> Self {
        Self { repository }
    }

    pub fn ingest(
        &self,
        tenant_id: &str,
        manifest: &IngestionManifest,
        source: &Path,
        scanner: &dyn SecurityScanner,
    ) -> Result<IngestionRecord, DomainError> {
        manifest.validate()?;
        self.repository.ensure_tenant(tenant_id)?;
        inspect_pdf(source)?;
        scanner.scan(source)?;
        let source_digest = SqliteRepository::sha256_file(source)?;
        let ingestion_id = format!("ing_{}", Uuid::new_v4().simple());
        let ingestion_dir = self
            .repository
            .tenant_dir(tenant_id)?
            .join("quarantine")
            .join(&ingestion_id);
        fs::create_dir_all(&ingestion_dir).map_err(io_error)?;
        let source_path = ingestion_dir.join("source.pdf");
        fs::copy(source, &source_path).map_err(io_error)?;
        let manifest_json = serde_json::to_string_pretty(manifest)
            .map_err(|error| DomainError::invalid(error.to_string()))?;
        fs::write(ingestion_dir.join("manifest.json"), &manifest_json).map_err(io_error)?;
        let now = Utc::now().to_rfc3339();
        self.repository
            .connection()?
            .execute(
                "INSERT INTO ingestions(
                    tenant_id, ingestion_id, state, manifest_json, source_digest, source_path,
                    created_at, updated_at
                 ) VALUES (?1, ?2, 'inspected', ?3, ?4, ?5, ?6, ?6)",
                params![
                    tenant_id,
                    ingestion_id,
                    manifest_json,
                    source_digest,
                    source_path.to_string_lossy(),
                    now
                ],
            )
            .map_err(db_error)?;
        let scanner_record = format!("scanner={}", scanner.scanner_id());
        fs::write(ingestion_dir.join("inspection.txt"), scanner_record).map_err(io_error)?;
        Ok(IngestionRecord {
            ingestion_id,
            state: "inspected".to_owned(),
            source_digest,
            review_digest: None,
            failure_reason: None,
            corpus_version: None,
        })
    }

    /// Extract text. The CLI calls this method from its dedicated hidden worker process.
    pub fn extract_worker(
        &self,
        tenant_id: &str,
        ingestion_id: &str,
    ) -> Result<IngestionRecord, DomainError> {
        let row = self.load_row(tenant_id, ingestion_id)?;
        if row.state != "inspected" {
            return Err(DomainError::invalid(
                "only inspected sources can be extracted",
            ));
        }
        let source_path = row.source_path;
        let manifest = row.manifest;
        let text = match pdf_extract::extract_text(&source_path) {
            Ok(text) if !text.trim().is_empty() => text,
            Ok(_) => {
                self.quarantine(tenant_id, ingestion_id, "image_only_or_empty")?;
                return Err(DomainError::new(
                    ErrorCode::NotApproved,
                    "PDF has no extractable text; OCR is not enabled",
                ));
            }
            Err(error) => {
                self.quarantine(tenant_id, ingestion_id, "pdf_extraction_failed")?;
                return Err(DomainError::new(
                    ErrorCode::NotApproved,
                    format!("PDF extraction failed: {error}"),
                ));
            }
        };
        let units = match_units(&text, &manifest.expected_units)?;
        let conflicts = if units.len() == manifest.expected_units.len() {
            vec![]
        } else {
            vec!["one or more expected headings were not matched exactly".to_owned()]
        };
        let review = ReviewBundle {
            ingestion_id: ingestion_id.to_owned(),
            source_digest: row.source_digest.clone(),
            manifest,
            units,
            conflicts,
        };
        let review_digest = content_digest("ingestion-review-v1", &review)?;
        let directory = source_path
            .parent()
            .ok_or_else(|| DomainError::new(ErrorCode::Internal, "invalid source path"))?;
        let extracted_path = directory.join("extracted.txt");
        let review_path = directory.join("review.json");
        fs::write(&extracted_path, text).map_err(io_error)?;
        fs::write(
            &review_path,
            serde_json::to_vec_pretty(&review)
                .map_err(|error| DomainError::invalid(error.to_string()))?,
        )
        .map_err(io_error)?;
        self.repository
            .connection()?
            .execute(
                "UPDATE ingestions SET state = 'awaiting_review', review_digest = ?3,
                    review_path = ?4, extracted_text_path = ?5, updated_at = ?6
                 WHERE tenant_id = ?1 AND ingestion_id = ?2",
                params![
                    tenant_id,
                    ingestion_id,
                    review_digest,
                    review_path.to_string_lossy(),
                    extracted_path.to_string_lossy(),
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(db_error)?;
        Ok(IngestionRecord {
            ingestion_id: ingestion_id.to_owned(),
            state: "awaiting_review".to_owned(),
            source_digest: row.source_digest,
            review_digest: Some(review_digest),
            failure_reason: None,
            corpus_version: None,
        })
    }

    pub fn approve(
        &self,
        tenant_id: &str,
        ingestion_id: &str,
        expected_review_digest: &str,
        approver: &str,
    ) -> Result<IngestionRecord, DomainError> {
        if approver.trim().is_empty() {
            return Err(DomainError::invalid("approver identity is required"));
        }
        let row = self.load_row(tenant_id, ingestion_id)?;
        if row.state != "awaiting_review"
            || row.review_digest.as_deref() != Some(expected_review_digest)
        {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                "review digest does not match the pending bundle",
            ));
        }
        let review_path = row
            .review_path
            .as_ref()
            .ok_or_else(|| DomainError::new(ErrorCode::Internal, "missing review path"))?;
        let review: ReviewBundle =
            serde_json::from_slice(&fs::read(review_path).map_err(io_error)?)
                .map_err(|error| DomainError::invalid(error.to_string()))?;
        if !review.conflicts.is_empty() {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                "review bundle contains unresolved structural conflicts",
            ));
        }
        self.repository
            .connection()?
            .execute(
                "UPDATE ingestions SET state = 'approved', approved_by = ?3, updated_at = ?4
                 WHERE tenant_id = ?1 AND ingestion_id = ?2",
                params![tenant_id, ingestion_id, approver, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        Ok(IngestionRecord {
            ingestion_id: ingestion_id.to_owned(),
            state: "approved".to_owned(),
            source_digest: row.source_digest,
            review_digest: row.review_digest,
            failure_reason: None,
            corpus_version: None,
        })
    }

    /// Builds the candidate fragments and calls `SqliteRepository::publish_ingestion`, which
    /// commits every fragment, the corpus version, and the ingestion's `published` transition in
    /// one `SQLite` transaction. Replaying this on an already-published ingestion returns its
    /// stored record before loading or rebuilding the review bundle (see ADR-007, issue #9).
    pub fn publish(
        &self,
        tenant_id: &str,
        ingestion_id: &str,
    ) -> Result<IngestionRecord, DomainError> {
        let row = self.load_row(tenant_id, ingestion_id)?;
        if row.state == "published" {
            if row.corpus_version.is_none() {
                return Err(DomainError::new(
                    ErrorCode::Internal,
                    "published ingestion is missing its corpus version",
                ));
            }
            return Ok(IngestionRecord {
                ingestion_id: ingestion_id.to_owned(),
                state: row.state,
                source_digest: row.source_digest,
                review_digest: row.review_digest,
                failure_reason: row.failure_reason,
                corpus_version: row.corpus_version,
            });
        }
        if row.state != "approved" {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                "only an approved ingestion can be published",
            ));
        }
        let review_path = row
            .review_path
            .as_ref()
            .ok_or_else(|| DomainError::new(ErrorCode::Internal, "missing review path"))?;
        let review: ReviewBundle =
            serde_json::from_slice(&fs::read(review_path).map_err(io_error)?)
                .map_err(|error| DomainError::invalid(error.to_string()))?;
        let corpus_digest = content_digest(
            "corpus-v1",
            &(
                &review.source_digest,
                &row.review_digest,
                &review.manifest,
                &review.units,
            ),
        )?;
        let corpus_version = derived_id("corpus_", &corpus_digest)?;
        let approver = row
            .approved_by
            .clone()
            .ok_or_else(|| DomainError::new(ErrorCode::Internal, "missing approver"))?;
        let approved_at = Utc::now();
        let mut fragments = Vec::with_capacity(review.units.len());
        for unit in review.units {
            let reference = compliatory_core::NormativeRef {
                standard_id: review.manifest.standard_id.clone(),
                edition: review.manifest.edition.clone(),
                amendments: review.manifest.amendments.clone(),
                language: review.manifest.language.clone(),
                locator: unit.locator,
                source_digest: review.source_digest.clone(),
                source_page: unit.source_page,
            };
            fragments.push(DocumentFragment::new(
                compliatory_core::ContentLayer::Normative,
                reference,
                Availability::Available,
                Some(unit.exact_text),
                None,
                Some(Approval {
                    corpus_version: corpus_version.clone(),
                    approved_at,
                    approved_by: approver.clone(),
                }),
                Some(unit.heading),
            )?);
        }
        let published_corpus_version = self.repository.publish_ingestion(
            tenant_id,
            ingestion_id,
            &corpus_version,
            &row.source_digest,
            &approver,
            &fragments,
        )?;
        Ok(IngestionRecord {
            ingestion_id: ingestion_id.to_owned(),
            state: "published".to_owned(),
            source_digest: row.source_digest,
            review_digest: row.review_digest,
            failure_reason: None,
            corpus_version: Some(published_corpus_version),
        })
    }

    pub fn record(
        &self,
        tenant_id: &str,
        ingestion_id: &str,
    ) -> Result<IngestionRecord, DomainError> {
        let row = self.load_row(tenant_id, ingestion_id)?;
        Ok(IngestionRecord {
            ingestion_id: ingestion_id.to_owned(),
            state: row.state,
            source_digest: row.source_digest,
            review_digest: row.review_digest,
            failure_reason: row.failure_reason,
            corpus_version: row.corpus_version,
        })
    }

    fn load_row(&self, tenant_id: &str, ingestion_id: &str) -> Result<StoredRow, DomainError> {
        self.repository
            .connection()?
            .query_row(
                "SELECT state, manifest_json, source_digest, source_path, review_digest,
                        review_path, failure_reason, approved_by, corpus_version
                 FROM ingestions WHERE tenant_id = ?1 AND ingestion_id = ?2",
                params![tenant_id, ingestion_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error)?
            .map(|row| {
                Ok(StoredRow {
                    state: row.0,
                    manifest: serde_json::from_str(&row.1)
                        .map_err(|error| DomainError::invalid(error.to_string()))?,
                    source_digest: row.2,
                    source_path: PathBuf::from(row.3),
                    review_digest: row.4,
                    review_path: row.5.map(PathBuf::from),
                    failure_reason: row.6,
                    approved_by: row.7,
                    corpus_version: row.8,
                })
            })
            .transpose()?
            .ok_or_else(|| DomainError::new(ErrorCode::UnknownReference, "ingestion not found"))
    }

    fn quarantine(
        &self,
        tenant_id: &str,
        ingestion_id: &str,
        reason: &str,
    ) -> Result<(), DomainError> {
        self.repository
            .connection()?
            .execute(
                "UPDATE ingestions SET state = 'quarantined', failure_reason = ?3, updated_at = ?4
                 WHERE tenant_id = ?1 AND ingestion_id = ?2",
                params![tenant_id, ingestion_id, reason, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

struct StoredRow {
    state: String,
    manifest: IngestionManifest,
    source_digest: String,
    source_path: PathBuf,
    review_digest: Option<String>,
    review_path: Option<PathBuf>,
    failure_reason: Option<String>,
    approved_by: Option<String>,
    corpus_version: Option<String>,
}

fn inspect_pdf(path: &Path) -> Result<(), DomainError> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    if metadata.len() == 0 || metadata.len() > MAX_PDF_BYTES {
        return Err(DomainError::new(
            ErrorCode::NotApproved,
            "PDF size is outside the accepted range",
        ));
    }
    let bytes = fs::read(path).map_err(io_error)?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(DomainError::new(
            ErrorCode::NotApproved,
            "source does not have a PDF signature",
        ));
    }
    if bytes
        .windows(b"/Encrypt".len())
        .any(|window| window == b"/Encrypt")
    {
        return Err(DomainError::new(
            ErrorCode::NotApproved,
            "encrypted PDFs remain in quarantine",
        ));
    }
    if !bytes
        .windows(b"%%EOF".len())
        .any(|window| window == b"%%EOF")
    {
        return Err(DomainError::new(
            ErrorCode::NotApproved,
            "PDF appears incomplete",
        ));
    }
    Ok(())
}

fn match_units(text: &str, expected: &[ExpectedUnit]) -> Result<Vec<ReviewedUnit>, DomainError> {
    let mut positions = Vec::with_capacity(expected.len());
    for unit in expected {
        let matches: Vec<_> = text.match_indices(&unit.heading).collect();
        if matches.len() != 1 {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                format!(
                    "heading {:?} matched {} times; manual remapping is required",
                    unit.heading,
                    matches.len()
                ),
            ));
        }
        positions.push((matches[0].0, unit));
    }
    positions.sort_by_key(|(position, _)| *position);
    let mut reviewed = Vec::with_capacity(positions.len());
    for (index, (start, unit)) in positions.iter().enumerate() {
        let end = positions
            .get(index + 1)
            .map_or(text.len(), |(position, _)| *position);
        let exact_text = text[*start..end].trim().to_owned();
        if exact_text.is_empty() {
            return Err(DomainError::new(
                ErrorCode::NotApproved,
                "matched structural unit is empty",
            ));
        }
        reviewed.push(ReviewedUnit {
            locator: unit.locator.clone(),
            heading: unit.heading.clone(),
            source_page: unit.source_page,
            exact_text,
            confidence_basis: "unique_exact_heading_match".to_owned(),
        });
    }
    Ok(reviewed)
}

fn db_error(error: rusqlite::Error) -> DomainError {
    DomainError::new(
        ErrorCode::Internal,
        format!("database operation failed: {error}"),
    )
}

fn io_error(error: std::io::Error) -> DomainError {
    DomainError::new(
        ErrorCode::Internal,
        format!("filesystem operation failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use compliatory_application::{AuthContext, RegulatoryRepository};
    use lopdf::{
        Document, Object, Stream,
        content::{Content, Operation},
        dictionary,
    };

    use super::*;

    #[test]
    fn encrypted_pdf_envelope_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("encrypted.pdf");
        fs::write(&source, b"%PDF-1.7\n/Encrypt\n%%EOF").unwrap();
        assert_eq!(
            inspect_pdf(&source).unwrap_err().code,
            ErrorCode::NotApproved
        );
    }

    #[test]
    fn unique_headings_create_atomic_units() {
        let expected = vec![
            ExpectedUnit {
                locator: Locator::clause("1"),
                heading: "Clause One".to_owned(),
                source_page: Some(1),
            },
            ExpectedUnit {
                locator: Locator::clause("2"),
                heading: "Clause Two".to_owned(),
                source_page: Some(2),
            },
        ];
        let units = match_units(
            "Front matter\nClause One\nSynthetic A\nClause Two\nSynthetic B",
            &expected,
        )
        .unwrap();
        assert_eq!(units.len(), 2);
        assert!(!units[0].exact_text.contains("Clause Two"));
    }

    #[test]
    fn text_pdf_requires_review_digest_before_immutable_publication() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.pdf");
        write_text_pdf(
            &source,
            "Clause One Synthetic first paragraph. Clause Two Synthetic second paragraph.",
        );
        let repository = SqliteRepository::open(directory.path().join("data")).unwrap();
        let service = IngestionService::new(&repository);
        let manifest = IngestionManifest {
            standard_id: "SYN-IMPORT".to_owned(),
            edition: "1".to_owned(),
            amendments: vec![],
            language: "en".to_owned(),
            usage_right: "Original synthetic test document".to_owned(),
            expected_units: vec![
                ExpectedUnit {
                    locator: Locator::clause("1"),
                    heading: "Clause One".to_owned(),
                    source_page: Some(1),
                },
                ExpectedUnit {
                    locator: Locator::clause("2"),
                    heading: "Clause Two".to_owned(),
                    source_page: Some(1),
                },
            ],
        };
        let ingested = service
            .ingest("tenant-a", &manifest, &source, &SyntheticFixtureScanner)
            .unwrap();
        let extracted = service
            .extract_worker("tenant-a", &ingested.ingestion_id)
            .unwrap();
        let wrong_digest = format!("sha256:{}", "0".repeat(64));
        assert_eq!(
            service
                .approve(
                    "tenant-a",
                    &ingested.ingestion_id,
                    &wrong_digest,
                    "reviewer"
                )
                .unwrap_err()
                .code,
            ErrorCode::NotApproved
        );
        service
            .approve(
                "tenant-a",
                &ingested.ingestion_id,
                extracted.review_digest.as_deref().unwrap(),
                "subject:reviewer",
            )
            .unwrap();
        let published = service.publish("tenant-a", &ingested.ingestion_id).unwrap();
        assert!(published.corpus_version.is_some());
        let review_path = service
            .load_row("tenant-a", &ingested.ingestion_id)
            .unwrap()
            .review_path
            .unwrap();
        fs::remove_file(review_path).unwrap();
        let replayed = service.publish("tenant-a", &ingested.ingestion_id).unwrap();
        assert_eq!(replayed.corpus_version, published.corpus_version);
        let auth = AuthContext::local_service("tenant-a", "test");
        let fragment = repository
            .fragment_by_reference(
                &auth,
                &compliatory_core::NormativeRef {
                    standard_id: "SYN-IMPORT".to_owned(),
                    edition: "1".to_owned(),
                    amendments: vec![],
                    language: "en".to_owned(),
                    locator: Locator::clause("1"),
                    source_digest: ingested.source_digest,
                    source_page: Some(1),
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(fragment.layer, compliatory_core::ContentLayer::Normative);
        assert!(fragment.approval.is_some());
    }

    fn write_text_pdf(path: &Path, text: &str) {
        let mut document = Document::with_version("1.5");
        let page_tree_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Td", vec![50.into(), 780.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap();
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let page_object_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => page_tree_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_object_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => page_tree_id,
        });
        document.trailer.set("Root", catalog_id);
        document.compress();
        document.save(path).unwrap();
    }
}
