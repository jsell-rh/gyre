# Review: task-110 — Implement tenant & workspace invitation flow

**Candidate:** `248d72b3c1a168ae075aefe2f377a3da3cf4be64`
**Base:** `6ffd679985be9826193f4dc089d9b1f16766273c`
**Round:** re-verification after merge onto base 6ffd6799 (task-130 auth.rs
rewrite). No product code changed this round (3293fd07 task/process only;
a0dd0984 merge).
**Verdict:** **PASS** — approved.

## Scope reviewed

Full diff base→candidate (32 files, +7713/−32), focused on (a) merge integrity
with task-130's `validate_jwt`/`AgentJwtClaims` rewrite and (b) independent
re-check of the task contract (previously approved across 9+ repair rounds;
this branch carried it forward).

## Evidence (full detail: /tmp/stage/review-evidence/summary.md)

- All 18 mechanical invariant scripts pass (arch, ABAC route registry + exempt
  handlers, MCP write tools, migration versions + SQL portability, dead message
  kinds, scope-literal/fabricated-default, lossy-secret, in-memory stores,
  inert enforcement, mem port contracts, forged scope fields, byte-slice,
  relative-path defaults, unbounded HTTP, forwarded headers).
- `cargo test -p gyre-domain` 8/8; `cargo test -p gyre-adapters` full 522/522
  (incl. DB-enforced pending-scope unique index, migration 000064 dedup,
  transition_status wrong-source rejection, claim_external_id sanctioned
  repoint); `gyre-common`/`gyre-ports`/`gyre-cli` 150/1/125 all pass.
- `cargo check -p gyre-server` clean — sqlite+postgres+mem adapters (postgres
  unconditionally compiled) and the merged auth.rs type-check together.
- `cargo fmt --check` clean on all four touched crates.
- `check-task-commit-attribution.sh`: PASS against the candidate's committed
  frontmatter (the initial FAIL was the controller's pre-review working-tree
  reset of task-110.md to `commits: []`; candidate file restored for the run,
  controller state restored after).
- Server test binaries blocked by the sandbox 4 GiB wrapper — required
  verifier probes recorded: `cargo test -p gyre-server --lib api::invitations`
  (26 tests), `cargo test -p gyre-server --lib auth::tests` (the three
  jwt_email_link_* merge-regression guards), `cargo test --all`, CI clippy.

## Findings

None blocking.

- Merge integrity: the only auth.rs changes are the `email_verified` claim,
  the `find_or_create_user` → `find_or_create_user_tenant` generalization
  (Option tenant, verified-email invitation claim branch), and
  `pub(crate) resolve_unique_username`; every removed base line corresponds to
  the old signature/bindings — task-130 behavior is preserved. Email
  normalization is consistent end-to-end (invite lowercases; accept stores the
  lowercased namespace; the SSO claim searches lowercased).
- All 5 spec'd tenant endpoints functional: four registered in api/mod.rs with
  ABAC mappings; the magic-link `POST /api/v1/invite/{token}/accept` (plus
  decline) correctly lives on the public router in lib.rs — the invitee has no
  credentials; the 256-bit CSPRNG token is the auth factor, and any presented
  Bearer identity must match the invitation email or is 403'd.
- Token security: ring SystemRandom 32 bytes, only SHA-256 hash persisted,
  unique index on token_hash. Single-use acceptance enforced by a conditional
  UPDATE (from-status in WHERE) across SQLite/Postgres/mem, claimed before side
  effects with rollback to Pending on failure; expired tokens rejected with no
  user/membership created.
- Tenant containment verified in every management handler (list scoped to
  caller tenant; revoke/invite cross-tenant 403; invitee cross-tenant 403;
  cross-tenant pre-assigned workspaces skipped + logged). Role gates enforced
  in-handler because the dev token bypasses ABAC, matching repo invariant
  rules; ABAC route registry script confirms registration.
- Migrations 2026-11-13-000063/64/65 use the next unused sequence numbers and
  portable SQL; partial unique indexes make pending-scope uniqueness a
  database-level arbiter with a pre-index dedup safety net.
- Expiry job real: spawned at startup (main.rs), env-configurable interval
  with a zero-duration panic guard, conditional Pending→Expired transitions
  (a racing accept is never overwritten), rows never deleted.
- Policy per-tenant via kv_store, fails closed on corrupt rows. Bulk invite
  accepts the spec's array shape with per-entry error reporting. Local-mode
  credential delivery mints a first API key only for invitation-created
  accounts (hash-only persistence, same contract as the bootstrap key);
  pre-existing SSO/SCIM accounts get none — token possession cannot bypass an
  existing auth factor.
- Coverage matrix rows 5/6/7/25 updated to `implemented` — consistent with the
  verified behavior, not hollow.

Generated-output note: no web/ files in this diff; bundle regeneration is the
verifier's stage.
