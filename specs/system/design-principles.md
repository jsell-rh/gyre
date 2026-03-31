# Design Principles

These are invariants, not guidelines. Every decision in Gyre must trace back to one or more of these.

| Principle | Detail |
|---|---|
| Simplicity | Minimal infrastructure stacks; avoid unnecessary complexity |
| Vertical scaling | Scale up before scaling out |
| NixOS as foundation | Single definition builds a server, Docker image, QEMU VM, LXC container, etc. Also the only safe way to give agents sudo - immutable, declarative system config. |
| Pluggable infrastructure | No hard dependency on any single runtime (k8s, containers, etc.) - customize later |
| Source control as foundation | All work flows through version control |
| Security by default | Agents are authenticated, auditable, and sandboxed |
| Internationalization | Full i18n/language support from day one |
| Server-side logic | Push config and logic to the server - clients stay thin. One fix propagates to all clients instantly (e.g., rotating an API key requires zero client updates). |
| Optimize for speed, not perfection | Don't prevent all failures - **feel failure domains**, recover fast. Speed up the development loop. If a gate slows you down, engineer away the reason it exists. |
| Engineer the Ralph Loop | **Everything** is about engineering the Ralph loop. Every design decision - infrastructure, tooling, architecture, agent orchestration - should be evaluated by: does this make the Ralph loop faster, tighter, or more reliable? See [`agent-runtime.md`](agent-runtime.md) §1 for the canonical definition. |
| Reconciliation as a primitive | Declare desired state, observe actual state, converge, repeat. Agents, infrastructure, credentials, lifecycle - all driven by reconciliation loops, not imperative scripts. The Ralph loop is a specific case of this universal pattern. |
| No shortcuts | The most correct way is mandated. Best practices are applied, not approximated. Time is not a constraint - correctness is. Agents have the throughput to do it right; there is no excuse for cutting corners. |
| Single-minded agents | One agent, one task. Spin up, execute, tear down. A task can be complex (e.g., "decompose this epic and delegate to sub-agents") but each agent has exactly one purpose. No long-lived multi-purpose agents. |
| Specs first, always | Specs live in-line with code and are always up to date. No implementation without an approved spec. Spec changes come before code changes - never the reverse. |
| Opinionated by design | This orchestrator exists to maximize throughput and quality of code. Where an opinion improves throughput or quality, enforce it. Don't be neutral when a strong default is clearly better. |
