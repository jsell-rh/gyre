# Review — task-261 (SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme)

Spec: `specs/system/identity-security.md` §Agent Identity Stack Layer 1.
Candidate under review: `1da6263a9706450ec2138f6c40e63eb0002f20b6` (base `bafc03531aec9c9ddd05c52497aa10d0d8856fe5`).
Verdict: **approved** — all five acceptance criteria verified against source plus independent probe evidence; both prior durable findings (ca00525f, a619ffb4) confirmed repaired.

## Round 3 (this review)

Probes run (evidence under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-spiffe` → **27/27 passed** (`gyre-spiffe-tests.log`), including `rejects_svid_signed_by_unknown_key`, `rejects_expired_svid`, `rejects_not_yet_valid_svid`, `rejects_foreign_trust_domain`, `rejects_svid_for_different_session`, `rejects_no_svid`, `rejects_tampered_leaf_bytes`, and the deterministic leading-zero-nibble serial regression test.
- Independent adversarial probe (reviewer-authored binary against `gyre-spiffe` by path, `independent-svid-probe.rs` / `independent-svid-probe.log`, exit 0): genuine SVID verifies with matching serials; **a rogue-CA SVID carrying a well-formed SPIFFE ID for the same session is rejected** ("not signed by any key in the trust bundle"); expired rejected; foreign-session rejected; empty presentation rejected; leaf URI SAN is exactly `spiffe://{td}/agent/{role}/session/{id}`; **30/30 random-serial SVIDs verify with issuance/verification serial agreement** (the round-1 flaky class); the issued PKCS#8 private key matches the leaf's SubjectPublicKeyInfo (SVID = cert + key per spec).
- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --test api_integration` → both clean (`gyre-server-checks.log`; test-target coverage error-probed by the implementer's round, log retained).
- Mechanical gates: `check-arch.sh`, `check-abac-route-registry.sh` (new `/attest` route has its ABAC mapping), `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh` (bundle fetch is 10 s-bounded reqwest), `check-in-memory-state-stores.sh` (SVID records in the port-backed KV store), `check-fail-open-ref-resolution.sh`, `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh` — all OK.
- `bash -n` on both e2e scripts; jq and node heartbeat payload builders exercised with a multi-line PEM fixture — both carry the chain verbatim in valid JSON.
- Pre-task behavior confirmed on the base commit: `verify_attestation` at `bafc03531a` skips when either stack string is empty and `pid_is_alive` probes `/proc/{pid}` — both deleted in the candidate; `attested_pid_alive` now reports the SVID verification result, not a process probe.

Verified working (no findings):

- **Real crypto, not structure checks.** `gyre-spiffe::verify_svid` performs actual ECDSA signature verification (`x509_parser` `verify_signature` against the trust bundle's CA SPKIs), enforces validity windows on leaf and signing CA, requires exactly one URI SAN with no other name forms, and binds trust domain + role + session id. The trust bundle (JWK Set, `use: "svid"`, `x5c`) at `/.well-known/spiffe/keys.json` is the sole anchor; external mode fetches the operator-configured bundle URL latency-bounded and fails closed on non-bundle documents.
- **One fail-closed verification surface, no bypass.** REST `PUT /agents/{id}/heartbeat`, `POST /agents/{id}/attest`, and MCP `gyre_agent_heartbeat` all route through `spiffe::attest_session` → `verify_workload_identity`: SVID record must exist (unattested session rejected), an SVID must be presented, chain + validity + session + serial must verify, and the presented stack fingerprint must be non-empty and match the SVID-backed record. Rejected heartbeats return 403, emit a Failure `AgentHeartbeat` audit event, and do not advance liveness (asserted by `agent_heartbeat_without_svid_is_rejected`). Sweep of every other `heartbeat`-touching path: the stale-detector's system-orchestrator refresh and compose teardown timestamps are server-internal bookkeeping for entities with no workload process (pre-existing, documented), not verification surfaces.
- **Issuance gated on spawn-time authorization evidence.** `issue_session_svid` is called only from the authorized launch paths (`launch_agent_process`, `launch_agent_workload`, ralph re-spawn) post-ABAC with the resolved compute target and the session's stack evidence; failed issuance fails the launch closed. `GYRE_SVID_PEM`/`GYRE_SVID_KEY`/`GYRE_SPIFFE_ID`/`GYRE_STACK_HASH` are injected into the workload environment; the docker entrypoint, agent-runner, and both e2e dev scripts present them.
- **wl_* JWT claims preserved and bound.** `mint_worker_scoped`/`mint_with_workload` now carry `wl_hostname`/`wl_compute_target`/`wl_stack_hash`; the spawn and re-spawn paths resolve the stack evidence once (`resolve_session_stack_evidence`: registered M14.1 stack, else a SHA-256 over the server-authorized launch configuration) and feed the same value to the JWT claim, the SVID issuance, and the workload attestation record — not a free-form self-report.
- **Prior findings repaired.** ca00525f: serials on both issuance and verification (and the Workload API leaf parse) go through `canonical_serial_hex`; my probe's 30/30 random-serial run and the deterministic top-byte-0x01 regression test confirm the fix. a619ffb4: `api_integration::agent_heartbeat` now seeds the SVID record through the issuance path and presents chain + bound stack (same repair shape as the mcp.rs fix in 0c37ba3c); the e2e script heartbeats present the injected SVID. The `GYRE_SPIFFE_CA` doc shows the actual `SerializedCa` shape.
- **Coverage honesty.** Only rows 1–2 of `specs/coverage/system/identity-security.md` were updated, with concrete source evidence and an explicit note deferring gyre-server test execution to the verifier; unrelated rows untouched. The task record's test-evidence section accurately describes what ran here vs. what the verifier must run.

Deferred to the independent verifier (gyre-server test compilation exceeds this sandbox's 4 GiB budget; `cargo check` covers both targets):

- `cargo test -p gyre-server --test api_integration` (in particular `agent_heartbeat`).
- `cargo test -p gyre-server --lib` (heartbeat/SVID tests, including `agent_heartbeat_without_svid_is_rejected`, `heartbeat_with_tampered_stack_is_rejected`, `heartbeat_audit_carries_attestation_record_details`, workload.rs attest + trust-bundle endpoint tests, and `mcp_scope_worker_cannot_heartbeat_foreign_agent`).
- `cargo test -p gyre-spiffe`.

Minor (non-blocking, recorded for completeness):

- The MCP `gyre_agent_heartbeat` missing-field error string contains an odd wide space run ("X.509-SVID                  (the GYRE_SVID_PEM …)"); cosmetic only.
- `load_spiffe_service` panics at startup on invalid operator SPIFFE material (`GYRE_SPIFFE_CA`, mismatched socket/bundle config) — intentional fail-closed design, documented in `docs/server-config.md`.

Test-inflation check: none found. The rejection tests assert specific `VerificationError` variants and 403 outcomes against real issuance paths (foreign CA, tampered stack, empty stack, absent SVID), and my independent probe reproduces the task's verification intent outside the in-tree test code.

Scope check: no out-of-scope edits — diff touches only the task's crates, wiring, docs, dev scripts, task record, and its own coverage rows; no `web/dist/` changes (server-side task).
