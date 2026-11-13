# Review — task-195: Manifest-driven gate selection for MRs referencing a spec

- **Candidate:** `6316b8c86281ae7f16b145e8abdb056e1c47c950`
- **Base:** `349a32ac6f5557786f82d17c13f8815de600e837`
- **Reviewer scope:** independent re-review of the rebase-repair round; no prior
  approval carried forward. All findings re-derived from the diff and fresh probes.
- **Verdict:** **approved** (no blocking findings).

## What was reviewed

The full diff `349a32ac..6316b8c8`: `gate_executor.rs` (manifest gate selection,
persona-constrained review path, forge-spawned gate agents + teardown),
`api/spawn.rs` (shared `launch_agent_workload`/`dispatch_agent_launch` extraction,
validated bare-`persona` spawn field), `api/merge_requests.rs` (reviewer identity
binding in `submit_review`), `abac_middleware.rs` (reviewer allow-list doc),
`mcp.rs` (`gyre_spawn_worker` persona param), deletion of unrouted legacy
`api/agent_messages.rs` + `messages.rs` (verified unrouted at base), web
`selfReviewerId` flows, coverage/task/docs updates, and the
fabricated-scope-defaults exemption entry removal (count 2→1, comment-only +
one dead entry removed — no growth; checker passes).

## Acceptance-criteria verification

1. **Both manifest personas run as GateResult rows** —
   `trigger_gates_runs_manifest_gates_for_spec_ref_mr` builds a real git repo
   with `specs/manifest.yaml` at HEAD, a registered repo/workspace/MR with
   `spec_ref = "specs/system/identity-security.md@<sha>"`, calls production
   `trigger_gates_for_mr`, asserts 2 `manifest-gate:` result rows (security +
   accountability) and `check_gates_for_mr == Ok(false)` while pending.
2. **No regression without spec_ref / manifest** —
   `trigger_gates_no_spec_ref_runs_repo_gates_only` and
   `trigger_gates_repo_without_manifest_runs_repo_gates_only` assert exactly the
   repo-level gates run (1 result, correct gate id).
3. **No double-run on persona overlap** —
   `trigger_gates_manifest_gate_wins_over_repo_gate_for_same_persona` seeds a
   repo-level `personas/security.md` AgentReview gate and asserts the security
   persona runs exactly once, as the manifest gate (manifest-wins is justified:
   the manifest gate carries the min_attestation_level/stack_hash constraints).
4. **Real persona path, merge-blocking** — `run_agent_review_gate` routes
   constraint-carrying no-command gates to `run_persona_review_gate`, which
   polls `evaluate_persona_reviews` and **fails closed** at timeout (never
   auto-passes); `persona_gate_fails_without_qualifying_review` kills any
   regression to the stub. `check_gates_for_mr` defaults unknown gate ids to
   `required = true` (verified in source), so Failed blocks / Pending holds;
   `trigger_gates_failed_manifest_gate_blocks_merge` asserts the Err names the
   manifest gate. Constraints are enforced for real: persona match from the
   `agent_personas` KV, `attestation_level >= min` from the reviewer's actual
   workload attestation, exact `stack_hash` when pinned — each with a dedicated
   failing-case test (below-minimum, stack mismatch, wrong persona).
5. **Test that fails if sourcing is removed** — the pure helper
   `manifest_gates_for_mr(Option<&SpecManifest>, spec_ref)` is unit-tested on
   the resolved gate set (paths with/without `specs/` prefix and `@sha`, bare
   paths, missing manifest/spec_ref/entry/gates, persona dedupe); the trigger
   test above fails if `resolve_manifest_gates_for_mr` stops feeding it.
6. **Builds/checks** — see probes below.

Agent-instruction compliance: reuses task-193's `read_manifest` (no second
reader; manifest read at HEAD, matching task-193's policy source — the SHA in
`spec_ref` is a blob SHA, not a treeish); manifest gates persist into the same
`state.gate_results` store `check_gates_for_mr` reads (no parallel table); the
persona path is real, spawned gate agents get `review:submit`-scoped EdDSA JWTs
whose subject `submit_review` binds to the reviewer identity (forgery tests:
agent-token caller bound to own identity; non-admin human cannot record for
another reviewer; admin on-behalf allowed).

Supporting security work verified as sound, not scope creep: the bare `persona`
spawn field is validated fail-closed (`resolve_persona_in_chain`, Approved
only, before any state is created — `spawn_bare_persona_fails_closed`), because
`agent_personas` is the trust source the manifest gate reads; teardown kills
via the owning `spawned_backends` backend (regression-killing test with a
Container-registered local process); scoped tokens are denied the REST inbox
route (`scoped_review_token_cannot_read_agent_inbox_route`).

## Probes run (evidence in /tmp/stage/review-evidence/)

| Probe | Result |
|---|---|
| `bash scripts/check-arch.sh` | exit 0 |
| `bash scripts/check-abac-route-registry.sh` | exit 0 |
| `bash scripts/check-fabricated-scope-defaults.sh` | exit 0 |
| `/tmp/stage/bin/cargo check -p gyre-server --tests` | exit 0 (9m24s; type-checks all test code) |
| `cd web && npm ci` | exit 0 |
| `npx vitest run` (full web suite) | 112 files, 2298 passed / 41 skipped, exit 0 |
| `cargo test -p gyre-server --lib gate_executor` | **blocked by the build wrapper** (4 GiB budget) — verifier must run (below) |
| `git diff b9cb5a5f 6316b8c8 -- crates/` | empty — no Rust source changed since the previously verified published head; this round is a rebase resolution (web/src merge + dist restore to base, `git diff 349a32ac -- web/dist` empty) |

## Tests the independent verifier must execute

- `cargo build --all`
- `cargo test -p gyre-server --lib gate_executor` (manifest gate selection,
  persona-gate pass/fail paths, trigger/union/dedup, gate-agent spawn, scoped
  token, teardown + backend-routed kill, re-enqueue reuse)
- `cargo test -p gyre-server --lib spawn` and `--lib merge_requests`
  (bare-persona fail-closed spawn; reviewer identity binding)
- `cargo test -p gyre-server --test task134_review_probe` (incl.
  `scoped_review_token_cannot_read_agent_inbox_route`)
- `cargo test --all`

## Non-blocking observations

1. `resolve_manifest_gates_for_mr` returns empty on MR/repo store lookup
   errors — fail-open on a transient infra failure, consistent with the
   pre-existing repo-gate listing posture (`list_by_repo_id().unwrap_or_default()`)
   the task explicitly preserved. Worth hardening in a follow-up, not a
   regression introduced here.
2. Stale `Failed` GateResult rows from a previous enqueue keep blocking a
   re-enqueued MR (deterministic manifest gate ids reuse the same
   `gate_results` semantics repo-level gates already have at base) —
   pre-existing platform semantics, applies equally to both gate sources.
3. Generated-output note (verifier-owned, non-blocking): running `npm ci` in
   `web/` regenerates `web/dist` bundle hashes as a side effect; the review
   restored `web/dist` to the candidate state (identical to assignment base).
   The verifier rebuilds and binds publication to the tested tree.
