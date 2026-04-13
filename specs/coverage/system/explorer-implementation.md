# Coverage: Explorer Implementation

**Spec:** [`system/explorer-implementation.md`](../../system/explorer-implementation.md)
**Last audited:** 2026-04-13
**Coverage:** 5/33

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Overview | 2 | task-assigned | task-065 | |
| 2 | Architecture | 2 | task-assigned | task-065 | |
| 3 | WebSocket Protocol | 2 | task-assigned | task-069 | |
| 4 | Endpoint | 3 | task-assigned | task-069 | |
| 5 | Messages: Client → Server | 3 | task-assigned | task-069 | |
| 6 | Messages: Server → Client | 3 | task-assigned | task-069 | |
| 7 | LLM Agent (Claude Agent SDK) | 2 | task-assigned | task-070 | |
| 8 | Agent Configuration | 3 | task-assigned | task-070 | |
| 9 | MCP Tools Available to the Agent | 3 | task-assigned | task-068 | |
| 10 | Agent System Prompt | 3 | task-assigned | task-070 | |
| 11 | Self-Check Loop | 3 | task-assigned | task-070 | |
| 12 | Saved Views | 2 | verified | - | Port trait + SQLite adapter + REST + frontend (ExplorerChat.svelte) |
| 13 | DB Schema | 3 | verified | - | Migration 000042 creates table; 000044-000046 refine FK/unique constraints. All spec fields present. |
| 14 | System Default Views | 3 | verified | - | 6 defaults (spec has 4 + 2 extras: Spec Coverage, Ungoverned Risk). Lazy-seeded on first list_views. |
| 15 | REST API | 3 | verified | - | Full CRUD at /repos/:id/views (saved_views.rs) + /workspaces/:id/explorer-views. Auth + tenant isolation. |
| 16 | Frontend Components | 2 | task-assigned | task-065 | Overview section |
| 17 | ExplorerCanvas (Svelte) | 3 | task-assigned | task-065 | |
| 18 | ExplorerChat (Svelte) | 3 | task-assigned | task-071 | |
| 19 | ExplorerView (Svelte) | 3 | task-assigned | task-071 | |
| 20 | Server Implementation | 2 | task-assigned | task-069 | |
| 21 | Explorer WebSocket Handler | 3 | task-assigned | task-069 | |
| 22 | Graph Summary MCP Tool | 3 | task-assigned | task-068 | |
| 23 | Dry-Run MCP Tool | 3 | task-assigned | task-068 | |
| 24 | Migration Plan | 2 | n/a | - | Context section — "green field, no migration needed" |
| 25 | Phase 1: Canvas + Filters | 3 | task-assigned | task-065 | |
| 26 | Phase 2: Chat + Agent | 3 | task-assigned | task-071 | |
| 27 | Phase 3: Saved Views | 3 | verified | - | DB migrations, REST CRUD, saved views dropdown + save button in ExplorerChat, system defaults seeded lazily. |
| 28 | Phase 4: Polish | 3 | task-assigned | task-078 | |
| 29 | Testing | 2 | n/a | - | Section heading only |
| 30 | Unit Tests | 3 | task-assigned | task-079 | |
| 31 | Integration Tests | 3 | task-assigned | task-080 | |
| 32 | Visual Tests | 3 | task-assigned | task-080 | |
| 33 | Relationship to Other Specs | 2 | n/a | - | Cross-reference section |
