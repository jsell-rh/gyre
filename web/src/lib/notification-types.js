// Canonical NotificationType ids for the profile notification-preference
// toggles (HSI §12: "enable/disable any NotificationType variant").
//
// Source of truth: the `NotificationType` enum in
// crates/gyre-common/src/notification.rs -- every variant's `as_str()` name,
// in enum declaration order. The backend PUT /users/me/notification-preferences
// rejects names that are not canonical as_str() spellings, and the inbox
// preference filter only matches these stored names, so this list must stay
// in lockstep with the Rust enum.
//
// scripts/check-notification-toggle-coverage.sh mechanically compares this
// list against the Rust enum variants and against the locale labels; a
// missing variant (a type the human cannot disable) or a stray legacy parse
// alias ('SpecPendingApproval'/'BudgetWarning' -- an id that never matches a
// stored notification, i.e. a silently inert toggle) fails CI.
export const NOTIF_TYPE_IDS = [
  // user-management.md §Notification Entity taxonomy
  'SpecApprovalRequested', 'PersonaApprovalRequested',
  'AgentEscalation', 'AgentBudgetWarning', 'AgentBudgetExhausted',
  'AgentFailed',
  'GateFailure', 'GatePassed',
  'MrMerged', 'MrReverted', 'MergeQueuePaused',
  'BreakingChangeDetected', 'SpecDriftDetected', 'DependencyStale',
  'SecurityFinding',
  'InvitationReceived', 'MembershipChanged',
  'SystemAlert',
  // HSI §2 inbox types
  'AgentNeedsClarification', 'CrossWorkspaceSpecChange',
  'ConflictingInterpretations', 'MetaSpecDrift',
  'TrustSuggestion', 'SpecAssertionFailure',
  'AbandonedBranch', 'AgentCompleted', 'SuggestedSpecLink',
  'SpecRejected', 'ConstraintViolation',
  'CascadeTestTriggered', 'CascadeTestFailed', 'DependencyChainTooDeep',
  'AtomicGroupFailure', 'SpecConflict', 'MergeQueueEscalation',
  'ComputeTargetUnavailable',
  'ImpersonationRequest', 'ImpersonationAlert', 'SecretExpiring',
  'PersonaChangePendingApproval',
  // user-management.md §Ownership Transfer & Reclamation (task-121 lineage):
  // sent to every TenantAdmin when a workspace loses its last active Owner.
  'WorkspaceOwnerless',
];
