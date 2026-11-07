# Review — task-108 (Implement admin user impersonation)

Spec: `identity-security.md` §Impersonation (+ `observability.md` §Audit Event Types for
`ImpersonationStarted`/`ImpersonationEnded`).
Candidate: `81d8e4daa2d8dcdd487be0f247760debcd7f1ef0` (base `4d6759a0`; task commits
`cad88393d`, `2d511c21`).
Verdict: **needs-revision** (2 substantive findings + 1 minor).

## What was verified as genuinely working

Focused runs on the clean candidate tree (evidence: `review-evidence/task-108-focused-tests.txt`):

- `cargo test -p gyre-server --lib api::impersonation` → 9/9 pass.
- `cargo test -p gyre-server --lib auth::` → 37/37 pass (incl. `impersonation_jwt_round_trip_and_expiry`:
  wrong-issuer rejection, zero-leeway `exp` rejection, agent-JWT fall-through).
- `cargo test -p gyre-server --lib abac_middleware` → 10/10 pass.

Genuinely implemented and verified by reading the diff and the passing tests:

- **Consent gate is real.** The one-time 8-digit CSPRNG approval code is delivered only to the
  target (Inbox notification + WS push); only its SHA-256 hash is stored; comparison is
  constant-time (`tokens_equal`); the admin-facing response omits the code (documented, correct
  deviation from the plan's response shape — returning it would let the admin self-approve).
  Non-target callers and wrong codes are rejected; denied requests cannot be approved afterwards.
- **Token is a real cryptographically-validated mandate.** EdDSA JWT (`sub` = target,
  `impersonator_id`/`session_id`/`impersonating: true` claims), TTL from `GYRE_IMPERSONATION_TTL`
  (default 900 s), zero-leeway `exp`, and revocation-before-expiry via the persisted session
  record (`resolve_impersonation_auth` rejects missing/ended/expired sessions and cross-checks
  both identities against the record).
- **Lifecycle events.** `ImpersonationStarted`/`ImpersonationEnded` with the spec `detail` fields,
  attributed (`user_id`) to the impersonator; `impersonation_expiry` job registered and spawned
  from main; explicit end revokes the token and emits `Ended` with `duration_secs`/`actions_taken`.
- **Notifications.** `ImpersonationRequest` (priority 2) and `ImpersonationAlert` (priority 1,
  "Admin {name} is impersonating your account") types; WS `NotificationPush` matched by
  server-verified user identity (`ws_connection_users`), never client-supplied ids. Email is
  correctly out (no email infrastructure exists; the plan conditioned it on configuration).
- **Authorization constraints.** Admin role + real user identity required (shared system token
  rejected); Admin targets prohibited (no SuperAdmin in `UserRole` — plan's "or is prohibited"
  resolved to prohibited); max 1 active session per admin; no impersonation chaining (impersonated
  callers cannot request/approve/deny); only the impersonating admin may end. All four routes are
  in the `ResourceResolver`; request/end are `admin`-resource routes with in-handler defense in
  depth; approve/deny are exempt with per-handler target-identity checks. Exemption-file edits are
  line-number shifts only (no new entries).

## Findings (probe evidence: `review-evidence/task-108-probe-output.txt`, probe source kept at
`review-evidence/task-108-probe-source.rs`; the temporary in-tree probe module was reverted —
`git status` clean at candidate `81d8e4daa` after the run)

1. **Audit trail is not "full" — exempt/other surfaces are unattributed** (abac_middleware.rs).
   The `impersonation_action` recording sits after the ABAC-exempt early return, so impersonated
   calls to exempt routes are neither audited nor counted. PROBE1 (run through the real
   `abac_middleware` + `api_router`): impersonated `GET /api/v1/users/me/notifications` — reading
   the target's private notifications — returns 200 with **0** `impersonation_action` events and
   `actions_taken` unchanged (0). Also unattributed: `/api/v1/notifications/:id/dismiss`,
   `/resolve`, `/api/v1/trace-spans/:id/payload`, unresolved patterns, and the git HTTP / MCP /
   WebSocket paths, which all authenticate the impersonation token but never record attribution.
   This violates the spec's "Full audit trail of all actions taken during impersonation" and
   contradicts the shipped/coverage claim that "every API call made under an impersonation token"
   is recorded. Fix: emit/count at a layer that exempt routes also cross (e.g. extract auth and
   record before the exempt passthrough, or at the shared auth-resolution point), and cover or
   explicitly scope git/MCP/WS.

2. **`approve_request` reveals request status before the identity check** (impersonation.rs).
   Status and expiry are checked before `req.target_user_id != caller_user_id`. PROBE2: a
   non-target user probing an approved request gets `409 "impersonation request … is already
   approved"` instead of `403` — leaking which requests exist and whether the target consented,
   to arbitrary authenticated users. `deny_request` orders this correctly (identity first);
   approve must match. The shipped notes' claim "checked before any status is revealed" is
   inaccurate for approve.

3. **Minor — `actions_taken` counting race.** `record_impersonated_action` does an unlocked
   read-modify-write of the session record from a fire-and-forget task; concurrent impersonated
   calls can both persist `N+1`, undercounting the session and the `ImpersonationEnded` event.

## Notes for the verifier

- Full workspace suite, arch scripts, and Clippy remain verification-stage obligations; the
  focused suites above are green on this machine.
- The two probe tests were added temporarily inside the `api::impersonation` test module, run
  with `--nocapture`, and then reverted with `git checkout --`; the tree was verified clean
  afterward.
