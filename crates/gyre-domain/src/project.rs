use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A software project managed by Gyre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub name: String,
    pub repository_url: String,
}

impl Project {
    pub fn new(id: Id, name: impl Into<String>, repository_url: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            repository_url: repository_url.into(),
        }
    }
}
