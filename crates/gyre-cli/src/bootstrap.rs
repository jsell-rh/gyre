//! Bootstrap logic for `gyre bootstrap` (platform-model.md §8).
//!
//! Pure, unit-testable pieces: persona prompt registry, slug derivation,
//! default gate detection, starter-kit generation, and summary rendering.
//! The orchestration (HTTP calls, step ordering) lives in main.rs.

use std::path::Path;

// ─── Persona prompts ──────────────────────────────────────────────────────────
// Single source of truth: `gyre_domain::BUILTIN_PERSONA_DEFS`, the same
// definitions the server seeds from (platform-model.md §2). task-140
// consolidated the CLI's private copies here — two independent embeds of
// the four personas could drift, restoring different prompts on
// re-registration than a server restart re-seeds.
pub use gyre_domain::BUILTIN_PERSONA_DEFS as BUILTIN_PERSONAS;

// ─── Slug derivation ──────────────────────────────────────────────────────────

/// Derive a URL-safe slug from a name. Mirrors the server's workspace slug
/// logic (workspaces.rs): lowercase, non-alphanumeric to hyphen, collapse,
/// trim. Needed client-side because POST /api/v1/tenants requires a slug.
pub fn derive_slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ─── Default gate detection ───────────────────────────────────────────────────

/// A default quality gate to configure (spec §8 step 7).
pub struct DefaultGate {
    pub name: &'static str,
    pub gate_type: &'static str,
    pub command: &'static str,
}

/// Detect default gates from a local repo checkout:
/// - Cargo.toml present  -> TestCommand `cargo test` + LintCommand `cargo clippy`
/// - scripts/check-arch.sh present -> LintCommand architecture check
pub fn detect_default_gates(repo_path: &Path) -> Vec<DefaultGate> {
    let mut gates = Vec::new();
    if repo_path.join("Cargo.toml").exists() {
        gates.push(DefaultGate {
            name: "cargo-test",
            gate_type: "test_command",
            command: "cargo test --all",
        });
        gates.push(DefaultGate {
            name: "cargo-clippy",
            gate_type: "lint_command",
            command: "cargo clippy --all-targets -- -D warnings",
        });
    }
    if repo_path.join("scripts").join("check-arch.sh").exists() {
        gates.push(DefaultGate {
            name: "arch-lint",
            gate_type: "lint_command",
            command: "bash scripts/check-arch.sh",
        });
    }
    gates
}

// ─── Starter kit ──────────────────────────────────────────────────────────────

/// Write the starter repo structure (spec §8 "Starter Kit") into `root`:
/// specs/manifest.yaml, specs/index.md, specs/system/design-principles.md,
/// AGENTS.md, .prek.yaml. Existing files are never overwritten.
pub fn write_starter_kit(root: &Path) -> Result<(), std::io::Error> {
    let specs_dir = root.join("specs");
    std::fs::create_dir_all(specs_dir.join("system"))?;

    let manifest = root.join("specs").join("manifest.yaml");
    if !manifest.exists() {
        std::fs::write(&manifest, STARTER_MANIFEST)?;
    }
    let index = root.join("specs").join("index.md");
    if !index.exists() {
        std::fs::write(&index, STARTER_INDEX)?;
    }
    let principles = root
        .join("specs")
        .join("system")
        .join("design-principles.md");
    if !principles.exists() {
        std::fs::write(&principles, STARTER_DESIGN_PRINCIPLES)?;
    }
    let agents = root.join("AGENTS.md");
    if !agents.exists() {
        std::fs::write(&agents, STARTER_AGENTS_MD)?;
    }
    let prek = root.join(".prek.yaml");
    if !prek.exists() {
        std::fs::write(&prek, STARTER_PREK_YAML)?;
    }
    Ok(())
}

/// Repo-relative paths the starter kit owns. `push_and_sync_specs` commits
/// exactly these when a `--starter-kit` run leaves them uncommitted — the
/// push would otherwise send HEAD without them and the spec ledger would
/// stay empty while bootstrap reports success.
pub const STARTER_KIT_PATHS: &[&str] = &[
    "specs/manifest.yaml",
    "specs/index.md",
    "specs/system/design-principles.md",
    "AGENTS.md",
    ".prek.yaml",
];

// ─── Spec registration (§8 step 6) ───────────────────────────────────────────

