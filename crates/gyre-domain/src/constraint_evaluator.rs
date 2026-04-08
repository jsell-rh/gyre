//! CEL constraint evaluator for authorization provenance (§3).
//!
//! Evaluates output constraints (CEL predicates) against a context assembled
//! from the actual state of work. The evaluator is fail-closed: evaluation
//! errors are treated as failures.

use cel_interpreter::{Context, Program, Value};
use gyre_common::attestation::{
    GateConstraint, InputContent, OutputConstraint, ScopeConstraint, VerificationResult,
};

// ── §3.3 CEL Evaluation Context ───────────────────────────────────────

/// The output portion of the CEL evaluation context (§3.3).
///
/// Computed from the actual git diff at verification time — not self-reported
/// by the agent.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OutputContext {
    /// Files modified in the diff.
    pub changed_files: Vec<String>,
    /// Files newly created.
    pub added_files: Vec<String>,
    /// Files removed.
    pub deleted_files: Vec<String>,
    /// Diff statistics.
    pub diff_stats: DiffStatsContext,
    /// Full commit message.
    pub commit_message: String,
    /// Git commit SHA.
    pub commit_sha: String,
}

/// Diff statistics within the output context (§3.3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffStatsContext {
    /// Lines inserted.
    pub insertions: u64,
    /// Lines deleted.
    pub deletions: u64,
}

/// The agent portion of the CEL evaluation context (§3.3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentContext {
    /// Agent identifier (e.g., "agent:worker-42").
    pub id: String,
    /// Agent persona (e.g., "security").
    pub persona: String,
    /// Hash of the agent's container image stack.
    pub stack_hash: String,
    /// Agent attestation level (0–3).
    pub attestation_level: i64,
    /// Hash of the meta-spec set the agent is running.
    pub meta_spec_set_sha: String,
    /// Who spawned this agent.
    pub spawned_by: String,
    /// Task identifier.
    pub task_id: String,
    /// Container identifier.
    pub container_id: String,
    /// Hash of the container image.
    pub image_hash: String,
}

/// The target portion of the CEL evaluation context (§3.3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetContext {
    /// Target repository ID.
    pub repo_id: String,
    /// Workspace ID.
    pub workspace_id: String,
    /// Branch being pushed/merged to.
    pub branch: String,
    /// Repository default branch.
    pub default_branch: String,
}

/// The action being performed — "push" or "merge" (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Push,
    Merge,
}

/// All components needed to build the CEL evaluation context (§3.3).
pub struct ConstraintInput<'a> {
    pub input: &'a InputContent,
    pub output: &'a OutputContext,
    pub agent: &'a AgentContext,
    pub target: &'a TargetContext,
    pub action: Action,
}

// ── Context Building ──────────────────────────────────────────────────

