//! Version computation and changelog generation from conventional commits.
//!
//! This module is the core logic for the release automation feature.
//! It has no server dependencies — only git CLI and types from gyre-domain.

use serde::{Deserialize, Serialize};

// ── Conventional commit parsing ───────────────────────────────────────────────

/// Semver bump level derived from conventional commit analysis.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BumpLevel {
    None,
    Patch,
    Minor,
    Major,
}

/// A parsed conventional commit.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedCommit {
    pub sha: String,
    pub commit_type: String,
    pub scope: Option<String>,
    pub description: String,
    pub is_breaking: bool,
}

/// Parse a conventional commit message into a `ParsedCommit`.
/// Accepts the full commit message (subject + body). The subject line is
/// extracted from the first line; `BREAKING CHANGE:` footers are detected
/// anywhere in the message body. Returns `None` if the message does not
/// match the conventional format.
pub fn parse_conventional(sha: &str, message: &str) -> Option<ParsedCommit> {
    let subject = message.lines().next().unwrap_or("").trim();
    let has_breaking_footer = message.contains("BREAKING CHANGE:");

    // Find the `: ` separator.
    let colon_pos = subject.find(": ")?;
    let type_scope = &subject[..colon_pos];
    let description = subject[colon_pos + 2..].trim().to_string();
    if description.is_empty() {
        return None;
    }

    // Detect `!` modifier immediately before `:`.
    let (type_scope_clean, bang) = if let Some(stripped) = type_scope.strip_suffix('!') {
        (stripped, true)
    } else {
        (type_scope, false)
    };

    let is_breaking = bang || has_breaking_footer;

    // Extract type and optional scope.
    let (commit_type, scope) = if let Some(open) = type_scope_clean.find('(') {
        if let Some(close) = type_scope_clean.find(')') {
            let t = type_scope_clean[..open].to_string();
            let s = type_scope_clean[open + 1..close].to_string();
            (t, Some(s))
        } else {
            return None; // malformed parentheses
        }
    } else {
        (type_scope_clean.to_string(), None)
    };

    if commit_type.is_empty()
        || !commit_type
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }

    Some(ParsedCommit {
        sha: sha.to_string(),
        commit_type,
        scope,
        description,
        is_breaking,
    })
}

/// Determine the bump level for a commit type string (ignoring breaking changes).
pub fn type_bump(commit_type: &str) -> BumpLevel {
    match commit_type {
        "feat" => BumpLevel::Minor,
        "fix" | "perf" => BumpLevel::Patch,
        _ => BumpLevel::None,
    }
}

// ── Semver helpers ────────────────────────────────────────────────────────────

/// Parse a semver tag like `v1.2.3` or `1.2.3` into (major, minor, patch).
pub fn parse_semver(tag: &str) -> Option<(u64, u64, u64)> {
    let v = tag.strip_prefix('v').unwrap_or(tag);
    // Strip any pre-release suffix (e.g., `-dev.42`).
    let v = v.split('-').next().unwrap_or(v);
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    let patch = parts[2].parse().ok()?;
    Some((major, minor, patch))
}

/// Compute the next version string given the current tag (or None) and bump level.
pub fn compute_next_version(current_tag: Option<&str>, bump: &BumpLevel) -> String {
    match current_tag {
        Some(tag) => {
            if let Some((major, minor, patch)) = parse_semver(tag) {
                match bump {
                    BumpLevel::Major => format!("v{}.0.0", major + 1),
                    BumpLevel::Minor => format!("v{major}.{}.0", minor + 1),
                    BumpLevel::Patch => format!("v{major}.{minor}.{}", patch + 1),
                    BumpLevel::None => tag.to_string(), // no change
                }
            } else {
                "v0.1.0".to_string()
            }
        }
        None => match bump {
            BumpLevel::Major => "v1.0.0".to_string(),
            _ => "v0.1.0".to_string(),
        },
    }
}

