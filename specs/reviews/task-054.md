# Review: TASK-054 — Conversations Endpoint Integration Test

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** PASS — 0 findings

## Scope

TASK-054 adds integration tests for the existing `GET /api/v1/conversations/:sha` endpoint (HSI §5). The handler was already complete; this task adds test coverage only.

## Verification Summary

### Handler Analysis (`conversations.rs`)
- Route registered at `lib.rs:661`, outside ABAC middleware (per-handler auth) ✓
- `AuthenticatedAgent` extractor enforces authentication ✓
- Per-handler ABAC check via `check_workspace_abac_for_read` when JWT claims present ✓
- Tenant isolation via `tenant_id` on both `get_metadata` and `get` calls ✓

### Test Analysis (`conversation_integration.rs`)

1. **Success path** (`get_conversation_returns_decompressed_blob`): Seeds a zstd-compressed blob via `ConversationRepository::store`, GETs the endpoint, asserts 200 + `Content-Type: application/octet-stream` + `X-Gyre-Conversation-Sha` header + body matches original uncompressed JSON. Exercises the full round-trip through handler → `get_metadata` → `get` → decompression. ✓

2. **Not-found path** (`get_conversation_nonexistent_returns_404`): Requests a nonexistent SHA, asserts 404. ✓

3. **Auth enforcement** (`get_conversation_unauthenticated_returns_401`): Seeds a conversation (so 401 isn't masking 404), sends request without Authorization header, asserts 401. Tests the `AuthenticatedAgent` extractor rejection at the system boundary. ✓

### Checks Performed
- No dead code, no unused types
- Test assertions are substantive (status, headers, body content)
- `Ctx` pattern consistent with other integration tests (`api_integration.rs`)
- `seed_conversation` correctly uses `tenant_id = "default"` matching global token auth
- `MemConversationRepository` correctly decompresses on `get` — round-trip is genuine
- All 3 tests pass: `cargo test --test conversation_integration` → 3/3 ok

## Findings

(none)

---

# Review Round R2 — Independent (candidate c7e788cb)

**Reviewer:** Independent review agent
**Base:** 1901941b · **Candidate:** c7e788cb
**Verdict:** APPROVED — 0 blocking findings

## Scope inspected

Diff base→candidate touches only `specs/tasks/task-054.md` (+33/−3): the Shipped
record and frontmatter reconciliation. No production code changed. The task's
deliverable — `crates/gyre-server/tests/conversation_integration.rs` — is present
in **both** base and candidate trees: it entered this repository's shared lineage
via `d7940e857` (ancestor of the assignment base) and was adapted to the fallible
`seed_builtin_policies` API by checkpoint `ed36c1b68`. The task premise
("Integration test: MISSING") was stale for this lineage; the candidate documents
that accurately instead of duplicating the suite, which is the correct action
under the goal's no-duplicate-tests rule.

## Independent verification performed

1. **Test ↔ production cross-check (source level).** Each assertion maps to real
   handler behavior in `crates/gyre-server/src/api/conversations.rs`:
   - 200 + `Content-Type: application/octet-stream` + `X-Gyre-Conversation-Sha`
     echo + body equality — matches the handler's success arm verbatim.
   - 404 — matches the `get_metadata` → `Ok(None)` arm.
   - 401 — `AuthenticatedAgent` extractor returns
     `(StatusCode::UNAUTHORIZED, "Missing Bearer token")` at auth.rs:913 when no
     bearer token is present; the test seeds a conversation first so 401 cannot
     mask 404.
2. **Round-trip genuineness.** `MemConversationRepository::store` keys by SHA-256
   of the stored (compressed) bytes; `get` enforces tenant match then zstd-
   decompresses (mem.rs:3568+). The test compresses before `store` and asserts
   body equality with the **uncompressed** original — the assertion fails if the
   handler returns stored bytes or decompression breaks. Not self-confirming.
3. **Tenant wiring.** Global token auth resolves `tenant_id =
   system_principal_tenant()` = `"default"` (auth.rs:518), matching the test's
   seed tenant — the metadata lookup genuinely resolves.
4. **Harness validity.** `Ctx::new` (TcpListener :0 → `build_state` →
   `seed_builtin_policies(&state).await.expect(..)` → `axum::serve` spawn) is
   byte-for-byte the pattern of the working `api_integration.rs` suite; the
   fallible `seed_builtin_policies` signature matches abac_middleware.rs:879.
   No `[[test]]`/`autotests` gating in Cargo.toml — the target is discovered and
   runs under `cargo test --all`. `zstd`/`reqwest` deps present.
5. **Type check.** `cargo check -p gyre-server --test conversation_integration`
   → finished clean, 0 errors (log: /tmp/stage/review-evidence/task-054-cargo-check.log).
6. **Route registration.** `/api/v1/conversations/:sha` → `get_conversation`
   at lib.rs:847-848; `pub mod conversations` at api/mod.rs:17; port wired via
   `store!` at lib.rs:1383-1385.

## Sandbox limitation (not a code defect)

`cargo test -p gyre-server --test conversation_integration` is rejected by the
build wrapper in this sandbox (gyre-server codegen + model exceeds the 4 GiB
budget). **Verifier must run:**
- `cargo test -p gyre-server --test conversation_integration` (expect 3 passed)
- `cargo test --all` (acceptance criterion)
TCP listener probes are supported per capabilities.json, so the suite is
runnable in the verifier environment.

## Non-blocking observations

- Frontmatter `commits: []` in the candidate: the contract's `6e752e6d` does not
  resolve in this repository's history, and this branch's commits are docs-only
  (not product-surface), so `check-task-commit-attribution.sh` is unaffected.
  The body still records the contract SHA under "## Git Commits".
- The ABAC criterion is covered by the unauthenticated-401 test, exactly as the
  task plan defines it ("ABAC: unauthenticated request returns 401/403"). A
  JWT-claims cross-tenant 403 test would additionally exercise the per-handler
  `check_workspace_abac_for_read` denial arm; noted as future hardening, not a
  contract gap.
