# Review — task-262 (SPIFFE federation: trust-bundle exchange between Gyre instances)

Spec: `specs/system/identity-security.md` §Federation, bullet 3 — "SPIFFE federation
handles the workload attestation layer independently."
Commits under review: `14773a01` (product), `c811e31c` (coverage/process), from base
`4e88f18`. Candidate: `c811e31c05fc07b3e7e5e60feaa49dbd36b98ad9`.
Verdict: **approved** — real implementation, fail-closed on every acceptance
criterion, no test inflation, no findings. Server-side test *execution* is deferred
to the independent verifier by the review-stage build wrapper (restriction recorded
in `/tmp/stage/review-evidence/`); compilation, the protocol-level behavior, and the
full HTTP-path wiring were independently verified here.

## Round 1 (2026-11-12)

### Probes run (this sandbox, via /tmp/stage/bin/cargo; logs in /tmp/stage/review-evidence/)

- `cargo test -p gyre-spiffe` → **36 passed, 0 failed** — includes all 8 new
  `federation::tests` and `verify::tests::extract_spiffe_id_reads_unverified_id`.
- `cargo check -p gyre-server --tests` → **clean** (9m02s, no errors) — type-checks
  every new server test, confirming the implementer's claim.
- `cargo clippy -p gyre-spiffe --tests` → **0 warnings**.
- Gate scripts all exit 0: arch, abac-route-registry, abac-exempt-handlers,
  unbounded-external-http, in-memory-state-stores, inert-enforcement,
  forged-scope-fields, byte-slice-truncation. Commit attribution verified manually
  (the full-history script times out in this sandbox): product commit `14773a01` is
  in task-262.md `commits:`; `c811e31c` is a `process:`-type specs-only commit the
  gate excludes by design.
- `cargo test -p gyre-server --lib spiffe_federation::` → **blocked by the
  review-stage wrapper** (exit 64, server codegen budget) — infrastructure
  restriction, not a code defect; see "Deferred to verifier" below.

### Verified working

