#![forbid(unsafe_code)]

//! `SQLite` and tenant-partitioned filesystem adapter.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use chrono::Utc;
use compliatory_application::{
    AuthContext, Permission, RegulatoryRepository, RelatedFragment, RepositorySearch,
};
use compliatory_core::{
    ContentLayer, DocumentFragment, DomainError, ErrorCode, NormativeProfile, NormativeRef,
    ProfileRef, Relation, WorkPacket,
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

const MIGRATION: &str = r"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS tenants (
    tenant_id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS corpus_versions (
    tenant_id TEXT NOT NULL,
    corpus_version TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('quarantine', 'approved', 'published', 'superseded')),
    created_at TEXT NOT NULL,
    approved_by TEXT,
    PRIMARY KEY (tenant_id, corpus_version),
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id)
);

CREATE TABLE IF NOT EXISTS fragments (
    tenant_id TEXT NOT NULL,
    fragment_id TEXT NOT NULL,
    standard_id TEXT NOT NULL,
    edition TEXT NOT NULL,
    language TEXT NOT NULL,
    locator_key TEXT NOT NULL,
    locator_kind TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    layer TEXT NOT NULL,
    corpus_version TEXT,
    title TEXT,
    body_for_search TEXT NOT NULL,
    content_json TEXT NOT NULL,
    PRIMARY KEY (tenant_id, fragment_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id)
);

CREATE INDEX IF NOT EXISTS fragments_reference_idx
ON fragments(tenant_id, standard_id, edition, language, locator_key, layer);

CREATE VIRTUAL TABLE IF NOT EXISTS fragments_fts USING fts5(
    tenant_id UNINDEXED,
    fragment_id UNINDEXED,
    title,
    body
);

CREATE TABLE IF NOT EXISTS profiles (
    tenant_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    version TEXT NOT NULL,
    content_json TEXT NOT NULL,
    PRIMARY KEY (tenant_id, profile_id, version),
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id)
);

CREATE TABLE IF NOT EXISTS relations (
    tenant_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    source_fragment_id TEXT NOT NULL,
    target_fragment_id TEXT NOT NULL,
    PRIMARY KEY (tenant_id, relation_type, source_fragment_id, target_fragment_id),
    FOREIGN KEY (tenant_id, source_fragment_id) REFERENCES fragments(tenant_id, fragment_id),
    FOREIGN KEY (tenant_id, target_fragment_id) REFERENCES fragments(tenant_id, fragment_id)
);

CREATE TABLE IF NOT EXISTS packets (
    tenant_id TEXT NOT NULL,
    packet_id TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    content_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, packet_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id)
);

CREATE TABLE IF NOT EXISTS access_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant_id TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    action TEXT NOT NULL,
    object_id TEXT,
    decision TEXT NOT NULL,
    duration_ms INTEGER,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ingestions (
    tenant_id TEXT NOT NULL,
    ingestion_id TEXT NOT NULL,
    state TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    source_digest TEXT NOT NULL,
    source_path TEXT NOT NULL,
    review_digest TEXT,
    review_path TEXT,
    extracted_text_path TEXT,
    failure_reason TEXT,
    approved_by TEXT,
    corpus_version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, ingestion_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id)
);
";

pub struct SqliteRepository {
    connection: Mutex<Connection>,
    data_dir: PathBuf,
}

