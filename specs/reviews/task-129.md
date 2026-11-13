# Review — task-129 (Implement ABAC evaluation engine, API, and audit integration)

Spec: `specs/system/abac-policy-engine.md` §Evaluation Engine–§Dry-Run Evaluation.
Candidate `930022e9` (round 4; product commits `0dcb1017`, `81ccc284`, `0c148e28`, `9ba4dd40`) against base `349a32ac`.
Verdict: **needs-revision**. Evidence: `/tmp/stage/review-evidence/review-round4.md`.

## Round 4 scope

Rounds 1–3 findings (agent-repo-scope hollow seed, `?subject=` param binding, system-full-access privilege escalation + dead-code bypass) were re-verified as repaired, and the full diff was re-examined for new defects.

## Round-3 repairs — verified fixed

**F1 (escalation via spoofed `gyre-system-token` display_name): REPAIRED.** `gyre_domain::SYSTEM_ABAC_IDENTITY` is reserved at every display_name write path (PUT /users/me `api/users.rs:170`; admin create `:1371`; SCIM create incl. userName fallback `api/scim.rs:318`; SCIM update `:377`; SSO provisioning `auth.rs:1651` — display_name is the RAW preferred_username, sanitization only touches `username`, so no transformation channel; SSO re-login of a tainted row 401s at `auth.rs:1633`) and contained at evaluation time: `abac_middleware.rs:1984-2015` denies any non-system principal whose agent_id is the reserved string before evaluation, with warn log + audited PolicyDecision. The guard is sound: the genuine global token has agent_id `"system"` (`auth.rs:1069`), so it cannot fire on it, and every agent_id source is either guarded or server-minted (worker JWT ids, `{host}/{sub}` federation prefix). Live test `spoofed_system_identity_denied_before_evaluation` exercises the API-key→display_name channel.

**F2 (unreachable subject.id mapping / bypass-only system token): REPAIRED.** The early `is_system_principal()` return is gone; the system principal flows through evaluation and is allowed BY `system-full-access` (1000 > tenant-isolation 950; the only immutable Deny keys on `subject.type != "user"` and the principal's Admin role derives `"user"`). The mechanism — not a bypass — is asserted through the decision audit trail (`matched_policy == "builtin-system-access"`) in `system_token_allowed_by_system_full_access_policy` and `builtin_tenant_isolation_denies_cross_tenant_live`. git_http's own `is_system_principal` allowances are independent and unaffected.

Round-1/2 fixes re-confirmed: `agent-repo-scope` seeded enabled at 750 with the §Policy Examples dynamic-reference conditions (`gyre-domain/src/policy.rs:343-396`; live worker-JWT test at `abac_middleware.rs:6799`); `/policies/effective` binds the documented `?subject=` param and builds real-store subject contexts.

## Probes (this sandbox; server tests deferred to the verifier per the build wrapper)

- `cargo test -p gyre-domain` → **507 + 8 passed, 0 failed**.
- `cargo check -p gyre-server --tests` → **exit 0**.
- `check-arch`, `check-abac-route-registry`, `check-scope-literal-defaults`, `check-fabricated-scope-defaults`, `check-inert-enforcement`, `check-mem-port-contracts`, `check-fail-open-ref-resolution`, `check-lossy-secret-conversion` → **all pass**.
- Verifier must run the focused server suites listed in the evidence file, then full `cargo test --all`.

## Findings

- [open] **F1 (major): policy scope/scope_id is not enforced on the HTTP evaluation path.** `policy_engine::evaluate` (`policy_engine.rs:364`) retains policies only on `enabled && applies_to(action, resource_type)` — `applies_to` (`gyre-domain/src/policy.rs:104-111`) never compares `scope`/`scope_id` to the request's resource scope chain — and the middleware feeds it the entire store (`abac_middleware.rs:2284`). A `POST /api/v1/policies` policy with `scope=workspace, scope_id=ws-A` is evaluated for requests in every workspace and every tenant: a ws-A Allow grants Developers in ws-B (and t-2 — tenant-isolation sees same-tenant subject+resource, so a t-1 Admin's policy widens permissions in t-2); a scoped Deny over-restricts globally. Contradicts §Policy Language ("Workspace // Applies to all repos in workspace", "Repo // Applies to one repo"), §Evaluation Flow step 5, §Policy Composition ("within its scope") — the sections coverage rows 9/10 flip to implemented. The merge path already implements the semantics (`merge_processor.rs:3727-3733` filters workspace-scoped policies to the MR's workspace), demonstrating the intended contract. Seeded builtins all use `scope_id: None`, so shipped behavior is correct — the defect is reachable through the documented scoped-policy CRUD/UI model and has no test (nothing asserts a scoped policy fails to apply out of scope). Repair: filter the candidate set by scope/scope_id against the resolved resource scope chain (`scope_id: None` = all at that level, keeping builtins global), with engine + live tests.
- [open] **F2 (minor): stale security comments contradict the round-3 design.** `auth.rs` ~666–669 (`jwt_claims` doc: "Global token or API key: None — ABAC checks are bypassed for these") and `git_http.rs:89` ("it bypasses ABAC on REST") are now false (API-key principals were never bypassed; the global token is allowed by policy since this round). Same false-comment class that misled round 2; fix in the same repair round.

## Non-findings (checked, acceptable)

- Custom policies may exceed priority 1000 and override non-immutable builtins (incl. tenant-isolation) — explicitly permitted by §Built-In Policies ("Non-immutable built-in policies can be overridden by higher-priority custom policies"); policy creation is Admin-gated by the seeded `builtin:policy-admin-only` (live-tested).
- Dry-run `/policies/evaluate` and `/policies/effective?subject=` answer hypothetical/other-subject questions — informational only, role-permitted by design (§API); the unrestricted `merge_jwt_claims` is confined to the dry-run handler.
- The spoof guard sits after the exempt/unresolved early returns: on those routes a spoofed-identity caller is treated like any user (per-handler tenant/workspace checks) — no escalation.
- Stale `web/dist/` not regenerated here — verifier-owned, out of review scope per assignment; no authored frontend changes in this diff.

## Summary

The round-3 escalation and dead-code repairs are real, complete across every spoof channel I could find, and proven by audit-trail-asserting live tests; the engine, seeding, caching, audit integration, and API are substantially correct and well-tested. But the evaluation engine does not enforce policy scope — a workspace/repo/tenant-scoped policy silently applies everywhere, contradicting the spec sections this task marks implemented and producing cross-workspace/cross-tenant policy effects. Revision required on F1 (F2 opportunistically).

— Reviewer, 2026-11-13 (round 4)
