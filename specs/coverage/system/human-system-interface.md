# Coverage: Human-System Interface

**Spec:** [`system/human-system-interface.md`](../../system/human-system-interface.md)
**Last audited:** 2026-04-13 (§1 audited; §2+ classified this cycle)
**Coverage:** 4/53

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Novel Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Navigation Model | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | Stable Sidebar, Adaptive Content | 3 | task-assigned | task-082 | No desktop sidebar exists (App.svelte:1032 "no sidebar"). Mobile drawer only with 5 items. Spec requires permanent 6-item sidebar. |
| 4 | Scope Indicator (Breadcrumb) | 3 | implemented | - | Persistent clickable breadcrumb in App.svelte topbar. Minor deviation: tenant shown as "All Workspaces" vs tenant name. |
| 5 | What Each Nav Item Shows at Each Scope | 3 | task-assigned | task-083 | No sidebar nav items exist. Content uses modes/tabs/sections instead of spec's 6-item × 3-scope matrix. |
| 6 | Workspace Attribution on Items | 3 | implemented | - | Partial — workspace badges at tenant scope, status bar (trust, budget, WS, presence) implemented. Detailed presence mechanics (debounce, graceful disconnect, session cap, PresenceEvicted) not verified. Consider splitting. |
| 7 | Deep Links | 3 | implemented | - | URL routing, user_workspace_state table, last_seen_at Axum middleware (middleware.rs:104), briefing time dropdown all exist. URL patterns differ from spec examples. |
| 8 | Keyboard Navigation | 3 | implemented | - | Cmd+K, /, ?, Esc all match spec. Uses g-key sequences (g h, g s, g 1-8) instead of spec's Cmd+1-6. |
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
| 21 | Saved Views (Curated, Shared) | 3 | not-started | - | View spec grammar, ownership, built-in views. Overlap with explorer-implementation.md §Saved Views (verified). |
| 22 | Generated Views (LLM-Powered, On-Demand) | 3 | task-assigned | task-089 | LLM query translation + flow layout |
| 23 | Three Lenses | 3 | task-assigned | task-090 | Structural (default), Evaluative, Observable (future) |
| 24 | 4. Agent Communication | 2 | n/a | - | Section heading only — no implementable requirement. |
| 25 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 26 | Scoped Inline Chat | 3 | task-assigned | task-091 | Chat scoped to agent/orchestrator/briefing/MR contexts |
| 27 | Hard Interrupt | 3 | task-assigned | task-091 | Pause/Stop/Message buttons on agent detail panel |
| 28 | Agent Completion Summaries | 3 | not-started | - | Extends agent.complete with summary; AgentCompleted MessageKind |
| 29 | Interrogation Agents | 3 | not-started | - | "Ask why" spawns restricted interrogation agent with ABAC |
| 30 | 5. Conversation-to-Code Provenance | 2 | n/a | - | Section heading only — no implementable requirement. |
| 31 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 32 | Design | 3 | not-started | - | ConversationRepository, TurnCommitLink, conversation.upload MCP tool |
| 33 | 6. Cross-Workspace Spec Dependencies | 2 | n/a | - | Section heading only — no implementable requirement. |
| 34 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 35 | Design | 3 | not-started | - | @workspace_slug/repo_name/spec_path format, resolution |
| 36 | What the System Does With Cross-Workspace Links | 3 | not-started | - | Notifications, briefing integration, explorer visualization, orchestrator awareness |
| 37 | Cross-Workspace Change Notification | 3 | not-started | - | Spec link watcher, SpecChanged events, dependent workspace notifications |
| 38 | 7. Multi-Human Collaboration | 2 | n/a | - | Section heading only — no implementable requirement. |
| 39 | Presence Awareness | 3 | not-started | - | UserPresence WsMessage, presence map, GET /workspaces/:id/presence |
| 40 | Conflict Prevention | 3 | not-started | - | Concurrent spec edit warning, optimistic concurrency |
| 41 | Shared Views | 3 | not-started | - | Explorer views shared across workspace members |
| 42 | 8. Inbox Detail | 2 | n/a | - | Section heading only — no implementable requirement. |
| 43 | Action Types (Priority Order) | 3 | not-started | - | 10 notification action types with inline actions; notification infrastructure exists |
| 44 | 9. Briefing Detail | 2 | n/a | - | Section heading only — no implementable requirement. |
| 45 | Structure | 3 | not-started | - | Briefing response schema: completed, in_progress, cross_workspace, exceptions, metrics |
| 46 | Data Sources | 3 | not-started | - | Data source mapping per briefing section |
| 47 | Briefing Q&A | 3 | not-started | - | POST /workspaces/:id/briefing/ask endpoint exists; need spec compliance |
| 48 | 10. Observable Lens (Future-Proofing) | 2 | n/a | - | Section heading only — no implementable requirement. |
| 49 | Design Constraints | 3 | not-started | - | Architectural constraints for future Observable lens |
| 50 | 11. CLI/MCP Parity Constraint | 2 | not-started | - | CLI commands: gyre briefing, gyre inbox, gyre explore, gyre trace, gyre spec assist |
| 51 | 12. User Profile | 2 | n/a | - | Section heading only — no implementable requirement. |
| 52 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 53 | What the Profile Is | 3 | not-started | - | /profile page: Identity, Notification Prefs, Memberships, Judgment Ledger |
| 54 | What the Profile Is NOT | 3 | n/a | - | Anti-requirements/guidelines — no implementable requirement. |
| 55 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