impl SqliteRepository {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, DomainError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(data_dir.join("tenants")).map_err(io_error)?;
        let connection = Connection::open(data_dir.join("compliatory.db")).map_err(db_error)?;
        connection.execute_batch(MIGRATION).map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            data_dir,
        })
    }

    pub fn open_in_memory() -> Result<Self, DomainError> {
        let connection = Connection::open_in_memory().map_err(db_error)?;
        connection.execute_batch(MIGRATION).map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            data_dir: PathBuf::from(":memory:"),
        })
    }

    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn ensure_tenant(&self, tenant_id: &str) -> Result<(), DomainError> {
        validate_identifier(tenant_id)?;
        self.connection()?
            .execute(
                "INSERT OR IGNORE INTO tenants(tenant_id, created_at) VALUES (?1, ?2)",
                params![tenant_id, Utc::now().to_rfc3339()],
            )
            .map_err(db_error)?;
        if self.data_dir != Path::new(":memory:") {
            fs::create_dir_all(self.tenant_dir(tenant_id)?).map_err(io_error)?;
        }
        Ok(())
    }

    pub fn insert_fragment(
        &self,
        tenant_id: &str,
        fragment: &DocumentFragment,
    ) -> Result<(), DomainError> {
        self.ensure_tenant(tenant_id)?;
        let json = serde_json::to_string(fragment)
            .map_err(|error| DomainError::invalid(error.to_string()))?;
        let corpus_version = fragment
            .approval
            .as_ref()
            .map(|approval| approval.corpus_version.as_str());
        let content = fragment
            .exact_text
            .as_deref()
            .or(fragment.explanatory_text.as_deref())
            .or(fragment.title.as_deref())
            .unwrap_or_default();
        let searchable = format!(
            "{} {} {}",
            fragment.reference.standard_id,
            fragment.reference.locator.display_key(),
            content
        );
        let connection = self.connection()?;
        let transaction = connection.unchecked_transaction().map_err(db_error)?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO fragments(
                    tenant_id, fragment_id, standard_id, edition, language, locator_key,
                    locator_kind, source_digest, layer, corpus_version, title, body_for_search,
                    content_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    tenant_id,
                    fragment.fragment_id,
                    fragment.reference.standard_id,
                    fragment.reference.edition,
                    fragment.reference.language,
                    fragment.reference.locator.display_key(),
                    serde_json::to_string(&fragment.reference.locator.kind)
                        .map_err(|error| DomainError::invalid(error.to_string()))?
                        .trim_matches('"'),
                    fragment.reference.source_digest,
                    layer_name(fragment.layer),
                    corpus_version,
                    fragment.title,
                    &searchable,
                    json,
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM fragments_fts WHERE tenant_id = ?1 AND fragment_id = ?2",
                params![tenant_id, fragment.fragment_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO fragments_fts(tenant_id, fragment_id, title, body)
                 VALUES (?1, ?2, ?3, ?4)",
                params![tenant_id, fragment.fragment_id, fragment.title, &searchable],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    pub fn insert_profile(
        &self,
        tenant_id: &str,
        profile: &NormativeProfile,
    ) -> Result<(), DomainError> {
        self.ensure_tenant(tenant_id)?;
        let json = serde_json::to_string(profile)
            .map_err(|error| DomainError::invalid(error.to_string()))?;
        self.connection()?
            .execute(
                "INSERT OR REPLACE INTO profiles(tenant_id, profile_id, version, content_json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    tenant_id,
                    profile.profile.profile_id,
                    profile.profile.version,
                    json
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn insert_relation(
        &self,
        tenant_id: &str,
        relation_type: &str,
        source_fragment_id: &str,
        target_fragment_id: &str,
    ) -> Result<(), DomainError> {
        self.connection()?
            .execute(
                "INSERT OR REPLACE INTO relations(
                    tenant_id, relation_type, source_fragment_id, target_fragment_id
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    tenant_id,
                    relation_type,
                    source_fragment_id,
                    target_fragment_id
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn publish_corpus(
        &self,
        tenant_id: &str,
        corpus_version: &str,
        source_digest: &str,
        approved_by: &str,
    ) -> Result<(), DomainError> {
        self.ensure_tenant(tenant_id)?;
        self.connection()?
            .execute(
                "INSERT OR REPLACE INTO corpus_versions(
                    tenant_id, corpus_version, source_digest, status, created_at, approved_by
                 ) VALUES (?1, ?2, ?3, 'published', ?4, ?5)",
                params![
                    tenant_id,
                    corpus_version,
                    source_digest,
                    Utc::now().to_rfc3339(),
                    approved_by
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn tenant_dir(&self, tenant_id: &str) -> Result<PathBuf, DomainError> {
        validate_identifier(tenant_id)?;
        Ok(self.data_dir.join("tenants").join(tenant_id))
    }

    pub fn sha256_file(path: &Path) -> Result<String, DomainError> {
        let bytes = fs::read(path).map_err(io_error)?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }

    pub fn connection(&self) -> Result<MutexGuard<'_, Connection>, DomainError> {
        self.connection
            .lock()
            .map_err(|_| DomainError::new(ErrorCode::Internal, "database lock poisoned"))
    }

    fn allowed_layers(auth: &AuthContext) -> Vec<&'static str> {
        let mut layers = Vec::new();
        if auth.permissions.contains(&Permission::CatalogRead) {
            layers.push("catalog");
        }
        if auth.permissions.contains(&Permission::GuidanceRead) {
            layers.push("guidance");
        }
        if auth.permissions.contains(&Permission::NormativeRead) {
            layers.push("normative");
        }
        // Tenant notes are internal evidence and follow catalog read in the local slice.
        if auth.permissions.contains(&Permission::CatalogRead) {
            layers.push("tenant");
        }
        layers
    }

    fn load_fragment_rows(&self, auth: &AuthContext) -> Result<Vec<DocumentFragment>, DomainError> {
        let allowed = Self::allowed_layers(auth);
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT layer, content_json FROM fragments WHERE tenant_id = ?1")
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![auth.tenant_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        let mut fragments = Vec::new();
        for row in rows {
            let (layer, json) = row.map_err(db_error)?;
            if allowed.contains(&layer.as_str()) {
                fragments.push(
                    serde_json::from_str(&json)
                        .map_err(|error| DomainError::invalid(error.to_string()))?,
                );
            }
        }
        Ok(fragments)
    }

    fn audit(&self, auth: &AuthContext, action: &str, object_id: Option<&str>, decision: &str) {
        if let Ok(connection) = self.connection() {
            let _ = connection.execute(
                "INSERT INTO access_audit(
                    tenant_id, subject_id, action, object_id, decision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    auth.tenant_id,
                    auth.subject_id,
                    action,
                    object_id,
                    decision,
                    Utc::now().to_rfc3339()
                ],
            );
        }
    }
}

impl RegulatoryRepository for SqliteRepository {
    fn search(
        &self,
        auth: &AuthContext,
        query: &RepositorySearch,
    ) -> Result<Vec<DocumentFragment>, DomainError> {
        let fts_query = format!("\"{}\"", query.query.replace('"', "\"\""));
        let matching_ids = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare(
                    "SELECT fragment_id FROM fragments_fts
                     WHERE tenant_id = ?1 AND fragments_fts MATCH ?2
                     ORDER BY bm25(fragments_fts), fragment_id",
                )
                .map_err(db_error)?;
            let rows = statement
                .query_map(params![auth.tenant_id, fts_query], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| {
                    DomainError::invalid(format!("invalid full-text search query: {error}"))
                })?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row.map_err(db_error)?);
            }
            ids
        };
        let mut fragments = self.load_fragment_rows(auth)?;
        fragments.retain(|fragment| {
            matching_ids.contains(&fragment.fragment_id)
                && (query.standard_ids.is_empty()
                    || query.standard_ids.contains(&fragment.reference.standard_id))
                && (query.editions.is_empty()
                    || query.editions.contains(&fragment.reference.edition))
                && (query.layers.is_empty() || query.layers.contains(&fragment.layer))
                && (query.locator_kinds.is_empty()
                    || query
                        .locator_kinds
                        .contains(&fragment.reference.locator.kind))
        });
        fragments.sort_by(|left, right| {
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
        });
        let offset = usize::try_from(query.offset).unwrap_or(usize::MAX);
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let result = fragments.into_iter().skip(offset).take(limit).collect();
        self.audit(auth, "search", None, "allowed");
        Ok(result)
    }

    fn packet_candidates(
        &self,
        auth: &AuthContext,
        profile: &ProfileRef,
        focus_refs: &[NormativeRef],
    ) -> Result<Vec<DocumentFragment>, DomainError> {
        let profile = self
            .profile(auth, profile)?
            .ok_or_else(|| DomainError::new(ErrorCode::UnknownReference, "profile not found"))?;
        let references: Vec<_> = if focus_refs.is_empty() {
            profile
                .criteria
                .iter()
                .flat_map(|criterion| criterion.references.clone())
                .collect()
        } else {
            focus_refs.to_vec()
        };
        let fragments = self
            .load_fragment_rows(auth)?
            .into_iter()
            .filter(|fragment| {
                references
                    .iter()
                    .any(|reference| same_structural_reference(&fragment.reference, reference))
            })
            .collect();
        self.audit(auth, "packet_candidates", None, "allowed");
        Ok(fragments)
    }

    fn related_fragments(
        &self,
        auth: &AuthContext,
        source_refs: &[NormativeRef],
        relation_types: &[String],
    ) -> Result<Vec<RelatedFragment>, DomainError> {
        let source_ids: Vec<_> = self
            .load_fragment_rows(auth)?
            .into_iter()
            .filter(|fragment| {
                source_refs
                    .iter()
                    .any(|reference| same_structural_reference(&fragment.reference, reference))
            })
            .map(|fragment| fragment.fragment_id)
            .collect();
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT relation_type, source_fragment_id, target_fragment_id
                 FROM relations WHERE tenant_id = ?1",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![auth.tenant_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(db_error)?;
        let mut matches = Vec::new();
        for row in rows {
            let (relation_type, source, target) = row.map_err(db_error)?;
            if source_ids.contains(&source)
                && (relation_types.is_empty() || relation_types.contains(&relation_type))
            {
                matches.push((relation_type, source, target));
            }
        }
        drop(statement);
        drop(connection);
        let available = self.load_fragment_rows(auth)?;
        Ok(matches
            .into_iter()
            .filter_map(|(relation_type, source, target)| {
                available
                    .iter()
                    .find(|fragment| fragment.fragment_id == target)
                    .cloned()
                    .map(|fragment| RelatedFragment {
                        fragment,
                        relation: Relation {
                            relation_type,
                            source_fragment_id: source,
                            target_fragment_id: target,
                        },
                    })
            })
            .collect())
    }

    fn fragment_by_id(
        &self,
        auth: &AuthContext,
        fragment_id: &str,
    ) -> Result<Option<DocumentFragment>, DomainError> {
        let result = self
            .load_fragment_rows(auth)?
            .into_iter()
            .find(|fragment| fragment.fragment_id == fragment_id);
        self.audit(
            auth,
            "fragment_read",
            Some(fragment_id),
            if result.is_some() {
                "allowed"
            } else {
                "not_found"
            },
        );
        Ok(result)
    }

    fn fragment_by_reference(
        &self,
        auth: &AuthContext,
        reference: &NormativeRef,
    ) -> Result<Option<DocumentFragment>, DomainError> {
        let mut matches: Vec<_> = self
            .load_fragment_rows(auth)?
            .into_iter()
            .filter(|fragment| same_structural_reference(&fragment.reference, reference))
            .filter(|fragment| {
                reference.source_digest.is_empty()
                    || fragment.reference.source_digest == reference.source_digest
            })
            .collect();
        matches.sort_by_key(|fragment| match fragment.layer {
            ContentLayer::Normative => 0,
            ContentLayer::Guidance => 1,
            ContentLayer::Catalog => 2,
            ContentLayer::Tenant => 3,
        });
        Ok(matches.into_iter().next())
    }

    fn fragment_by_digest(
        &self,
        auth: &AuthContext,
        content_digest: &str,
    ) -> Result<Option<DocumentFragment>, DomainError> {
        Ok(self
            .load_fragment_rows(auth)?
            .into_iter()
            .find(|fragment| fragment.content_digest == content_digest))
    }

    fn profile(
        &self,
        auth: &AuthContext,
        profile: &ProfileRef,
    ) -> Result<Option<NormativeProfile>, DomainError> {
        let json: Option<String> = self
            .connection()?
            .query_row(
                "SELECT content_json FROM profiles
                 WHERE tenant_id = ?1 AND profile_id = ?2 AND version = ?3",
                params![auth.tenant_id, profile.profile_id, profile.version],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        json.map(|value| {
            serde_json::from_str(&value).map_err(|error| DomainError::invalid(error.to_string()))
        })
        .transpose()
    }

    fn save_packet(&self, auth: &AuthContext, packet: &WorkPacket) -> Result<(), DomainError> {
        let json = serde_json::to_string(packet)
            .map_err(|error| DomainError::invalid(error.to_string()))?;
        self.connection()?
            .execute(
                "INSERT OR IGNORE INTO packets(
                    tenant_id, packet_id, content_digest, content_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    auth.tenant_id,
                    packet.packet_id,
                    packet.content_digest,
                    json,
                    packet.created_at.to_rfc3339()
                ],
            )
            .map_err(db_error)?;
        self.audit(auth, "packet_write", Some(&packet.packet_id), "allowed");
        Ok(())
    }

    fn packet(
        &self,
        auth: &AuthContext,
        packet_id: &str,
    ) -> Result<Option<WorkPacket>, DomainError> {
        let json: Option<String> = self
            .connection()?
            .query_row(
                "SELECT content_json FROM packets WHERE tenant_id = ?1 AND packet_id = ?2",
                params![auth.tenant_id, packet_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let result = json
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| DomainError::invalid(error.to_string()))
            })
            .transpose()?;
        self.audit(
            auth,
            "packet_read",
            Some(packet_id),
            if result.is_some() {
                "allowed"
            } else {
                "not_found"
            },
        );
        Ok(result)
    }

    fn current_corpus_versions(&self, auth: &AuthContext) -> Result<Vec<String>, DomainError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT corpus_version FROM corpus_versions
                 WHERE tenant_id = ?1 AND status = 'published' ORDER BY corpus_version",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![auth.tenant_id], |row| row.get(0))
            .map_err(db_error)?;
        let mut versions = Vec::new();
        for row in rows {
            versions.push(row.map_err(db_error)?);
        }
        Ok(versions)
    }
}

fn same_structural_reference(left: &NormativeRef, right: &NormativeRef) -> bool {
    // Discovery and packet expansion may intentionally omit amendments to search every known
    // amendment of an edition. Citation validation uses exact `NormativeRef` equality instead;
    // do not reuse this helper for citation or approval decisions.
    left.standard_id == right.standard_id
        && left.edition == right.edition
        && (right.amendments.is_empty() || left.amendments == right.amendments)
        && left.language == right.language
        && left.locator == right.locator
}

fn layer_name(layer: ContentLayer) -> &'static str {
    match layer {
        ContentLayer::Catalog => "catalog",
        ContentLayer::Guidance => "guidance",
        ContentLayer::Normative => "normative",
        ContentLayer::Tenant => "tenant",
    }
}

