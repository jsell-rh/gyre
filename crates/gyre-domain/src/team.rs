//! Teams (user-management.md §Team Management).
//!
//! Teams are organizational, not authorization: they never grant permissions
//! beyond workspace membership. They carry a workspace-unique, URL-safe
//! `slug` (the reference form for notification routing and spec ownership,
//! e.g. `team:platform`), an optional `description`, and a `lead` — the user
//! responsible for the team.
//!
//! Membership is modeled by [`TeamMembership`] (not an id list on the team)
//! so each membership carries its own `added_at` timestamp, per the spec.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,
    /// Workspace-unique, URL-safe reference (notification routing and spec
    /// ownership address teams as `team:{slug}`).
    pub slug: String,
    pub description: Option<String>,
    /// User responsible for the team (a workspace member).
    pub lead: Id,
    pub created_at: u64,
}

impl Team {
    pub fn new(
        id: Id,
        workspace_id: Id,
        name: impl Into<String>,
        slug: impl Into<String>,
        lead: Id,
        now: u64,
    ) -> Self {
        Self {
            id,
            workspace_id,
            name: name.into(),
            slug: slug.into(),
            description: None,
            lead,
            created_at: now,
        }
    }

    /// Validate a team slug (user-management.md §Team Management: unique
    /// within the workspace, URL-safe). Same shape as usernames: 1–64 chars
    /// of `[a-z0-9-]`, no leading/trailing/consecutive separators.
    pub fn validate_slug(slug: &str) -> Result<(), String> {
        if slug.is_empty() || slug.len() > 64 {
            return Err("team slug must be 1-64 characters".to_string());
        }
        if !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(
                "team slug must be URL-safe: lowercase letters, digits, or '-'".to_string(),
            );
        }
        let edge = |c: char| c == '-';
        if edge(slug.chars().next().unwrap()) || edge(slug.chars().last().unwrap()) {
            return Err("team slug must not start or end with '-'".to_string());
        }
        if slug.contains("--") {
            return Err("team slug must not contain consecutive '-'".to_string());
        }
        Ok(())
    }

    /// Derive a URL-safe slug from a team name ("Platform Team" →
    /// "platform-team"). Returns None when nothing URL-safe remains — the
    /// caller must require an explicit slug then, never fabricate one.
    pub fn sanitize_slug(name: &str) -> Option<String> {
        let mut out = String::with_capacity(name.len());
        for c in name.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                out.push(c);
            } else if c.is_ascii_uppercase() {
                out.push(c.to_ascii_lowercase());
            } else if c == '-' || c == '_' || c == '.' || c == ' ' || c == '@' {
                out.push('-');
            }
            // All other characters are dropped.
        }
        // Collapse runs of separators and trim edges.
        let mut collapsed = String::with_capacity(out.len());
        let mut prev_sep = false;
        for c in out.chars() {
            let is_sep = c == '-';
            if is_sep && prev_sep {
                continue;
            }
            collapsed.push(c);
            prev_sep = is_sep;
        }
        let trimmed = collapsed.trim_matches('-');
        if trimmed.is_empty() || trimmed.len() > 64 {
            return None;
        }
        Some(trimmed.to_string())
    }
}

/// A user's membership in a team (user-management.md §Team Management).
/// `added_at` records when the user joined the team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembership {
    pub team_id: Id,
    pub user_id: Id,
    pub added_at: u64,
}

impl TeamMembership {
    pub fn new(team_id: Id, user_id: Id, added_at: u64) -> Self {
        Self {
            team_id,
            user_id,
            added_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_validation_accepts_url_safe() {
        for ok in ["platform", "platform-team", "team2", "a", &"x".repeat(64)] {
            assert!(Team::validate_slug(ok).is_ok(), "{ok} should be valid");
        }
    }

    #[test]
    fn slug_validation_rejects_unsafe() {
        for bad in [
            "",
            "-platform",
            "platform-",
            "platform--team",
            "Platform",
            "platform team",
            "platform/team",
            "platform_team",
            "plattform!",
            "plattförm",
            &"x".repeat(65),
        ] {
            assert!(Team::validate_slug(bad).is_err(), "{bad} should be invalid");
        }
    }

    #[test]
    fn slug_derivation_from_name() {
        assert_eq!(
            Team::sanitize_slug("Platform Team").as_deref(),
            Some("platform-team")
        );
        assert_eq!(Team::sanitize_slug("  dev  ").as_deref(), Some("dev"));
        assert_eq!(Team::sanitize_slug("A_B-c!d").as_deref(), Some("a-b-cd"));
        // Nothing URL-safe remains — caller must require an explicit slug.
        assert_eq!(Team::sanitize_slug("---"), None);
        assert_eq!(Team::sanitize_slug("!!!"), None);
    }

    #[test]
    fn team_construction_carries_slug_and_lead() {
        let lead = Id::new("u-lead");
        let team = Team::new(
            Id::new("t1"),
            Id::new("ws1"),
            "Platform",
            "platform",
            lead.clone(),
            1000,
        );
        assert_eq!(team.slug, "platform");
        assert_eq!(team.lead, lead);
        assert_eq!(team.created_at, 1000);
        assert!(team.description.is_none());
    }

    #[test]
    fn membership_carries_added_at() {
        let m = TeamMembership::new(Id::new("t1"), Id::new("u1"), 4242);
        assert_eq!(m.added_at, 4242);
        assert_eq!(m.team_id, Id::new("t1"));
        assert_eq!(m.user_id, Id::new("u1"));
    }
}
