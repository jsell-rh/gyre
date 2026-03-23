use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub workspace_id: Option<Id>,
}

impl Project {
    pub fn new(id: Id, name: impl Into<String>, created_at: u64) -> Self {
        Self {
            id,
            name: name.into(),
            description: None,
            created_at,
            updated_at: created_at,
            workspace_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_project() {
        let p = Project::new(Id::new("p1"), "My Project", 1000);
        assert_eq!(p.name, "My Project");
        assert!(p.description.is_none());
        assert_eq!(p.created_at, 1000);
        assert_eq!(p.updated_at, 1000);
    }

    #[test]
    fn test_project_with_description() {
        let mut p = Project::new(Id::new("p2"), "Gyre", 2000);
        p.description = Some("Autonomous dev platform".to_string());
        assert_eq!(p.description.as_deref(), Some("Autonomous dev platform"));
    }
}
