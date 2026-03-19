# Lessons from Gas Town (Yegge)

Key architectural patterns from Steve Yegge's Gas Town orchestrator worth considering:

| Pattern | Insight | Application |
|---|---|---|
| **GUPP (Universal Propulsion)** | "If there's work on your hook, you MUST run it." Agents self-start on boot by checking their work queue. Work survives crashes because it's persistent (git-backed), not in context. | Agents should auto-resume. Work definitions must outlive agent sessions. The Ralph loop continues across session boundaries. |
| **Nondeterministic Idempotence** | Workflows are durable chains of small steps. If an agent dies mid-step, the next session picks up. Path is nondeterministic, outcome converges. Like Temporal, but without deterministic replay. | This is reconciliation applied to workflows. Each Ralph loop step should be a persistent, claimable unit of work. |
| **Molecules (composable workflows)** | Work broken into small sequential steps agents check off. Templates ("formulas") compose, loop, and gate. Much more granular than "one big prompt." | Consider expressing Ralph loops as persistent workflow chains, not just prompt instructions. Steps are checkpoints that survive crashes. |
| **Wisps (ephemeral work)** | Not all orchestration state needs persisting. Ephemeral beads for patrol/coordination prevent polluting the permanent record. | Distinguish between durable work (features, specs) and ephemeral orchestration (patrol checks, nudges, health pings). |
| **Patrol + backoff** | Agents run continuous loops with exponential backoff when idle. Delegate grunt work to helpers (Dogs) so the patrol agent stays focused. | The Manager Agent should backoff when no work exists, not spin. Delegation to sub-agents for investigation keeps the manager's loop tight. |
| **Convoy (delivery unit)** | Wrap related work into a trackable unit with dashboards. Activity feed is the primary signal. | Maps to the activity dashboard. Every Ralph loop cycle should roll up into a visible, trackable delivery. |
| **The nudge problem** | Agents don't always self-start. Need a mechanism to kick them. A minimal "Boot" agent just checks if the orchestrator is alive every 5 minutes. | Build nudging into Gyre. Don't assume agents will self-propel - verify and kick. |
| **Work as persistent identity** | Agents are persistent identities (in git), sessions are ephemeral cattle. Agent != session. | Aligns with single-minded agents + lifetimes. The task is the persistent thing; the agent session is disposable. |
