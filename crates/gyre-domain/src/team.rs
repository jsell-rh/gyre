use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,
    pub description: Option<String>,
    pub member_ids: Vec<Id>,
    pub created_at: u64,
}

impl Team {
    pub fn new(id: Id, workspace_id: Id, name: impl Into<String>, now: u64) -> Self {
        Self {
            id,
            workspace_id,
            name: name.into(),
            description: None,
            member_ids: Vec::new(),
            created_at: now,
        }
    }

    pub fn add_member(&mut self, user_id: Id) {
        if !self.member_ids.contains(&user_id) {
            self.member_ids.push(user_id);
        }
    }

    pub fn remove_member(&mut self, user_id: &Id) {
        self.member_ids.retain(|id| id != user_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_member_management() {
        let mut team = Team::new(Id::new("t1"), Id::new("ws1"), "Platform", 1000);
        let u1 = Id::new("u1");
        let u2 = Id::new("u2");

        team.add_member(u1.clone());
        team.add_member(u2.clone());
        // Duplicate add is idempotent
        team.add_member(u1.clone());
        assert_eq!(team.member_ids.len(), 2);

        team.remove_member(&u1);
        assert_eq!(team.member_ids.len(), 1);
        assert_eq!(team.member_ids[0], u2);
    }
}
