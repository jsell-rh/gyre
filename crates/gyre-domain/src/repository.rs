use gyre_common::Id;
use serde::{Deserialize, Serialize};

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
        }
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
    }
}