/// Push the local repo checkout to the server's bare repo, then trigger the
/// server-side spec-ledger sync so the platform registry is populated at
/// first-run time (§8 step 6: "If repo contains specs/manifest.yaml, parse
/// and register specs").
///
/// `repo_path` must be a git work tree with at least one commit. A push is
/// required because the server's ledger sync reads specs from the default
/// branch of its bare repo -- there is no other transport for spec content.
pub async fn push_and_sync_specs(
    api: &crate::client::GyreClient,
    repo_path: &Path,
    repo_id: &str,
    clone_url: &str,
) -> anyhow::Result<usize> {
    // Uncommitted starter-kit files must land in a commit or the push below
    // sends HEAD without them and the ledger stays empty while the CLI
    // reports success (spec §8 step 6 + starter-kit branch). Commit only
    // paths the starter kit owns; leave the user's own staging untouched.
    // `git add` aborts when an explicit pathspec matches no file, so only
    // the kit paths that actually exist are staged — a repo can carry a
    // pre-existing specs/manifest.yaml without the rest of the kit
    // (§8 step 6 applies to any manifest-bearing repo, not just
    // --starter-kit runs), and staging the missing siblings would fail
    // the whole step for those repos.
    let kit_paths_present: Vec<&str> = STARTER_KIT_PATHS
        .iter()
        .filter(|p| repo_path.join(p).exists())
        .copied()
        .collect();
    let status = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["status", "--porcelain", "--"])
        .args(&kit_paths_present)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run git status: {e}"))?;
    if !status.status.success() {
        return Err(anyhow::anyhow!(
            "git status failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    if !status.stdout.is_empty() {
        let add = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["add", "--"])
            .args(&kit_paths_present)
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("failed to run git add: {e}"))?;
        if !add.status.success() {
            return Err(anyhow::anyhow!(
                "git add failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            ));
        }
        let commit = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["commit", "-m", "chore(specs): add gyre starter kit"])
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("failed to run git commit: {e}"))?;
        if !commit.status.success() {
            return Err(anyhow::anyhow!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit.stderr).trim()
            ));
        }
    }

    // Push the local branch to the server's bare repo. HOW the push runs
    // depends on what the server's main currently holds (round-11 F11: an
    // unconditional `--force` rewound merged MR commits out of main on
    // resume runs — the merge processor advances the bare repo's main with
    // server-side merge commits, and a stale local checkout forcing over
    // them silently removed merged history):
    //
    //   * no remote main yet          -> plain push (creates the branch)
    //   * remote main is behind HEAD  -> plain push (fast-forward; git
    //                                     itself rejects anything else)
    //   * remote main is the server's disposable seed commit (see below)
    //                                  -> replace it, pinned to the exact
    //                                     seed SHA via --force-with-lease
    //   * anything else (diverged — e.g. main advanced with merged MRs
    //     while the local checkout has not pulled)
    //                                  -> refuse: bootstrap never discards
    //                                     server-side history
    //
    // The server seeds a repo's main with a single root commit carrying an
    // empty tree (repos.rs `create_repo` -> `create_initial_commit`) purely
    // so HEAD is valid for `git worktree add`. The user's local history is
    // unrelated to that seed, so the first-run push cannot fast-forward —
    // replacing the seed with the real history is the documented first-run
    // semantics (round-3 finding 2: the unforced push made EVERY first-run
    // fail at this step). The lease pins the replacement to the seed we
    // just inspected, so a concurrent push between our fetch and our push
    // fails the lease instead of being clobbered.
    let remote_main = ls_remote_main(repo_path, clone_url, api.token()).await?;
    let mut push_cmd = git_with_auth(repo_path, api.token());
    push_cmd.arg("push");
    if let Some(remote_sha) = remote_main {
        // Bring the remote main commit (and its tree) into the local object
        // store so ancestry and seed shape can be inspected offline.
        fetch_main_object(repo_path, clone_url, api.token()).await?;
        if !is_ancestor_of_head(repo_path, &remote_sha).await? {
            if is_disposable_seed(repo_path, &remote_sha).await? {
                push_cmd.arg(format!("--force-with-lease=refs/heads/main:{remote_sha}"));
            } else {
                anyhow::bail!(
                    "refusing to push: the server's main ({remote_sha}) has \
                     commits this checkout does not (e.g. MRs merged on the \
                     server); pull or rebase onto the server branch first — \
                     bootstrap will not overwrite them"
                );
            }
        }
        // Ancestor (or equal): a plain push fast-forwards the remote, or is
        // a no-op when the remote already matches HEAD (resume re-run).
    }
    push_cmd.arg(clone_url).arg("HEAD:refs/heads/main");
    let push = push_cmd
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;
    if !push.status.success() {
        return Err(anyhow::anyhow!(
            "git push failed: {}",
            String::from_utf8_lossy(&push.stderr).trim()
        ));
    }

    // Sync the ledger against the server repo's current default-branch HEAD.
    let synced = api
        .sync_specs(repo_id)
        .await
        .map_err(|e| anyhow::anyhow!("spec sync call failed: {e}"))?;
    // The ledger count and the HEAD it synced against are both meaningful:
    // a zero count against a known HEAD distinguishes "manifest parsed but
    // no specs" from "sync never ran".
    tracing::debug!(
        head_sha = %synced.head_sha,
        registered = synced.registered,
        "bootstrap: spec ledger synced"
    );
    Ok(synced.registered)
}

