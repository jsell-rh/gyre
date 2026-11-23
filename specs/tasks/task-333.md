---
title: "Human-approver default [owner]: omitted human_approvers reserves approval to the spec owner"
spec_ref: "Spec gap spec-c08e139362b99830187a32b4 / approver-owner-default / a458cd7cdf30"
depends_on: []
progress: ready-for-review
commits: ["db44a5cec5fe14719e1c6a335caae4070704e9e4", "9ac5e048d2b8d02f77070ed8df0415ab4609ea1a"]
---

## Required behavior

Manifest Schema row `approval.human_approvers` defaults to `[owner]`. Today `is_valid_human_approval` (crates/gyre-server/src/spec_registry.rs:336-348) treats an omitted/empty list as unrestricted — any active human approval at current_sha is valid — and the single-spec GET path's team expansion (crates/gyre-server/src/api/specs.rs:1164-1175) leaves an empty list empty, so a manifest entry that sets an approval mode but omits human_approvers lets any human approve where the spec reserves approval to the owner. Implement the spec default: the effective human-approver set for a manifest entry is `approval.human_approvers` when non-empty, otherwise `[owner]` (owner is a required field; the default applies to entries with no approval config at all, whose schema defaults are mode human_and_agent + human_approvers [owner]). Apply it (a) in `is_valid_human_approval` so `resolve_approval_status` (the §9 resolution that drives ledger approval_status, GET /api/v1/specs, /pending, /drifted) enforces it, and (b) at the team-expansion site in api/specs.rs so `team:{slug}` resolution composes with the owner default (an unresolvable team reference still approves nobody — that part is satisfied and must not regress). Do not alter agent-approver validity (attestation/stack_hash/persona), team-member resolution semantics, or the approval event schema — those sections are satisfied.

## Acceptance Criteria

- [ ] A manifest entry with approval mode human_only (and human_and_agent), owner user:alice, and NO human_approvers, resolved at current_sha with only an active human approval event from user:bob, yields approval_status Pending; the same event from user:alice yields Approved.
- [ ] GET /api/v1/specs and GET /api/v1/specs/{path} reflect that resolution: a bob-only approval leaves the spec pending, an alice approval approves it.
- [ ] Team refs still work alongside the default: an entry listing team:eng resolves Approved on a member's approval (user:carol), and an unresolvable team:nonexistent approves nobody whether or not the owner default also applies.

## Verification intent

Probe resolve_approval_status (unit) or the resolution through GET /api/v1/specs (integration): manifest entry owner user:alice, mode human_only, no human_approvers; seed an active human approval event for user:bob at current_sha; assert status == Pending. At the base this fails — `is_valid_human_approval` returns true for any human when the list is empty (`a.human_approvers.is_empty() || ...`), so bob's approval marks the spec Approved where the spec reserves approval to the owner. The twin case (alice's approval) must stay Approved, so a fix that simply inverts the predicate cannot pass both.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-registry.md`. Source: `52d7b66b7b2f0a4dc1d63a1b544a5a7364f79b40`; plan: `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.

