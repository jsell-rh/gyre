use gyre_common::Id;
use serde::{Deserialize, Serialize};

use crate::task::TaskPriority;

/// Declarative spec for an agent multi-agent hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCompose {
    /// Schema version, currently "1".
    pub version: String,
    pub repo_id: Id,
    pub agents: Vec<AgentSpec>,
}

/// Spec for a single agent in a compose.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    /// Maps to agent persona / role description.
    pub role: String,
    /// Name of the parent agent (must reference another agent in this compose).
    pub parent: Option<String>,
    pub capabilities: Vec<String>,
    pub task: Option<TaskSpec>,
    pub branch: Option<String>,
    pub lifetime_secs: Option<u64>,
}

/// Inline task specification within an AgentSpec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub title: String,
    pub description: Option<String>,
    pub priority: TaskPriority,
}

impl AgentCompose {
    /// Validate the compose spec: no cycles in parent refs, unique names.
    /// Returns Ok(ordered) where ordered is the agents in topological order (parents first).
    pub fn validate_and_sort(&self) -> Result<Vec<&AgentSpec>, String> {
        // Check unique names
        let mut names = std::collections::HashSet::new();
        for spec in &self.agents {
            if !names.insert(spec.name.as_str()) {
                return Err(format!("duplicate agent name: {}", spec.name));
            }
        }

        // Check all parent references are valid
        for spec in &self.agents {
            if let Some(parent) = &spec.parent {
                if !names.contains(parent.as_str()) {
                    return Err(format!(
                        "agent '{}' references unknown parent '{}'",
                        spec.name, parent
                    ));
                }
            }
        }

        // Topological sort (detect cycles)
        let mut in_degree: std::collections::HashMap<&str, usize> =
            self.agents.iter().map(|s| (s.name.as_str(), 0)).collect();
        let mut children: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();

        for spec in &self.agents {
            if let Some(parent) = &spec.parent {
                *in_degree.entry(spec.name.as_str()).or_insert(0) += 1;
                children
                    .entry(parent.as_str())
                    .or_default()
                    .push(spec.name.as_str());
            }
        }

        let mut queue: std::collections::VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&name, _)| name)
            .collect();

        let mut ordered = Vec::new();
        while let Some(name) = queue.pop_front() {
            ordered.push(name);
            if let Some(kids) = children.get(name) {
                for &child in kids {
                    let deg = in_degree.entry(child).or_insert(0);
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }

        if ordered.len() != self.agents.len() {
            return Err("cycle detected in parent references".to_string());
        }

        // Map back to AgentSpec refs in topological order
        let spec_map: std::collections::HashMap<&str, &AgentSpec> =
            self.agents.iter().map(|s| (s.name.as_str(), s)).collect();

        Ok(ordered.into_iter().map(|n| spec_map[n]).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_compose(agents: Vec<AgentSpec>) -> AgentCompose {
        AgentCompose {
            version: "1".to_string(),
            repo_id: Id::new("repo-1"),
            agents,
        }
    }

    fn make_spec(name: &str, parent: Option<&str>) -> AgentSpec {
        AgentSpec {
            name: name.to_string(),
            role: "developer".to_string(),
            parent: parent.map(|s| s.to_string()),
            capabilities: vec!["rust-dev".to_string()],
            task: Some(TaskSpec {
                title: format!("{} task", name),
                description: None,
                priority: TaskPriority::Medium,
            }),
            branch: None,
            lifetime_secs: None,
        }
    }

    #[test]
    fn test_valid_compose_no_parent() {
        let compose = make_compose(vec![make_spec("agent-a", None), make_spec("agent-b", None)]);
        let result = compose.validate_and_sort();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_valid_compose_with_hierarchy() {
        let compose = make_compose(vec![
            make_spec("child", Some("parent")),
            make_spec("parent", None),
        ]);
        let result = compose.validate_and_sort().unwrap();
        // parent must come before child
        let parent_pos = result.iter().position(|s| s.name == "parent").unwrap();
        let child_pos = result.iter().position(|s| s.name == "child").unwrap();
        assert!(parent_pos < child_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let compose = make_compose(vec![
            AgentSpec {
                name: "a".to_string(),
                role: "dev".to_string(),
                parent: Some("b".to_string()),
                capabilities: vec![],
                task: None,
                branch: None,
                lifetime_secs: None,
            },
            AgentSpec {
                name: "b".to_string(),
                role: "dev".to_string(),
                parent: Some("a".to_string()),
                capabilities: vec![],
                task: None,
                branch: None,
                lifetime_secs: None,
            },
        ]);
        assert!(compose.validate_and_sort().is_err());
    }

    #[test]
    fn test_unknown_parent() {
        let compose = make_compose(vec![make_spec("child", Some("nonexistent"))]);
        let err = compose.validate_and_sort().unwrap_err();
        assert!(err.contains("unknown parent"));
    }

    #[test]
    fn test_duplicate_name() {
        let compose = make_compose(vec![make_spec("dup", None), make_spec("dup", None)]);
        let err = compose.validate_and_sort().unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn test_yaml_serialization() {
        let compose = make_compose(vec![make_spec("agent-a", None)]);
        let json = serde_json::to_string(&compose).unwrap();
        let decoded: AgentCompose = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.agents.len(), 1);
    }
}
