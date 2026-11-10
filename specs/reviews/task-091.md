# Review — task-091 (HSI Scoped Inline Chat + Hard Interrupt)

Spec: `specs/system/human-system-interface.md` §4 "Scoped Inline Chat" + "Hard Interrupt".
Candidate: `7cde402c7b77b6b8328671511ea863894ae7b58d` on `pipeline/task-091/47635539ffc24ada860879e7a06b622c-1`, base `078ec12a5745379c254fe7dd00261f02e28d86cb`.
Commits under review: `58e4568219fd5324a2d2a2b30642602e36a47180` (implementation), `a8d036f466482a9a00157a496d10000d0f36d25c` (verify), plus branch docs/process commits `5741c6320`/`7cde402c7`.
Verdict: **approved** — every acceptance criterion verified with independent evidence; no findings.

## Provenance

The candidate diff vs base is docs-only (task file: progress → ready-for-review, Shipped summary, commits frontmatter expanded to full SHAs). Both listed commits are verified ancestors of the base — the implementation (58e456821, frontend-only, 8 files) was merged to main before this branch, and a file-level inspection at the candidate HEAD confirms all task-091 features are intact. This matches the Shipped "Provenance" section; no product-surface changes were needed on this branch, and none were fabricated.

## What was verified (candidate HEAD, fresh probes; evidence in /tmp/stage/review-evidence/)

- **Scoped chat component.** `web/src/lib/InlineChat.svelte` renders `Message to {recipient} ▸` (i18n `inline_chat.message_to`), supports `llm-qa`/`spec-edit` recipient types, and exports `focus()` for the Message hard-interrupt button.
- **Agent detail panel.** `DetailPanel.svelte` chat tab renders `InlineChat` below the delivered-message thread; `sendAgentSteeringMessage` posts `{kind:'user_message', tier:'directed', payload:{content}}`. Hard Interrupt row (Pause/Stop/Message) renders while status is active/running/spawning: **Pause** sends `status_update` with `{status:'pause_requested', summary:'Human requested pause'}` and toggles to "Pause requested"; **Stop** runs `confirm('Stop agent {name}? Work will be preserved in the worktree.')` then `api.adminKillAgent` (`POST /admin/agents/:id/kill`, registered at api/mod.rs:518); **Message** switches to the chat tab and focuses the input via the exported `focus()`.
- **MR detail panel.** `chat` tab resolves the author agent once in the MR loader; active author → `InlineChat` targeting the author; completed/dead author → "Agent completed" `EmptyState` with an Ask Why button; no author → explicit no-conversation state.
- **Workspace orchestrator.** `WorkspaceHome.svelte` identifies the orchestrator by name convention (agents carry no role field — disclosed in the task file), prefers an active instance, hides the widget when none exists, and sends Directed priority messages.
- **Briefing Q&A.** `web/src/components/Briefing.svelte` embeds `InlineChat` (recipientType llm-qa) below the briefing, wired to `api.briefingAsk` → `POST /workspaces/:id/briefing/ask` — LLM Q&A, not Directed-tier, per spec.
- **Backend chain is real (no server changes needed, confirmed).** `messages.rs:96-98` — Custom kind + `tier:"directed"` opts into `MessageTier::Directed`; user-origin senders are membership-checked (Forbidden otherwise); Directed messages are signed and persisted; `poll_messages` (`GET /agents/:id/messages`, api/mod.rs:272) enforces agent-caller identity and `list_after` (sqlite/message.rs:193) returns all agent-destined stored messages regardless of tier — so human Directed messages are drained by `message.poll`.
- **Tests pass.** Focused task-091 suites (`npm ci` + `npx vitest run`): 3 files, 63 passed | 41 skipped, exit 0 — exactly the Shipped claim. Full web suite at this HEAD: 99 files, 2010 passed, exit 0 (the acceptance criterion's caveat about pre-existing canvas/ResizeObserver failures does not reproduce here — stronger than claimed). Backend: `cargo test -p gyre-server --lib api::messages` → 14 passed.
- **Tests bite (mutation probes, both restored; tree verified clean).** (1) Mutating the Pause payload `pause_requested` → `resume_requested` fails the Pause test (1 failed | 8 passed). (2) Dropping `tier:'directed'` from the steering send fails the Directed-send test (1 failed | 8 passed). The Stop tests include a negative case (confirm dismissed → no kill call).
- **Mechanical checks.** `scripts/check-task-commit-attribution.sh` → OK. Coverage rows 26/27 in `specs/coverage/system/human-system-interface.md` remain `task-assigned` — correct at review stage; flipping to implemented/verified belongs to verification/audit.

## Minor notes (non-blocking, recorded for completeness)

- The Shipped summary says "Briefing.svelte"; the actual path is `web/src/components/Briefing.svelte`. Cosmetic doc imprecision only.
- Orchestrator recipient targeting uses a name convention because the agent data model has no role field; this is disclosed in the task file and the widget hides rather than mis-targeting when no match exists. If a role field is ever added, the derived selector should switch to it.
- Historical note: `a8d036f4` (a pre-base verify commit) bundles a large unrelated call-graph refactor under a "mark task-091 complete" message. Pre-existing merged history, not introduced by this candidate; no action required here.

Test-inflation check: none found. The component tests assert exact wire-contract calls (kind/tier/payload/endpoint) and include negative cases; mutation probes confirm they fail when the production behavior is broken. The backend tier parse, Directed persistence, poll drain, and queue-depth enforcement are covered by real router-driving Rust tests.