/// Build a CEL evaluation context from the constraint input components (§3.3).
///
/// Serializes each component into the CEL context as a nested map, producing
/// the structure defined in §3.3: `input`, `output`, `agent`, `target`, `action`.
pub fn build_cel_context(ci: &ConstraintInput<'_>) -> Result<Context<'static>, anyhow::Error> {
    let mut ctx = Context::default();
    ctx.add_variable("input", ci.input)
        .map_err(|e| anyhow::anyhow!("failed to serialize input to CEL context: {e}"))?;
    ctx.add_variable("output", ci.output)
        .map_err(|e| anyhow::anyhow!("failed to serialize output to CEL context: {e}"))?;
    ctx.add_variable("agent", ci.agent)
        .map_err(|e| anyhow::anyhow!("failed to serialize agent to CEL context: {e}"))?;
    ctx.add_variable("target", ci.target)
        .map_err(|e| anyhow::anyhow!("failed to serialize target to CEL context: {e}"))?;
    // Action is a simple string value in the CEL context.
    let action_str = match ci.action {
        Action::Push => "push",
        Action::Merge => "merge",
    };
    ctx.add_variable_from_value(
        "action",
        Value::String(std::sync::Arc::new(action_str.to_string())),
    );
    Ok(ctx)
}

// ── §3.4 Constraint Evaluation ────────────────────────────────────────

/// Evaluate a single output constraint against the CEL context (§3.4).
///
/// Returns `Ok(true)` if the constraint passes, `Ok(false)` if it fails,
/// or `Err` if the CEL expression is malformed or evaluation errors occur.
/// Callers should treat `Err` as a constraint failure (fail-closed).
pub fn evaluate_constraint(
    constraint: &OutputConstraint,
    ctx: &Context<'_>,
) -> Result<bool, anyhow::Error> {
    let program = Program::compile(&constraint.expression)
        .map_err(|e| anyhow::anyhow!("CEL parse error in constraint '{}': {e}", constraint.name))?;
    let result = program.execute(ctx).map_err(|e| {
        anyhow::anyhow!(
            "CEL evaluation error in constraint '{}': {e}",
            constraint.name
        )
    })?;
    match result {
        Value::Bool(b) => Ok(b),
        other => Err(anyhow::anyhow!(
            "constraint '{}' evaluated to non-boolean: {:?}",
            constraint.name,
            other
        )),
    }
}

/// Evaluate all constraints sequentially with fail-closed semantics (§3.4).
///
/// The first failure stops evaluation and rejects the action. CEL evaluation
/// errors (malformed expressions, type errors, missing fields) are treated as
/// failures. There is no "evaluation error -> allow" path.
///
/// Constraints from all three sources (explicit, strategy-implied, gate) should
/// be combined and passed together — they are additive.
pub fn evaluate_all(constraints: &[OutputConstraint], ctx: &Context<'_>) -> VerificationResult {
    let mut children = Vec::new();
    for constraint in constraints {
        match evaluate_constraint(constraint, ctx) {
            Ok(true) => {
                children.push(VerificationResult {
                    label: constraint.name.clone(),
                    valid: true,
                    message: "constraint passed".to_string(),
                    children: vec![],
                });
            }
            Ok(false) => {
                children.push(VerificationResult {
                    label: constraint.name.clone(),
                    valid: false,
                    message: format!("constraint failed: {}", constraint.name),
                    children: vec![],
                });
                // First failure stops evaluation (§3.4).
                return VerificationResult {
                    label: "constraint evaluation".to_string(),
                    valid: false,
                    message: format!("constraint failed: {}", constraint.name),
                    children,
                };
            }
            Err(e) => {
                // Fail-closed: evaluation errors are treated as failures (§3.4).
                children.push(VerificationResult {
                    label: constraint.name.clone(),
                    valid: false,
                    message: format!("constraint evaluation error: {e}"),
                    children: vec![],
                });
                return VerificationResult {
                    label: "constraint evaluation".to_string(),
                    valid: false,
                    message: format!("constraint evaluation error: {e}"),
                    children,
                };
            }
        }
    }
    VerificationResult {
        label: "constraint evaluation".to_string(),
        valid: true,
        message: format!("{} constraint(s) passed", constraints.len()),
        children,
    }
}

// ── §3.2 Strategy-Implied Constraints ─────────────────────────────────

/// Convert a glob pattern (e.g., `src/payments/**`) to a regex pattern
/// for use in CEL `matches()` expressions.
///
/// Handles `**` (recursive directory match) and `*` (single-segment match).
fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next(); // consume second *
                                  // Consume trailing slash if present (e.g., "**/" → match any path prefix).
                    if chars.peek() == Some(&'/') {
                        chars.next();
                    }
                    regex.push_str(".*");
                } else {
                    // Single * matches anything except /
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push('.'),
            '.' | '+' | '^' | '$' | '(' | ')' | '{' | '}' | '[' | ']' | '|' | '\\' => {
                regex.push('\\');
                regex.push(c);
            }
            _ => regex.push(c),
        }
    }
    regex.push('$');
    regex
}