- **AC1 (bundle exchange → SVID verifies, same SPIFFE ID A's verifiers extract).**
  `gyre-spiffe/src/federation.rs` implements the SPIFFE federation model:
  `ExchangedBundles` (registry keyed by trust domain, refreshed on interval),
  `BundleSource` (fetch transport), `verify_federated_svid` (peek the claimed trust
  domain via the new `extract_spiffe_id`, resolve THAT domain's exchanged bundle,
  run full chain validation + session binding). The executed protocol test
  `federated_svid_verifies_only_with_exchanged_bundle` is the two-configuration
  probe: the same SVID is rejected "not federated" without the exchange and verifies
  with it, cross-checked against A-side `verify_svid` for identical SPIFFE ID and
  serial. Server side, `HttpBundleSource` fetches over a 10 s-bounded reqwest client
  and validates the document with `parse_trust_bundle` (garbage never becomes an
  anchor); the service-level and HTTP-level tests exercise the same decision over a
  real local listener (execution deferred, see below).
- **AC2 (never-exchanged domain rejected even with a trusted OIDC JWT).** The layers
  are independent by construction: `anchors_for` fails closed on a non-federated
  domain *before* any crypto, and the HTTP handler gates on
  `federated_caller_claims` (caller must be a federated JWT) before attesting — so
  the OIDC trust that authenticates the caller never vouches for the SVID. Executed:
  `never_exchanged_trust_domain_rejected_alongside_federated_peer` (a genuine
  third-domain SVID is rejected while a different peer's bundle IS exchanged).
  Reverse direction also holds: the local surface `POST /api/v1/agents/{id}/attest`
  requires a locally-spawned SVID record and verifies against the local bundle with
  the local expected trust domain (spiffe.rs:478-518), so a federated SVID cannot be
  attested through the local path either.
- **AC3 (rotation, fail-closed refresh).** Executed:
  `bundle_rotation_follows_refreshed_bundle` — within the interval the rotated SVID
  fails closed (UntrustedSigner) against the old-bundle cache with no re-fetch;
  after expiry the registry re-fetches, the rotated SVID verifies, and the old-CA
  SVID stops verifying (verification FOLLOWS the refreshed bundle);
  `expired_refresh_failure_fails_closed` and `withdrawn_bundle_fails_closed_after_interval`
  — a refresh that cannot complete past the interval is an error, never a silently
  stale bundle.
- **AC4 (no static credentials exchanged).** Federation config is
  `trust_domain=endpoint_url` endpoints only (`GYRE_SPIFFE_FEDERATION_PEERS`); all
  key material enters through the public bundle endpoint fetch
  (`HttpBundleSource`) exactly as the OIDC layer fetches JWKS via discovery.
  `parse_peer_entry` validates the trust domain through the SPIFFE ID parser,
  rejects the local trust domain (local SVIDs use the local surface) and non-http(s)
  schemes, and appends the standard `/.well-known/spiffe/keys.json` path to bare
  origins — the path Gyre itself serves (spiffe.rs `TRUST_BUNDLE_PATH`, mounted in
  lib.rs:1003).
- **No SSRF via presented data:** the SVID's claimed trust domain is only used as an
  exact-match lookup key against the operator-configured peer list; the fetch URL
  always comes from configuration, never from the presented certificate.
- **ABAC wiring is real:** both new routes are in the `RouteResourceMapping`
  registry (resource `federation`, attest → `write`), no exemptions; gate scripts
  confirm. Static trace of the deferred HTTP tests' authorization path: federated
  caller (roles `[Agent]`, subject.type "agent") is allowed by
  `builtin-agent-scoped-access` (700); every higher-priority Deny is keyed out
  (tenant-isolation needs `resource.tenant_id`, workspace-membership-required keys
  on subject.type=="user", agent-repo-scope fails closed on the unresolvable
  `$resource.repo_id` of a non-repo-addressed resource). mem `test_state` seeds the
  builtins.
- **Record lifecycle is fail-closed:** a successful presentation upserts the caller's
  `workload_attestations` record (keyed by the caller's own federated agent id —
  a caller can only affect its own record); a failed re-verification flips an
  existing record to `alive=false` and clears `svid_verified_at`; a first failed
  presentation creates nothing (GET workload 404s). The remote `wl_*` JWT claims are
  recorded as metadata only — never verification inputs.
- **Test quality:** no inflation found. All tests use real CAs, real issued SVIDs,
  real chain validation; the mock/test sources only replace the *transport*, and the
  server tests use a real local HTTP listener for the bundle endpoint, OIDC
  discovery, and JWKS. Assertions are on rejection *reasons* (not-federated vs
  UntrustedSigner vs fetch failure), so each failure mode is distinguished.
- **Coverage honesty:** row 7 (Federation) moved `verified → implemented` with
  task-262 attribution and concrete source evidence; the note explicitly records
  which server tests are deferred rather than claiming they ran. Acceptance
  checkboxes left for review. OIDC-issuer federation (Layer 2) untouched, as
  required. Docs (api-reference, server-config) updated for both env vars and both
  endpoints.

### Deferred to the independent verifier (build wrapper blocks server test codegen)

The verifier MUST execute, and bind publication to their results:
1. `cargo test -p gyre-server --lib spiffe_federation::`
2. `cargo test -p gyre-server --lib api::federation::tests`
   (contains the verification-intent probe end to end: federated agent authenticates
   via real OIDC discovery + JWKS in BOTH configurations; the SVID verifies only
   with the exchanged bundle; record 404 → alive lifecycle; local-caller rejection).

In lieu of execution, compilation was verified (`cargo check -p gyre-server --tests`
clean) and the full runtime path was traced in source: middleware auth order (local
JWT fails on foreign issuer/key → `validate_federated_jwt` with real discovery at
the exact paths the test server serves), SSRF guard passes (same-origin echo),
`ApiError::Forbidden` JSON shape `{"error": msg}` matches the assertions,
`AgentSigningKey::mint` sets `sub` matching the SVID session binding, and
`test_state_inner` wires a real `reqwest::Client`. Full reasoning in
`/tmp/stage/review-evidence/README.md`.

### Minor observations (non-blocking, no action required)

- `HttpBundleSource::fetch` reads the response body without a size bound
  (`response.text()`), mirroring the pre-existing JWKS discovery fetch; the endpoint
  is operator-configured (same trust class as `GYRE_TRUSTED_ISSUERS`) and the fetch
  is latency-bounded (10 s). Not a regression.
- SVID presentation is chain-validation-only (no proof-of-possession of the leaf
  private key) and the session binding is the JWT `sub` string — consistent with the
  local attest surface's established model (task-261); a cross-peer sub-string
  collision would require a colluding federated peer deliberately issuing a
  colliding SVID, and the recorded `trust_domain`/`spiffe_id` still attribute the
  identity honestly. Not part of this task's contract.
- The pre-existing local surface `POST /api/v1/agents/{id}/attest` does not bind the
  caller to the path agent id (`_auth` unused) — unchanged by this diff, out of
  scope here, noted for a future hardening task.

Verdict: **approved**. All four acceptance criteria are implemented with real,
fail-closed production code; the protocol-level behavior is proven by executed
tests; the server-level tests are written, compile, and are recorded as mandatory
verifier probes per the stage's build restrictions.
