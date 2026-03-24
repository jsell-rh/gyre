use crate::budget::BudgetConfig;
use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Enterprise/org boundary. Maps to a Keycloak realm or OIDC issuer.
/// Every user and workspace belongs to exactly one tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: Id,
    pub name: String,
    pub slug: String,               // URL-safe identifier
    pub oidc_issuer: Option<String>, // Keycloak realm URL
    pub budget: Option<BudgetConfig>,
    pub max_workspaces: Option<u32>,
    pub created_at: u64,
}

impl Tenant {
    pub fn new(
        id: Id,
        name: impl Into<String>,
        slug: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            slug: slug.into(),
            oidc_issuer: None,
            budget: None,
            max_workspaces: None,
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_new() {
        let t = Tenant::new(Id::new("t1"), "Acme Corp", "acme-corp", 1000);
        assert_eq!(t.name, "Acme Corp");
        assert_eq!(t.slug, "acme-corp");
        assert!(t.oidc_issuer.is_none());
        assert!(t.budget.is_none());
        assert!(t.max_workspaces.is_none());
    }
}
