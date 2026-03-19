# Lessons from agent-boss (Prior Art)

Studied the `agent-boss` prototype. Not recreating it - rethinking from first principles. But these lessons carry forward:

## Carry Forward

| Concept | What Worked | How to Evolve |
|---|---|---|
| **Agent hierarchy** | Parent/child relationships with cycle detection enabled natural team structure (Boss - CTO - Managers - Workers). | Hierarchy should be declarative in the agent-compose spec, not just runtime state. Gyre should enforce hierarchy-scoped permissions (a manager can only see/control its subtree). |
| **Agent-compose.yaml** | Declarative team blueprint (like docker-compose for agents) - portable, versionable, importable. Defines agents, roles, personas, hierarchy, initial prompts. | Keep this pattern. Evolve: add Ralph loop parameters, lifetime budgets, MCP server assignments, resource limits. Make it the single source of truth for team topology. |
| **Blackboard / shared state** | KnowledgeSpace gave all agents a shared view of the world. Useful for coordination without direct messaging. | Consider whether blackboard is the right primitive or if reconciliation + event streams are better. Blackboard gets noisy at scale (60-100KB for 11 agents). |
| **Structured inter-agent messaging** | HTTP + SSE + priority levels (info/directive/urgent). Per-agent inboxes with cursor-based polling. | WebSocket instead of SSE+polling. Messages should be typed (not just text) - task assignment, review request, escalation, status update. |
| **Interrupt ledger & metrics** | Tracked every agent interruption - 93% were traceable to fixable root causes. Enabled systematic loop tightening. | This is essential. Every interrupt is a cache miss in the Ralph loop. Track, categorize, and systematically eliminate. |
| **Personas (reusable domain expertise)** | Composable prompt fragments injected at spawn. Version history. | Good pattern. Should be scoped per-repo (not global namespace). Version and test personas like code. |
| **Sticky fields** | Agent status updates preserve context (branch, PR, session) across updates without resending. | Useful UX pattern - carry forward. |
| **Protocol template injection** | Collaboration norms baked into ignition prompt - agents don't need separate manuals. | Keep. But make the protocol template a versioned, testable artifact in the repo. |

## Don't Repeat

| Problem | Why It Failed | Alternative |
|---|---|---|
| **No spec-driven enforcement** | Agents could start coding without approved specs. No validation that output matched spec. | Specs are gates. The Manager Agent must validate spec existence and approval before dispatching work. Sub-agents validate output against spec in their Ralph loop. |
| **No failure rollback** | When a plan produced a broken stage graph or auto-merge broke tests, only option was human escalation. | Reconciliation pattern: detect drift, auto-revert or quarantine, then reassess. Agents should be able to roll back their own changes. |
| **No learned decision patterns** | Interrupt data existed but wasn't automated. Same decisions re-made manually. | Build a decision library. When the same interrupt pattern resolves the same way 3+ times, codify it as a rule. |
| **Cross-space isolation** | Lessons from one team didn't flow to others. Personas, allowlists, ADRs all siloed. | Platform-level knowledge that flows across all agent teams. Golden principles are global. |
| **No cost tracking** | No visibility into LLM costs per agent/task/space. | Track token usage, model costs, compute time per agent per task. Essential for budget management. |
| **No mutual exclusion** | Two agents could modify the same file/branch with no coordination primitive. | Worktree isolation (git worktrees per agent) + branch ownership. Gyre should prevent collisions, not rely on message discipline. |
| **Fragile approval detection** | Tmux text scraping for `[?BOSS]` was brittle. | Structured API for approvals. Agents request approval via typed message, platform routes to human, returns decision. No text scraping. |
| **Scalability unknown** | Only tested with 11 agents. | Design for 50+ agents from the start. Event-driven, not polling-based. |
