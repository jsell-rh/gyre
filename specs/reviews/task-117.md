# Review: TASK-117 — Implement compute target model

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-05 (R1)
**Verdict:** needs-revision

**Candidate:** `976b339afcd1ef253c5819251184097adaa1d917` (assigned; branch
`pipeline/task-117/883d25c41e1649edb3a8353567db6cf8-6`, base `110180fb`)
**Spec:** `agent-runtime.md` §3 Compute Target Model (Abstraction, Supported
Backends, Nix-Based Image Build, Tenant and Workspace Configuration)

**Verification performed:** read the full base→candidate diff (26 files,
+3211/−248) and all new/changed compute-target sources (ports/compute.rs,
adapters compute/{container,ssh,kubernetes,local}.rs, api/compute_targets.rs,
workspaces.rs, spawn.rs, admin.rs, abac_middleware.rs, mem.rs, flake.nix);
ran `cargo test -p gyre-adapters compute --lib` (40 passed, 0 failed, 4
ignored — reproduced), `cargo check -p gyre-server --lib` (clean), the six
key static gates (all exit 0), and two focused probes saved under
`/tmp/stage/review-evidence/` (see README.md there for commands, sources,
exit codes, and output).

---

## Findings

- [ ] **F1 (blocking): the candidate's own merge commit duplicated the
  entire spawn.rs test block — the gyre-server test target does not
  compile, `cargo test --all` fails, and every claimed gyre-server test
  result is unreproducible at this candidate.** The candidate `976b339a`
  is the merge of `f7631cfb` (branch) into `110180fb` (base). Both
  parents contain exactly ONE copy of `mod tests` content keyed by
  `spawn_creates_agent_active_with_token`; the merge result contains TWO
  verbatim copies (spawn.rs lines 2162–3160 ≡ 3162–4158; `diff` shows only
  trailing blank lines). Consequence, reproduced:
  `cargo test -p gyre-server --lib api::compute_targets` exits 101 with
  `could not compile gyre-server (lib test) due to 36 previous errors` —
  all 36 are E0428 "defined multiple times" for the duplicated test
  functions (log: review-evidence/server-libtest-compute-targets.log).
  `cargo check -p gyre-server --lib` passes, so production code is fine —
  the breakage is confined to the merge-mangled test module. This fails
  the task's acceptance criterion "`cargo test --all` passes" and means
  the shipped evidence ("compute_targets 10 passed", "workspaces 15
  passed", "spawn 17 passed", "abac_middleware 9 passed") cannot have run
  against this exact candidate — those runs predate the final checkpoint
  merge. Note also that the breaking commit `976b339a` is not in the
  task's `commits:` frontmatter (the list ends at the pre-merge branch
  commits), so the verifier's commit-scoped rounds would not even see it.
  Fix: re-do the merge resolution for spawn.rs keeping one copy of the
  test block (the second copy adds nothing — it is byte-identical), then
  re-run the full gyre-server lib suite at the resulting candidate.

- [ ] **F2 (code/scope): the request-supplied `compute_target_id` in
  `POST /api/v1/agents/spawn` is resolved globally with no tenant
  validation — a caller can bind their agent to ANOTHER tenant's compute
  target.** In `spawn_agent_core` (spawn.rs ~360–396), when
  `req.compute_target_id` is set the handler does
  `state.compute_targets.get_by_id(...)` and only 404s when the id is
  unknown; it never compares `entity.tenant_id` to the caller's tenant
  (or the workspace's tenant). The sibling paths validate: the
  workspace-binding path (`PUT /api/v1/workspaces/:id`, workspaces.rs
  ~238–275) explicitly rejects cross-tenant and unknown ids, and the
  tenant-default path derives from the workspace record — only the
  request path trusts the caller. Probe (temporary test, source restored
  and verified clean afterward): seeded
  `ComputeTargetEntity { id: "ct-foreign", tenant_id: "other-tenant" }`,
  spawned as `test-token` (Admin of tenant "default") with
  `"compute_target_id": "ct-foreign"` → **201 Created,
  `compute_target_id: "ct-foreign"`** (log:
  review-evidence/cross-tenant-spawn-probe.log). In production this runs
  the agent on the foreign tenant's infrastructure and feeds the foreign
  tenant's backend config blob (SSH identity/host, k8s context,
  namespace, image) into the local spawn dispatch — the exact
  caller-supplied-scope-field anti-forgery class the repo's standing
  constraints call out (cf. check-forged-scope-fields.sh). There is also
  no test for it: the compute-target spawn tests cover workspace-assigned
  and tenant-default resolution only. Fix: after `get_by_id`, reject with
  404 when `entity.tenant_id != auth.tenant_id` (same
  indistinguishable-from-missing shape as the workspaces handler), and
  add a cross-tenant spawn test mirroring
  `update_workspace_rejects_cross_tenant_compute_target`.

- [ ] **F3 (minor, documented deviation): the `wl_image_hash` JWT claim is
  never populated — the image hash lands only in the kv-stored workload
  attestation record.** Spec §3 Nix-Based Image Build: "The image hash is
  recorded in workload attestation (`wl_image_hash` JWT claim)."
  `mint_with_workload` (auth.rs:189) carries the claim field, but the
  only caller is `mint()` passing `wl_image_hash: None` (auth.rs:181),
  and the spawn flow pre-mints the JWT before the digest is knowable
  (spawn.rs ~460), so no token ever carries it; the per-backend digests
  (`image_digest` / `remote_image_digest` / `pod_image_digest`, all real
  CLI probes) land in `WorkloadAttestation.image_hash` via
  `attest_agent_with_container`, surfaced by `GET /api/v1/agents/:id/workload`.
  The deviation is acknowledged in the task's Shipped notes, but per the
  repo's spec-lifecycle rule a deviation from the spec text should be
  amended in the spec (or the claim actually populated, e.g. on token
  refresh/re-mint after spawn), not just documented in the task.

---

## What was verified as real

For the record, the substantive implementation behind the broken merge is
genuine and matches the spec: the `ComputeTarget` port with the five spec
methods; `ContainerTarget` enforcing `--network=none`/`--memory=2g`/
`--pids-limit=512`/`--user=65534:65534` at spawn with opt-in bridge;
`SshDockerTarget` running `docker run` on the remote host over batch-mode
SSH with per-argument `shell_quote` escaping and the same security
defaults; `KubernetesTarget` creating Pods via `kubectl apply` from a
generated manifest (runAsUser/Group 65534, runAsNonRoot, no privilege
escalation, memory limit, sanitized RFC 1123 pod names, delete/phase-check
lifecycle); tenant-scoped persistence behind `ComputeTargetRepository`
(SQLite/Postgres/mem with duplicate-name guards — adapters suite
independently reproduced: 40 passed); full tenant-scoped CRUD API with
Admin-gated mutations and ABAC resolver mappings (exemption count lowered
53→51 with the stale entries removed, as the gate rules permit);
workspace selection with a validated fallback chain; and a real
`docker/gyre-agent/flake.nix` (pinned nixpkgs, `npm ci` from the committed
lockfile, User 65534:65534). The two probes above are the defects; F1
alone is sufficient to require a fix round.

**Required before approval:** F1 (compile) and F2 (tenant validation +
test). F3 should be resolved by spec amendment or claim population, or
explicitly accepted by the spec owner.
