//! CLI configuration: stored at ~/.gyre/config as JSON.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// Base HTTP URL of the Gyre server (e.g. "http://localhost:3333").
    pub server: String,
    /// Bearer token for this agent.
    pub token: Option<String>,
    /// Agent ID assigned at init.
    pub agent_id: Option<String>,
    /// Human-readable agent name.
    pub agent_name: Option<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".gyre").join("config")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self {
                server: "http://localhost:3333".to_string(),
                ..Default::default()
            });
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).context("parsing ~/.gyre/config")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn require_token(&self) -> Result<&str> {
        self.token
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Not initialized. Run `gyre init` first."))
    }

    pub fn require_agent_id(&self) -> Result<&str> {
        self.agent_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Not initialized. Run `gyre init` first."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_token_errors_when_missing() {
        let cfg = Config::default();
        assert!(cfg.require_token().is_err());
    }

    #[test]
    fn require_token_ok_when_set() {
        let cfg = Config {
            token: Some("tok".to_string()),
            ..Default::default()
        };
        assert_eq!(cfg.require_token().unwrap(), "tok");
    }

    #[test]
    fn require_agent_id_errors_when_missing() {
        let cfg = Config::default();
        assert!(cfg.require_agent_id().is_err());
    }

    #[test]
    fn require_agent_id_ok_when_set() {
        let cfg = Config {
            agent_id: Some("agent-42".to_string()),
            ..Default::default()
        };
        assert_eq!(cfg.require_agent_id().unwrap(), "agent-42");
    }

    #[test]
    fn config_serializes_to_json() {
        let cfg = Config {
            server: "http://localhost:3333".to_string(),
            token: Some("abc".to_string()),
            agent_id: Some("id-1".to_string()),
            agent_name: Some("test".to_string()),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.server, cfg.server);
        assert_eq!(parsed.token, cfg.token);
        assert_eq!(parsed.agent_id, cfg.agent_id);
    }

    #[test]
    fn config_path_ends_with_gyre_config() {
        let path = Config::path();
        assert!(path.ends_with(".gyre/config"));
    }
}
