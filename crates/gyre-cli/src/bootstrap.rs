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

pub const STARTER_MANIFEST: &str = r#"version: 1
specs:
  - path: specs/system/design-principles.md
    title: Design Principles
    owner: admin
    kind: system
    approval: human
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
                p.system_prompt.len() > 500,
                "persona {} prompt too short: {} bytes",
                p.slug,
                p.system_prompt.len()
            );
            assert!(!p.capabilities.is_empty(), "persona {} needs capabilities", p.slug);
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
            assert!(p.system_prompt.starts_with('#'), "spec-file prompt for {}", p.slug);
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
        // Must parse as SpecManifest: version u32 + specs: Vec<SpecEntry> with
        // path/title/owner. (serde_yaml is available via gyre-common? No -
        // parse structurally with a minimal shape check.)
        let v: serde_yaml::Value = serde_yaml::from_str(STARTER_MANIFEST).unwrap();
        assert_eq!(v["version"].as_u64(), Some(1));
        let specs = v["specs"].as_sequence().expect("specs must be a list");
        assert!(!specs.is_empty());
        let first = &specs[0];
        assert!(first["path"].as_str().unwrap().starts_with("specs/"));
        assert!(first["title"].as_str().is_some());
        assert!(first["owner"].as_str().is_some());
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