[
  {
    "id": "0c04e18483ce2f9bf15e2c1c",
    "decision": "gap",
    "requirement_quote": "| `approval.human_approvers` | string[] | [owner] | Users who can approve; a `team:{slug}` entry counts every member of that workspace team as an approver (user-management.md §Team Management — spec ownership; an unresolvable team reference approves nobody) |",
    "detail": "Most of the schema is implemented: SpecEntry parses path/title/owner/approval.mode/human_approvers/agent_approvers/gates/auto_create_tasks/auto_invalidate_on_change/superseded_by with the specced defaults (approval.mode defaults to human_and_agent), and `team:{slug}` entries are expanded through real workspace-membership lookups at approval-resolution time, with an unresolvable team leaving only the literal `team:slug` string in the list — which matches no real approver id, so it approves nobody. The gap is the default for `approval.human_approvers`: the spec says the default is `[owner]`, but the implementation treats an omitted/empty list as unrestricted — any human approval is valid. No code applies an owner default at parse time or in the validity resolver, so a manifest entry that sets an approval mode but omits human_approvers lets any human approve where the spec reserves approval to the owner.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "        && approval\n            .map(|a| a.human_approvers.is_empty() || a.human_approvers.contains(&event.approver_id))\n            .unwrap_or(true)",
        "detail": "Human-approval validity explicitly treats an empty human_approvers list as unrestricted (any human), and membership is literal containment — the specced [owner] default is never applied, so omitting human_approvers widens approval from owner-only to any-human."
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "quote": "pub struct ApprovalConfig {\n    pub mode: ApprovalMode,\n    #[serde(default)]\n    pub human_approvers: Vec<String>,",
        "detail": "Plain serde empty-vec default; there is no normalization step that substitutes [owner] when the list is omitted (inspected the full parse path: parse_manifest is a direct serde_yaml::from_str with no post-processing)."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "        if crate::notifications::parse_team_ref(approver).is_some() {\n            let resolved =\n                crate::notifications::resolve_team_members(state, workspace_id, approver).await;",
        "detail": "The team:{slug} half of the row IS implemented: approvers are expanded through real membership resolution before the validity check, and the `_ => expanded.push(approver.clone())` arm keeps the unresolvable literal so it matches no user id (approves nobody), as specced."
      }
    ]
  }
]

## Shipped

The Manifest Schema default for `approval.human_approvers` — `[owner]` — is now enforced at both resolution sites, so a manifest entry that sets an approval mode but omits `human_approvers` reserves approval to the spec owner instead of widening it to any human.

- `is_valid_human_approval` (crates/gyre-server/src/spec_registry.rs:342) now takes the full `SpecEntry` and resolves the effective human-approver set as `approval.human_approvers` when non-empty, otherwise `[owner]` (owner is a required field). `resolve_approval_status` (§9) enforces it for every consumer of ledger `approval_status`: GET /api/v1/specs, /pending, /drifted, the approve path, and the revoke path's recompute.
- `resolve_new_approval_status` (crates/gyre-server/src/api/specs.rs:1154) materializes the same default on the manifest entry before team expansion, so `team:{slug}` resolution composes with the owner default (including a `team:slug` owner, whose members become the approvers). The unresolvable-team behavior is unchanged: the literal `team:slug` string is kept verbatim and matches no approver id — an unresolvable team owner approves nobody (fail-closed).
- Agent-approver validity (attestation/stack_hash/persona), team-member resolution semantics, and the approval event schema are untouched.

### Tests

- Unit (crates/gyre-server/src/spec_registry.rs): `test_resolve_omitted_human_approvers_reserve_to_owner` — owner user:alice, mode human_only, no human_approvers; bob's active approval at current_sha → Pending, alice's → Approved (fails at the task base, where an empty list meant unrestricted). `test_resolve_omitted_human_approvers_owner_scopes_human_and_agent` — same `[owner]` default scopes the human half of a human_and_agent pair.
- E2E (crates/gyre-server/src/api/specs.rs): `omitted_human_approvers_default_to_owner_e2e` — real git manifest repo, real ledger, team + membership created through the team API, real API-key human/agent approvals; asserts ledger status, GET /api/v1/specs (list), and GET /api/v1/specs/{path} reflect the owner default; also proves team:eng member (user:carol) still approves a team-listed spec and an unresolvable team:nonexistent owner approves nobody.

### Verification status

- `cargo check -p gyre-server` exit 0; `cargo check -p gyre-server --tests` exit 0 (the unit + e2e tests above type-check).
- Per the assignment's 4 GiB constraint, gyre-server test execution is deferred to the independent verifier: `cargo test -p gyre-server --lib` (focused filters recorded in /tmp/stage/review-evidence/verifier-tests.txt).
- This assignment continued from checkpoint 9ac5e048; the only source change since is a behavior-preserving rustfmt reformat of `is_valid_human_approval`.
