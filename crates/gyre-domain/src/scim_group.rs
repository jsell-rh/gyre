//! SCIM Group provisioning entities (identity-security.md §SCIM
//! Provisioning / §Group-to-Workspace Mapping).
//!
//! A [`ScimGroup`] is an IdP-pushed RFC 7643 §4.2 Group resource; its
//! membership (`ScimGroupMember`) drives Gyre `WorkspaceMembership`
//! records through the per-workspace admin-configured
//! [`ScimWorkspaceMapping`]s. Unmapped groups have no membership effect.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

use crate::workspace_membership::WorkspaceRole;

/// A SCIM Group resource (RFC 7643 §4.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScimGroup {
    pub id: Id,
    /// Immutable IdP join key (RFC 7644 externalId). Empty when the IdP
    /// pushes no externalId — never fabricated.
    pub external_id: String,
    /// Human-readable group name (RFC 7643 displayName). Also usable as a
    /// mapping join key for IdPs that push no externalId.
    pub display_name: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ScimGroup {
    pub fn new(id: Id, external_id: String, display_name: String, now: u64) -> Self {
        Self {
            id,
            external_id,
            display_name,
            created_at: now,
            updated_at: now,
        }
    }

    /// The [`ScimWorkspaceMapping::idp_group`] keys this group matches:
    /// the externalId first (the opaque IdP identifier the mapping
    /// example in identity-security.md uses — a group DN), then the
    /// displayName (for IdPs that push no externalId). Order matters for
    /// diagnostics only — the effect application consults every key.
    pub fn mapping_keys(&self) -> Vec<String> {
        let mut keys = Vec::with_capacity(2);
        if !self.external_id.is_empty() {
            keys.push(self.external_id.clone());
        }
        keys.push(self.display_name.clone());
        keys
    }
}

/// Membership of one user in one SCIM group. Uniquely keyed by
/// `(group_id, user_id)` — re-pushing the same member must not duplicate
/// the row (task-266 idempotency requirement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScimGroupMember {
    pub group_id: Id,
    pub user_id: Id,
    pub added_at: u64,
    /// The IdP-pushed RFC 7643 §2.4 `display` sub-attribute of the member
    /// reference (the human-readable name the IdP holds for the member).
    /// Persisted so group reads round-trip exactly what the IdP pushed;
    /// `None` when the push carried no `display` — the API layer then
    /// derives it from the member user's display name (best effort).
    /// Re-pushing an existing member refreshes it (upsert), never
    /// duplicates the row.
    pub display: Option<String>,
}

/// Admin-configured IdP-group → workspace-role mapping
/// (identity-security.md §Group-to-Workspace Mapping). Keyed by
/// `(workspace_id, idp_group)`; `replace_for_workspace` swaps the whole
/// per-workspace set atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScimWorkspaceMapping {
    pub workspace_id: Id,
    /// The IdP group identifier: matched against a pushed group's
    /// externalId, then displayName (see [`ScimGroup::mapping_keys`]).
    pub idp_group: String,
    pub workspace_role: WorkspaceRole,
    pub updated_at: u64,
}

impl ScimWorkspaceMapping {
    pub fn new(
        workspace_id: Id,
        idp_group: String,
        workspace_role: WorkspaceRole,
        now: u64,
    ) -> Self {
        Self {
            workspace_id,
            idp_group,
            workspace_role,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_keys_prefer_external_id_and_fall_back_to_display_name() {
        let with_ext = ScimGroup::new(
            Id::new("g1"),
            "cn=platform-team,ou=groups,dc=example,dc=com".to_string(),
            "Platform Team".to_string(),
            1000,
        );
        assert_eq!(
            with_ext.mapping_keys(),
            vec![
                "cn=platform-team,ou=groups,dc=example,dc=com".to_string(),
                "Platform Team".to_string()
            ]
        );

        let no_ext = ScimGroup::new(Id::new("g2"), String::new(), "Leads".to_string(), 1000);
        assert_eq!(no_ext.mapping_keys(), vec!["Leads".to_string()]);
    }
}
