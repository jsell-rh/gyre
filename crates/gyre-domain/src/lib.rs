//! Pure domain logic for the Gyre autonomous development platform.
//!
//! # Hexagonal Architecture Invariant
//!
//! This crate MUST NOT import `gyre-adapters` or any infrastructure crate
//! (databases, HTTP clients, file I/O, etc.). Domain logic depends only on:
//! - `gyre-common` for shared types and errors
//!
//! Violations are caught by `scripts/check-arch.sh` and CI.

pub mod activity;
pub mod agent;
pub mod agent_card;
pub mod agent_tracking;
pub mod analytics;
pub mod attestation;
pub mod audit;
pub mod budget;
pub mod compose;
pub mod compute_target;
pub mod constraint_evaluator;
pub mod container_audit;
pub mod dependency;
pub mod extractor;
pub mod git_types;
pub mod go_extractor;
pub mod llm_config;
pub mod lsp_call_graph;
pub mod merge_queue;
pub mod merge_request;
pub mod message_type;
pub mod meta_spec;
pub mod network_peer;
pub mod notification;
pub mod policy;
pub mod prompt_template;
pub mod python_extractor;
pub mod quality_gate;
pub mod repository;
pub mod review;
pub mod rust_extractor;
pub mod spec_approval;
pub mod spec_assertions;
pub mod spec_ledger;
pub mod spec_policy;
pub mod task;
pub mod team;
pub mod tenant;
pub mod tree_sitter_utils;
pub mod typescript_extractor;
pub mod user;
pub mod user_profile;
pub mod view_query_resolver;
pub mod workspace;
pub mod workspace_membership;

pub use activity::ActivityEvent;
pub use agent::{Agent, AgentError, AgentStatus, AgentUsage, DisconnectedBehavior, MetaSpecUsed};
pub use agent_card::AgentCard;
pub use agent_tracking::{AgentCommit, AgentWorktree, LoopConfig, Session};
pub use analytics::{AnalyticsEvent, CostEntry};
pub use attestation::{AttestationBundle, AttestationGateResult, MergeAttestation};
pub use audit::{AuditEvent, AuditEventType};
pub use budget::{BudgetCallRecord, BudgetConfig, BudgetUsage};
pub use compose::{AgentCompose, AgentSpec, TaskSpec};
pub use compute_target::{ComputeTargetEntity, ComputeTargetType};
pub use constraint_evaluator::{
    build_cel_context, collect_all_constraints, derive_strategy_constraints, evaluate_all,
    evaluate_constraint, Action, AgentContext, ConstraintInput, DiffStatsContext, OutputContext,
    TargetContext,
};
pub use container_audit::ContainerAuditRecord;
pub use dependency::{
    BreakingChange, BreakingChangeBehavior, DependencyEdge, DependencyPolicy, DependencyStatus,
    DependencyType, DetectionMethod,
};
pub use extractor::{ExtractionError, ExtractionResult, LanguageExtractor};
pub use git_types::{BranchInfo, CommitInfo, DiffResult, FileDiff, MergeResult};
pub use go_extractor::GoExtractor;
pub use llm_config::{is_valid_function_key, LlmFunctionConfig, VALID_FUNCTION_KEYS};
pub use merge_queue::{MergeQueueEntry, MergeQueueEntryStatus};
pub use merge_request::{
    DependencySource, DiffStats, MergeRequest, MergeRequestDependency, MrError, MrStatus,
};
pub use message_type::MessageType;
pub use meta_spec::{
    MetaSpec, MetaSpecApprovalStatus, MetaSpecBinding, MetaSpecKind, MetaSpecScope, MetaSpecVersion,
};
pub use network_peer::NetworkPeer;
pub use notification::{Notification, NotificationType};
pub use policy::{
    builtin_policies, trust_policies_for_level, Condition, ConditionOp, ConditionValue, Policy,
    PolicyDecision, PolicyEffect, PolicyScope,
};
pub use prompt_template::{PromptTemplate, LLM_FUNCTION_KEYS};
pub use python_extractor::PythonExtractor;
pub use quality_gate::{GateResult, GateStatus, GateType, QualityGate};
pub use repository::{RepoStatus, Repository};
pub use review::{Review, ReviewComment, ReviewDecision};
pub use rust_extractor::RustExtractor;
pub use spec_approval::SpecApproval;
pub use spec_assertions::{
    evaluate_assertions, parse_assertions, AssertionResult, Comparison, ParsedAssertion, Predicate,
    Subject,
};
pub use spec_ledger::{ApprovalStatus, SpecApprovalEvent, SpecLedgerEntry};
pub use spec_policy::SpecPolicy;
pub use task::{Task, TaskError, TaskPriority, TaskStatus, TaskType};
pub use team::Team;
pub use tenant::Tenant;
pub use typescript_extractor::TypeScriptExtractor;
pub use user::{GlobalRole, Theme, User, UserPreferences, UserRole};
pub use user_profile::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken};
pub use workspace::{Persona, PersonaApprovalStatus, PersonaScope, TrustLevel, Workspace};
pub use workspace_membership::{WorkspaceMembership, WorkspaceRole};
