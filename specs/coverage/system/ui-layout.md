# Coverage: UI Layout & Interaction Patterns

**Spec:** [`system/ui-layout.md`](../../system/ui-layout.md)
**Last audited:** 2026-04-13
**Coverage:** 0/38

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | 1. Application Shell | 2 | n/a | - | Superseded by ui-navigation.md §1 |
| 2 | Fixed Structure | 3 | n/a | - | Superseded by ui-navigation.md §1 |
| 3 | Entrypoint Flow | 3 | n/a | - | Superseded by ui-navigation.md §5 |
| 4 | 2. Content Area Layouts | 2 | task-assigned | task-170 | ViewSpec types define what layouts are available |
| 5 | Full-Width | 3 | task-assigned | task-175 | Used by Inbox, Briefing, Specs list |
| 6 | Split (Main + Detail Panel) | 3 | task-assigned | task-173 | Drill-down interaction pattern |
| 7 | Canvas + Controls | 3 | task-assigned | task-178 | Architecture tab canvas |
| 8 | LLM Endpoint Contract | 3 | task-assigned | task-171 | SSE streaming + prompt templates |
| 9 | Role | 2 | task-assigned | task-171 | LLM Endpoint Contract subsection |
| 10 | Available Data | 2 | task-assigned | task-171 | LLM Endpoint Contract subsection |
| 11 | Output Format | 2 | task-assigned | task-171 | LLM Endpoint Contract subsection |
| 12 | Constraints | 2 | task-assigned | task-171 | LLM Endpoint Contract subsection |
| 13 | Editor Split | 3 | task-assigned | task-172 | Editor Split component |
| 14 | 3. Interaction Patterns | 2 | task-assigned | task-173 | Standardized patterns |
| 15 | Scope Transitions | 3 | task-assigned | task-173 | Cross-fade, pushState |
| 16 | Drill-Down (Entity Detail) | 3 | task-assigned | task-173 | Detail panel slide-in |
| 17 | Inline Expansion (Inbox/Briefing) | 3 | task-assigned | task-173 | Accordion pattern |
| 18 | Contextual Chat | 3 | task-assigned | task-173 | Recipient indicator chat |
| 19 | LLM-Assisted Spec Editing | 3 | task-assigned | task-185 | Inline suggestions with Accept/Edit/Dismiss |
| 20 | 4. View Specification Grammar | 2 | task-assigned | task-170 | Grammar structure |
| 21 | Structure | 3 | task-assigned | task-170 | JSON view spec schema |
| 22 | Data Layer | 3 | task-assigned | task-170 | concept, node_types, depth, etc. |
| 23 | Layout Layer | 3 | task-assigned | task-170 | graph, hierarchical, layered, etc. |
| 24 | Highlight Layer | 3 | task-assigned | task-170 | spec_path, node_ids highlighting |
| 25 | Encoding Layer | 3 | task-assigned | task-170 | color, size, border, opacity mapping |
| 26 | Extensibility | 3 | task-assigned | task-170 | Layout registry pattern |
| 27 | LLM Constraints | 3 | task-assigned | task-170 | Read-only, grammar-bound |
| 28 | 5. Explorer at Each Scope | 2 | task-assigned | task-174 | Scope-level rendering |
| 29 | Tenant Scope — Workspace Cards | 3 | task-assigned | task-174 | Grid of workspace cards |
| 30 | Workspace Scope — Realized Architecture | 3 | task-assigned | task-174 | Repos as nodes, dependencies as edges |
| 31 | Repo Scope — Architecture Detail | 3 | task-assigned | task-174 | C4 Level 2 with drill-down |
| 32 | 6. Specs View Layout | 2 | task-assigned | task-175 | Spec list + detail panel |
| 33 | 7. Inbox Layout | 2 | task-assigned | task-175 | Decision cards with actions |
| 34 | Item Structure | 3 | task-assigned | task-175 | Priority badge, accordion |
| 35 | Action Buttons per Item Type | 3 | task-assigned | task-175 | 10 priority levels |
| 36 | 8. Briefing Layout | 2 | task-assigned | task-175 | Narrative sections |
| 37 | 9. Meta-specs Preview Loop Layout | 2 | task-assigned | task-176 | 3-state preview workflow |
| 38 | 10. Rendering Technology | 2 | task-assigned | task-177 | SVG canvas + engines |
| 39 | Canvas Rendering | 3 | task-assigned | task-177 | SVG, auto-filter thresholds |
| 40 | Layout Engines | 3 | task-assigned | task-177 | ELK, d3-force, d3-scale |
| 41 | Interaction Events | 3 | task-assigned | task-177 | ViewEvent interface |
| 42 | Relationship to Existing Specs | 2 | n/a | - | Documentation — no implementation |
