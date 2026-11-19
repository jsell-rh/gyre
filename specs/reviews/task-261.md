# Review — task-261 (SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme)

Spec: `specs/system/identity-security.md` Layer 1 (gap spec-31d3ae72e12e22c52442e309 / spiffe-workload-attestation).
Candidate: `1452fb91766d093678097bd6903462515b0b60da` (base `67cc03988b938c4635973761c08108cee8f8459d`).
Commits under review: `c9a4d033`, `354a5e7a`, `ea0f2073`, `747c0890`, `14c63ffb`, `1452fb91`.
Verdict: **needs-revision** — two blocking findings (F1 intermittent rejection of genuine SVIDs on the verification surface; F2 deterministically broken pre-existing MCP test). The core protocol implementation itself is real and well-formed.

## What is genuinely implemented (verified)

- `crates/gyre-spiffe` is a real SPIFFE implementation, not structural fakery: `TrustDomainCa` issues ECDSA P-256 SVIDs via rcgen (single URI SAN carrying `spiffe://{td}/agent/{role}/session/{id}`, leaf not a CA, short TTL); `verify_svid` performs actual cryptographic chain validation (`x509_parser` `verify_signature` of the leaf against the trust-bundle anchors), enforces leaf + signer validity windows, requires exactly one URI SAN with no other name forms, and binds trust domain/role/session fail-closed. The verification-intent probes (unknown signer, expired, not-yet-valid, foreign trust domain, wrong session/role, tampered leaf, no SVID) each have a real unit test that fails against a broken implementation.
- The G10 local scheme is gone: `pid_is_alive` and the skip-on-empty `verify_attestation` are deleted (grep across `crates/` finds no remainder); `WorkloadAttestation` is metadata plus the verification result.
- One verification surface, no bypass variant: `spiffe::attest_session` → `verify_workload_identity` is shared by `PUT /agents/{id}/heartbeat` (403 + `agent_heartbeat` Failure audit, liveness not advanced), the new `POST /agents/{id}/attest` (ABAC-mapped), and MCP `gyre_agent_heartbeat` (`svid` required).
- Trust bundle published in SPIFFE bundle format (JWK Set, `use: "svid"`, `x5c`) at `/.well-known/spiffe/keys.json`, public like the OIDC JWKS; the bundle is the sole anchor (verification reads anchors only from `SpiffeService::anchors()`).
- External SPIRE mode (`GYRE_SPIFFE_WORKLOAD_API_SOCKET` + `GYRE_SPIFFE_TRUST_BUNDLE_URL`) with a latency-bounded gRPC-over-UDS Workload API client whose tests hand-encode the documented protobuf wire format.
- `wl_*` JWT claims preserved and now bound: `wl_stack_hash` is resolved once (`resolve_session_stack_evidence`: registered M14.1 stack, else SHA-256 over the server-authorized launch config) and the same value is bound into the SVID record, the workload env (`GYRE_STACK_HASH`), and the attestation record.
- SVID issuance is called only from the authorized spawn paths (spawn core, gate workloads, Ralph re-spawn); a failed issuance fails the launch closed.

## Round 1 probes (evidence in /tmp/stage/review-evidence)

- `/tmp/stage/bin/cargo test -p gyre-spiffe` → **24/25 pass, `ca::tests::issues_svid_that_verifies_against_its_bundle` FAILED intermittently** (see F1). 20 consecutive runs of that test: 3 FAILED / 17 ok (~15%).
- `/tmp/stage/bin/cargo check -p gyre-server` (cold) → clean, exit 0.
- `scripts/check-abac-route-registry.sh`, `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh` → all OK.
- Source-level reproduction analysis of the broken MCP test (F2) — gyre-server test compilation exceeds the sandbox 4 GiB budget; the verifier must run it.

## Findings

### F1 (blocking, code): serial-format mismatch between issuance and verification intermittently rejects genuine SVIDs

