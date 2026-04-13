# Coverage: Human-System Interface

**Spec:** [`system/human-system-interface.md`](../../system/human-system-interface.md)
**Last audited:** - (§1 The Novel Problem + §1 Navigation Model audited 2026-04-13)
**Coverage:** 4/53

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Novel Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Navigation Model | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | Stable Sidebar, Adaptive Content | 3 | not-started | - | No desktop sidebar exists (App.svelte:1032 "no sidebar"). Mobile drawer only with 5 items. Spec requires permanent 6-item sidebar. |
| 4 | Scope Indicator (Breadcrumb) | 3 | implemented | - | Persistent clickable breadcrumb in App.svelte topbar. Minor deviation: tenant shown as "All Workspaces" vs tenant name. |
| 5 | What Each Nav Item Shows at Each Scope | 3 | not-started | - | No sidebar nav items exist. Content uses modes/tabs/sections instead of spec's 6-item × 3-scope matrix. |
| 6 | Workspace Attribution on Items | 3 | implemented | - | Partial — workspace badges at tenant scope, status bar (trust, budget, WS, presence) implemented. Detailed presence mechanics (debounce, graceful disconnect, session cap, PresenceEvicted) not verified. Consider splitting. |
| 7 | Deep Links | 3 | implemented | - | URL routing, user_workspace_state table, last_seen_at Axum middleware (middleware.rs:104), briefing time dropdown all exist. URL patterns differ from spec examples. |
| 8 | Keyboard Navigation | 3 | implemented | - | Cmd+K, /, ?, Esc all match spec. Uses g-key sequences (g h, g s, g 1-8) instead of spec's Cmd+1-6. |
| 9 | 2. Trust Gradient | 2 | task-assigned | task-077 | |
| 10 | The Problem | 3 | task-assigned | task-077 | |
| 11 | Trust Levels | 3 | task-assigned | task-077 | |
| 12 | What Each Level Controls | 3 | task-assigned | task-077 | |
| 13 | Mechanical Implementation | 3 | task-assigned | task-077 | |
| 14 | 2a. Policies ↔ Trust Level Integration | 3 | not-started | - | |
| 15 | Trust Suggestions | 3 | not-started | - | |
| 16 | 3. The Explorer: Progressive Architecture Discovery | 2 | not-started | - | |
| 17 | The Zero-Knowledge Problem | 3 | not-started | - | |
| 18 | Design Principle: The Built Software Is Always Alive | 3 | not-started | - | |
| 19 | Test-Time Trace Capture | 3 | not-started | - | |
| 20 | Default Views (Automatic, No LLM) | 3 | not-started | - | |
| 21 | Saved Views (Curated, Shared) | 3 | not-started | - | |
| 22 | Generated Views (LLM-Powered, On-Demand) | 3 | not-started | - | |
| 23 | Three Lenses | 3 | not-started | - | |
| 24 | 4. Agent Communication | 2 | not-started | - | |
| 25 | The Problem | 3 | not-started | - | |
| 26 | Scoped Inline Chat | 3 | not-started | - | |
| 27 | Hard Interrupt | 3 | not-started | - | |
| 28 | Agent Completion Summaries | 3 | not-started | - | |
| 29 | Interrogation Agents | 3 | not-started | - | |
| 30 | 5. Conversation-to-Code Provenance | 2 | not-started | - | |
| 31 | The Problem | 3 | not-started | - | |
| 32 | Design | 3 | not-started | - | |
| 33 | 6. Cross-Workspace Spec Dependencies | 2 | not-started | - | |
| 34 | The Problem | 3 | not-started | - | |
| 35 | Design | 3 | not-started | - | |
| 36 | What the System Does With Cross-Workspace Links | 3 | not-started | - | |
| 37 | Cross-Workspace Change Notification | 3 | not-started | - | |
| 38 | 7. Multi-Human Collaboration | 2 | not-started | - | |
| 39 | Presence Awareness | 3 | not-started | - | |
| 40 | Conflict Prevention | 3 | not-started | - | |
| 41 | Shared Views | 3 | not-started | - | |
| 42 | 8. Inbox Detail | 2 | not-started | - | |
| 43 | Action Types (Priority Order) | 3 | not-started | - | |
| 44 | 9. Briefing Detail | 2 | not-started | - | |
| 45 | Structure | 3 | not-started | - | |
| 46 | Data Sources | 3 | not-started | - | |
| 47 | Briefing Q&A | 3 | not-started | - | |
| 48 | 10. Observable Lens (Future-Proofing) | 2 | not-started | - | |
| 49 | Design Constraints | 3 | not-started | - | |
| 50 | 11. CLI/MCP Parity Constraint | 2 | not-started | - | |
| 51 | 12. User Profile | 2 | not-started | - | |
| 52 | The Problem | 3 | not-started | - | |
| 53 | What the Profile Is | 3 | not-started | - | |
| 54 | What the Profile Is NOT | 3 | not-started | - | |
| 55 | Relationship to Existing Specs | 2 | not-started | - | |
