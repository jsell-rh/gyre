---
title: "Deliver persona operational fields in constraints prompt"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / persona-constraints-delivery / 4a3f940495a8"
depends_on: [task-298]
progress: not-started
commits: []
---

## Required behavior

Per agent-runtime.md §5: 'system://constraints delivers budget limits and operational parameters from the persona's operational fields.' prompt_constraints (crates/gyre-server/src/mcp.rs:4523) and the spawn-time constraints injection must include a persona operational-parameters section sourced from the persona bound to the agent — model, temperature, max_tokens, and budget — read from real storage via the canonical persona surface established by the registry replacement (the meta-spec registry persona kind carrying the Persona operational fields, once /api/v1/personas is removed by orchestrator-boundary-amendments). The existing sections (workspace budget, trust level, scope boundaries, agent lifetime, iteration ceiling) remain; agents with no bound persona report that explicitly rather than fabricating defaults. The values must be the stored persona's values — a change to the persona's operational fields is reflected on the next constraints read or spawn.

## Acceptance Criteria

- [ ] For an agent spawned with a persona whose operational fields are set (e.g. model X, temperature 0.2, max_tokens 8192, budget 500000), the system://constraints MCP prompt contains those exact values read from storage.
- [ ] Editing the bound persona's operational fields changes the served constraints on the next read/spawn (no cached or hardcoded stand-ins).
- [ ] An agent with no bound persona receives an explicit 'no persona bound' statement in the operational-parameters section, not fabricated defaults.

## Verification intent

Set a bound persona's max_tokens to a distinctive value (e.g. 12345) in storage, then read system://constraints for that agent — at the base the response contains only scope boundaries, trust level, workspace budget, lifetime, and iteration-ceiling values drawn from other sources (prompt_constraints at mcp.rs:4523 assembles exactly those sections); the Persona operational fields (model, temperature, max_tokens, budget) are consumed by no execution path anywhere (only CRUD DTOs reference them), so the distinctive value is absent and the probe fails at the base.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "47d309e09aac1e2e04d334d0",
    "decision": "gap",
    "requirement_quote": "`system://constraints` delivers budget limits and operational parameters from the persona's operational fields.",
    "detail": "The three-section prompt structure and the four MCP prompts are implemented and wired end-to-end: the meta-spec system prompt is assembled and injected at spawn (GYRE_META_SPEC_PROMPT + set SHA, re-assembled on re-spawn); the protocol section is a single system-managed source (Ralph loop rules, MCP tool catalog, the 60-second heartbeat requirement, escalation rules, completion signaling — 'users don't edit this'), injected at spawn and served via system://protocol; task context (task details, pinned spec ref, branch/repo, gate-failure messages from previous iterations and unacked inbox messages) is delivered via the inbox and the system://context MCP prompt; heartbeat is enforced by the gyre_agent_heartbeat tool plus the 60-second stale-agent detector. GAP: the specced mapping of system://constraints to the persona's operational fields is not implemented — prompt_constraints delivers workspace budget configuration, trust level, scope boundaries, agent lifetime and the repo iteration ceiling, while the Persona entity's operational fields (model, temperature, max_tokens, budget) are stored and editable via /api/v1/personas but consumed by no execution path anywhere (only the CRUD DTOs reference them). 'Operational parameters from the persona's operational fields' therefore have no delivery mechanism; the constraints prompt draws from different sources than the spec mandates.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "quote": "\"system://constraints\" => prompt_constraints(state, auth).await,",
        "detail": "All four specced MCP prompts are dispatched (persona/protocol/context/constraints); the constraints prompt is served — but from workspace budget config, trust level and repo ceilings, not from the persona's operational fields."
      },
      {
        "file": "crates/gyre-server/src/api/prompts.rs",
        "quote": "- Send a heartbeat every 60 seconds; missing heartbeats mark you Dead and trigger re-spawn or cleanup.",
        "detail": "Positive wiring: the system-managed protocol section carries the Ralph loop rules, tool usage, the 60-second heartbeat requirement, escalation and completion signaling, and is not part of the meta-spec registry."
      },
      {
        "file": "crates/gyre-domain/src/workspace.rs",
        "quote": "pub model: Option<String>,",
        "detail": "The Persona entity's operational fields (model, temperature, max_tokens, budget) exist on the struct — the source the spec says system://constraints must draw from."
      },
      {
        "file": "crates/gyre-server/src/api/personas.rs",
        "quote": "persona.temperature = req.temperature;",
        "detail": "The persona operational fields are only ever written/read by the personas CRUD DTOs — no spawn, prompt-assembly or constraints path consumes them, so the specced delivery from persona operational fields does not exist."
      }
    ]
  }
]