/// Compute version drift between a pinned version and the current version.
///
/// Returns the number of minor versions the pinned version is behind the current.
/// If there is a major version difference, each major version counts as 10 minor versions
/// to ensure it always exceeds reasonable `max_version_drift` thresholds.
/// Returns `None` if either version string cannot be parsed.
pub fn compute_version_drift(pinned: &str, current: &str) -> Option<u32> {
    let (p_major, p_minor, _p_patch) = parse_semver(pinned)?;
    let (c_major, c_minor, _c_patch) = parse_semver(current)?;

    if c_major > p_major {
        // Major version drift: each major counts as 10 minor versions.
        let major_diff = (c_major - p_major) as u32;
        let minor_in_current = c_minor as u32;
        Some(major_diff * 10 + minor_in_current)
    } else if c_major == p_major && c_minor > p_minor {
        Some((c_minor - p_minor) as u32)
    } else {
        Some(0)
    }
}

// ── Git helpers ───────────────────────────────────────────────────────────────

/// Find the latest semver git tag in a bare repo (e.g. `v1.2.3`).
/// Returns `None` if no matching tag exists.
pub async fn latest_semver_tag(repo_path: &str) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["tag", "--list", "v[0-9]*.*[0-9]", "--sort=-version:refname"])
        .current_dir(repo_path)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// List commits (sha, full message) since `from_ref` up to `to_ref`.
/// If `from_ref` is `None`, lists all commits reachable from `to_ref`.
pub async fn commits_since(
    repo_path: &str,
    from_ref: Option<&str>,
    to_ref: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let range = match from_ref {
        Some(tag) => format!("{tag}..{to_ref}"),
        None => to_ref.to_string(),
    };

    // Use %x00 (null) as field separator between SHA and full message (%B),
    // and %x01 (SOH) as record separator between commits. Using %B (full
    // message) ensures BREAKING CHANGE: footers in the body are included.
    let output = tokio::process::Command::new("git")
        .args(["log", "--pretty=format:%H%x00%B%x01", &range])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git log failed: {stderr}");
    }

    let text = String::from_utf8(output.stdout)?;
    let commits = text
        .split('\x01')
        .filter_map(|record| {
            let record = record.trim();
            if record.is_empty() {
                return None;
            }
            let mut parts = record.splitn(2, '\x00');
            let sha = parts.next()?.trim().to_string();
            let message = parts.next().unwrap_or("").trim().to_string();
            if sha.is_empty() {
                None
            } else {
                Some((sha, message))
            }
        })
        .collect();

    Ok(commits)
}

// ── Changelog rendering ───────────────────────────────────────────────────────

/// Render a markdown changelog from structured sections.
pub fn render_changelog_markdown(
    version: &str,
    date: &str,
    sections: &[ChangelogSection],
) -> String {
    let mut md = format!("# {version} ({date})\n");

    for section in sections {
        if section.entries.is_empty() {
            continue;
        }
        md.push_str(&format!("\n## {}\n", section.title));
        for e in &section.entries {
            let scope_str = e
                .scope
                .as_ref()
                .map(|s| format!("**{s}:** "))
                .unwrap_or_default();
            let mut attrs = Vec::new();
            if let Some(ref name) = e.agent_name {
                attrs.push(name.clone());
            } else if let Some(ref id) = e.agent_id {
                attrs.push(id.clone());
            }
            if let Some(ref tid) = e.task_id {
                attrs.push(tid.clone());
            }
            let attr_str = if attrs.is_empty() {
                String::new()
            } else {
                format!(" ({})", attrs.join(", "))
            };
            md.push_str(&format!("- {scope_str}{}{attr_str}\n", e.description));
        }
    }

    md
}