/// Derive strategy-implied constraints from `InputContent` and workspace
/// configuration (§3.2).
///
/// These constraints are not signed by the user — they are derived from the
/// signed content at verification time. Changing the content invalidates the
/// user's signature, making these tamper-proof.
///
/// # Constraint sources
///
/// - `persona_constraints` → agent persona must match
/// - `meta_spec_set_sha` → agent meta-spec set must match
/// - `scope.allowed_paths` → all changed files must match allowed globs
/// - `scope.forbidden_paths` → no changed file may match forbidden globs
/// - `workspace_trust_level` → additional constraints for supervised workspaces (if provided)
/// - `required_attestation_level` → agent attestation level must meet minimum (if provided)
pub fn derive_strategy_constraints(
    content: &InputContent,
    workspace_trust_level: Option<&str>,
    required_attestation_level: Option<i64>,
) -> Vec<OutputConstraint> {
    let mut constraints = Vec::new();

    // §3.2 — From persona_constraints: agent persona must match one of the
    // required personas.  Uses a single membership constraint so that multiple
    // persona entries are satisfiable (a scalar `agent.persona` cannot equal
    // two different values simultaneously under AND semantics).
    if !content.persona_constraints.is_empty() {
        let names: Vec<&str> = content
            .persona_constraints
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        constraints.push(OutputConstraint {
            name: format!("strategy: agent persona must match one of {:?}", names),
            expression: "input.persona_constraints.exists(p, p.name == agent.persona)".to_string(),
        });
    }

    // §3.2 — From meta_spec_set_sha: agent's meta-spec set must match.
    constraints.push(OutputConstraint {
        name: "strategy: agent meta-spec set must match approved set".to_string(),
        expression: "agent.meta_spec_set_sha == input.meta_spec_set_sha".to_string(),
    });

    // §3.2 — From scope.allowed_paths: changed files must match allowed globs.
    derive_path_constraints(&content.scope, &mut constraints);

    // §3.2 — From workspace trust level: supervised workspaces get stricter constraints.
    if let Some(trust_level) = workspace_trust_level {
        if trust_level == "supervised" {
            // Supervised workspaces require attestation level >= 2.
            constraints.push(OutputConstraint {
                name: "strategy: supervised workspace requires attestation level >= 2".to_string(),
                expression: "agent.attestation_level >= 2".to_string(),
            });
        }
    }

    // §3.2 — From attestation level policy: agent attestation level must meet minimum.
    if let Some(level) = required_attestation_level {
        constraints.push(OutputConstraint {
            name: format!("strategy: attestation level must be >= {level}"),
            expression: format!("agent.attestation_level >= {level}"),
        });
    }

    constraints
}

/// Derive path constraints from `ScopeConstraint` (§3.2).
///
/// - `allowed_paths` (if non-empty): all changed files must match at least one allowed glob.
/// - `forbidden_paths`: no changed file may match any forbidden glob.
fn derive_path_constraints(scope: &ScopeConstraint, constraints: &mut Vec<OutputConstraint>) {
    // Allowed paths: empty means "any file" (no constraint).
    if !scope.allowed_paths.is_empty() {
        // Build a CEL expression that checks every changed file matches at least one allowed path.
        // For a single allowed path:  output.changed_files.all(f, f.matches("^src/payments/.*$"))
        // For multiple:               output.changed_files.all(f, f.matches("^a/.*$") || f.matches("^b/.*$"))
        let conditions: Vec<String> = scope
            .allowed_paths
            .iter()
            .map(|glob| format!("f.matches(\"{}\")", glob_to_regex(glob)))
            .collect();
        let combined = conditions.join(" || ");
        constraints.push(OutputConstraint {
            name: "strategy: changed files must match allowed paths".to_string(),
            expression: format!("output.changed_files.all(f, {combined})"),
        });
    }

    // Forbidden paths: always enforced.
    for glob in &scope.forbidden_paths {
        let regex = glob_to_regex(glob);
        constraints.push(OutputConstraint {
            name: format!("strategy: changed files must not match forbidden path '{glob}'"),
            expression: format!("output.changed_files.all(f, !f.matches(\"{regex}\"))"),
        });
    }
}