/// Build a `git` command scoped to `repo_path` whose HTTP bearer token is
/// passed through git's environment config (GIT_CONFIG_COUNT /
/// GIT_CONFIG_KEY_n / GIT_CONFIG_VALUE_n, git >= 2.31) instead of a
/// `-c http.extraHeader=...` argument (round-11 F12): child argv is
/// readable via `ps` and `/proc/<pid>/cmdline` for the lifetime of every
/// network-touching git call, while environment config keeps the token
/// out of it.
fn git_with_auth(repo_path: &Path, token: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(repo_path);
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "http.extraHeader")
        .env(
            "GIT_CONFIG_VALUE_0",
            format!("Authorization: Bearer {token}"),
        );
    cmd
}

/// Run a git command that must succeed and return its trimmed stdout.
async fn git_stdout(mut cmd: tokio::process::Command, what: &str) -> anyhow::Result<String> {
    let out = cmd
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run git {what}: {e}"))?;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "git {what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The SHA of the server's `refs/heads/main`, or `None` when the server
/// repo has no main branch yet (e.g. its seed failed to initialize).
async fn ls_remote_main(
    repo_path: &Path,
    clone_url: &str,
    token: &str,
) -> anyhow::Result<Option<String>> {
    let mut cmd = git_with_auth(repo_path, token);
    cmd.arg("ls-remote").arg(clone_url).arg("refs/heads/main");
    let out = git_stdout(cmd, "ls-remote").await?;
    Ok(out
        .lines()
        .next()
        .and_then(|l| l.split_ascii_whitespace().next())
        .map(|sha| sha.to_string()))
}

/// Fetch the server's `refs/heads/main` into the local object store (via
/// FETCH_HEAD) so its commit and tree can be inspected offline. No local
/// branch or remote-tracking ref is created or moved.
async fn fetch_main_object(repo_path: &Path, clone_url: &str, token: &str) -> anyhow::Result<()> {
    let mut cmd = git_with_auth(repo_path, token);
    cmd.arg("fetch")
        .arg("--no-tags")
        .arg(clone_url)
        .arg("refs/heads/main");
    git_stdout(cmd, "fetch").await?;
    Ok(())
}

/// Whether `commit` is an ancestor of (or equal to) the local HEAD —
/// i.e. pushing HEAD is a fast-forward for the remote branch.
async fn is_ancestor_of_head(repo_path: &Path, commit: &str) -> anyhow::Result<bool> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run git merge-base: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(anyhow::anyhow!(
            "git merge-base failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// Whether `commit` is the server's disposable first-run seed: a root
/// commit (no parents) whose tree is empty — the exact shape
/// `create_initial_commit` writes (treebuilder over nothing, no parents).
/// Real history never matches: merge commits have parents, and any commit
/// carrying actual content has a non-empty tree.
async fn is_disposable_seed(repo_path: &Path, commit: &str) -> anyhow::Result<bool> {
    let parents = {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("-C").arg(repo_path);
        cmd.args(["show", "-s", "--format=%P", commit]);
        git_stdout(cmd, "show").await?
    };
    if !parents.is_empty() {
        return Ok(false);
    }
    let tree_entries = {
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("-C").arg(repo_path);
        cmd.args(["ls-tree", commit]);
        git_stdout(cmd, "ls-tree").await?
    };
    Ok(tree_entries.is_empty())
}

/// Starter-kit manifest template (§8 "Starter Kit": "Default manifest with
/// design-principles spec"). Lives in its own file so the server's
/// round-trip test (spec_registry tests) can consume the exact shipped
/// bytes through the real `parse_manifest` — the F5 defect class was a
/// template that parsed nowhere but the CLI's structural check.
pub const STARTER_MANIFEST: &str = include_str!("bootstrap/starter-manifest.yaml");

const STARTER_INDEX: &str = r#"# Spec Index

| Spec | Kind | Approval | Status |
|---|---|---|---|
| [Design Principles](system/design-principles.md) | system | human | draft |

## Purpose

This repo is managed by Gyre. Specs are the source of truth: no implementation
without an approved spec. Add spec entries to `specs/manifest.yaml` and list
them here.
"#;

const STARTER_DESIGN_PRINCIPLES: &str = r#"# Design Principles

The non-negotiable rules for this codebase. Every spec and every change must
trace back to one of these principles.

1. **Correctness first.** The most correct way is mandated. No shortcuts.
2. **Specs are the artifact.** Humans design, agents implement. No code
   without an approved spec.
3. **Single-minded agents.** One agent, one task, one purpose.
4. **Security by default.** Everything authenticated, auditable, sandboxed.
5. **Fix the environment, not the agent.** When something fails, engineer
   the failure class away, don't retry blindly.
"#;

const STARTER_AGENTS_MD: &str = r#"# AGENTS.md - Agent Entry Point

This repository is managed by Gyre.

- **Specs live in `specs/`.** Read `specs/index.md` first. Every task traces
  back to a spec.
- **Tasks are assigned, not chosen.** Work only your assigned task.
- **Gates are enforced.** Tests and lint must pass before merge.

When spawned through Gyre, your full protocol context (persona, norms,
task, constraints) is injected at spawn time. This file exists for humans
and for agents connecting outside Gyre's spawn process.
"#;

const STARTER_PREK_YAML: &str = r#"repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --all --
        language: system
        types: [rust]
        pass_filenames: false
      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy --all-targets -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false
      - id: conventional-commits
        name: conventional commit message
        entry: bash -c 'echo "commit messages must follow conventional commits (feat|fix|docs|refactor|test|chore): subject"'
        language: system
        stages: [commit-msg]
"#;

// ─── Summary rendering ────────────────────────────────────────────────────────

/// Bootstrap result data rendered into the final summary (spec §8 step 9).
#[derive(Default)]
pub struct BootstrapSummary {
    pub tenant_id: String,
    pub tenant_name: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub repo_id: String,
    pub repo_name: String,
    pub admin_username: Option<String>,
    /// Raw API key - shown exactly once.
    pub api_key: Option<String>,
    pub server_url: String,
    pub clone_url: Option<String>,
    /// Repo orchestrator process-launch outcome (None before step 8).
    pub orchestrator_agent_id: Option<String>,
    /// Specs registered in the platform ledger during step 6.
    pub specs_registered: usize,
    /// Truthful process-launch status of the repo orchestrator: "running",
    /// "launch_failed", or None when a live one already existed (409 resume).
    pub orchestrator_launch_status: Option<String>,
    /// Failure reason when the orchestrator process failed to launch.
    pub orchestrator_launch_detail: Option<String>,
    /// Gate names configured in step 7.
    pub gates_configured: Vec<String>,
    /// Persona slugs registered (and pre-approved) in step 5.
    pub personas_registered: Vec<String>,
}

impl BootstrapSummary {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Bootstrap complete!\n\n");
        out.push_str(&format!(
            "  Tenant:     {} ({})\n",
            self.tenant_name, self.tenant_id
        ));
        out.push_str(&format!(
            "  Workspace:  {} ({})\n",
            self.workspace_name, self.workspace_id
        ));
        out.push_str(&format!(
            "  Repo:       {} ({})\n",
            self.repo_name, self.repo_id
        ));
        if !self.personas_registered.is_empty() {
            out.push_str(&format!(
                "  Personas:   {}\n",
                self.personas_registered.join(", ")
            ));
        }
        if !self.gates_configured.is_empty() {
            out.push_str(&format!(
                "  Gates:      {}\n",
                self.gates_configured.join(", ")
            ));
        }
        if let (Some(user), Some(key)) = (&self.admin_username, &self.api_key) {
            out.push_str(&format!(
                "\n  Admin user: {user}\n  API key (shown once): {key}\n"
            ));
        }
        out.push_str(&format!("\n  Server URL: {}\n", self.server_url));
        if let Some(clone) = &self.clone_url {
            out.push_str(&format!("  Clone URL:  {clone}\n"));
        }
        // F3: the orchestrator status claim must match what actually
        // happened at launch — a persisted row is not "running".
        match (
            &self.orchestrator_agent_id,
            self.orchestrator_launch_status.as_deref(),
        ) {
            (Some(agent), Some("running")) => {
                out.push_str(&format!(
                    "\nYour repo orchestrator is running (agent {agent}).\n"
                ));
            }
            (Some(agent), Some("launch_failed")) => {
                out.push_str(&format!(
                    "\nYour repo orchestrator was registered (agent {agent}) \
                     but its process failed to launch"
                ));
                if let Some(detail) = &self.orchestrator_launch_detail {
                    out.push_str(&format!(": {detail}"));
                }
                out.push_str(
                    ".\nAuto-restart is enabled; the stale detector retries once its \
                     heartbeat times out, or configure a compute target with a valid \
                     command (GYRE_ORCHESTRATOR_COMMAND).\n",
                );
            }
            (Some(agent), _) => {
                out.push_str(&format!(
                    "\nYour repo orchestrator agent is registered (agent {agent}); \
                     launch status unknown.\n"
                ));
            }
            (None, _) => {
                out.push_str(
                    "\nA repo orchestrator was already active for this repo - kept as-is.\n",
                );
            }
        }
        out.push_str(&format!("Visit {} for the dashboard.\n", self.server_url));
        out
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_personas_cover_spec_table() {
        // platform-model.md §2 table: exactly these four, pre-approved.
        let slugs: Vec<&str> = BUILTIN_PERSONAS.iter().map(|p| p.slug).collect();
        assert_eq!(
            slugs,
            vec![
                "workspace-orchestrator",
                "repo-orchestrator",
                "accountability",
                "security"
            ]
        );
    }

    #[test]
    fn builtin_persona_prompts_are_substantive() {
        for p in BUILTIN_PERSONAS {
            assert!(
                p.system_prompt.len() > 500,
                "persona {} prompt too short: {} bytes",
                p.slug,
                p.system_prompt.len()
            );
            assert!(
                !p.capabilities.is_empty(),
                "persona {} needs capabilities",
                p.slug
            );
        }
    }

    /// Single source of truth for the §2 table: the CLI registers exactly
    /// the same definitions the server seeds — `gyre_domain::BUILTIN_PERSONA_DEFS`
    /// re-exported as `BUILTIN_PERSONAS`. Before task-140's consolidation the
    /// CLI embedded private copies under `bootstrap/personas/`; byte-identity
    /// with `specs/personas/*.md` was only ever hand-verified, so the two
    /// registration paths could silently drift. This now fails to compile
    /// any other way: there is no second definition to drift.
    #[test]
    fn builtin_personas_are_the_domain_seed_definitions() {
        assert_eq!(
            BUILTIN_PERSONAS.as_ptr() as *const (),
            gyre_domain::BUILTIN_PERSONA_DEFS.as_ptr() as *const ()
        );
        // The domain test suite independently asserts slug order, tenant
        // scope, pre-approval, and SHA-256 content hashes for these same
        // definitions; here we assert the registration surface is identical
        // (name/slug/prompt/capabilities/protocols), not just co-located.
        for p in BUILTIN_PERSONAS {
            assert!(
                p.system_prompt.starts_with('#'),
                "spec-file prompt for {}",
                p.slug
            );
        }
    }

    #[test]
    fn slug_derivation_mirrors_server() {
        assert_eq!(derive_slug("Acme Corp"), "acme-corp");
        assert_eq!(derive_slug("Platform Team"), "platform-team");
        assert_eq!(derive_slug("  dev  "), "dev");
        assert_eq!(derive_slug("A_B-c!d"), "a-b-c-d");
        assert_eq!(derive_slug("---"), "");
    }

    #[test]
    fn gate_detection_empty_dir_yields_no_gates() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_default_gates(tmp.path()).is_empty());
    }

    #[test]
    fn gate_detection_cargo_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let gates = detect_default_gates(tmp.path());
        let names: Vec<&str> = gates.iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["cargo-test", "cargo-clippy"]);
        assert_eq!(gates[0].gate_type, "test_command");
        assert_eq!(gates[0].command, "cargo test --all");
        assert_eq!(gates[1].gate_type, "lint_command");
    }

    #[test]
    fn gate_detection_arch_script() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("scripts")).unwrap();
        std::fs::write(
            tmp.path().join("scripts").join("check-arch.sh"),
            "#!/bin/bash\n",
        )
        .unwrap();
        let gates = detect_default_gates(tmp.path());
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].name, "arch-lint");
        assert_eq!(gates[0].gate_type, "lint_command");
    }

    #[test]
    fn starter_kit_creates_all_five_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_starter_kit(tmp.path()).unwrap();
        for path in [
            "specs/manifest.yaml",
            "specs/index.md",
            "specs/system/design-principles.md",
            "AGENTS.md",
            ".prek.yaml",
        ] {
            assert!(tmp.path().join(path).exists(), "missing {path}");
        }
    }

    #[test]
    fn starter_kit_never_overwrites_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
        std::fs::write(tmp.path().join("specs").join("index.md"), "CUSTOM").unwrap();
        write_starter_kit(tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("specs").join("index.md")).unwrap();
        assert_eq!(content, "CUSTOM");
    }

    #[test]
    fn starter_manifest_parses_as_documented_shape() {
        // Must parse as the server's SpecManifest (spec_registry.rs):
        // version u32 + specs: Vec<SpecEntry> with path/title/owner, and
        // approval (when present) as ApprovalConfig {mode: ...} -- the F5
        // defect was a bare `approval: human` string that fails
        // parse_manifest, silently emptying the spec ledger on push.
        let v: serde_yaml::Value = serde_yaml::from_str(STARTER_MANIFEST).unwrap();
        assert_eq!(v["version"].as_u64(), Some(1));
        let specs = v["specs"].as_sequence().expect("specs must be a list");
        assert!(!specs.is_empty());
        let first = &specs[0];
        // SpecEntry.path is relative to specs/ (sync_spec_ledger prefixes
        // "specs/" when resolving the file): system/design-principles.md.
        assert_eq!(
            first["path"].as_str().unwrap(),
            "system/design-principles.md"
        );
        assert_eq!(first["kind"].as_str(), Some("system"));
        assert_eq!(first["requires_approval"].as_bool(), Some(true));
        assert!(first["title"].as_str().is_some());
        assert!(first["owner"].as_str().is_some());
        // Approval must be a mapping with a known mode, never a scalar.
        if let Some(approval) = first.get("approval") {
            let mode = approval
                .get("mode")
                .and_then(|m| m.as_str())
                .unwrap_or_else(|| panic!("approval must be {{mode: ...}}, got: {approval:?}"));
            assert!(
                matches!(mode, "human_only" | "agent_only" | "human_and_agent"),
                "unknown approval mode: {mode}"
            );
        }
    }

    #[test]
    fn summary_renders_ids_key_and_urls() {
        let s = BootstrapSummary {
            tenant_id: "t1".into(),
            tenant_name: "Acme Corp".into(),
            workspace_id: "w1".into(),
            workspace_name: "Platform Team".into(),
            repo_id: "r1".into(),
            repo_name: "gyre".into(),
            admin_username: Some("jsell".into()),
            api_key: Some("gyre_secret".into()),
            server_url: "http://localhost:3000".into(),
            clone_url: Some("http://localhost:3000/git/platform-team/gyre".into()),
            orchestrator_agent_id: Some("a1".into()),
            orchestrator_launch_status: Some("running".into()),
            gates_configured: vec!["cargo-test".into()],
            specs_registered: 1,
            ..Default::default()
        };
        let text = s.render();
        assert!(text.contains("t1"));
        assert!(text.contains("w1"));
        assert!(text.contains("r1"));
        assert!(text.contains("gyre_secret"));
        assert!(text.contains("http://localhost:3000"));
        assert!(text.contains("platform-team/gyre"));
        assert!(text.contains("Your repo orchestrator is running"));
        assert!(text.contains("Visit http://localhost:3000 for the dashboard"));
    }

    #[test]
    fn summary_without_admin_key_omits_key_section() {
        let s = BootstrapSummary {
            tenant_id: "t".into(),
            tenant_name: "dev".into(),
            workspace_id: "w".into(),
            workspace_name: "default".into(),
            repo_id: "r".into(),
            repo_name: "repo".into(),
            server_url: "http://localhost:3000".into(),
            ..Default::default()
        };
        let text = s.render();
        assert!(!text.contains("API key"));
    }

    #[test]
    fn summary_does_not_claim_running_when_launch_failed() {
        // task-099 F3: a persisted agent row whose process failed to launch
        // is NOT a running orchestrator. The summary must say what happened.
        let s = BootstrapSummary {
            tenant_id: "t".into(),
            tenant_name: "dev".into(),
            workspace_id: "w".into(),
            workspace_name: "default".into(),
            repo_id: "r".into(),
            repo_name: "repo".into(),
            server_url: "http://localhost:3000".into(),
            orchestrator_agent_id: Some("a1".into()),
            orchestrator_launch_status: Some("launch_failed".into()),
            orchestrator_launch_detail: Some("No such file or directory (os error 2)".into()),
            ..Default::default()
        };
        let text = s.render();
        assert!(
            !text.contains("orchestrator is running"),
            "launch_failed must not be reported as running: {text}"
        );
        assert!(text.contains("failed to launch"));
        assert!(text.contains("No such file or directory"));
    }

    #[test]
    fn summary_reports_already_live_kept_as_is() {
        // 409 resume path: bootstrap keeps the existing live orchestrator;
        // the summary must not claim it spawned a new one.
        let s = BootstrapSummary {
            tenant_id: "t".into(),
            tenant_name: "dev".into(),
            workspace_id: "w".into(),
            workspace_name: "default".into(),
            repo_id: "r".into(),
            repo_name: "repo".into(),
            server_url: "http://localhost:3000".into(),
            ..Default::default()
        };
        let text = s.render();
        assert!(text.contains("already active"));
        assert!(!text.contains("is running"));
    }

    // ── push_and_sync_specs push-safety tests (round-11 F11/F12) ──────────
    //
    // Real git plumbing against a real bare "server" repo; only the
    // GyreClient's sync-specs call is served by the mock HTTP server.
    // These pin the resume contract: the first run replaces the server's
    // disposable empty seed, a local-ahead re-run fast-forwards, and a
    // re-run from a checkout that has not pulled after server-side MR
    // merges must NOT rewind main.

    /// Run git synchronously in `dir`, with a test identity so commits work
    /// in fresh repos. Panics on failure; test-fixture only.
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@gyre.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@gyre.test")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} in {} failed: {}",
            args,
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A bare "server" repo shaped exactly like repos.rs creates them:
    /// main pointing at a single root commit with an empty tree
    /// (create_initial_commit's treebuilder-over-nothing, no parents).
    fn init_server_bare_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "--bare", "-b", "main"]);
        let empty_tree = git(path, &["mktree"]);
        let seed = git(
            path,
            &["commit-tree", empty_tree.as_str(), "-m", "Initial commit"],
        );
        git(path, &["update-ref", "refs/heads/main", seed.as_str()]);
    }

    /// A user checkout: one commit with a README, plus an uncommitted
    /// specs/manifest.yaml (exercising the starter-kit auto-commit path).
    /// Its history is unrelated to the server seed, like every real
    /// first-run checkout.
    fn init_local_repo(path: &Path) {
        git(path, &["init", "-b", "main"]);
        // Identity must live in the repo config so the commits made INSIDE
        // push_and_sync_specs (starter-kit auto-commit) succeed too.
        git(path, &["config", "user.email", "t@gyre.test"]);
        git(path, &["config", "user.name", "t"]);
        std::fs::write(path.join("README.md"), "hello\n").unwrap();
        git(path, &["add", "README.md"]);
        git(path, &["commit", "-m", "first"]);
        std::fs::create_dir_all(path.join("specs")).unwrap();
        std::fs::write(path.join("specs").join("manifest.yaml"), "version: 1\n").unwrap();
    }

    fn server_main(path: &Path) -> String {
        git(path, &["rev-parse", "refs/heads/main"])
    }

    fn local_head(path: &Path) -> String {
        git(path, &["rev-parse", "HEAD"])
    }

    fn mock_sync_client(responses: usize) -> crate::client::GyreClient {
        let scripted = (0..responses)
            .map(|_| {
                (
                    "200 OK".to_string(),
                    r#"{"registered":1,"head_sha":"synced"}"#.to_string(),
                )
            })
            .collect();
        let (base, _captured) = crate::client::test_http::spawn_mock_http_server_sequence(scripted);
        crate::client::GyreClient::new(base, "test-token".to_string())
    }

    #[tokio::test]
    async fn push_sync_first_run_replaces_seed_and_resume_refuses_to_rewind_merged_main() {
        let server = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        init_server_bare_repo(server.path());
        init_local_repo(local.path());
        let api = mock_sync_client(1);
        let clone_url = server.path().to_string_lossy().to_string();

        // First run: the unrelated local history must replace the empty
        // seed (a plain push would be rejected as non-fast-forward).
        let registered = push_and_sync_specs(&api, local.path(), "r-1", &clone_url)
            .await
            .expect("first-run push must succeed");
        assert_eq!(registered, 1);
        // The uncommitted manifest was auto-committed and is on main
        // (the helper panics if the object is missing).
        git(
            local.path(),
            &["cat-file", "-e", "HEAD:specs/manifest.yaml"],
        );
        assert_eq!(server_main(server.path()), local_head(local.path()));

        // Simulate the server-side merge processor advancing main: an MR
        // branch merged into main in a scratch clone, pushed to the bare
        // repo — exactly what a merged MR leaves behind.
        let scratch = tempfile::tempdir().unwrap();
        git(scratch.path(), &["clone", &clone_url, "."]);
        git(scratch.path(), &["config", "user.email", "t@gyre.test"]);
        git(scratch.path(), &["config", "user.name", "t"]);
        git(scratch.path(), &["checkout", "-b", "feat"]);
        std::fs::write(scratch.path().join("feat.txt"), "x\n").unwrap();
        git(scratch.path(), &["add", "feat.txt"]);
        git(scratch.path(), &["commit", "-m", "feat"]);
        git(scratch.path(), &["push", "origin", "feat"]);
        git(scratch.path(), &["checkout", "main"]);
        git(
            scratch.path(),
            &["merge", "--no-ff", "origin/feat", "-m", "Merge MR"],
        );
        git(scratch.path(), &["push", "origin", "main"]);
        let merged = server_main(server.path());
        assert_ne!(merged, local_head(local.path()));

        // Re-run bootstrap from the stale local checkout (never pulled):
        // the push must be refused, not forced over the merge commit.
        let err = push_and_sync_specs(&api, local.path(), "r-1", &clone_url)
            .await
            .expect_err("a checkout behind merged main must not push");
        assert!(
            err.to_string().contains("refusing to push"),
            "error must explain the refusal, got: {err}"
        );
        assert_eq!(
            server_main(server.path()),
            merged,
            "server main was rewound out from under merged MR history"
        );
    }

    #[tokio::test]
    async fn push_sync_resume_fast_forwards_when_local_is_ahead() {
        let server = tempfile::tempdir().unwrap();
        let local = tempfile::tempdir().unwrap();
        init_server_bare_repo(server.path());
        init_local_repo(local.path());
        let api = mock_sync_client(2);
        let clone_url = server.path().to_string_lossy().to_string();

        push_and_sync_specs(&api, local.path(), "r-1", &clone_url)
            .await
            .expect("first-run push must succeed");

        // New local commits after the first run: the resume re-run must
        // push them (fast-forward), not skip the step.
        std::fs::write(local.path().join("more.txt"), "y\n").unwrap();
        git(local.path(), &["add", "more.txt"]);
        git(local.path(), &["commit", "-m", "second"]);
        push_and_sync_specs(&api, local.path(), "r-1", &clone_url)
            .await
            .expect("resume push with local-ahead history must succeed");
        assert_eq!(server_main(server.path()), local_head(local.path()));
    }

    #[test]
    fn git_auth_token_never_appears_in_child_argv() {
        // F12: the bearer token must reach git through environment config,
        // never through `-c http.extraHeader=...` argv (readable via ps and
        // /proc/<pid>/cmdline for the lifetime of every network push).
        let cmd = git_with_auth(Path::new("/tmp"), "secret-token-xyz");
        let std_cmd = cmd.as_std();
        let argv = std_cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !argv.contains("secret-token-xyz"),
            "token leaked into git argv: {argv}"
        );
        let env_value = std_cmd
            .get_envs()
            .find(|(k, _)| *k == "GIT_CONFIG_VALUE_0")
            .and_then(|(_, v)| v)
            .map(|v| v.to_string_lossy().to_string())
            .expect("GIT_CONFIG_VALUE_0 must be set");
        assert_eq!(env_value, "Authorization: Bearer secret-token-xyz");
    }
}
