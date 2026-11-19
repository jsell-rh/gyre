# Review — task-261 (SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme)

Spec: `specs/system/identity-security.md` §Agent Identity Stack Layer 1.
Candidate under review: `36bc2d13` (repair round 2; base `61b6080d`).
Commits: `c9a4d033`, `354a5e7a`, `ea0f2073`, `14c63ffb`, `0c37ba3c`,
`2934153b` (+ docs/process commits).
Verdict: **needs-revision** — one material finding (F1, a deterministically
broken pre-existing integration test); the three findings from review round 1
are verified fixed.

## Round 2 probes (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-spiffe` → **27 passed / 0 failed**, including the
  task's verification-intent probes (unknown-signer SVID for the same
  session, expired, not-yet-valid, foreign trust domain, wrong session,
  tampered leaf bytes, no SVID, empty-anchor bundle).
- Flake recheck of round-1 F1 (serial-format mismatch): CA tests × 20 runs →
  **0 FAILED** (was 3/20 on round 1). All three serial sites now format
  through `gyre_spiffe::canonical_serial_hex`, and the worst case
  (top byte 0x01, leading zero nibble) is pinned deterministically by
  `issued_and_verified_serials_agree_on_leading_zero_nibbles`.
- `cargo check -p gyre-server` → clean, 0 warnings (full server test
  compilation exceeds the sandbox 4 GiB budget — see verifier list below).
- Mechanical invariant scripts all pass: arch, ABAC route registry (new
  `/api/v1/agents/:id/attest` route has its `RouteResourceMapping` entry),
  exempt handlers, MCP write tools, dead message kinds, in-memory state
  stores (SVID records are port-backed KV, not `Arc<Mutex<HashMap>>`),
  unbounded external HTTP (bundle fetch is `.timeout()`-bounded), inert
  enforcement, migration checks, and the security-constraint lints.

## Round-1 findings — repair verification

- **R1 (serial mismatch, flaky ~12% of local-CA sessions): FIXED.**
  Issuance (`ca.rs`), verification (`verify.rs`), and the external Workload
  API leaf parse (`spiffe.rs`) share `canonical_serial_hex`; measured
  0/20 flake.
- **R2 (mcp_scope_worker_cannot_heartbeat_foreign_agent broken by the
  mandatory svid field): FIXED** in `0c37ba3c` — the test seeds the worker's
  SVID through `issue_session_svid` and presents the issued chain + bound
  stack fingerprint on the self-heartbeat leg; the foreign-agent leg still
  proves the pre-dispatch scope denial is payload-independent. Execution
  deferred to the verifier.
- **R3 (GYRE_SPIFFE_CA documented the wrong JSON shape): FIXED** —
  docs/server-config.md now documents `{key_pkcs8_b64, cert_pem}` (the
  actual `SerializedCa`); the duplicated clippy allow on
  `mint_worker_scoped` is gone.

## Verified working (no findings)

- **The Layer-1 replacement is real.** `gyre-spiffe`: fail-closed
  `SpiffeId` parse/build of `spiffe://{td}/agent/{role}/session/{id}`;
  ECDSA P-256 CA (rcgen) whose leaves carry the SPIFFE ID as the sole URI
  SAN (no other name forms), ClientAuth EKU, short TTL; `verify_svid` does
  real signature verification (`x509-parser::verify_signature` against the
  bundle's CA keys — the unknown-key-same-session probe from the
  verification intent is covered), leaf + signer validity windows,
  exactly-one-URI-SAN extraction, and session binding; SPIFFE bundle format
  (JWK Set, `use: "svid"`, `x5c`) round-trips its own CA; Workload API
  client is gRPC-over-UDS with hand-encoded wire-format tests and a
  latency bound.