/// Collect all constraints from the three sources (§3.2): explicit user
/// constraints, strategy-implied constraints, and gate constraints.
///
/// All sources are additive — the full constraint set is the union of all three.
pub fn collect_all_constraints(
    explicit: &[OutputConstraint],
    strategy_implied: &[OutputConstraint],
    gate_constraints: &[GateConstraint],
) -> Vec<OutputConstraint> {
    let mut all =
        Vec::with_capacity(explicit.len() + strategy_implied.len() + gate_constraints.len());
    all.extend_from_slice(explicit);
    all.extend_from_slice(strategy_implied);
    for gc in gate_constraints {
        all.push(gc.constraint.clone());
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::attestation::{PersonaRef, ScopeConstraint};

    // ── Test helpers ──────────────────────────────────────────────────

    fn sample_input_content() -> InputContent {
        InputContent {
            spec_path: "specs/system/payments.md".to_string(),
            spec_sha: "abc123".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            persona_constraints: vec![PersonaRef {
                name: "security".to_string(),
            }],
            meta_spec_set_sha: "def456".to_string(),
            scope: ScopeConstraint {
                allowed_paths: vec!["src/payments/**".to_string()],
                forbidden_paths: vec!["src/auth/**".to_string()],
            },
        }
    }

    fn sample_output_context() -> OutputContext {
        OutputContext {
            changed_files: vec![
                "src/payments/handler.rs".to_string(),
                "src/payments/mod.rs".to_string(),
            ],
            added_files: vec!["src/payments/refund.rs".to_string()],
            deleted_files: vec![],
            diff_stats: DiffStatsContext {
                insertions: 142,
                deletions: 7,
            },
            commit_message: "feat(payments): add refund endpoint per specs/system/payments.md"
                .to_string(),
            commit_sha: "789abc".to_string(),
        }
    }

    fn sample_agent_context() -> AgentContext {
        AgentContext {
            id: "agent:worker-42".to_string(),
            persona: "security".to_string(),
            stack_hash: "sha256:abc".to_string(),
            attestation_level: 3,
            meta_spec_set_sha: "def456".to_string(),
            spawned_by: "user:jsell".to_string(),
            task_id: "TASK-007".to_string(),
            container_id: "container-1".to_string(),
            image_hash: "sha256:img".to_string(),
        }
    }

    fn sample_target_context() -> TargetContext {
        TargetContext {
            repo_id: "repo-1".to_string(),
            workspace_id: "ws-1".to_string(),
            branch: "main".to_string(),
            default_branch: "main".to_string(),
        }
    }

    fn make_constraint_input<'a>(
        input: &'a InputContent,
        output: &'a OutputContext,
        agent: &'a AgentContext,
        target: &'a TargetContext,
    ) -> ConstraintInput<'a> {
        ConstraintInput {
            input,
            output,
            agent,
            target,
            action: Action::Push,
        }
    }

    // ── build_cel_context ─────────────────────────────────────────────

    #[test]
    fn build_context_succeeds() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        // Verify we can resolve basic fields.
        let program = Program::compile(r#"input.spec_path == "specs/system/payments.md""#).unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn context_action_is_string() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let program = Program::compile(r#"action == "push""#).unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn context_nested_fields_accessible() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        // Access nested scope field.
        let program = Program::compile(r#"input.scope.forbidden_paths.size() == 1"#).unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn context_output_diff_stats_accessible() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let program = Program::compile("output.diff_stats.insertions == 142").unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    // ── evaluate_constraint ───────────────────────────────────────────

    #[test]
    fn evaluate_simple_true_constraint() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraint = OutputConstraint {
            name: "agent persona check".to_string(),
            expression: r#"agent.persona == "security""#.to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).unwrap());
    }

    #[test]
    fn evaluate_simple_false_constraint() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraint = OutputConstraint {
            name: "wrong persona".to_string(),
            expression: r#"agent.persona == "developer""#.to_string(),
        };
        assert!(!evaluate_constraint(&constraint, &ctx).unwrap());
    }

    #[test]
    fn evaluate_malformed_cel_is_error() {
        let ctx = Context::default();
        let constraint = OutputConstraint {
            name: "bad expression".to_string(),
            expression: "this is not valid CEL !!!".to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).is_err());
    }

    #[test]
    fn evaluate_missing_variable_is_error() {
        let ctx = Context::default();
        let constraint = OutputConstraint {
            name: "missing var".to_string(),
            expression: "nonexistent.field == true".to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).is_err());
    }

    #[test]
    fn evaluate_non_boolean_result_is_error() {
        let ctx = Context::default();
        let constraint = OutputConstraint {
            name: "non-bool".to_string(),
            expression: "1 + 1".to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).is_err());
    }

    // ── evaluate_all ──────────────────────────────────────────────────

    #[test]
    fn evaluate_all_passes_when_all_pass() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraints = vec![
            OutputConstraint {
                name: "persona check".to_string(),
                expression: r#"agent.persona == "security""#.to_string(),
            },
            OutputConstraint {
                name: "attestation level".to_string(),
                expression: "agent.attestation_level >= 3".to_string(),
            },
        ];
        let result = evaluate_all(&constraints, &ctx);
        assert!(result.valid);
        assert_eq!(result.children.len(), 2);
    }

    #[test]
    fn evaluate_all_stops_at_first_failure() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraints = vec![
            OutputConstraint {
                name: "passes".to_string(),
                expression: r#"agent.persona == "security""#.to_string(),
            },
            OutputConstraint {
                name: "fails".to_string(),
                expression: r#"agent.persona == "developer""#.to_string(),
            },
            OutputConstraint {
                name: "never reached".to_string(),
                expression: "true".to_string(),
            },
        ];
        let result = evaluate_all(&constraints, &ctx);
        assert!(!result.valid);
        // Only 2 children: the first that passed and the second that failed.
        // The third was never evaluated.
        assert_eq!(result.children.len(), 2);
        assert!(result.children[0].valid);
        assert!(!result.children[1].valid);
    }

    #[test]
    fn evaluate_all_error_is_failure() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraints = vec![OutputConstraint {
            name: "malformed".to_string(),
            expression: "this is not valid CEL !!!".to_string(),
        }];
        let result = evaluate_all(&constraints, &ctx);
        assert!(!result.valid);
        assert!(result.message.contains("error"));
    }

    #[test]
    fn evaluate_all_empty_constraints_passes() {
        let ctx = Context::default();
        let result = evaluate_all(&[], &ctx);
        assert!(result.valid);
        assert_eq!(result.message, "0 constraint(s) passed");
    }

    // ── glob_to_regex ─────────────────────────────────────────────────

    #[test]
    fn glob_to_regex_double_star() {
        assert_eq!(glob_to_regex("src/payments/**"), "^src/payments/.*$");
    }

    #[test]
    fn glob_to_regex_single_star() {
        assert_eq!(glob_to_regex("src/*.rs"), "^src/[^/]*\\.rs$");
    }

    #[test]
    fn glob_to_regex_question_mark() {
        assert_eq!(glob_to_regex("test?.rs"), "^test.\\.rs$");
    }

    #[test]
    fn glob_to_regex_escapes_special_chars() {
        assert_eq!(glob_to_regex("file.txt"), "^file\\.txt$");
    }

    #[test]
    fn glob_to_regex_no_wildcards() {
        assert_eq!(
            glob_to_regex("exact/path/file.rs"),
            "^exact/path/file\\.rs$"
        );
    }

    // ── derive_strategy_constraints ───────────────────────────────────

    #[test]
    fn derive_persona_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, None, None);
        let persona = constraints
            .iter()
            .find(|c| c.name.contains("persona"))
            .expect("should derive persona constraint");
        assert_eq!(
            persona.expression,
            "input.persona_constraints.exists(p, p.name == agent.persona)"
        );
    }

    #[test]
    fn derive_meta_spec_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, None, None);
        let meta = constraints
            .iter()
            .find(|c| c.name.contains("meta-spec"))
            .expect("should derive meta-spec constraint");
        assert_eq!(
            meta.expression,
            "agent.meta_spec_set_sha == input.meta_spec_set_sha"
        );
    }

    #[test]
    fn derive_allowed_path_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, None, None);
        let path = constraints
            .iter()
            .find(|c| c.name.contains("allowed paths"))
            .expect("should derive allowed path constraint");
        assert!(path.expression.contains("output.changed_files.all"));
        assert!(path.expression.contains("f.matches"));
    }

    #[test]
    fn derive_forbidden_path_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, None, None);
        let path = constraints
            .iter()
            .find(|c| c.name.contains("forbidden"))
            .expect("should derive forbidden path constraint");
        assert!(path.expression.contains("!f.matches"));
    }

    #[test]
    fn derive_no_allowed_paths_when_empty() {
        let mut content = sample_input_content();
        content.scope.allowed_paths = vec![];
        let constraints = derive_strategy_constraints(&content, None, None);
        assert!(
            !constraints.iter().any(|c| c.name.contains("allowed paths")),
            "empty allowed_paths should not produce a constraint"
        );
    }

    #[test]
    fn derive_supervised_workspace_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, Some("supervised"), None);
        let supervised = constraints
            .iter()
            .find(|c| c.name.contains("supervised"))
            .expect("should derive supervised constraint");
        assert_eq!(supervised.expression, "agent.attestation_level >= 2");
    }

    #[test]
    fn derive_no_supervised_constraint_for_autonomous() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, Some("autonomous"), None);
        assert!(
            !constraints.iter().any(|c| c.name.contains("supervised")),
            "autonomous workspace should not produce supervised constraint"
        );
    }

    #[test]
    fn derive_attestation_level_constraint() {
        let content = sample_input_content();
        let constraints = derive_strategy_constraints(&content, None, Some(3));
        let level = constraints
            .iter()
            .find(|c| c.name.contains("attestation level must be"))
            .expect("should derive attestation level constraint");
        assert_eq!(level.expression, "agent.attestation_level >= 3");
    }

    #[test]
    fn derive_multiple_persona_constraints_produces_single_membership_constraint() {
        let mut content = sample_input_content();
        content.persona_constraints = vec![
            PersonaRef {
                name: "security".to_string(),
            },
            PersonaRef {
                name: "compliance".to_string(),
            },
        ];
        let constraints = derive_strategy_constraints(&content, None, None);
        let persona_constraints: Vec<_> = constraints
            .iter()
            .filter(|c| c.name.contains("persona"))
            .collect();
        // Must produce a single membership constraint, not one per entry.
        assert_eq!(persona_constraints.len(), 1);
        assert_eq!(
            persona_constraints[0].expression,
            "input.persona_constraints.exists(p, p.name == agent.persona)"
        );
    }

    #[test]
    fn multi_persona_constraint_passes_when_agent_matches_one() {
        // An agent with persona "security" should satisfy a constraint list
        // containing ["security", "compliance"] — the agent matches one entry.
        let mut content = sample_input_content();
        content.persona_constraints = vec![
            PersonaRef {
                name: "security".to_string(),
            },
            PersonaRef {
                name: "compliance".to_string(),
            },
        ];
        let output = sample_output_context();
        let agent = sample_agent_context(); // persona = "security"
        let target = sample_target_context();
        let ci = make_constraint_input(&content, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&content, None, None);
        let persona = strategy
            .iter()
            .find(|c| c.name.contains("persona"))
            .expect("should have persona constraint");
        let result = evaluate_constraint(persona, &ctx).unwrap();
        assert!(
            result,
            "agent with persona 'security' should satisfy ['security', 'compliance']"
        );
    }

    #[test]
    fn multi_persona_constraint_fails_when_agent_matches_none() {
        let mut content = sample_input_content();
        content.persona_constraints = vec![
            PersonaRef {
                name: "security".to_string(),
            },
            PersonaRef {
                name: "compliance".to_string(),
            },
        ];
        let output = sample_output_context();
        let mut agent = sample_agent_context();
        agent.persona = "developer".to_string(); // Matches neither entry.
        let target = sample_target_context();
        let ci = make_constraint_input(&content, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&content, None, None);
        let persona = strategy
            .iter()
            .find(|c| c.name.contains("persona"))
            .expect("should have persona constraint");
        let result = evaluate_constraint(persona, &ctx).unwrap();
        assert!(
            !result,
            "agent with persona 'developer' should NOT satisfy ['security', 'compliance']"
        );
    }

    #[test]
    fn derive_multiple_forbidden_paths() {
        let mut content = sample_input_content();
        content.scope.forbidden_paths =
            vec!["src/auth/**".to_string(), "src/secrets/**".to_string()];
        let constraints = derive_strategy_constraints(&content, None, None);
        let forbidden: Vec<_> = constraints
            .iter()
            .filter(|c| c.name.contains("forbidden"))
            .collect();
        assert_eq!(forbidden.len(), 2);
    }

    // ── collect_all_constraints ───────────────────────────────────────

    #[test]
    fn collect_merges_all_sources() {
        let explicit = vec![OutputConstraint {
            name: "explicit".to_string(),
            expression: "true".to_string(),
        }];
        let strategy = vec![OutputConstraint {
            name: "strategy".to_string(),
            expression: "true".to_string(),
        }];
        let gate = vec![GateConstraint {
            gate_id: "g1".to_string(),
            gate_name: "review".to_string(),
            constraint: OutputConstraint {
                name: "gate".to_string(),
                expression: "true".to_string(),
            },
            signed_by: vec![1, 2, 3],
        }];
        let all = collect_all_constraints(&explicit, &strategy, &gate);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "explicit");
        assert_eq!(all[1].name, "strategy");
        assert_eq!(all[2].name, "gate");
    }

    // ── Integration: strategy constraints actually evaluate ───────────

    #[test]
    fn strategy_constraints_pass_for_matching_agent() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&input, None, Some(3));
        let result = evaluate_all(&strategy, &ctx);
        assert!(
            result.valid,
            "strategy constraints should pass for matching agent: {:?}",
            result
        );
    }

    #[test]
    fn strategy_constraints_fail_for_wrong_persona() {
        let input = sample_input_content();
        let output = sample_output_context();
        let mut agent = sample_agent_context();
        agent.persona = "developer".to_string(); // Wrong persona.
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&input, None, None);
        let result = evaluate_all(&strategy, &ctx);
        assert!(!result.valid, "should fail for wrong persona");
        assert!(result.message.contains("persona"));
    }

    #[test]
    fn strategy_constraints_fail_for_wrong_meta_spec_sha() {
        let input = sample_input_content();
        let output = sample_output_context();
        let mut agent = sample_agent_context();
        agent.meta_spec_set_sha = "wrong_sha".to_string();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&input, None, None);
        let result = evaluate_all(&strategy, &ctx);
        assert!(!result.valid, "should fail for wrong meta-spec SHA");
    }

    #[test]
    fn strategy_path_constraint_fails_for_out_of_scope_file() {
        let input = sample_input_content();
        let mut output = sample_output_context();
        output.changed_files.push("src/other/file.rs".to_string()); // Out of scope.
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let strategy = derive_strategy_constraints(&input, None, None);
        let result = evaluate_all(&strategy, &ctx);
        assert!(!result.valid, "should fail for out-of-scope file");
    }

    #[test]
    fn strategy_forbidden_path_constraint_fails() {
        let input = sample_input_content();
        let mut output = sample_output_context();
        output.changed_files = vec!["src/auth/middleware.rs".to_string()]; // Forbidden.
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        // Use only the forbidden path constraint to test isolation.
        let mut content_no_allowed = input.clone();
        content_no_allowed.scope.allowed_paths = vec![];
        let strategy = derive_strategy_constraints(&content_no_allowed, None, None);
        let result = evaluate_all(&strategy, &ctx);
        assert!(!result.valid, "should fail for forbidden path");
    }

    #[test]
    fn strategy_attestation_level_fails_for_low_level() {
        let input = sample_input_content();
        let output = sample_output_context();
        let mut agent = sample_agent_context();
        agent.attestation_level = 1; // Too low.
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraints = vec![OutputConstraint {
            name: "attestation level".to_string(),
            expression: "agent.attestation_level >= 3".to_string(),
        }];
        let result = evaluate_all(&constraints, &ctx);
        assert!(!result.valid, "should fail for low attestation level");
    }

    // ── Explicit user constraint examples from §3.2 ──────────────────

    #[test]
    fn explicit_constraint_scope_to_payments() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraint = OutputConstraint {
            name: "scope to payments".to_string(),
            expression: r#"output.changed_files.all(f, f.startsWith("src/payments/"))"#.to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).unwrap());
    }

    #[test]
    fn explicit_constraint_no_cargo_changes() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraint = OutputConstraint {
            name: "no dependency changes".to_string(),
            expression: r#"output.changed_files.all(f, f != "Cargo.toml" && f != "Cargo.lock")"#
                .to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).unwrap());
    }

    #[test]
    fn explicit_constraint_commit_message_references_spec() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let constraint = OutputConstraint {
            name: "commit references spec".to_string(),
            expression: r#"output.commit_message.contains("specs/system/payments.md")"#.to_string(),
        };
        assert!(evaluate_constraint(&constraint, &ctx).unwrap());
    }

    // ── End-to-end: all constraint sources combined ───────────────────

    #[test]
    fn full_evaluation_with_all_sources() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        // Explicit constraints.
        let explicit = vec![OutputConstraint {
            name: "commit references spec".to_string(),
            expression: r#"output.commit_message.contains("specs/system/payments.md")"#.to_string(),
        }];

        // Strategy-implied constraints.
        let strategy = derive_strategy_constraints(&input, None, Some(3));

        // Gate constraints.
        let gate = vec![GateConstraint {
            gate_id: "gate-review".to_string(),
            gate_name: "code review".to_string(),
            constraint: OutputConstraint {
                name: "gate: no deleted files".to_string(),
                expression: "output.deleted_files.size() == 0".to_string(),
            },
            signed_by: vec![1, 2, 3],
        }];

        let all = collect_all_constraints(&explicit, &strategy, &gate);
        let result = evaluate_all(&all, &ctx);
        assert!(result.valid, "full evaluation should pass: {:?}", result);
    }

    #[test]
    fn persona_constraints_indexed_access() {
        // Verify that persona_constraints[0].name works in CEL context.
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = make_constraint_input(&input, &output, &agent, &target);
        let ctx = build_cel_context(&ci).unwrap();

        let program =
            Program::compile(r#"input.persona_constraints[0].name == "security""#).unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn merge_action_evaluates() {
        let input = sample_input_content();
        let output = sample_output_context();
        let agent = sample_agent_context();
        let target = sample_target_context();
        let ci = ConstraintInput {
            input: &input,
            output: &output,
            agent: &agent,
            target: &target,
            action: Action::Merge,
        };
        let ctx = build_cel_context(&ci).unwrap();

        let program = Program::compile(r#"action == "merge""#).unwrap();
        let result = program.execute(&ctx).unwrap();
        assert_eq!(result, Value::Bool(true));
    }
}