fn validate_identifier(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(DomainError::invalid("invalid internal identifier"));
    }
    Ok(())
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
    use compliatory_core::{
        Approval, Availability, ContentLayer, DocumentFragment, Locator, NormativeRef,
    };

    use super::*;

    fn fragment(corpus: &str) -> DocumentFragment {
        DocumentFragment::new(
            ContentLayer::Normative,
            NormativeRef {
                standard_id: "SYN-TEST".to_owned(),
                edition: "1".to_owned(),
                amendments: vec![],
                language: "en".to_owned(),
                locator: Locator::clause("1"),
                source_digest: format!("sha256:{}", "1".repeat(64)),
                source_page: Some(1),
            },
            Availability::Available,
            Some("Synthetic requirement.".to_owned()),
            None,
            Some(Approval {
                corpus_version: corpus.to_owned(),
                approved_at: Utc::now(),
                approved_by: "reviewer".to_owned(),
            }),
            Some("Synthetic clause".to_owned()),
        )
        .unwrap()
    }

    #[test]
    fn tenant_isolation_applies_to_direct_fragment_reads() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        repository.ensure_tenant("tenant-a").unwrap();
        repository.ensure_tenant("tenant-b").unwrap();
        let fragment = fragment("corpus_a");
        repository.insert_fragment("tenant-a", &fragment).unwrap();

        let auth_a = AuthContext::local_service("tenant-a", "agent");
        let auth_b = AuthContext::local_service("tenant-b", "agent");
        assert!(
            repository
                .fragment_by_id(&auth_a, &fragment.fragment_id)
                .unwrap()
                .is_some()
        );
        assert!(
            repository
                .fragment_by_id(&auth_b, &fragment.fragment_id)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn non_normative_permission_hides_normative_fragment() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let fragment = fragment("corpus_a");
        repository.insert_fragment("tenant-a", &fragment).unwrap();
        let mut auth = AuthContext::local_service("tenant-a", "agent");
        auth.permissions.remove(&Permission::NormativeRead);
        assert!(
            repository
                .fragment_by_id(&auth, &fragment.fragment_id)
                .unwrap()
                .is_none()
        );
    }
}