- **One fail-closed verification surface, no bypass.**
  `spiffe::verify_workload_identity` requires an issuance-path SVID record,
  a presented chain, anchor-only chain validation, serial identity with the
  issued leaf, and a non-empty stack fingerprint matching the record
  (empty/tampered presentations are rejected — the retired G10
  skip-on-empty semantics are gone). `attest_session` is the single surface
  behind `PUT /agents/{id}/heartbeat` (403 + Failure audit on rejection,
  liveness not advanced), `POST /agents/{id}/attest`, and MCP
  `gyre_agent_heartbeat`. `pid_is_alive`/`verify_attestation` are deleted;
  `WorkloadAttestation` is metadata + verification result.
- **Issuance is gated on spawn-path authorization evidence** (only
  `launch_agent_process` / `launch_agent_workload` / ralph re-spawn call
  `issue_session_svid`, post-ABAC, with the resolved compute target and the
  session's stack evidence); a failed issuance fails the launch closed.
  Dispatch-rejection semantics (launched=false rather than a 500) are
  preserved.
- **Trust bundle is the sole anchor**, published at
  `/.well-known/spiffe/keys.json` (unauthenticated, like JWKS); external
  SPIRE mode anchors on the operator-configured bundle URL and fails closed
  on non-bundle documents.
- **wl_* claims preserved and bound**: `wl_stack_hash`/`wl_compute_target`/
  `wl_hostname` are minted from the same `resolve_session_stack_evidence`
  value the SVID record binds (registered M14.1 stack, else a SHA-256 over
  the server-authorized launch config).
- Coverage rows 1–2 of `specs/coverage/system/identity-security.md` updated
  with concrete source evidence and honest deferral notes; no unrelated
  rows touched.

## Findings

### F1 (blocking) — pre-existing integration test broken deterministically

`crates/gyre-server/tests/api_integration.rs` `agent_heartbeat` (~line 392)
creates an agent via plain `POST /api/v1/agents` (no spawn → no SVID
record) and PUTs `json!({})` to `/api/v1/agents/{id}/heartbeat`, asserting
`is_success()`. The new handler rejects with 403 ("no SVID record for
agent …: session was never attested") before recording liveness, so the
assert fails on every run. Same regression class as R2, which `0c37ba3c`
fixed in `mcp.rs` — but the candidate made zero changes under
`crates/gyre-server/tests/`. Reproduce (verifier; server test compilation
exceeds this sandbox's 4 GiB cap):
`cargo test -p gyre-server --test api_integration agent_heartbeat`.
Fix like the mcp test: seed an SVID via `issue_session_svid` against
`ctx.state` and present `chain_pem` + the bound stack fingerprint (or
assert the 403 explicitly as a second case).

### F2 (non-blocking) — legacy dev e2e scripts no longer heartbeat successfully

`scripts/e2e-agent.sh:32,149` and `scripts/e2e-agent-claude.sh:61` PUT the
agent heartbeat with no SVID (silently tolerated via `|| true`); agents
driven by these helpers now fail every heartbeat silently and are exposed
to stale-agent reaping. The reference docker entrypoint/agent-runner were
updated but these were not. No CI assertion fails.

### Cosmetic

`mcp.rs` missing-svid `tool_error` message contains a long literal run of
spaces; `SvidRecord.serial` doc comment ("empty for Workload API SVIDs
pre-parse") is stale — the external path computes the serial; the
`api/mod.rs:349` route-group comment still says "(G10)".

## Tests the independent verifier must run

- `cargo test -p gyre-server --test api_integration agent_heartbeat` —
  **expected to FAIL on this candidate (F1)**.
- `cargo test -p gyre-server --lib mcp_scope_worker_cannot_heartbeat_foreign_agent`
- `cargo test -p gyre-server --lib api::agents::tests` (heartbeat
  accept/reject/tampered-stack/no-svid/audit paths)
- `cargo test -p gyre-server --lib api::workload::tests` (attest endpoint,
  trust-bundle well-known endpoint)
- `cargo test -p gyre-spiffe` (green in this sandbox: 27/27)
