# Review — task-091 (HSI Scoped Inline Chat + Hard Interrupt)

Spec: `specs/system/human-system-interface.md` §4 "Scoped Inline Chat" + "Hard Interrupt".
Candidate under review: `015340bafd069b1ac32f8f95214d6effc2de48d7` (branch `pipeline/task-091/bfe34dfa3e094a5e93d3511469c38388-1`), base `8729385d`.
Commits under review: `3e22e490` (product: role-based orchestrator chat recipient), `58e45682`/`a8d036f4` (original HSI §4 implementation, ancestors of the base, re-verified at the candidate), `837de7cb`/`5741c6320`/`015340baf` (docs/process).
Verdict: **needs-revision** — one material finding (F1, stale shipped bundle); everything else verified clean. Evidence: `/tmp/stage/review-evidence/task-091/`.

## Round history

Prior round flagged requirements gap d382faa3 (orchestrator chat recipient selected by name convention). The candidate is the repair round.

## Probes run

- `cd web && npm ci` then `npx vitest run` on the five task suites (`WorkspaceHomeSections`, `DetailPanelChat`, `InlineChat`, `DetailPanel`, `Briefing`) → **129 passed** / 41 pre-existing old-layout skips (evidence 01). Adjacent `api.test.js` + `WorkspaceHome.test.js` → 24/24.
- Mutation probe: reverted `WorkspaceHome.svelte` to the pre-repair name-based filter → both new regression tests **fail** (2 failed), restored exactly (sha256 verified) (evidence 03). The tests are real guards, not self-confirming.
- `cargo check -p gyre-server` → clean (evidence 02). Test-profile compilation is blocked by the sandbox build wrapper (4 GiB); required verifier probes recorded in evidence 05.
- `bash scripts/check-task-commit-attribution.sh` → OK against the committed candidate state (evidence 04).
- Dist bundle inspection (evidence 06) → F1 below.

## Verified working (no findings)

- **Role-based recipient selection is real and guarded.** `AgentResponse.orchestrator_type` is a required field populated unconditionally by `From<Agent>` (compile-enforced; Display is snake_case, matching the serde-renamed domain enum); `OrchestratorAgentResponse` carries it exactly once via the flattened `AgentResponse`, guarded by the existing generic duplicate-key wire regression (`repo_orchestrator_response_serializes_no_key_twice`, asserts on the serialized string for ANY key). CLI `SpawnOrchestratorAgent` still parses (flattened shape unchanged). Old persisted agent rows deserialize as `Worker` (`#[serde(default)]`). `WorkspaceHome.svelte` filters on `orchestrator_type === 'workspace_orchestrator'`, preferring active; no name matching remains in `web/src`.
- **New server test is meaningful.** `agent_response_exposes_orchestrator_type` (agents.rs) drives the real router (`GET /workspaces/:id/agents` and `GET /agents/:id`) with a custom-named workspace orchestrator, a name-matching repo orchestrator, and a name-matching worker, asserting all three tiers on the wire. Helpers and constructor signatures hand-verified (cfg(test) not compilable in this sandbox — verifier must run `cargo test -p gyre-server --lib api::agents`).
- **HSI §4 base contract re-verified at the candidate.** InlineChat renders `Message to {recipient} ▸` and exports `focus()`; DetailPanel chat sends Custom `user_message` with `tier: "directed"`; Pause sends `status_update` with `{status: "pause_requested", summary: ...}`; Stop confirms then `POST /admin/agents/:id/kill`; Message focuses the chat; MR chat targets the author agent with an "Agent completed / Ask Why" fallback; Briefing Q&A uses `POST /workspaces/:id/briefing/ask`. Delivery is real: `POST /workspaces/:id/messages` maps Custom+directed to Directed tier (signed, persisted), `origin_from_auth` yields `MessageOrigin::User` for user tokens, and `message.poll` (`GET /agents/:id/messages`) drains persisted messages via `list_after`.
- **Attribution:** committed candidate frontmatter records all three product SHAs; docs/process commits are out of the check's scope by design.

## Findings

- [-] **F1 (major): the committed `web/dist/` bundle still ships the pre-repair name-based recipient selection — gap d382faa3 survives in the Rust-only build path.** `web/dist` was last regenerated at `d65920511` (task-123), which predates the repair commit `3e22e490`; the candidate does not touch `web/dist`. The shipped bundle `web/dist/assets/index-jpFOlnlZ.js` contains verbatim the expression the repair deleted — `wsAgents.filter((a) => (a.name ?? "").toLowerCase().includes("orchestrator"))` — and zero occurrences of `orchestrator_type`. `crates/gyre-server/src/spa.rs` RustEmbeds `web/dist` and `build.rs` skips the npm build under `SKIP_WEB_BUILD=1` (the documented quick-start build), so that deployment path serves a workspace overview where a repo orchestrator or a name-matching worker can be the priority-chat recipient and a custom-named workspace orchestrator is never found — exactly what this round claims to repair. Same finding class as task-208 review F1. Repair: `cd web && npm run build`, commit the regenerated dist (`build(web): regenerate dist with role-based orchestrator chat recipient (task-091)`), append the SHA to task-091.md `commits:` frontmatter, and verify the bundle greps clean (`includes("orchestrator")` absent, `orchestrator_type` present).

Minor (non-blocking, recorded for completeness):

- The unstaged working-tree modification of `specs/tasks/task-091.md` (present before review began, matching the assigned task body: `progress: complete`, `commits: [58e45682]`) reverts the committed frontmatter's three-SHA list; `check-task-commit-attribution.sh` fails against the working-tree copy but passes against the committed candidate. Controller stage artifact, not a code defect — flagged so the verifier is not misled.

## Verifier-required probes

    cargo test -p gyre-server --lib api::agents
    cargo test -p gyre-server --lib api::orchestrator
    cargo test -p gyre-server --lib api::spawn
    cargo test -p gyre-server --lib mcp::
    cd web && npm ci && npx vitest run src/__tests__/WorkspaceHomeSections.test.js src/__tests__/DetailPanelChat.test.js src/__tests__/InlineChat.test.js src/__tests__/DetailPanel.test.js src/__tests__/Briefing.test.js

Test-inflation check: none. Both new frontend regression tests fail under mutation of the production filter; the new server test drives the real router and asserts tier values for adversarial name/role combinations.
