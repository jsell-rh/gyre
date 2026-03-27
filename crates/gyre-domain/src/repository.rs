use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoStatus {
    #[default]
    Active,
    Archived,
}

impl std::fmt::Display for RepoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoStatus::Active => write!(f, "Active"),
            RepoStatus::Archived => write!(f, "Archived"),
        }
    }
}

impl std::str::FromStr for RepoStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Active" => Ok(RepoStatus::Active),
            "Archived" => Ok(RepoStatus::Archived),
            other => Err(format!("unknown RepoStatus: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: Id,
    pub name: String,
    pub path: String,
    pub default_branch: String,
    pub created_at: u64,
    pub is_mirror: bool,
    pub mirror_url: Option<String>,
    pub mirror_interval_secs: Option<u64>,
    pub last_mirror_sync: Option<u64>,
    pub workspace_id: Id,
    pub description: Option<String>,
    pub status: RepoStatus,
    pub updated_at: u64,
}

impl Repository {
    pub fn new(
        id: Id,
        workspace_id: Id,
        name: impl Into<String>,
        path: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            workspace_id,
            name: name.into(),
            path: path.into(),
            default_branch: "main".to_string(),
            created_at,
            is_mirror: false,
            mirror_url: None,
            mirror_interval_secs: None,
            last_mirror_sync: None,
            description: None,
            status: RepoStatus::Active,
            updated_at: created_at,
        }
    }

    pub fn archive(&mut self) {
        self.status = RepoStatus::Archived;
    }

    pub fn unarchive(&mut self) {
        self.status = RepoStatus::Active;
    }

    pub fn is_archived(&self) -> bool {
        self.status == RepoStatus::Archived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_repository_defaults() {
        let r = Repository::new(
            Id::new("r1"),
            Id::new("ws1"),
            "my-repo",
            "/path/to/repo",
            1000,
        );
        assert_eq!(r.default_branch, "main");
        assert_eq!(r.name, "my-repo");
        assert_eq!(r.path, "/path/to/repo");
        assert_eq!(r.workspace_id.as_str(), "ws1");
        assert_eq!(r.status, RepoStatus::Active);
        assert!(r.description.is_none());
        assert_eq!(r.updated_at, 1000);
    }

    #[test]
    fn test_archive_unarchive() {
        let mut r = Repository::new(Id::new("r1"), Id::new("ws1"), "my-repo", "/path", 1000);
        assert!(!r.is_archived());
        r.archive();
        assert!(r.is_archived());
        assert_eq!(r.status, RepoStatus::Archived);
        r.unarchive();
        assert!(!r.is_archived());
        assert_eq!(r.status, RepoStatus::Active);
    }

    #[test]
    fn test_repo_status_roundtrip() {
        use std::str::FromStr;
        assert_eq!(RepoStatus::from_str("Active").unwrap(), RepoStatus::Active);
        assert_eq!(
            RepoStatus::from_str("Archived").unwrap(),
            RepoStatus::Archived
        );
        assert!(RepoStatus::from_str("Invalid").is_err());
    }
}
