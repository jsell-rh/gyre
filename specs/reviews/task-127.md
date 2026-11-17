# Review — task-127 (Implement user management UI pages)

**Candidate:** `f783802d857b1eed0d7ca39b754e6431d30a467d`
**Base:** `1f86b0064bdd4e3d14cb1e48bdb61270804fcf42`
**Reviewer stage:** independent review (pipeline/task-127/765c61bf…-1)
**Verdict:** NOT approved — one test-quality finding (flaky acceptance-gate
test). No production-code defects found.

## Scope reviewed

Diff base→candidate: 34 files, +5251/−232. Backend additions
(`PUT /api/v1/users/:id/role`, `GET /api/v1/invite/:token/preview`,
`NotificationRepository::list_active_for_user` across port + SQLite/PG/mem/
broadcasting adapters, `?active=true` drawer listing), the Svelte surfaces
(`InvitationAccept.svelte` new; `App.svelte`, `TenantSettings.svelte`,
`WorkspaceSettings.svelte`, `UserProfile.svelte`, `NotificationBell.svelte`,
`NotificationDrawer.svelte`, `lib/api.js`), tests, i18n, docs, spec
amendments (`ui-navigation.md` §1 bell amendment, coverage matrix row 31),
and the task record.

## What was verified (evidence under /tmp/stage/review-evidence)

1. **Backend contract is real, not structural.**
   - `update_user_role` (api/users.rs): TenantAdmin gate via `is_tenant_admin`
     (re-reads the stored record), same-tenant containment, self-change → 409,
     deactivated target → 409, unknown role → 400, GlobalRole↔UserRole set in
     lockstep, audit event `user_global_role_changed` with previous/new role.
     Route registered in `api/mod.rs` with a `RouteResourceMapping::api` entry
     in `abac_middleware.rs` (same split as `deactivate`).
   - `preview_invitation` (api/invitations.rs): read-only magic-link preview —
     an expired-but-Pending row is REPORTED Expired without persisting the
     transition (asserted by test); no credential/token material in the
     response; unknown token → 404; registered unauthenticated in `lib.rs`
     next to the accept/decline token routes (token-as-auth-factor model).
   - `list_active_for_user`: added to the port trait with doc-comment
     contract, implemented in SQLite (with a real-storage test), PostgreSQL,
     mem (code guard, matching the port's empty-set semantics), and the
     broadcasting wrapper. The `?active=true` listing deliberately skips the
     disabled-type exclusion so the drawer and the unread badge agree.
2. **Mechanical invariants pass at the candidate:**
   `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
   `check-arch.sh`, `check-mem-port-contracts.sh`,
   `check-rustfmt-diff.py <base>`, `check-lossy-secret-conversion.sh`,
   `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`,
   `check-in-memory-state-stores.sh` — all exit 0.
3. **Type-level Rust evidence:** `cargo check -p gyre-server --lib --tests`
   (via /tmp/stage/bin/cargo) finished clean in 9m14s — no errors, no
   warnings beyond the build-script web-asset notes.
4. **Focused adapter test:** `cargo test -p gyre-adapters
   list_active_for_user` → 1 passed (real SQLite storage; resolved/dismissed
   excluded, read-but-active kept, full listing unchanged).
5. **Frontend source is real Svelte 5 (runes) code** consuming the
   task-110/124/125 endpoints: api.js `skipAuth` correctly strips the Bearer
   header on magic-link calls (asserted at fetch level in api.test.js);
   every `$t()` key used by the changed components exists in
   `locales/en.json` (checked mechanically); the decisions badge removal is
   backed by the documented `ui-navigation.md` §1 bell amendment, and
   `decisionsCount` still feeds the WorkspaceHome Decisions section.
6. **Full web suite:** `npm ci` + `npm test` → 2545 passed / 41 skipped and
   **1 failed** — see finding.

## Finding (blocking, test category)

`web/src/__tests__/InvitationAccept.test.js` — flaky time-boundary
assertion. The token-flow preview test seeds
`expires_at = Math.floor(Date.now()/1000) + 30*60` (line 189) and asserts
the rendered label `.toBe('expires in 30m')` (line 200). The component's
`expiryLabel` divides remaining seconds by 60 with `Math.floor`, so any
≥1s delay between mock seeding and render (routine under the full suite's
parallel load — the run took 603s) produces "expires in 29m" and a red
suite. Reproduced once in this review's full `npm test` run; the same file
passed 15/15 in three isolated runs, confirming a timing race, not a logic
break. The authenticated-flow test (lines 82/87) carries the identical
race. The acceptance criterion "`cd web && npm test` passes" therefore does
not hold reliably. Trivial fix: seed with a margin or accept a
`/(29|30)m/` range (or freeze timers). Production expiry-label logic itself
is correct (whole-minute division, fix 72a4bd09).

## Server tests recorded for the independent verifier

- `cargo test -p gyre-server --lib invitations::` (preview ×4:
  tenant-preview without auth header, workspace-preview, expired-pending
  not persisted, unknown token 404)
- `cargo test -p gyre-server --lib users::` (`update_user_role` ×2:
  lockstep/audit/idempotence; cross-tenant + deactivated guards;
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`)
- `cargo test -p gyre-adapters list_active_for_user` (already run here: ok)
- Full `cargo test --all` per the repo-wide gate.

## Non-blocking observations

- `web/dist/` was not regenerated in this checkpoint — owned by the
  deterministic verifier; not a review finding per assignment rules.
- The bell amendment to `ui-navigation.md` is a spec-lifecycle change made
  by the implementing branch (documented in-line with rationale); it
  reconciles user-management.md's "bell icon with unread badge" with the
  older decisions-count badge wording. Judged coherent, not fakery.
