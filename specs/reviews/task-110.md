# Review — task-110: Implement tenant & workspace invitation flow

**Candidate:** `daf84ce019ef17fc068416ebe40399dabea3c9a0`
**Base:** `f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3`
**Round:** independent review (controller stage), evidence under
`/tmp/stage/review-evidence/` (summary.md + script logs).

## Verdict: NOT APPROVED (1 blocking finding)

## What was verified as real

- **Entities/ports/adapters:** `TenantInvitation`, `WorkspaceInvitation`,
  `InvitationStatus`, `InvitationPolicy` (gyre-domain) with every spec
  field; port traits with a conditional `transition_status` single-use
  claim contract; SQLite + Postgres + mem implementations with
  pending-scope uniqueness enforced in code AND by partial unique indexes
  (migration `2026-11-13-000064`, including pre-index raced-duplicate
  dedupe). All mechanical invariant scripts exit 0 (arch, ABAC registry,
  migration versions/portability, mem port contracts, inert enforcement,
  scope-literal/fabricated defaults, lossy secrets, in-memory stores,
  unbounded HTTP, exempt handlers — see evidence table).
- **Endpoints:** all five specced tenant-invitation endpoints plus
  policy get/put, workspace invite/list/revoke, invitee-facing
  accept/decline and `GET /users/me/invitations`, and token-based
  accept/decline for both invitation kinds (registered outside
  `require_auth_middleware` — the token is the auth factor — with
  rate-limit/tracing layers still applied). Per-handler TenantAdmin /
  Owner-Admin guards, tenant containment, `max_pending_invitations` cap,
  `allow_re_invite=false` blocking, and single-use claims before side
  effects with rollback all present and covered by behavior-bound HTTP
  tests.
- **Tokens:** 32 CSPRNG bytes (ring `SystemRandom`), stored only as
  SHA-256; raw token only in the create response.
- **SSO linking:** first SSO login claims an invitation-created local
  account only on `email_verified=true` and same-tenant email match, via
  the sanctioned `claim_external_id` port transition (refuses taken
  ids); unverified-email and cross-tenant claims covered by real tests in
  `auth.rs`.
- **Expiry:** background job spawned from `main.rs`, env-configurable
  interval, startup tick, conditional Pending→Expired transitions;
  `run_expiry_once` unit-tested (accept path also lazily expires).
- **Probes:** `cargo check -p gyre-server` exit 0; `cargo test -p
  gyre-domain` 8/8; `cargo test -p gyre-adapters invitation` 5/5 against
  real SQLite; `cargo test -p gyre-adapters sqlite::user` 18/18.

## Blocking finding

**F1 — Local-mode acceptance issues no credential; the account cannot
authenticate afterwards** (`crates/gyre-server/src/api/invitations.rs`,
`AcceptInviteRequest`/`AcceptInviteResponse`, ~lines 611-635 and the
`accept_tenant_invitation_claimed` user-creation path).

Spec `user-management.md` §Tenant-Level User Onboarding (Local mode):
"Invitee receives a magic link **or sets a password**"; flow step 3
"Local mode: prompted to set display name **+ password**"; step 5
"Invitee lands on their dashboard"; "Local user accounts are first-class,
not second-class... The platform must work without any external identity
system."

The shipped accept endpoint accepts `display_name`/`username` only — no
password field, no API key or session token minted in the response — and
the platform has no password-login path at all (zero password-handling
code in `auth.rs`, the CLI, or the web login, which is bearer-token
based). The only unauthenticated credential mint is `POST /api/v1/users`
(admin bootstrap), which creates a *new* user and cannot serve an
already-accepted invitee; API keys are otherwise issued only via the
authenticated `/users/me/tokens`.

Consequence: in a local-mode (air-gapped / no-Keycloak) tenant — the
spec's stated critical scenario — an invited user who accepts via
`POST /api/v1/invite/{token}/accept` receives an account and memberships
that can never authenticate again. The SSO-mode path is complete; the
local-mode path dead-ends, yet coverage row 5 marks the section
"implemented" ("accept mints a first-class local user") with no note of
the gap and no spec amendment recording a scoping decision. This
contradicts the repo goal's rule 3 (never mark a section implemented
without production code satisfying it).

Reproduce: invite a user in a tenant with no OIDC; capture `invite_url`;
POST the accept; observe the response contains no credential; then note
that no endpoint accepts any proof-of-identity from this user to obtain
a bearer token.

Acceptable closures (any one): mint a first API key in the accept
response (mirroring the `POST /api/v1/users` bootstrap pattern), or
implement password set-at-accept plus a local password-login route, or
amend the spec via the spec lifecycle and correct the coverage row.

## Non-blocking observations

1. No test exercises the invitee-facing routes through the full
   `api_router` + ABAC middleware chain (all invitation tests mount
   handler-only routers). A ReadOnly-global-role invitee would be
   default-denied by ABAC on the authenticated accept route (the
   token-based accept still works). Recommend a full-chain test.
2. `resolve_workspace_invitee` and the duplicate-pending checks scan
   full user/invitation lists in memory — correct but O(n).
3. `check-task-commit-attribution.sh` fails only against the
   controller-modified working tree (`commits: []`); the committed
   candidate carries the full 8-commit attribution. Not a defect.

## Verifier must run

- `cargo test -p gyre-server --lib api::invitations`
- `cargo test -p gyre-server --lib auth::` (email-link claim tests)
- `cargo test --all`
- `cargo clippy --all-targets --all-features -- -W clippy::all`
- `python3 scripts/check-rustfmt-diff.py f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3`