/// Approximate ISO date (YYYY-MM-DD) from seconds since Unix epoch.
pub fn epoch_secs_to_date(secs: u64) -> String {
    let days = secs / 86400;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

// ── Response types for the API ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ChangelogEntry {
    pub sha: String,
    pub commit_type: String,
    pub scope: Option<String>,
    pub description: String,
    pub is_breaking: bool,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChangelogSection {
    pub title: String,
    pub entries: Vec<ChangelogEntry>,
}

#[derive(Serialize)]
pub struct ReleasePrepareResponse {
    /// Latest semver tag found, e.g. "v1.2.3". Null if no tags exist yet.
    pub current_tag: Option<String>,
    /// Computed next version string, e.g. "v1.3.0".
    pub next_version: String,
    /// Type of bump applied: "major", "minor", "patch", or "none".
    pub bump_type: BumpLevel,
    /// Number of commits since the last tag (or total if no tag).
    pub commit_count: usize,
    /// True if there are releasable commits (feat/fix/perf/breaking).
    pub has_release: bool,
    /// The branch analyzed.
    pub branch: String,
    /// Pre-rendered markdown changelog.
    pub changelog: String,
    /// Structured sections for programmatic use.
    pub sections: Vec<ChangelogSection>,
    /// Merge request opened for the release, if requested.
    pub mr_id: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_feat_with_scope() {
        let cc = parse_conventional("abc", "feat(auth): add OIDC login").unwrap();
        assert_eq!(cc.commit_type, "feat");
        assert_eq!(cc.scope.as_deref(), Some("auth"));
        assert_eq!(cc.description, "add OIDC login");
        assert!(!cc.is_breaking);
    }

    #[test]
    fn parse_fix_no_scope() {
        let cc = parse_conventional("abc", "fix: correct JWT expiry check").unwrap();
        assert_eq!(cc.commit_type, "fix");
        assert!(cc.scope.is_none());
        assert_eq!(cc.description, "correct JWT expiry check");
    }

    #[test]
    fn parse_breaking_bang() {
        let cc = parse_conventional("abc", "feat(auth)!: replace token auth with OIDC").unwrap();
        assert!(cc.is_breaking);
        assert_eq!(cc.commit_type, "feat");
        assert_eq!(cc.scope.as_deref(), Some("auth"));
    }

    #[test]
    fn parse_breaking_footer() {
        let msg = "feat(auth): replace auth\n\nBREAKING CHANGE: old tokens no longer work";
        let cc = parse_conventional("abc", msg).unwrap();
        assert!(cc.is_breaking);
    }

    #[test]
    fn parse_non_conventional_returns_none() {
        assert!(parse_conventional("abc", "fixed the thing").is_none());
        assert!(parse_conventional("abc", "").is_none());
    }

    #[test]
    fn parse_empty_description_returns_none() {
        assert!(parse_conventional("abc", "feat: ").is_none());
    }

    #[test]
    fn feat_gives_minor_bump() {
        assert_eq!(type_bump("feat"), BumpLevel::Minor);
    }

    #[test]
    fn fix_gives_patch_bump() {
        assert_eq!(type_bump("fix"), BumpLevel::Patch);
    }

    #[test]
    fn perf_gives_patch_bump() {
        assert_eq!(type_bump("perf"), BumpLevel::Patch);
    }

    #[test]
    fn chore_gives_no_bump() {
        assert_eq!(type_bump("chore"), BumpLevel::None);
    }

    #[test]
    fn parse_semver_valid() {
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v0.10.0"), Some((0, 10, 0)));
    }

    #[test]
    fn parse_semver_no_prefix() {
        assert_eq!(parse_semver("2.3.4"), Some((2, 3, 4)));
    }

    #[test]
    fn parse_semver_invalid() {
        assert_eq!(parse_semver("not-a-version"), None);
        assert_eq!(parse_semver("v1.2"), None);
    }

    #[test]
    fn compute_next_from_tag() {
        assert_eq!(
            compute_next_version(Some("v1.2.3"), &BumpLevel::Minor),
            "v1.3.0"
        );
        assert_eq!(
            compute_next_version(Some("v1.2.3"), &BumpLevel::Patch),
            "v1.2.4"
        );
        assert_eq!(
            compute_next_version(Some("v1.2.3"), &BumpLevel::Major),
            "v2.0.0"
        );
        assert_eq!(
            compute_next_version(Some("v1.2.3"), &BumpLevel::None),
            "v1.2.3"
        );
    }

    #[test]
    fn compute_next_no_tag() {
        assert_eq!(compute_next_version(None, &BumpLevel::Minor), "v0.1.0");
        assert_eq!(compute_next_version(None, &BumpLevel::Major), "v1.0.0");
    }

    #[test]
    fn bump_level_ordering() {
        assert!(BumpLevel::Major > BumpLevel::Minor);
        assert!(BumpLevel::Minor > BumpLevel::Patch);
        assert!(BumpLevel::Patch > BumpLevel::None);
    }

    #[test]
    fn epoch_secs_to_date_known() {
        // 2026-03-20 = 20532 days since epoch = 20532 * 86400 secs
        let d = epoch_secs_to_date(20532 * 86400);
        assert_eq!(d, "2026-03-20");
    }

    #[test]
    fn render_changelog_empty() {
        let md = render_changelog_markdown("v1.0.0", "2026-03-20", &[]);
        assert_eq!(md, "# v1.0.0 (2026-03-20)\n");
    }

    #[test]
    fn render_changelog_with_entries() {
        let sections = vec![ChangelogSection {
            title: "Features".to_string(),
            entries: vec![ChangelogEntry {
                sha: "abc123".to_string(),
                commit_type: "feat".to_string(),
                scope: Some("auth".to_string()),
                description: "add OIDC login".to_string(),
                is_breaking: false,
                agent_id: Some("agent-1".to_string()),
                agent_name: Some("worker-1".to_string()),
                task_id: Some("TASK-001".to_string()),
            }],
        }];
        let md = render_changelog_markdown("v1.1.0", "2026-03-20", &sections);
        assert!(md.contains("## Features"));
        assert!(md.contains("**auth:** add OIDC login"));
        assert!(md.contains("worker-1"));
        assert!(md.contains("TASK-001"));
    }

    // ── Version drift computation tests (TASK-021) ────────────────────

    #[test]
    fn drift_same_version() {
        assert_eq!(compute_version_drift("1.2.3", "1.2.3"), Some(0));
        assert_eq!(compute_version_drift("v1.2.3", "v1.2.3"), Some(0));
    }

    #[test]
    fn drift_minor_versions() {
        assert_eq!(compute_version_drift("1.2.3", "1.5.0"), Some(3));
        assert_eq!(compute_version_drift("v1.0.0", "v1.3.0"), Some(3));
    }

    #[test]
    fn drift_major_version() {
        // Major diff: 1 major * 10 + current minor
        assert_eq!(compute_version_drift("1.2.3", "2.0.0"), Some(10));
        assert_eq!(compute_version_drift("1.2.3", "2.3.0"), Some(13));
        assert_eq!(compute_version_drift("1.2.3", "3.0.0"), Some(20));
    }

    #[test]
    fn drift_current_behind_pinned() {
        // Current is not behind pinned — drift should be 0.
        assert_eq!(compute_version_drift("1.5.0", "1.2.3"), Some(0));
    }

    #[test]
    fn drift_unparseable_returns_none() {
        assert_eq!(compute_version_drift("not-a-version", "1.2.3"), None);
        assert_eq!(compute_version_drift("1.2.3", "bad"), None);
    }

    #[test]
    fn drift_with_v_prefix() {
        assert_eq!(compute_version_drift("v1.2.0", "v1.5.0"), Some(3));
    }

    #[test]
    fn drift_prerelease_stripped() {
        assert_eq!(compute_version_drift("1.2.3-beta.1", "1.5.0"), Some(3));
    }
}
