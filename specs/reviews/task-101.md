# Review — task-101 (Platform Model §8 — Protocol Injection at Agent Spawn)

Spec: `specs/system/platform-model.md` §8 "Protocol Injection" (coverage row
52; the task frontmatter's `coverage_sections` lists row 50 "Dev Mode (No
OIDC)" — a known PM-owned metadata mislabel, disclosed in the task notes and
the coverage file).
Candidate: `c54469fd0305f754179c8823ee5c96ade1d29551` (base `9f446d0e`;
product commit `78d7774d`, correctly attributed in the task's `commits:`
frontmatter; `c54469fd` is docs/specs delivery recording).
Verdict: **complete / approved**.

## Probes run (evidence: /tmp/stage/review-evidence/probes.md)

- `cargo check -p gyre-server --tests` → exit 0 (8m54s, includes web asset
  build). All new/extended tests compile. Test *execution* of gyre-server is
  blocked by the stage wrapper (codegen exceeds the 4 GiB review budget) —
  required verifier probes recorded below and in the evidence file.
- All mechanical invariant scripts pass: check-arch, check-abac-route-registry,
  check-mcp-write-tools, check-abac-exempt-handlers class (byte-slice,
  relative-path, inert-enforcement, scope-literal, fabricated-scope,
  lossy-secret, forwarded-header, in-memory-state, unbounded-http,
  fail-open-ref, mem-port-contracts, dead-message-kinds) — 15/15 exit 0.
- `cargo fmt -p gyre-server -- --check` → clean.
- Source-level verification of every shipped claim (details in evidence file):
  single-source protocol norms (render_protocol_prompt delegates to
  api::prompts::protocol_prompt; GYRE_PROTOCOL_PROMPT / GYRE_PROMPT_PROTOCOL /
  system://protocol byte-identical by construction, pinned by tests), tool
  catalog names all match registered MCP tools, budget snapshot reads the
  same budget_configs/budget_usages stores + key format as enforcement on
  all four context paths, trust level read from the workspace row on both
  constraints paths, persona resolution fail-closed to Approved
  (resolve_persona_in_chain + orchestrator relaunch), persona attestation
  vars pinned at spawn (workers and orchestrators) and MCP
  `system://persona` metadata.persona looked up from agent.persona_id with
  no fabricated identity.

## Findings

None blocking.

- **Tests kill real bugs**: the budget-snapshot assertion runs against seeded
  config+usage (`40000 used today of 100000 (60000 remaining)`); persona
  attestation assertions run against a persona bumped to a non-trivial
  version with `refresh_content_hash()` before spawn (looked-up values, not
  constants); the MCP attestation test performs a real `prompts/get` round
  trip through `build_router` as the spawned agent; the single-source test
  fails if the two protocol render paths ever diverge again.
- **Dev mode**: test_state uses `auth_token: "test-token"`,
  `jwt_config: None` — exactly the dev-mode server configuration — and the
  spawn tests authenticate through the static-token path; prompt assembly
  reads task/repo/workspace/budget state, not caller OIDC claims, so
  dev-mode agents receive the identical injection. Parity documented in
  docs/server-config.md. (Structural parity rather than a live
  `bootstrap --dev` run; row 50 was independently verified by task-099.)
- Non-blocking observations recorded in the evidence file: MCP
  `metadata.persona` reflects the live persona row rather than a
  spawn-pinned copy (a persona edit resets approval to Pending, so
  mis-attribution of approved content cannot occur silently);
  `budget_snapshot_section` maps store errors to "no limits configured"
  exactly like the pre-existing `constraints_prompt` style (informational
  prompt; enforcement unaffected); the trust-level test's `Supervised`
  assertion alone would not distinguish hardcoding (lookup verified by code
  reading).

## Required verifier probes

```
cargo test -p gyre-server --lib prompt_assembly
cargo test -p gyre-server --lib api::prompts
cargo test -p gyre-server --lib spawn_injects_all_four_system_prompts_with_persona
cargo test -p gyre-server --lib mcp_persona_prompt_carries_bound_persona_attestation
cargo test -p gyre-server --lib mcp_prompts_get_protocol_delivers_ralph_loop_rules
cargo test -p gyre-server --lib orchestrator_launch_injects_system_prompts
cargo test -p gyre-server --lib spawn_persona_slug
cargo test --all        # task acceptance criterion
```
