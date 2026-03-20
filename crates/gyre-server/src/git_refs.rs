//! Helpers for reading and writing custom git ref namespaces.
//!
//! All operations are best-effort: errors are logged but never propagated to
//! callers, so a missing repo or missing commit never fails an API request.

use tokio::process::Command;

/// Validate a refname: must not contain `..` and must not start with `-`.
fn refname_safe(refname: &str) -> bool {
    !refname.contains("..") && !refname.starts_with('-')
}

/// Resolve `refname` in `repo_path` to its SHA-1, or `None` if it doesn't exist.
pub async fn resolve_ref(repo_path: &str, refname: &str) -> Option<String> {
    if !refname_safe(refname) {
        tracing::warn!(refname, "resolve_ref: unsafe refname rejected");
        return None;
    }
    let out = Command::new("git")
        .args(["rev-parse", "--verify", refname])
        .current_dir(repo_path)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(sha);
        }
    }
    None
}

/// Write (or update) `refname` in `repo_path` to point to `sha`.
pub async fn write_ref(repo_path: &str, refname: &str, sha: &str) {
    if !refname_safe(refname) {
        tracing::warn!(refname, "write_ref: unsafe refname rejected");
        return;
    }
    // Validate sha is 40-char hex to prevent argument injection
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        tracing::warn!(sha, refname, "write_ref: invalid SHA, skipping");
        return;
    }
    let status = Command::new("git")
        .args(["update-ref", "--", refname, sha])
        .current_dir(repo_path)
        .status()
        .await;
    match status {
        Ok(s) if s.success() => {
            tracing::debug!(refname, sha, "wrote custom ref");
        }
        Ok(s) => {
            tracing::warn!(refname, sha, exit_code = ?s.code(), "write_ref: git update-ref failed");
        }
        Err(e) => {
            tracing::warn!(refname, sha, error = %e, "write_ref: failed to run git");
        }
    }
}

/// Count refs that exist under `prefix` (e.g. `refs/agents/{id}/snapshots/`).
pub async fn count_refs_under(repo_path: &str, prefix: &str) -> usize {
    if !refname_safe(prefix) {
        tracing::warn!(prefix, "count_refs_under: unsafe prefix rejected");
        return 0;
    }
    let out = Command::new("git")
        .args(["for-each-ref", "--format=%(refname)", prefix])
        .current_dir(repo_path)
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.lines().filter(|l| !l.is_empty()).count()
        }
        _ => 0,
    }
}