`IssuedSvid.serial` is `hex::encode(serial_bytes)` — fixed-width (32 hex chars for the 16-byte random serial, first byte forced into 1..=127). `VerifiedSvid.serial` is `format!("{:x}", leaf.serial)` — big-integer hex with leading zero nibbles stripped. When the issued serial's top byte is `0x01..=0x0F` (probability 15/127 ≈ 12%) the two strings differ.

Consequences:
- The unit test `ca::tests::issues_svid_that_verifies_against_its_bundle` fails intermittently (measured 3/20 runs; observed panic: left `39ff8c73b7e40868b9f0ed5d8c80c0a` vs right `039ff8c73b7e40868b9f0ed5d8c80c0a`).
- Production: `spiffe.rs verify_workload_identity` compares `record.serial` (issuance form) against `verified.serial` (verification form) and rejects on mismatch. In local-CA mode (the default) roughly one session in eight receives a genuinely issued, chain-valid SVID that is then permanently rejected with "presented SVID serial … does not match the issued serial": every heartbeat/attest returns 403, the agent is audited as Failure, never flips `alive`, and falls into stale-agent reaping/re-spawn churn. This breaks acceptance criterion 1 ("a spawned agent session holds an X.509-SVID … and the server's verification path validates the chain") for a random subset of sessions.

Files: `crates/gyre-spiffe/src/ca.rs` (`random_serial`, `IssuedSvid.serial`), `crates/gyre-spiffe/src/verify.rs` (`VerifiedSvid.serial`), `crates/gyre-server/src/spiffe.rs` (serial check in `verify_workload_identity`). Fix: normalize both sides to one canonical serial representation (e.g. parse the issuance bytes as a big integer before hex-formatting, or compare integer values / fixed-width forms on both sides), and pin it with a test that issues SVIDs with a leading-zero serial byte.

The task record's claim "`cargo test -p gyre-spiffe` passes (25/25) locally" is not reproducible — the suite fails at a ~12–15% rate per run.

### F2 (blocking, code/test): pre-existing MCP heartbeat test broken deterministically

`mcp.rs mcp_scope_worker_cannot_heartbeat_foreign_agent` (~line 10694) self-heartbeats with `json!({ "agent_id": worker_id })` — no `svid` — and asserts the result text contains "Heartbeat recorded". The new handler unconditionally returns `tool_error("missing required field: svid — …")` when `svid` is absent, so the assertion fails on every run. The candidate's mcp.rs diff (41 changed lines) contains no test updates. The fixture (`register_worker`) creates the agent directly with no SVID record, so even adding the field would not make the "self heartbeat passes" leg pass without seeding a session SVID.

Reproduction (verifier): `cargo test -p gyre-server --lib mcp_scope_worker_cannot_heartbeat_foreign_agent` → expected FAILED at `assert!(text.contains("Heartbeat recorded"))`. The candidate deferred all gyre-server test execution to the verifier (memory cap) and ran only `cargo check --all-targets`, so this was never observed.

### F3 (minor, docs): GYRE_SPIFFE_CA shape documented incorrectly

`docs/server-config.md` documents `GYRE_SPIFFE_CA` as JSON `{ trust_domain, ca_cert_pem, ca_key_pem }`; the actual `SerializedCa` shape is `{ key_pkcs8_b64, cert_pem }`. An operator following the doc feeds the wrong JSON and hits the hard startup panic. Also cosmetic: `auth.rs` has a duplicated `#[allow(clippy::too_many_arguments)]` on `mint_worker_scoped`.

## Required verifier probes (beyond the standard suite)

- `cargo test -p gyre-spiffe` (repeat ~10× — the F1 failure is intermittent).
- `cargo test -p gyre-server --lib mcp_scope_worker_cannot_heartbeat_foreign_agent` (F2).
- `cargo test -p gyre-server --lib api::agents::tests api::workload::tests` (heartbeat/attest accept + reject paths, trust-bundle endpoint) — written by the candidate, never executed.
