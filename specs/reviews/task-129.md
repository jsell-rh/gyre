# Review: task-129 — ABAC evaluation engine, API, and audit integration

**Candidate:** `22ae53b67f341d9ef7648f24acfffa058588b3bb` (branch
`pipeline/task-129/1079ec5cf1404952af58e34993a21-1`, base `b6892403`)
**Reviewer stage:** round 10 post-repair (c459a69b fixture alignment)
**Verdict:** **approved**

## What was reviewed

Full diff base..candidate (22 files, +4858/−308), focused on:

- `gyre-server/src/policy_engine.rs` — engine core: attribute context
  (agent-claim allowlist), fail-closed dynamic `$ref` resolution,
  condition evaluation (incl. list set-semantics), canonical
  `scope_applies` filter, `policy_sort_key`, `evaluate`
  (immutable-Deny-first → priority-DESC/scope/deny tiebreakers →
  first-match-wins → default deny), `build_decision`,
  `record_decision_audit` (warns on dropped audit writes).
- `gyre-server/src/policy_cache.rs` — read-through
  `CachedPolicyRepository` (per-scope keys, invalidate-on-successful-
  mutation, explicit out-of-band invalidation handle for
  `apply_trust_transition`), wired into both `build_state` and
  `mem::test_state`.
- `gyre-server/src/abac_middleware.rs` — middleware migration: no
  system-token bypass (allowed BY `system-full-access`), reserved-identity
  containment before evaluation, scoped-gate-token allow-lists, full
  subject/resource/action/env extraction (real entity + membership +
  budget + queue-health lookups), decision recording on every path;
  `seed_builtin_policies` (fail-closed, drift-refreshed) called from
  main.rs before serving; `m34_builtin_policies` +
  `domain_builtin_policies` cover the full §Built-In Policies table
  including `builtin:policy-admin-only` (Admin-only policy management).
- `gyre-server/src/api/policies.rs` — all 8 §API endpoints, scope
  filtering, reserved-prefix and built-in/immutable guards on
  update/delete (incl. stored-name trust: guard), dry-run, decisions
  query, effective permissions with real-store subject resolution and
  the documented `?subject=` binding.
- `gyre-domain/src/policy.rs` — builtin definitions (system-full-access
  matched by reserved `subject.id`, tenant-isolation 950 fail-closed on
  missing tenant attrs, workspace-membership-required 940 keyed on the
  derived membership boolean, persona-human-approval 950, immutable
  require-human-spec-approval 999, agent-repo-scope 750 enabled with the
  `$resource.repo_id` reference).
- Audit integration on merge-time (`evaluate_merge_abac`,
  `evaluate_attestation_abac` — fail-closed store reads, requeue on
  evaluation unavailability) and push-time
  (`evaluate_push_attestation_abac`, audit-only per TASK-006 Phase 1,
  tenant from entity lookup, task-id correlation key).
- Reserved-identity hardening across every display_name write path
  (users.rs, scim.rs create+update, SSO provisioning/login) plus the
  middleware pre-evaluation deny.
- Round-10 fixture changes (c459a69b): priority-only moves of merge-time
  attestation fixtures into the documented user band (200–299) and one
  STRENGTHENED assertion (`evaluate_respects_policy_scope` now pins
  `matched_policy == builtin-default-deny`). No assertion weakened.

## Independent probe evidence

`/tmp/stage/review-evidence/engine-probe/` — a standalone crate
path-depending on the checkout's `gyre-domain`/`gyre-common`, compiling
the EXACT source text of `policy_engine.rs` lines 1–516 (extracted via
sed, sha256 `47f8f291…` re-verified against a fresh extraction after all
mutations were reverted). 10/10 probes pass, including:
immutable-Deny-beats-p1000-Allow (real immutable builtin), deny-over-allow
at equal key, repo>workspace scope tiebreak, round-4 cross-workspace scope
filter, real tenant-isolation vs system-full-access ordering, real
agent-repo-scope containment over a 700 role Allow, real
workspace-membership-required, fail-closed dynamic refs, agent-claim
allowlist (no workspace_role spoofing), <1ms mean latency over 200 runs
of the real builtin set.

Mutation killing power verified on the extracted copy (each reverted,
sha re-checked): skipping the immutable-first pass, dropping
`scope_applies` from the retain filter, and fail-open `$ref` resolution
each fail exactly the targeted probe.

Static verification (remote, pre-review, on this exact candidate):
rustfmt, clippy changed-lines, arch/hierarchy lints, ABAC route registry
+ exempt-handler checks, MCP write-tool gate, migration
versions/portability, and every repo invariant script — all clean.

## Notes (non-blocking)

- `cargo test --all` cannot run in this review sandbox (gyre-server test
  compilation exceeds the 4 GiB cgroup; wrapper rejects server codegen).
  Last remote run (round 9) failed 7 fixture-drift tests; round-10
  repaired them with assertions unchanged or strengthened. The
  deterministic verifier must run the full suite plus the focused probes
  listed in `/tmp/stage/review-evidence/engine-probe/README.md`
  (policy_engine, policy_cache, api::policies, attestation_abac,
  abac_middleware live tests).
- The candidate added a paragraph to
  `specs/system/abac-policy-engine.md` documenting the reserved
  `gyre-system-token` identity. It is a conservative clarification of
  implemented hardening, consistent with the existing spec text (code
  does not contradict the spec), and is reflected in the coverage notes.
  Noted for spec-lifecycle hygiene; not a defect.
- `GET /api/v1/policies/effective` resolves any `?subject=` against the
  real stores (not restricted to the caller). The §API table does not
  require self-only queries; policy reads are already role-gated. No
  action required, recorded as an observation.

## Conclusion

The evaluation engine, composition rules, built-in set, caching,
audit integration, and API match the spec sections
(§Evaluation Engine → §Dry-Run Evaluation) with real production code —
real entity/membership/budget lookups, real crypto-backed identities,
fail-closed discipline throughout, and live tests that assert both the
HTTP outcome and the policy-match provenance in the persisted decision
trail. Independent behavioral probes pass with demonstrated
mutation-killing power. No structural defects, missing enforcement,
unsafe scope handling, fake implementations, or weakened tests found.
