# Review — task-165 (Supply chain — gyre-stack.lock parsing and attestation level enforcement)

Spec: `specs/system/supply-chain.md` §gyre-stack.lock, §Attestation Levels, §Level 3: Gyre-Managed Runtime, §Policy per Level.
Candidate: `0f9cd02c` (base `db37fd02`; implementation carried by checkpoint commits `96dfd8b8` + `19a3d5af` listed in the task's `commits:` frontmatter).
Verdict: **approved**.

## Round 1

All probes run at the exact candidate on a clean tree; evidence under `/tmp/stage/review-evidence/` (`task165-*.log`).

Focused suites:

- `cargo test -p gyre-domain stack` → 9 passed (round-trip, integrity, drift naming, malformed TOML, missing-persona, fingerprint discrimination).
- `cargo test -p gyre-server --lib api::stack_attest` → 14 passed (register/get stack with Agent-role self-attestation, policy set/get/clear, min-level + warn mode, out-of-range level/mode 400s, legacy parse fallbacks, agent-role policy 403).
- `cargo test -p gyre-server --lib` (7 task-165 git_http push tests) → 7 passed (below-min block with ref undo, warn-mode event + stored level 1, meet-min stored 2, L3-from-workload-attestation under min-3, drift block, lockfile match, tampered-lockfile integrity rejection).
- `cargo test -p gyre-cli --test ws_integration` → 1 passed.
- Invariant gates at HEAD: migration versions, arch, relative-path defaults, ABAC route registry, inert enforcement, mem port contracts — all exit 0.

Mutation probes (source restored byte-for-byte after each; tree verified clean against `0f9cd02c`):

- Disabling the min-level comparison (`if false && level < min`) makes `push_below_min_attestation_level_blocked` and `push_below_min_level_warn_mode_flagged_and_accepted` both FAIL — the tests kill a real enforcement bug, they are not self-confirming.
- Neutering the lockfile integrity/drift check (`Ok(lock) => Ok(())`) makes `lockfile_drift_blocked_in_block_mode` and `tampered_lockfile_rejected_by_integrity_check` FAIL while `lockfile_match_accepted` stays green — the drift tests are real.

Live end-to-end (debug server at the candidate, real git client, agent JWT from `POST /agents/spawn`):

- Spawned a genuine Level-2 agent (local compute target, no container identity in the workload-attestation KV; JWT pre-minted without `wl_*` claims). Agent self-attested its stack with its own token (201) and read it back (200) — the CLI's read path is Agent-role accessible.
- `gyre stack lock` (CLI binary at the candidate, `~/.gyre/config` pointing at the server) wrote a real TOML `gyre-stack.lock` whose fingerprint equals the server-registered stack's fingerprint — CLI and server compute the same fingerprint over the shared `gyre-domain::stack` type.
- Push with matching lockfile under min-2/block policy → accepted; `GET /api/v1/agents/:id` returns `attestation_level: 2` (stored on the agent record).
- Same level-2 push under min-3/block policy → HTTP 403, server log shows `undid ref update after gate rejection`, server-side ref not advanced.
- Tampered lockfile (component edited, stale fingerprint) under min-2/block → HTTP 403, ref undone.
- Probe-correction note (reviewer error, not a code defect): an initial "push accepted under min-3" was caused by my probe creating two workspaces both named `t165-ws5` and setting the policy on the repo the git slug route did not resolve to; after targeting the actually-served repo, enforcement rejects correctly. No enforcement bypass exists.

Verified design points (no findings):

- `resolve_push_attestation_level` (git_http.rs): L3 = server-written workload-attestation KV with container identity (spawn.rs `attest_agent_with_container` for M19.4 container targets) or signature-validated agent JWT carrying `wl_container_id`+`wl_image_hash` (agent JWTs are minted by the server's Ed25519 `AgentSigningKey`; the Keycloak path also validates signatures before claims are trusted — no self-minted-claim escalation). L2 = registered `agent_stacks` KV; L1 otherwise. Ordering L3→L2→L1 is correct.
- `enforce_stack_policy` runs synchronously on every push with ref updates to a policy-bearing repo (not gate opt-in), rejects with 403 + `undo_ref_updates` + PushRejected in block mode, emits ConstraintViolation and accepts in warn mode — matching the spec's "rejected or flagged based on policy".
- `RepoStackPolicy.min_attestation_level` (1..=3) and `enforcement` (block default / warn) are validated on PUT (out-of-range level and unknown mode → 400); legacy KV entries derive the new fields from `required_level` with plain-string fallback preserved; `parse_stack_policy` keeps the two level fields consistent, and `set_stack_policy` only ever stores them equal, so the merge-constraint consumer (`get_repo_required_attestation_level`) sees a coherent policy.
- Lockfile drift/integrity: `read_git_file` reads `gyre-stack.lock` from the pushed tree at the new SHA; `verify_integrity()` recomputes the composite fingerprint from components; `check_drift()` compares against the pushing agent's attested stack and names differing components. Both follow the policy's enforcement mode.
- Migration `2026-10-10-000056_agent_attestation_level` is the next unused sequence number, plain portable `ALTER TABLE` SQL; sqlite/postgres round-trip plus mem whole-struct clone; `attestation_level` exposed on `GET /api/v1/agents/:id`.
- Provenance label mapping (server-verified / self-reported / unattested) now follows the resolved level rather than the old stack+policy heuristic — more spec-faithful than the base behavior.
- The `scripts/relative-path-defaults-exemptions.txt` change is a line-number update of an existing exemption (task-165 insertions moved the line), not a new entry; all exemption-freeze gates pass.

Non-blocking observations (recorded for the verifier):

- **Evidence-accuracy**: the Shipped section claims `ws_integration::test_auth_and_ping_roundtrip` "fails in this sandbox on TcpListener::bind errno 95 (`capabilities.json: tcp_listener_probe.supported=false`)". The current review sandbox's `capabilities.json` says `tcp_listener_probe.supported: true` and the test passes here (0.00s). The claimed blocker does not reproduce; since the excused test is green, nothing is concealed. The executor-round environment may have differed, so this is recorded as a discrepancy, not fakery. Verifier should run the full `cargo test --all` (owned by that stage) — the one test it was said to excuse passes in this sandbox.
- **Scope**: the candidate diff includes `web/dist` churn (re-bundled assets with new content hashes) from the pipeline checkpoint commits, with no `web/src` changes between base and candidate — build-artifact churn from `build.rs` web rebuilds during pipeline rounds, not task-165 product code. The task's acceptance criteria do not cover the UI; the verifier rebuilds web from source per its instructions.
- **Known partials** (disclosed in coverage rows 7/8/9 and consistent with the task's implementation plan): the lockfile is not approver-signed (integrity is the recomputed fingerprint, not a signature); L3 trust basis is server-observed container identity + server-minted JWT claims, not SPIFFE/eBPF; stack attestation remains unsigned (pre-existing rows 4/5/10). The task plan scoped L3 detection to the container-claim check, which is what shipped.
- The JWT-based L3 detection branch (`wl_container_id`+`wl_image_hash` claims) has no production minter today (spawn pre-mints without workload claims and stores container identity in the workload-attestation KV, which is the checked primary path); the branch is forward-looking but harmless and accurately described in the Shipped notes.

Acceptance criteria check (all independently verified):

- `gyre-stack.lock` schema defined and parseable — yes (gyre-domain `StackLockfile`, 9 unit tests, live-generated file parses and round-trips).
- `gyre stack lock` generates the lockfile — yes (live e2e against a running server).
- Attestation level 1/2/3 correctly resolved — yes (unit tests for all three; live L2 stored; mutation-killed tests).
- Repo stack policy supports `min_attestation_level` — yes (validated field, live PUT/GET round-trip).
- Below-minimum pushes rejected or warned per policy — yes (live 403 + ref undo in block; warn-mode event + acceptance in unit test).
- Stack drift detection compares lockfile vs submitted attestation — yes (unit + mutation + live tampered-lockfile rejection).
- `cargo test --all` — focused suites for every task-165 behavior green at the candidate; full-suite run belongs to the verification stage (the previously-claimed ws_integration blocker does not reproduce here and that test passes).
