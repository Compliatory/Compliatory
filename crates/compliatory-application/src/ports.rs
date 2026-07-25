use compliatory_core::{
    ContentLayer, DocumentFragment, DomainError, LocatorKind, NormativeProfile, NormativeRef,
    ProfileRef, Relation, WorkPacket,
};

use crate::AuthContext;

#[derive(Debug, Clone)]
pub struct RepositorySearch {
    pub query: String,
    pub standard_ids: Vec<String>,
    pub editions: Vec<String>,
    pub layers: Vec<ContentLayer>,
    pub locator_kinds: Vec<LocatorKind>,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct RelatedFragment {
    pub fragment: DocumentFragment,
    pub relation: Relation,
}

pub trait RegulatoryRepository: Send + Sync {
    fn search(
        &self,
        auth: &AuthContext,
        query: &RepositorySearch,
    ) -> Result<Vec<DocumentFragment>, DomainError>;

    fn packet_candidates(
        &self,
        auth: &AuthContext,
        profile: &ProfileRef,
        focus_refs: &[NormativeRef],
    ) -> Result<Vec<DocumentFragment>, DomainError>;

    fn related_fragments(
        &self,
        auth: &AuthContext,
        source_refs: &[NormativeRef],
        relation_types: &[String],
    ) -> Result<Vec<RelatedFragment>, DomainError>;

    fn fragment_by_id(
        &self,
        auth: &AuthContext,
        fragment_id: &str,
    ) -> Result<Option<DocumentFragment>, DomainError>;

    fn fragment_by_reference(
        &self,
        auth: &AuthContext,
        reference: &NormativeRef,
    ) -> Result<Option<DocumentFragment>, DomainError>;

    fn fragment_by_digest(
        &self,
        auth: &AuthContext,
        content_digest: &str,
    ) -> Result<Option<DocumentFragment>, DomainError>;

    fn profile(
        &self,
        auth: &AuthContext,
        profile: &ProfileRef,
    ) -> Result<Option<NormativeProfile>, DomainError>;

    fn save_packet(&self, auth: &AuthContext, packet: &WorkPacket) -> Result<(), DomainError>;

    fn packet(
        &self,
        auth: &AuthContext,
        packet_id: &str,
    ) -> Result<Option<WorkPacket>, DomainError>;

    fn current_corpus_versions(&self, auth: &AuthContext) -> Result<Vec<String>, DomainError>;
}
