//! Agent stack fingerprinting and the `gyre-stack.lock` lockfile
//! (supply-chain.md §Agent Stack Components, §gyre-stack.lock).
//!
//! An agent stack is the complete configuration under which an agent produces
//! code — the agentic equivalent of a build toolchain. The stack fingerprint
//! (SHA-256 over canonical sorted-key JSON) identifies the exact configuration.
//!
//! `gyre-stack.lock` pins that configuration in the repo itself, analogous to
//! a dependency lockfile: the CLI generates it from the agent's registered
//! stack (`gyre stack lock`), it is committed with the code, and the server
//! parses it from the pushed git tree during push processing to detect drift
//! between the repo's pinned stack and the pushing agent's attested stack.
//!
//! Domain placement: the CLI and the server must compute and verify the SAME
//! fingerprint over the SAME schema, so the type lives here where both can
//! import it without the CLI depending on the server crate.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Agent stack
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEntry {
    pub id: String,
    pub hash: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub version: String,
    pub config_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStack {
    /// SHA-256 hash of the AGENTS.md / CLAUDE.md file at agent startup.
    pub agents_md_hash: String,
    /// Pre-commit / pre-push hooks with their content hashes.
    pub hooks: Vec<HookEntry>,
    /// MCP servers the agent has configured.
    pub mcp_servers: Vec<McpServerEntry>,
    /// Model identifier (e.g. "claude-sonnet-4-6").
    pub model: String,
    /// CLI version string (e.g. "1.2.3").
    pub cli_version: String,
    /// SHA-256 hash of settings.json / settings.local.json.
    pub settings_hash: String,
    /// Optional SHA-256 hash of the persona / system-prompt file.
    pub persona_hash: Option<String>,
}

impl AgentStack {
    /// Compute a SHA-256 fingerprint of the stack by hashing canonical JSON
    /// with keys sorted alphabetically.  The resulting hex string uniquely
    /// identifies a particular agent configuration.
    pub fn fingerprint(&self) -> String {
        // Build a canonical JSON representation with sorted keys.
        let canonical = serde_json::json!({
            "agents_md_hash": self.agents_md_hash,
            "cli_version": self.cli_version,
            "hooks": self.hooks.iter().map(|h| serde_json::json!({
                "enabled": h.enabled,
                "hash": h.hash,
                "id": h.id,
            })).collect::<Vec<_>>(),
            "mcp_servers": self.mcp_servers.iter().map(|m| serde_json::json!({
                "config_hash": m.config_hash,
                "name": m.name,
                "version": m.version,
            })).collect::<Vec<_>>(),
            "model": self.model,
            "persona_hash": self.persona_hash,
            "settings_hash": self.settings_hash,
        });

        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        result.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ---------------------------------------------------------------------------
// gyre-stack.lock
// ---------------------------------------------------------------------------

/// Parsed `gyre-stack.lock` content — the reproducible state of the agent
/// stack, versioned with the code (supply-chain.md §gyre-stack.lock).
///
/// The file carries every stack component plus the composite fingerprint.
/// `lock_timestamp` records when the lock was generated; integrity is
/// checkable by recomputing the fingerprint from the components
/// (`verify_integrity`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackLockfile {
    /// Embedded composite stack fingerprint (`AgentStack::fingerprint()`).
    pub fingerprint: String,
    /// SHA-256 hash of the AGENTS.md / CLAUDE.md content.
    pub agents_md_hash: String,
    /// Pre-commit / pre-push hooks with their content hashes.
    pub hooks: Vec<HookEntry>,
    /// MCP servers pinned by the lock.
    pub mcp_servers: Vec<McpServerEntry>,
    /// Model identifier the stack ran.
    pub model: String,
    /// gyre CLI version string.
    pub cli_version: String,
    /// SHA-256 hash of settings.json / settings.local.json.
    pub settings_hash: String,
    /// Optional SHA-256 hash of the persona / system-prompt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_hash: Option<String>,
    /// Unix epoch seconds when the lockfile was generated.
    pub lock_timestamp: u64,
}

impl StackLockfile {
    /// Build a lockfile from a registered agent stack at generation time.
    pub fn from_stack(stack: &AgentStack, lock_timestamp: u64) -> Self {
        Self {
            fingerprint: stack.fingerprint(),
            agents_md_hash: stack.agents_md_hash.clone(),
            hooks: stack.hooks.clone(),
            mcp_servers: stack.mcp_servers.clone(),
            model: stack.model.clone(),
            cli_version: stack.cli_version.clone(),
            settings_hash: stack.settings_hash.clone(),
            persona_hash: stack.persona_hash.clone(),
            lock_timestamp,
        }
    }

    /// The stack components this lock pins, as an `AgentStack` (dropping the
    /// lock metadata) so the composite fingerprint can be recomputed.
    pub fn stack(&self) -> AgentStack {
        AgentStack {
            agents_md_hash: self.agents_md_hash.clone(),
            hooks: self.hooks.clone(),
            mcp_servers: self.mcp_servers.clone(),
            model: self.model.clone(),
            cli_version: self.cli_version.clone(),
            settings_hash: self.settings_hash.clone(),
            persona_hash: self.persona_hash.clone(),
        }
    }

    /// Recompute the fingerprint from the lock's components and compare with
    /// the embedded `fingerprint`. `Ok(())` means the lockfile is internally
    /// consistent — nobody edited a component without regenerating the
    /// fingerprint.
    pub fn verify_integrity(&self) -> Result<(), String> {
        let computed = self.stack().fingerprint();
        if computed == self.fingerprint {
            Ok(())
        } else {
            Err(format!(
                "gyre-stack.lock integrity failure: embedded fingerprint {} does not match components (computed {})",
                self.fingerprint, computed
            ))
        }
    }

    /// Compare this lock against a stack attestation. `Ok(())` when the
    /// attested stack matches the pinned stack; `Err(reason)` describes the
    /// drift otherwise.
    pub fn check_drift(&self, attested: &AgentStack) -> Result<(), String> {
        let attested_fp = attested.fingerprint();
        if attested_fp == self.fingerprint {
            Ok(())
        } else {
            // Name the first differing component so the drift reason is
            // actionable (regenerate the lock or fix the local config).
            let mut reasons: Vec<String> = Vec::new();
            if attested.agents_md_hash != self.agents_md_hash {
                reasons.push("agents_md_hash".to_string());
            }
            if attested.hooks != self.hooks {
                reasons.push("hooks".to_string());
            }
            if attested.mcp_servers != self.mcp_servers {
                reasons.push("mcp_servers".to_string());
            }
            if attested.model != self.model {
                reasons.push("model".to_string());
            }
            if attested.cli_version != self.cli_version {
                reasons.push("cli_version".to_string());
            }
            if attested.settings_hash != self.settings_hash {
                reasons.push("settings_hash".to_string());
            }
            if attested.persona_hash != self.persona_hash {
                reasons.push("persona_hash".to_string());
            }
            if reasons.is_empty() {
                reasons.push("unknown".to_string());
            }
            Err(format!(
                "stack drift: gyre-stack.lock pins fingerprint {} but agent attested {} (differs in: {})",
                self.fingerprint,
                attested_fp,
                reasons.join(", ")
            ))
        }
    }

    /// Serialize to TOML — the on-disk `gyre-stack.lock` format. Components
    /// are emitted as `[[hooks]]` / `[[mcp_servers]]` array-of-tables.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|e| format!("failed to serialize gyre-stack.lock: {e}"))
    }

    /// Parse `gyre-stack.lock` content (TOML).
    pub fn parse(content: &str) -> Result<Self, String> {
        toml::from_str(content).map_err(|e| format!("failed to parse gyre-stack.lock: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stack() -> AgentStack {
        AgentStack {
            agents_md_hash: "hash1".to_string(),
            hooks: vec![
                HookEntry {
                    id: "block-secrets".to_string(),
                    hash: "cafebabe".to_string(),
                    enabled: true,
                },
                HookEntry {
                    id: "cargo-fmt".to_string(),
                    hash: "deadbeef".to_string(),
                    enabled: false,
                },
            ],
            mcp_servers: vec![McpServerEntry {
                name: "odis".to_string(),
                version: "1.0".to_string(),
                config_hash: "abc123".to_string(),
            }],
            model: "claude-sonnet-4-6".to_string(),
            cli_version: "1.2.3".to_string(),
            settings_hash: "settings1".to_string(),
            persona_hash: Some("persona1".to_string()),
        }
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let stack = sample_stack();
        assert_eq!(stack.fingerprint(), stack.fingerprint());
        assert_eq!(stack.fingerprint().len(), 64);
    }

    #[test]
    fn different_stacks_have_different_fingerprints() {
        let stack1 = sample_stack();
        let stack2 = AgentStack {
            agents_md_hash: "hash2".to_string(),
            ..stack1.clone()
        };
        assert_ne!(stack1.fingerprint(), stack2.fingerprint());
    }

    #[test]
    fn lockfile_toml_roundtrip() {
        let stack = sample_stack();
        let lock = StackLockfile::from_stack(&stack, 1_700_000_000);
        let toml_str = lock.to_toml().unwrap();
        // Sanity: the file names its components as array-of-tables.
        assert!(toml_str.contains("[[hooks]]"));
        assert!(toml_str.contains("[[mcp_servers]]"));
        assert!(toml_str.contains("fingerprint"));

        let parsed = StackLockfile::parse(&toml_str).unwrap();
        assert_eq!(parsed.fingerprint, lock.fingerprint);
        assert_eq!(parsed.agents_md_hash, lock.agents_md_hash);
        assert_eq!(parsed.hooks, lock.hooks);
        assert_eq!(parsed.mcp_servers, lock.mcp_servers);
        assert_eq!(parsed.model, lock.model);
        assert_eq!(parsed.cli_version, lock.cli_version);
        assert_eq!(parsed.settings_hash, lock.settings_hash);
        assert_eq!(parsed.persona_hash, lock.persona_hash);
        assert_eq!(parsed.lock_timestamp, lock.lock_timestamp);
    }

    #[test]
    fn lockfile_from_stack_is_internally_consistent() {
        let stack = sample_stack();
        let lock = StackLockfile::from_stack(&stack, 1_700_000_000);
        assert!(lock.verify_integrity().is_ok());
        // Roundtripped lock is consistent too.
        let parsed = StackLockfile::parse(&lock.to_toml().unwrap()).unwrap();
        assert!(parsed.verify_integrity().is_ok());
    }

    #[test]
    fn lockfile_detects_tampered_components() {
        let stack = sample_stack();
        let mut lock = StackLockfile::from_stack(&stack, 1_700_000_000);
        // Tamper: change a component without regenerating the fingerprint.
        lock.model = "claude-opus-4-6".to_string();
        let err = lock.verify_integrity().unwrap_err();
        assert!(err.contains("integrity failure"), "got: {err}");
    }

    #[test]
    fn lockfile_no_drift_when_stack_matches() {
        let stack = sample_stack();
        let lock = StackLockfile::from_stack(&stack, 1_700_000_000);
        assert!(lock.check_drift(&stack).is_ok());
        // An equal-but-reconstructed stack also matches.
        let same = lock.stack();
        assert!(lock.check_drift(&same).is_ok());
    }

    #[test]
    fn lockfile_flags_drift_with_component_names() {
        let stack = sample_stack();
        let lock = StackLockfile::from_stack(&stack, 1_700_000_000);

        // Drift in two components: model + one hook hash.
        let drifted = AgentStack {
            model: "claude-opus-4-6".to_string(),
            hooks: vec![
                HookEntry {
                    id: "block-secrets".to_string(),
                    hash: "tampered".to_string(),
                    enabled: true,
                },
                HookEntry {
                    id: "cargo-fmt".to_string(),
                    hash: "deadbeef".to_string(),
                    enabled: false,
                },
            ],
            ..stack.clone()
        };
        let err = lock.check_drift(&drifted).unwrap_err();
        assert!(err.contains("stack drift"), "got: {err}");
        assert!(err.contains("model"), "reason should name model: {err}");
        assert!(err.contains("hooks"), "reason should name hooks: {err}");
        assert!(!err.contains("cli_version"), "unrelated component: {err}");
    }

    #[test]
    fn lockfile_parse_rejects_invalid_toml() {
        let err = StackLockfile::parse("not [valid toml").unwrap_err();
        assert!(err.contains("failed to parse"), "got: {err}");
    }

    #[test]
    fn lockfile_parse_accepts_missing_optional_persona() {
        let stack = AgentStack {
            persona_hash: None,
            ..sample_stack()
        };
        let lock = StackLockfile::from_stack(&stack, 0);
        let toml_str = lock.to_toml().unwrap();
        assert!(!toml_str.contains("persona_hash"));
        let parsed = StackLockfile::parse(&toml_str).unwrap();
        assert_eq!(parsed.persona_hash, None);
        assert!(parsed.verify_integrity().is_ok());
    }
}
