use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Where an agent runs — determines how Gyre spawns and manages process lifetime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComputeTargetType {
    /// OCI container runtime (Docker, Podman, etc.).
    Container,
    /// Remote host via SSH.
    Ssh,
    /// Kubernetes cluster.
    Kubernetes,
}

impl std::fmt::Display for ComputeTargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComputeTargetType::Container => write!(f, "Container"),
            ComputeTargetType::Ssh => write!(f, "Ssh"),
            ComputeTargetType::Kubernetes => write!(f, "Kubernetes"),
        }
    }
}

impl ComputeTargetType {
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "Container" => Some(ComputeTargetType::Container),
            "Ssh" => Some(ComputeTargetType::Ssh),
            "Kubernetes" => Some(ComputeTargetType::Kubernetes),
            _ => None,
        }
    }
}

/// Named compute target belonging to a tenant.
///
/// A workspace can reference a compute target via `compute_target_id`
/// to direct agent spawning to a specific infrastructure target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTargetEntity {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub target_type: ComputeTargetType,
    /// Target-specific configuration blob (image, host, user, kubeconfig, etc.).
    pub config: serde_json::Value,
    /// True if this is the tenant's default compute target.
    pub is_default: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ComputeTargetEntity {
    pub fn new(
        id: Id,
        tenant_id: Id,
        name: impl Into<String>,
        target_type: ComputeTargetType,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            tenant_id,
            name: name.into(),
            target_type,
            config: serde_json::Value::Object(Default::default()),
            is_default: false,
            created_at,
            updated_at: created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_target_type_round_trip() {
        for ty in [
            ComputeTargetType::Container,
            ComputeTargetType::Ssh,
            ComputeTargetType::Kubernetes,
        ] {
            let s = ty.to_string();
            let parsed = ComputeTargetType::from_db_str(&s).expect("parse should succeed");
            assert_eq!(parsed, ty);
        }
    }

    #[test]
    fn from_db_str_unknown_returns_none() {
        assert!(ComputeTargetType::from_db_str("docker").is_none());
    }

    #[test]
    fn new_has_empty_config_and_not_default() {
        let ct = ComputeTargetEntity::new(
            Id::new("ct1"),
            Id::new("t1"),
            "my-target",
            ComputeTargetType::Container,
            1000,
        );
        assert_eq!(ct.name, "my-target");
        assert!(!ct.is_default);
        assert_eq!(ct.config, serde_json::Value::Object(Default::default()));
    }
}
