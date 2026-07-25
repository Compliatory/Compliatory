use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use compliatory_core::{DomainError, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    CatalogRead,
    GuidanceRead,
    NormativeRead,
    PacketBuild,
    CitationValidate,
    CoverageCheck,
    AuditRead,
}

impl Permission {
    #[must_use]
    pub const fn scope(self) -> &'static str {
        match self {
            Self::CatalogRead => "catalog:read",
            Self::GuidanceRead => "guidance:read",
            Self::NormativeRead => "normative:read",
            Self::PacketBuild => "packet:build",
            Self::CitationValidate => "citation:validate",
            Self::CoverageCheck => "coverage:check",
            Self::AuditRead => "audit:read",
        }
    }

    #[must_use]
    pub fn from_scope(scope: &str) -> Option<Self> {
        match scope {
            "catalog:read" => Some(Self::CatalogRead),
            "guidance:read" => Some(Self::GuidanceRead),
            "normative:read" => Some(Self::NormativeRead),
            "packet:build" => Some(Self::PacketBuild),
            "citation:validate" => Some(Self::CitationValidate),
            "coverage:check" => Some(Self::CoverageCheck),
            "audit:read" => Some(Self::AuditRead),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuthContext {
    pub tenant_id: String,
    pub subject_id: String,
    pub permissions: BTreeSet<Permission>,
}

/// Claims already authenticated by a future HTTP/JWT adapter.
///
/// The application layer validates the security-sensitive audience, expiry, tenant and scopes
/// independently of the token format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccessTokenClaims {
    pub subject_id: String,
    pub tenant_id: String,
    pub audience: String,
    pub scopes: BTreeSet<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpAuthPolicy {
    canonical_resource: String,
}

impl HttpAuthPolicy {
    #[must_use]
    pub fn new(canonical_resource: impl Into<String>) -> Self {
        Self {
            canonical_resource: canonical_resource.into(),
        }
    }

    pub fn validate_claims(
        &self,
        claims: &AccessTokenClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthContext, DomainError> {
        if claims.audience != self.canonical_resource {
            return Err(DomainError::new(
                ErrorCode::NotEntitled,
                "access token audience mismatch",
            ));
        }
        if claims.expires_at <= now {
            return Err(DomainError::new(
                ErrorCode::NotEntitled,
                "access token expired",
            ));
        }
        if claims.tenant_id.is_empty() || claims.subject_id.is_empty() {
            return Err(DomainError::new(
                ErrorCode::NotEntitled,
                "token subject and tenant are required",
            ));
        }
        let permissions = claims
            .scopes
            .iter()
            .filter_map(|scope| Permission::from_scope(scope))
            .collect();
        Ok(AuthContext {
            tenant_id: claims.tenant_id.clone(),
            subject_id: claims.subject_id.clone(),
            permissions,
        })
    }
}

impl AuthContext {
    pub fn require(&self, permission: Permission) -> Result<(), DomainError> {
        if self.permissions.contains(&permission) {
            Ok(())
        } else {
            Err(DomainError::new(
                ErrorCode::NotEntitled,
                format!("missing permission {}", permission.scope()),
            ))
        }
    }

    #[must_use]
    pub fn rights_digest_input(&self) -> BTreeSet<String> {
        self.permissions
            .iter()
            .map(|permission| permission.scope().to_owned())
            .collect()
    }

    #[must_use]
    pub fn local_service(tenant_id: impl Into<String>, subject_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            subject_id: subject_id.into(),
            permissions: [
                Permission::CatalogRead,
                Permission::GuidanceRead,
                Permission::NormativeRead,
                Permission::PacketBuild,
                Permission::CitationValidate,
                Permission::CoverageCheck,
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn claims() -> AccessTokenClaims {
        AccessTokenClaims {
            subject_id: "subject:agent".to_owned(),
            tenant_id: "tenant-a".to_owned(),
            audience: "https://reg.example.test/mcp".to_owned(),
            scopes: ["catalog:read".to_owned(), "packet:build".to_owned()]
                .into_iter()
                .collect(),
            expires_at: Utc::now() + Duration::minutes(5),
        }
    }

    #[test]
    fn http_policy_derives_tenant_and_permissions_from_claims() {
        let context = HttpAuthPolicy::new("https://reg.example.test/mcp")
            .validate_claims(&claims(), Utc::now())
            .unwrap();
        assert_eq!(context.tenant_id, "tenant-a");
        assert!(context.permissions.contains(&Permission::PacketBuild));
    }

    #[test]
    fn http_policy_rejects_wrong_audience() {
        let error = HttpAuthPolicy::new("https://other.example.test/mcp")
            .validate_claims(&claims(), Utc::now())
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotEntitled);
    }
}
