use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub path: String,
    pub default_branch: String,
    pub created_at: u64,
}

impl Repository {
    pub fn new(
        id: Id,
        project_id: Id,
        name: impl Into<String>,
        path: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            project_id,
            name: name.into(),
            path: path.into(),
            default_branch: "main".to_string(),
            created_at,
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
            Id::new("p1"),
            "my-repo",
            "/path/to/repo",
            1000,
        );
        assert_eq!(r.default_branch, "main");
        assert_eq!(r.name, "my-repo");
        assert_eq!(r.path, "/path/to/repo");
    }
}
