# Review: TASK-261 — SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme

**Reviewer:** Independent review (stage)
**Date:** 2026-10-01
**Verdict:** approved
**Candidate:** `91fa2660f1e8bd3bca70148e511713c6e450c06f` (base `05024b3eb99d474cc523e0a365f14e543641e8f7`)
**Spec:** `specs/system/identity-security.md` §Agent Identity Stack / Layer 1

**Commits under review:** `c9a4d033`, `354a5e7a`, `ea0f2073`, `14c63ffb`, `0c37ba3c`, `2934153b`, `d9d97d4e` (all recorded in the task's `commits:` frontmatter; attribution check passes against the committed task file).

**Verification performed:** read the full base→candidate diff (all 40 files); ran `cargo test -p gyre-spiffe` (27 passed — including the task's core probe: rogue-CA SVID with a well-formed SPIFFE ID for the same session rejected as UntrustedSigner, expired/not-yet-valid, foreign trust domain, wrong session/role, no-SVID, tampered-leaf-bytes, empty-anchor-set, and the serial-format regression from round 1); ran `cargo check -p gyre-server` (clean); ran 24 mechanical repo checks; grep-verified the G10 retirement and the absence of bypass surfaces. Full evidence: `/tmp/stage/review-evidence/`.

---

## Findings

None blocking. The task contract is met with real production code:

- **Layer 1 uses the specced protocol.** New crate `gyre-spiffe`: `SpiffeId` (fail-closed parse/build of `spiffe://{trust-domain}/agent/{role}/session/{id}`), `TrustDomainCa` (ECDSA P-256 via rcgen, persisted/restartable, operator-provisionable, key/cert mismatch rejected), `verify_svid` (real x509-parser/ring `verify_signature` of the leaf against the trust bundle's anchors, validity windows on leaf and signer, exactly-one-URI-SAN, session binding), SPIFFE bundle format (JWK Set, `use: "svid"`, `x5c`), and a latency-bounded SPIFFE Workload API gRPC-over-UDS client whose protobuf decode is pinned to hand-encoded wire bytes.
- **The G10 scheme is gone.** `pid_is_alive` and `verify_attestation` (the `/proc/{pid}` probe and skip-on-empty string compare) are deleted; `WorkloadAttestation` is metadata plus the SVID verification result; the only production writer of `alive` is the verification surface.
- **One fail-closed verification surface, no bypass.** REST heartbeat, `POST /agents/{id}/attest`, and MCP `gyre_agent_heartbeat` all route through `attest_session` → `verify_workload_identity`: no SVID record → reject; no SVID presented → reject; chain/validity/session/serial checks against the published bundle; stack fingerprint must be non-empty and match the SVID-backed record (empty and tampered presentations rejected — both tested). Rejected heartbeats are audited as Failure and do not advance liveness.
- **Trust bundle is the sole anchor.** Served at `/.well-known/spiffe/keys.json` (tested unauthenticated 200); verification anchors parse the same bundle document; external-mode bundles are fetched with a timeout and fail closed on non-bundle documents; trust is never derived from the workload's own response.
- **Issuance gated on spawn-time authorization evidence.** `issue_session_svid` is called only from the post-ABAC launch paths with the resolved compute target and the session's stack evidence; failed issuance fails the launch closed.
- **`wl_*` JWT claims preserved and bound.** `wl_stack_hash`/`wl_hostname`/`wl_compute_target` are minted from the same single-resolved evidence the SVID record binds (spawn, re-spawn, and gate paths all use one resolution).
- **Mechanical checks pass** (arch, ABAC route registry, MCP write gate, inert enforcement, unbounded HTTP, byte-slice, relative paths, lossy secrets, in-memory stores, etc.). The three scripts that fail here (`check-crypto-verify`, `check-assertionless-tests`, `check-dead-test-code`) crash at awk parse time under this sandbox's mawk and are unchanged by this diff — pre-existing environment issues for the verifier to re-run under gawk.

## Required verifier probes (blocked here by the 4 GiB build wrapper)

- `cargo test -p gyre-spiffe`
- `cargo test -p gyre-server --lib -- api::workload:: spiffe::`
- `cargo test -p gyre-server --lib -- api::agents::tests::agent_heartbeat api::agents::tests::heartbeat_`
- `cargo test -p gyre-server --lib -- mcp::tests`
- `cargo test -p gyre-server --test api_integration -- agent_heartbeat`
- Re-run `scripts/check-crypto-verify.sh`, `check-assertionless-tests.sh`, `check-dead-test-code.sh` under gawk

## Non-blocking observations (recorded for follow-up, not defects)

1. External-mode (SPIRE Workload API) has no server-level integration test; `SpiffeService::external_with_transport` is `cfg(test)`-only and unused, and `verify_svid_for_record_id` has no direct unit test (its shared chain checks are covered by the 11 `verify_svid` tests). Components are individually tested; the mode fails closed throughout.
2. Chain validation checks the leaf signature directly against bundle anchors (SPIFFE anchor-set semantics) without RFC 5280 intermediate path building — stricter, fails closed; correct for Gyre's CA mode where the leaf is signed directly by the bundled CA.
3. `launch_agent_process` now propagates SVID-issuance failure as an error; the REST spawn surface leaves the already-created agent record/token behind on that path (the manual-retry semantics only cover compute-launch failures). Fail-closed behavior is correct; the orphaned record is an operational wart.
4. `bind_stack_fingerprint`'s doc comment still describes a first-time-binding branch the code no longer contains (issuance always binds non-empty evidence). Doc drift only.
