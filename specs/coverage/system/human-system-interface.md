# Coverage: Human-System Interface

**Spec:** [`system/human-system-interface.md`](../../system/human-system-interface.md)
**Last audited:** 2026-04-13 (full audit — §4-12 reclassified from not-started)
**Coverage:** 19/36 (19 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Novel Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Navigation Model | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | Stable Sidebar, Adaptive Content | 3 | task-assigned | task-082 | No desktop sidebar exists (App.svelte:1032 "no sidebar"). Mobile drawer only with 5 items. Spec requires permanent 6-item sidebar. |
| 4 | Scope Indicator (Breadcrumb) | 3 | implemented | - | Partial — breadcrumb exists in repo/settings modes (App.svelte:1056-1139) but: (1) no tenant segment shown (only workspace > repo, spec requires tenant > workspace > repo), (2) not always present (missing from workspace home, profile, cross-workspace home), (3) repo segment not always clickable (static span when no entity detail open). Consider splitting into separate task. |
| 5 | What Each Nav Item Shows at Each Scope | 3 | task-assigned | task-083 | No sidebar nav items exist. Content uses modes/tabs/sections instead of spec's 6-item × 3-scope matrix. |
| 6 | Workspace Attribution on Items | 3 | implemented | - | Partial — workspace name badges exist (CrossWorkspaceHome.svelte:938) but NOT clickable (spec requires link to workspace view; decision badges ARE clickable at :675). Status bar fully implemented (App.svelte:1565-1619: trust, budget, WS status, presence). Presence backend complete (ws.rs:182-295: session_id, 5-cap eviction, PresenceEvicted). Frontend presence sending NOT implemented: no client-side UserPresence messages, no session_id generation, no debounce, no view-change triggers. beforeunload handler exists but doesn't send disconnect message. Consider splitting UI badges vs presence mechanics. |
| 7 | Deep Links | 3 | implemented | - | Partial — Backend solid: user_workspace_state table (migration 000027), last_seen_at middleware (middleware.rs:104-175, 60s debounce, async upsert), briefing time dropdown (Briefing.svelte:25-56, all 5 options). URL routing partial: routes exist but patterns differ from spec (/workspaces/:slug/r/:repo/... vs spec's /repos/:id/...), missing /inbox and /workspaces/:id/inbox routes, no query param support for explorer filters/lens or specs path. Entrypoint diverges: goes to workspace home, not explorer→inbox flow per spec. |
| 8 | Keyboard Navigation | 3 | implemented | - | Partial — 4/10 shortcuts match spec: Cmd+K (App.svelte:380), Esc (:388), / (:400), ? (:408). Cmd+1-6 for global nav (Inbox/Briefing/Explorer/Specs/Meta-specs/Admin) NOT implemented. Instead uses g-key sequences (:417-470) for repo tab switching (different purpose). SearchBar.svelte:16-24 defines nav items with numeric icons 1-6 but no keyboard binding. |
| 9 | 2. Trust Gradient | 2 | task-assigned | task-077 | |
| 10 | The Problem | 3 | task-assigned | task-077 | |
| 11 | Trust Levels | 3 | task-assigned | task-077 | |
| 12 | What Each Level Controls | 3 | task-assigned | task-077 | |
| 13 | Mechanical Implementation | 3 | task-assigned | task-077 | |
| 14 | 2a. Policies ↔ Trust Level Integration | 3 | task-assigned | task-084 | Depends on task-077 |
| 15 | Trust Suggestions | 3 | task-assigned | task-085 | Depends on task-077; notification infrastructure exists |
| 16 | 3. The Explorer: Progressive Architecture Discovery | 2 | n/a | - | Section heading only — no implementable requirement. |
| 17 | The Zero-Knowledge Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 18 | Design Principle: The Built Software Is Always Alive | 3 | task-assigned | task-086 | Two layers of liveness: SDLC activity + test-time traces |
| 19 | Test-Time Trace Capture | 3 | task-assigned | task-087 | OTLP receiver, TraceCapture gate, span-to-graph linkage |
| 20 | Default Views (Automatic, No LLM) | 3 | task-assigned | task-088 | Boundary, Spec Realization, Change, System Trace views |
| 21 | Saved Views (Curated, Shared) | 3 | implemented | - | Full CRUD at /workspaces/:id/explorer-views (explorer_views.rs), ViewSpec+ViewQuery grammar (view_spec.rs, view_query.rs), ownership checks (creator or Admin), ABAC resource_type "explorer_view", UUID view_id, SQLite with workspace+tenant scoping, lazy-seeded built-in views. Partial — built-in view names diverge from spec (6 impl vs 4 spec: has Architecture Overview/Test Coverage Gaps/Hot Paths/Blast Radius/Spec Coverage/Ungoverned Risk instead of API Surface/Domain Model/Security Boundary/Test Coverage). |
| 22 | Generated Views (LLM-Powered, On-Demand) | 3 | task-assigned | task-089 | LLM query translation + flow layout |
| 23 | Three Lenses | 3 | task-assigned | task-090 | Structural (default), Evaluative, Observable (future) |
| 24 | 4. Agent Communication | 2 | n/a | - | Section heading only — no implementable requirement. |
| 25 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 26 | Scoped Inline Chat | 3 | task-assigned | task-091 | Chat scoped to agent/orchestrator/briefing/MR contexts |
| 27 | Hard Interrupt | 3 | task-assigned | task-091 | Pause/Stop/Message buttons on agent detail panel |
| 28 | Agent Completion Summaries | 3 | implemented | - | AgentCompletionSummary type (gyre-common/src/completion.rs). AgentCompleted MessageKind (Event tier). agent.complete MCP tool accepts summary field. Stored in MR attestation bundle. Notifications for uncertainties. |
| 29 | Interrogation Agents | 3 | implemented | - | spawn.rs: agent_type="interrogation", conversation_sha loading, 30-min JWT TTL. ABAC policies (migration 000026). Stale agent cleanup deletes policies. ConversationRepository::get for context. |
| 30 | 5. Conversation-to-Code Provenance | 2 | n/a | - | Section heading only — no implementable requirement. |
| 31 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 32 | Design | 3 | implemented | - | ConversationRepository port (gyre-ports/src/conversation.rs). SQLite adapter. conversations + turn_commit_links tables (migration 000021). conversation.upload MCP tool. X-Gyre-Conversation-Turn header. GET /api/v1/conversations/:sha endpoint. |
| 33 | 6. Cross-Workspace Spec Dependencies | 2 | n/a | - | Section heading only — no implementable requirement. |
| 34 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 35 | Design | 3 | implemented | - | Cross-workspace spec links with @workspace_slug/repo_name/spec_path format. Resolution via spec_registry.rs and domain_events.rs. Slug-based lookup tenant-scoped. |
| 36 | What the System Does With Cross-Workspace Links | 3 | implemented | - | SpecChanged Event-tier messages emitted on cross-workspace link changes. Inbox notifications for dependent workspace members. Briefing integration. Explorer dashed-line visualization. Orchestrator receives events. |
| 37 | Cross-Workspace Change Notification | 3 | implemented | - | Spec lifecycle push handler queries spec_links for inbound cross-workspace links. Creates notifications for dependent workspace members. SpecChanged events to workspace orchestrators. |
| 38 | 7. Multi-Human Collaboration | 2 | n/a | - | Section heading only — no implementable requirement. |
| 39 | Presence Awareness | 3 | implemented | - | UserPresence WsMessage variant (gyre-common/src/protocol.rs). In-memory presence map in ws.rs. GET /api/v1/workspaces/:id/presence endpoint. session_id per tab, 5-cap eviction, PresenceEvicted, 60s timeout. |
| 40 | Conflict Prevention | 3 | task-assigned | task-092 | Concurrent spec edit warning, optimistic concurrency |
| 41 | Shared Views | 3 | implemented | - | Explorer views shared by default — all workspace members can read. PUT/DELETE restricted to creator or Admin (per-handler auth). CRUD at /api/v1/workspaces/:id/explorer-views. |
| 42 | 8. Inbox Detail | 2 | n/a | - | Section heading only — no implementable requirement. |
| 43 | Action Types (Priority Order) | 3 | implemented | - | All 10 NotificationType variants defined (gyre-common/src/notification.rs). Priority ordering 1-10. Creation paths for each type (synchronous/async). Inline actions in Inbox UI. Filtering by priority range. |
| 44 | 9. Briefing Detail | 2 | n/a | - | Section heading only — no implementable requirement. |
| 45 | Structure | 3 | implemented | - | GET /api/v1/workspaces/:id/briefing with sections: completed, in_progress, cross_workspace, exceptions, metrics. LLM-synthesized summary. repo_id filter. last_seen_at default. MCP resource briefing://. |
| 46 | Data Sources | 3 | implemented | - | Completed: spec registry + task rollup + completion summaries. In Progress: task status + agent activity + Notification table. Cross-Workspace: spec link watcher. Exceptions: gate results + assertion failures. Metrics: budget usage. |
| 47 | Briefing Q&A | 3 | implemented | - | POST /api/v1/workspaces/:id/briefing/ask registered. Request: {question, history?}. Response: {answer, sources}. ABAC: workspace resource, generate action. History capped at 20. LLM read-only access. |
| 48 | 10. Observable Lens (Future-Proofing) | 2 | n/a | - | Section heading only — no implementable requirement. |
| 49 | Design Constraints | 3 | n/a | - | Architectural constraints for future Observable lens — no implementable requirement. Design must-not-preclude constraints; compliance verified by extension points existing in GraphNode metadata, lens system, briefing sections, message bus external event sources. |
| 50 | 11. CLI/MCP Parity Constraint | 2 | implemented | - | CLI: gyre briefing, gyre inbox, gyre explore, gyre trace, gyre spec (read/write/diff/graph), gyre divergence. MCP: briefing://, notifications://, graph.concept tool, trace://, spec.assist tool. All UI data surfaces available via REST. |
| 51 | 12. User Profile | 2 | n/a | - | Section heading only — no implementable requirement. |
| 52 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 53 | What the Profile Is | 3 | implemented | - | /profile route. user_profile adapter (sqlite/user_profile.rs). Migration 000036. PUT /api/v1/users/me. API tokens CRUD. user_notification_preferences table. Workspace memberships. GET /api/v1/users/me/judgments for judgment ledger. |
| 54 | What the Profile Is NOT | 3 | n/a | - | Anti-requirements/guidelines — no implementable requirement. |
| 55 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
