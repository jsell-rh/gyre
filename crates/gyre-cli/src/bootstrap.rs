//! Bootstrap logic for `gyre bootstrap` (platform-model.md §8).
//!
//! Pure, unit-testable pieces: persona prompt registry, slug derivation,
//! default gate detection, starter-kit generation, and summary rendering.
//! The orchestration (HTTP calls, step ordering) lives in main.rs.

use std::path::Path;

// ─── Persona prompts ──────────────────────────────────────────────────────────

pub const WORKSPACE_ORCHESTRATOR_PROMPT: &str =
    include_str!("bootstrap/personas/workspace-orchestrator.md");
pub const REPO_ORCHESTRATOR_PROMPT: &str = include_str!("bootstrap/personas/repo-orchestrator.md");
pub const ACCOUNTABILITY_PROMPT: &str = include_str!("bootstrap/personas/accountability.md");
pub const SECURITY_PROMPT: &str = include_str!("bootstrap/personas/security.md");

/// A built-in persona to register during bootstrap (platform-model.md §2).
pub struct BuiltinPersona {
    pub name: &'static str,
    pub slug: &'static str,
    pub prompt: &'static str,
    pub capabilities: &'static [&'static str],
    pub protocols: &'static [&'static str],
}

/// The four built-in personas, all pre-approved at Tenant scope (spec §8 step 5).
pub const BUILTIN_PERSONAS: &[BuiltinPersona] = &[
    BuiltinPersona {
        name: "Workspace Orchestrator",
        slug: "workspace-orchestrator",
        prompt: WORKSPACE_ORCHESTRATOR_PROMPT,
        capabilities: &["task.create", "spec.read", "cross-repo-analysis"],
        protocols: &["mcp", "escalation", "handoff"],
    },
    BuiltinPersona {
        name: "Repo Orchestrator",
        slug: "repo-orchestrator",
        prompt: REPO_ORCHESTRATOR_PROMPT,
        capabilities: &["task.create", "task.decompose", "agent.dispatch", "merge.queue"],
        protocols: &["mcp", "ralph-loop", "escalation", "handoff"],
    },
    BuiltinPersona {
        name: "Accountability Agent",
        slug: "accountability",
        prompt: ACCOUNTABILITY_PROMPT,
        capabilities: &["spec.read", "code.read", "drift.report"],
        protocols: &["mcp", "patrol"],
    },
    BuiltinPersona {
        name: "Security Agent",
        slug: "security",
        prompt: SECURITY_PROMPT,
        capabilities: &["code.read", "threat.report", "dependency.audit"],
        protocols: &["mcp", "patrol", "escalation"],
    },
];

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
    let principles = root.join("specs").join("system").join("design-principles.md");
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
) -> Result<usize> {
    // Push the current branch to the server's bare repo (all specs; the
    // server's post-receive hook also handles default-branch pushes).
    let push = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args([
            "-c",
            &format!("http.extraHeader=Authorization: Bearer {}", api.token()),
        ])
        .args(["push", clone_url, "HEAD:refs/heads/main"])
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
    Ok(synced.registered)
}

pub const STARTER_MANIFEST: &str = r#"version: 1

defaults:
  requires_approval: true
  auto_create_tasks: true
  auto_invalidate_on_change: true

specs:
  - path: system/design-principles.md
    title: Design Principles
    owner: admin
    kind: system
    approval:
      mode: human_only
    requires_approval: true
"#;

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
    pub orchestrator_agent_id: Option<String>,
    /// Specs registered in the platform ledger during step 6.
    pub specs_registered: usize,
    pub gates_configured: Vec<String>,
    pub personas_registered: Vec<String>,
}

impl BootstrapSummary {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Bootstrap complete!\n\n");
        out.push_str(&format!("  Tenant:     {} ({})\n", self.tenant_name, self.tenant_id));
        out.push_str(&format!(
            "  Workspace:  {} ({})\n",
            self.workspace_name, self.workspace_id
        ));
        out.push_str(&format!("  Repo:       {} ({})\n", self.repo_name, self.repo_id));
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
        if let Some(agent) = &self.orchestrator_agent_id {
            out.push_str(&format!(
                "\nYour repo orchestrator is running (agent {agent}).\n"
            ));
        } else {
            out.push_str("\nYour repo orchestrator is running.\n");
        }
        out.push_str(&format!(
            "Visit {} for the dashboard.\n",
            self.server_url
        ));
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
                p.prompt.len() > 500,
                "persona {} prompt too short: {} bytes",
                p.slug,
                p.prompt.len()
            );
            assert!(!p.capabilities.is_empty(), "persona {} needs capabilities", p.slug);
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
        let content =
            std::fs::read_to_string(tmp.path().join("specs").join("index.md")).unwrap();
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
            gates_configured: vec!["cargo-test".into()],
            personas_registered: vec!["repo-orchestrator".into()],
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
}
