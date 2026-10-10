# Coverage: ABAC Policy Engine

**Spec:** [`system/abac-policy-engine.md`](../../system/abac-policy-engine.md)
**Last audited:** 2026-10-10 (rows 2-7 implemented via task-128 — ABAC core: Policy entity, PolicyScope/PolicyEffect, Condition/ConditionOp/ConditionValue, attribute model with live extraction, dynamic references. See specs/tasks/task-128.md.)
**Coverage:** 6/19 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Core Concepts | 2 | implemented | task-128 | Attribute-based evaluation in production: policy_engine::evaluate runs on every authenticated request via abac_middleware (route-resolved resource type + method-mapped action), policies loaded from the PolicyRepository store, first-match-wins with default deny. |
| 3 | Attributes | 3 | implemented | task-128 | All four categories extracted live: subject (type/id/tenant_id/global_role from auth context; workspace_role/workspace_ids/team_ids from membership + team stores; persona/stack_hash/attestation_level/repo_scope normalized from agent JWT claims — mint_scoped carries them at spawn), resource (type/id/workspace_id/repo_id from route pattern + path), action (method_to_action), environment (env.ip from ConnectInfo socket peer — not forwarded headers, env.time, env.budget_remaining from real BudgetConfig+BudgetUsage, env.main_health from merge-processor queue state). Entity-lookup attributes (owner/team/approval_status/visibility) assigned to task-129 evaluation-flow work. |
| 4 | Policy Language | 2 | implemented | task-128 | Declarative policies with priority (higher first), scope specificity tiebreak (repo > workspace > tenant), Deny before Allow at equal priority, immutable-Deny evaluated before all priority-based evaluation (policy_engine.rs evaluate). |
| 5 | Policy Entity | 3 | implemented | task-128 | gyre-domain/src/policy.rs Policy matches spec exactly (id, name, description, scope, scope_id: Option<Id>, priority, effect, conditions, actions, resource_types, enabled, immutable, created_by: Id, created_at, updated_at) plus additive built_in for non-deletable system policies. PolicyScope {Tenant, Workspace, Repo}; PolicyEffect {Allow, Deny}. Persisted via PolicyRepository port (SQLite + Postgres adapters, migration 000007 + 000028 immutable). |
| 6 | Conditions | 3 | implemented | task-128 | Condition {attribute, operator, value}; ConditionOp has all 8 variants (Equals, NotEquals, In, NotIn, GreaterThan, LessThan, Contains, Exists); ConditionValue String/StringList/Number/Bool. Dynamic references ($resource.*, $subject.*) resolve at evaluation time preserving referenced type; unresolvable references fail closed (no fabricated value — Allow cannot grant). |
| 7 | Policy Examples | 3 | implemented | task-128 | Spec's tenant-level agent-repo-scope deny pattern expressible and enforced: conditions ANDed, subject.repo_scope vs $resource.repo_id dynamic comparison (dynamic_reference_resolves_resource_attr_at_eval_time test); builtin_policies + trust_policies_for_level generate real scoped policy sets used by live evaluation. |
| 8 | Evaluation Engine | 2 | task-assigned | task-129 | |
| 9 | Evaluation Flow | 3 | task-assigned | task-129 | |
| 10 | Policy Composition | 3 | task-assigned | task-129 | |
| 11 | Built-In Policies | 3 | task-assigned | task-129 | |
| 12 | Performance | 3 | task-assigned | task-129 | |
| 13 | Audit Integration | 2 | task-assigned | task-129 | |
| 14 | API | 2 | task-assigned | task-129 | |
| 15 | Dry-Run Evaluation | 3 | task-assigned | task-129 | |
| 16 | CLI | 2 | task-assigned | task-131 | |
| 17 | UI | 2 | task-assigned | task-132 | |
| 18 | MCP Integration | 2 | task-assigned | task-133 | |
| 19 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
