//! Secret domain types (platform-model.md §7 Secrets Delivery).
//!
//! A secret is a named credential value scoped to a tenant, workspace, repo,
//! or task. The plaintext value NEVER appears in this type (or anywhere else
//! outside the adapter layer): values flow only as method parameters and
//! return values of `SecretRepository` (see gyre-ports).

use crate::Id;
use serde::{Deserialize, Serialize};

/// The scope level a secret is attached to (platform-model.md §7).
///
/// Secrets cascade like budgets: an agent at repo scope receives tenant +
/// workspace + repo secrets (but not secrets from other repos); a task
/// additionally receives its own task-scoped secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecretScope {
    /// Shared across all workspaces in the tenant.
    Tenant,
    /// Shared across all repos in the workspace.
    Workspace,
    /// Specific to one repo.
    Repo,
    /// One-time, per-task.
    Task,
}

impl SecretScope {
    /// Stable string form used in DB columns and API surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretScope::Tenant => "tenant",
            SecretScope::Workspace => "workspace",
            SecretScope::Repo => "repo",
            SecretScope::Task => "task",
        }
    }

    /// Parse from the stable string form produced by [`SecretScope::as_str`].
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "tenant" => Some(SecretScope::Tenant),
            "workspace" => Some(SecretScope::Workspace),
            "repo" => Some(SecretScope::Repo),
            "task" => Some(SecretScope::Task),
            _ => None,
        }
    }
}

/// The lifecycle class of a secret (platform-model.md §7 Secret Types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecretType {
    /// Set by admin, persisted (encrypted at rest). E.g. `DATABASE_URL`.
    Static,
    /// Generated at spawn, revoked at teardown. E.g. per-session DB credentials.
    Ephemeral,
    /// Background job refreshes before expiry. E.g. OAuth refresh tokens.
    Rotated,
    /// Generated from identity, scoped to session. E.g. agent OIDC token.
    Derived,
}

impl SecretType {
    /// Stable string form used in DB columns and API surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretType::Static => "static",
            SecretType::Ephemeral => "ephemeral",
            SecretType::Rotated => "rotated",
            SecretType::Derived => "derived",
        }
    }

    /// Parse from the stable string form produced by [`SecretType::as_str`].
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "static" => Some(SecretType::Static),
            "ephemeral" => Some(SecretType::Ephemeral),
            "rotated" => Some(SecretType::Rotated),
            "derived" => Some(SecretType::Derived),
            _ => None,
        }
    }
}

/// A named secret attached to a scope (platform-model.md §7).
///
/// Metadata only: the encrypted value lives in the adapter's storage; the
/// plaintext value exists only transiently as a `SecretRepository` method
/// argument or return value. It must never be logged or serialized to JSON
/// with a value attached.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Secret {
    pub id: Id,
    /// Secret name, e.g. `DATABASE_URL`. Unique within a scope.
    pub name: String,
    /// The scope level this secret is attached to.
    pub scope: SecretScope,
    /// tenant_id, workspace_id, repo_id, or task_id depending on `scope`.
    pub scope_id: String,
    /// Lifecycle class of the secret.
    pub secret_type: SecretType,
    /// Identity that created the secret (user or agent id).
    pub created_by: String,
    /// When this secret was created (Unix epoch seconds).
    pub created_at: u64,
    /// When this secret expires (Unix epoch seconds). None = never.
    pub expires_at: Option<u64>,
    /// When this secret was last rotated (Unix epoch seconds). None = never.
    pub last_rotated_at: Option<u64>,
    /// Owning tenant for multi-tenant isolation.
    pub tenant_id: String,
}

impl Secret {
    /// Returns true if this secret has expired relative to the given timestamp.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.map(|e| now >= e).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_secret() -> Secret {
        Secret {
            id: Id::new("secret-1"),
            name: "DATABASE_URL".to_string(),
            scope: SecretScope::Repo,
            scope_id: "repo-1".to_string(),
            secret_type: SecretType::Static,
            created_by: "user:admin".to_string(),
            created_at: 1_700_000_000,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: "t1".to_string(),
        }
    }

    #[test]
    fn secret_scope_str_roundtrip() {
        for scope in [
            SecretScope::Tenant,
            SecretScope::Workspace,
            SecretScope::Repo,
            SecretScope::Task,
        ] {
            assert_eq!(SecretScope::from_str_opt(scope.as_str()), Some(scope));
        }
        assert_eq!(SecretScope::from_str_opt("bogus"), None);
    }

    #[test]
    fn secret_type_str_roundtrip() {
        for st in [
            SecretType::Static,
            SecretType::Ephemeral,
            SecretType::Rotated,
            SecretType::Derived,
        ] {
            assert_eq!(SecretType::from_str_opt(st.as_str()), Some(st));
        }
        assert_eq!(SecretType::from_str_opt("bogus"), None);
    }

    #[test]
    fn is_expired_with_no_expiry_is_false() {
        let secret = sample_secret();
        assert!(!secret.is_expired(u64::MAX));
    }

    #[test]
    fn is_expired_at_and_after_expiry() {
        let mut secret = sample_secret();
        secret.expires_at = Some(1_700_000_100);
        assert!(secret.is_expired(1_700_000_100));
        assert!(secret.is_expired(1_700_000_200));
        assert!(!secret.is_expired(1_700_000_099));
    }

    #[test]
    fn secret_json_has_no_value_field() {
        // The Secret type must never carry a value: serialize and confirm no
        // value-bearing key exists, so accidental field additions fail here.
        let json = serde_json::to_string(&sample_secret()).unwrap();
        assert!(!json.contains("value"));
        assert!(!json.contains("encrypted"));
        assert!(json.contains("DATABASE_URL"));
    }
}
