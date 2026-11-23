---
title: "Publish reviewed spec backlog for specs/development/agent-experience.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/development/agent-experience.md",
  "plan_id": "06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861",
  "audit": {
    "base": "87377fe145ebddf27ef975fba7829fb10f4bb136",
    "requirements": "fd38a2fca0f8d8b7511fbf0b6fa7de9ecb0fdecfe0c66883a82c592b95a786cc",
    "sections": [
      {
        "id": "b86326cb5e1a6d0da595d27b",
        "decision": "context",
        "requirement_quote": "Inspired by OpenAI's Codex harness learnings: the bottleneck is never the agent's ability to write code - it's the **environment's legibility and feedback loops**. From day one, this system must ship with:",
        "detail": "Title/introduction heading. It is pure framing and rationale (the Codex-harness lesson and the 'from day one' exhortation); it states no independently implementable requirement beyond what its six child sections carry. Each child section (Testing & Validation, Observability for Agents, Repository as System of Record, Spec Organization & Cache Hit Optimization, Entropy Management / Garbage Collection, Agent Legibility as a Design Goal) is audited separately below, so no requirement is double-counted under this parent.",
        "evidence": []
      },
      {
        "id": "17fa0c307f57c2691d7eb0af",
        "decision": "gap",
        "requirement_quote": "- Agents must be able to **boot, drive, and validate** the application per worktree/branch.\n- Ephemeral, isolated environments per agent task - including their own observability stack.\n- **Full sensory access** - every component must be testable by agents with the same fidelity a human would have:\n  - **TUI:** visual snapshot testing (rendered terminal output comparison).\n  - **Web UI:** agents see the UI at multiple screen sizes/scales, access console logs, DOM state, network requests (via Chrome DevTools Protocol or equivalent).\n  - **API:** agents can drive and inspect every endpoint.",
        "detail": "Partially implemented; the load-bearing 'full sensory access' requirements are missing. What exists: spawn provisions a per-agent working directory (jj workspace, falling back to a plain-git worktree keyed to the requested branch, recorded in the DB linked to agent/repo/task), containers are ephemeral (--rm) and hardened (--network=none, memory/pids limits, non-root), and gates run test commands inside a detached git worktree at head_sha, so boot/drive/validate per worktree is substantially real, and agents receive GYRE_SERVER_URL + a pre-minted JWT letting them drive REST endpoints with their own credentials. What is missing: (1) 'including their own observability stack' — the only per-run observability machinery in the repo is a gate-scoped OTLP trace receiver that explicitly disclaims being a general observability backend; there is no Loki/Prometheus/Tempo/Grafana anywhere, and the container default --network=none denies the egress an observability stack would need. (2) TUI visual snapshot testing — the TUI is a plain ratatui status dashboard; no rendered-terminal-output comparison facility exists (no insta/vhs/snapshot tooling in gyre-cli or Cargo.lock). (3) Web UI sensory access — the Playwright suite does visual regression (toHaveScreenshot) but at exactly one fixed 1280x720 viewport with no multiple screen sizes/scales, no deviceScaleFactor/setViewportSize anywhere in web/tests, no Chrome DevTools Protocol usage (only route-mocking), console capture limited to a single pageerror guard test, and the whole harness is a dev/CI fixture that boots the server itself (webServer in playwright.config.js), not machinery an agent can invoke per worktree/branch. (4) 'agents can drive and inspect every endpoint' — no generic API-driving mechanism exists; the MCP tool catalog covers platform operations (tasks, MRs, messages, graph) with no endpoint-driving or console/network-inspection tool. The sibling spec specs/development/frontend-testing.md layers (vitest components, Playwright E2E) are likewise developer/CI harnesses, not agent-facing sensory access.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// Plain-git worktree: created for every agent (the plain-git",
            "detail": "Spawn provisions a per-agent working directory (jj workspace preferred, plain-git worktree on the requested branch as fallback), recorded in the DB linked to agent, repo, task and branch — the per-worktree/branch isolation half of the first bullet is real production behavior."
          },
          {
            "file": "crates/gyre-adapters/src/compute/container.rs",
            "quote": "let network = self.network.as_deref().unwrap_or(\"none\");",
            "detail": "Agent containers are ephemeral (--rm, --detach) and isolated with hardened defaults (network none, 2g memory, 512 pids, uid 65534). Isolation/ephemerality is satisfied, but the default denies network egress and nothing provisions a per-environment observability stack — no collector, metrics, or log store is started per agent task."
          },
          {
            "file": "crates/gyre-server/src/otlp_receiver.rs",
            "quote": "This is NOT a general-purpose observability backend. It is scoped to",
            "detail": "The only per-run observability machinery in the repo is this gate-time OTLP trace receiver, started per gate run and stopped after test execution. Repo-wide search finds no Loki/Prometheus/Tempo/Grafana or any other per-agent-task observability stack, so the 'including their own observability stack' requirement is unimplemented."
          },
          {
            "file": "crates/gyre-cli/src/tui.rs",
            "quote": "use ratatui::{",
            "detail": "The TUI is a plain ratatui/crossterm status dashboard (connection status, ping). Inspected crates/gyre-cli and Cargo.lock for insta/vhs/screenshot/snapshot testing of rendered terminal output: none exists — the 'TUI: visual snapshot testing' bullet has no implementation at all."
          },
          {
            "file": "web/tests/e2e/explorer-visual.spec.js",
            "quote": "viewport: { width: 1280, height: 720 },",
            "detail": "The only visual-regression suite runs at one fixed viewport. Searched web/tests and web/src for deviceScaleFactor and setViewportSize: zero matches; no test exercises multiple screen sizes/scales, there is no Chrome DevTools Protocol usage (network interaction is route-mocking via page.route), and console-log capture is a single pageerror guard in app.spec.js. Additionally this is a dev/CI harness — web/playwright.config.js webServer boots gyre-server itself with GYRE_DATABASE_URL=memory — not machinery an agent can invoke against its own worktree, so it does not deliver 'full sensory access ... with the same fidelity a human would have' to agents."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_SERVER_URL\".to_string(), state.base_url.clone());",
            "detail": "Agents do receive the server URL and a pre-minted auth token (GYRE_AUTH_TOKEN) at spawn, so they can drive REST endpoints with their own credentials — a partial realization of the API bullet. But no generic mechanism lets an agent drive and inspect every endpoint: the MCP tool catalog in crates/gyre-server/src/mcp.rs (gyre_create_task, gyre_list_mrs, gyre_search, graph tools, etc.) contains no API-driving or request/network-inspection tool, and no per-worktree application-boot facility exists for agents."
          }
        ]
      },
      {
        "id": "c5b26c825b5cd17bd8cd4b61",
        "decision": "gap",
        "requirement_quote": "- Agents query their own **logs, metrics, and traces** (LogQL, PromQL, TraceQL or equivalent).\n- Agents can reason about performance, errors, and behavior from instrumentation - not just code.",
        "detail": "No query language or equivalent exists for agents over their own telemetry. (1) Logs: GET /api/v1/agents/:id/logs is a plain limit/offset tail (default 100, max 1000) over an in-memory AgentLogBuffer that a restart orphans — no filtering, search, time range, or query language; the reference agent runner only ever appends logs, never queries them. (2) Metrics: /metrics (registered on the root router, outside the auth-layered api_router) exposes server-process Prometheus text (http request totals, active agents, merge queue depth) as an operator scrape endpoint — no per-agent metrics dimension and no PromQL. (3) Traces: server OTel spans export to an external operator-side collector (default http://localhost:4317); no collector, storage, or query surface ships in-repo. The only real trace data an agent can reach is the gate-run trace for an MR (GET /api/v1/merge-requests/:id/trace and the MCP trace://{mr_id} resource), which covers test-command spans, not the agent's own runtime behavior. No MCP tool exists for querying logs, metrics, or traces. The closest realized behavior for 'reason about performance, errors, and behavior from instrumentation' is gate-trace inspection and gate-failure feedback, which is MR-scoped, not agent-self-scoped.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/agent_logs.rs",
            "quote": "let limit = params.limit.unwrap_or(100).min(1000);",
            "detail": "The entire agent log query surface is a limit/offset tail returning Vec<String> lines. Combined with crates/gyre-server/src/lib.rs:368 'pub agent_logs: Arc<log_compression::AgentLogBuffer>,' (in-memory buffer), this is a write-mostly plain tail, not a LogQL-or-equivalent query capability; the reference runner (docker/gyre-agent/agent-runner.mjs) only POSTs logs."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": ".route(\"/metrics\", get(metrics::metrics_handler))",
            "detail": "The metrics endpoint is registered on the root router (the auth/ABAC layers apply to api::api_router() only) and returns server-process Prometheus text — an operator scrape surface with no per-agent dimension and no query language. Repo-wide search for promql/logql/traceql/loki/tempo/grafana finds zero query-language implementations."
          },
          {
            "file": "crates/gyre-server/src/telemetry.rs",
            "quote": "Exports spans via OTLP/gRPC to the collector at `OTEL_EXPORTER_OTLP_ENDPOINT`",
            "detail": "Server spans export to an external operator-side collector ('The server works fine without a collector'); no collector, trace storage, or agent-facing trace query surface ships in the repository, so agents cannot query their own runtime traces."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"SDLC system trace for a merge request (HSI §3a). URI: trace://{mr_id}\",",
            "detail": "The only trace access exposed to agents is this MR gate-run trace resource (spans captured during a gate's test command). It is real instrumentation an agent can reason over, but it is scoped to MR gate runs — not the agent's own logs, metrics, or runtime traces — and the MCP tool catalog contains no log/metrics/trace query tool."
          }
        ]
      },
      {
        "id": "07a6866179016f8676c1640d",
        "decision": "gap",
        "requirement_quote": "- Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo.\n- **Mechanical enforcement** that docs stay current: linters, CI, doc-gardening agents.",
        "detail": "The artifact half is largely realized; the enforcement half is not. Realized: AGENTS.md is exactly the specced 'map, not a manual' entry point ('This file is the map. Follow the links below for detail.'), and design docs (specs/), execution plans (specs/tasks/ with enforced commits: attribution), architecture docs, and coverage matrices are all versioned in-repo. Missing: (1) quality grades — grep for 'quality grade'/'quality_grade' across crates/ returns zero matches; the concept exists only in spec prose, and its other source (specs/development/manager-agent.md) is marked Superseded in specs/index.md, so no production surface versions or tracks quality grades. (2) Mechanical enforcement that docs stay current — CI's arch-lint job runs ~25 scripts/check-*.sh lints, but they enforce code invariants (architecture boundaries, ABAC registry, byte-slice safety, etc.), not documentation currency; the one doc/spec-parity gate (scripts/check-cli-spec-parity.sh, header claiming 'Run during pre-commit and CI') is wired only into .pre-commit-config.yaml and appears nowhere in .github/workflows/ci.yml. (3) Doc-gardening agents — grep for 'garden'/'gardening' across scripts/, crates/ and .github/ returns zero matches; no production code path spawns an agent to detect or fix stale documentation. The recurring jobs that exist target spec-link staleness and meta-spec pins, not docs generally.",
        "evidence": [
          {
            "file": "scripts/check-task-commit-attribution.sh",
            "quote": "# check-task-commit-attribution.sh — Every task-labeled product-surface",
            "detail": "The artifact half of the section is realized: design docs, architecture docs and execution plans all live versioned in-repo (specs/, specs/tasks/, specs/reviews/, specs/coverage/), and this lint mechanically ties every task-labeled product-surface commit to its task's versioned `commits:` frontmatter. AGENTS.md (inspected) is exactly the specced 'map, not a manual' entry point ('**This file is the map. Follow the links below for detail.**') with progressive disclosure via Documentation Index and Codebase Map tables. What is missing is the quality-grades artifact and the doc-currency enforcement shown by the other evidence."
          },
          {
            "file": ".github/workflows/ci.yml",
            "quote": "run: bash scripts/check-arch.sh",
            "detail": "CI's arch-lint job runs the check-*.sh fleet, but inspected script purposes show they enforce code invariants (hexagonal boundaries, ABAC route registry, migration numbering, UTF-8 slicing, etc.) — none checks that documentation files stay current. No markdownlint, doc-link checker, or api-reference parity check exists in scripts/."
          },
          {
            "file": "scripts/check-cli-spec-parity.sh",
            "quote": "# Run during pre-commit and CI. Not exhaustive — supplements the",
            "detail": "The closest doc/spec-parity gate claims CI wiring in its header, but grep shows cli-spec-parity only in .pre-commit-config.yaml:35 — it is absent from .github/workflows/, so the doc-parity enforcement that does exist is pre-commit-only and bypassable (pre-commit is local)."
          },
          {
            "file": "specs/index.md",
            "quote": "| Manager Agent (superseded) | [`development/manager-agent.md`](development/manager-agent.md) | Superseded -- split into [`system/platform-model.md`](system/platform-model.md) + [`system/agent-runtime.md`](system/agent-runtime.md) §1 |",
            "detail": "Quality grades have no production support: grep for 'quality grade'/'quality_grade' in crates/ returns zero matches, and the only specs mentioning them are this section and the superseded manager-agent.md, whose Manager Agent (periodic quality-grade updates) does not exist in code. Combined with the zero matches for 'garden'/'gardening' across scripts/, crates/ and .github/ (inspected for any doc-gardening agent spawn path), both the quality-grades artifact and the doc-gardening-agent enforcement the section requires are missing."
          }
        ]
      },
      {
        "id": "fff96bf529e90ad943501101",
        "decision": "gap",
        "requirement_quote": "- All specs live in `specs/` with a `specs/index.md` entry point.\n- Think about spec structure in terms of **cache hits vs. misses**: if an agent tool call (search, read, etc.) finds what it needs on the first try, that's a cache hit. If it fails and requires additional lookups, that's a cache miss.\n- Specs should be structured and indexed to **maximize cache hits** - clear naming, cross-linking, predictable paths.",
        "detail": "Structurally strong but the entry point does not actually cover all specs, and nothing keeps it current. Realized: all specs live under specs/ in consistently kebab-case-named topic directories (system/, development/, personas/, milestones/, prior-art/), specs/index.md exists as the entry point with topic tables, cross-links and summaries, and pervasive cross-linking between specs (e.g. AGENTS.md's Key Architecture Specs table). Missing: the index is hand-maintained and already incomplete — active canonical specs specs/system/ui-navigation.md and specs/system/repo-lifecycle.md (and specs/system/ralph-loop.md) exist on disk but have no row in specs/index.md (grep of index.md for these filenames returns nothing; the only ralph row is the different, superseded development/ralph-loops.md). An agent whose first lookup goes through the specced entry point for UI navigation or repo lifecycle misses and must fall back to search — exactly the cache miss the section defines and says to avoid. Nothing mechanically validates index completeness or cross-link integrity: no script in scripts/ checks that every specs/**/*.md is reachable from specs/index.md. The runtime GET /api/v1/specs/index is a separate auto-generated surface built from the manifest ledger, not a validation of specs/index.md, and manifest registration enforcement (find_unregistered_specs, used to reject pushes adding unregistered specs) is policy-gated per repo and explicitly excludes index.md.",
        "evidence": [
          {
            "file": "specs/index.md",
            "quote": "This is the entry point for all Gyre specs. Read this first.",
            "detail": "The specced entry point exists, organized into topic tables (System / Development / Prior Art / Personas / Milestones / Open Questions) with cross-links and one-line summaries, and naming across specs/ is consistent and descriptive — the structure/naming/cross-linking practice the section asks for is genuinely present."
          },
          {
            "file": "specs/system/ui-navigation.md",
            "quote": "# UI Navigation Model",
            "detail": "This active canonical spec (it supersedes human-system-interface.md §1 and is linked from AGENTS.md's Key Architecture Specs table) has no row in specs/index.md — grep of index.md for 'ui-navigation' returns nothing. The same holds for specs/system/repo-lifecycle.md ('# Repo Lifecycle', defines repository CRUD lifecycle) and specs/system/ralph-loop.md. These are first-lookup cache misses through the specced entry point, the precise failure mode the section says to design against."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "// GET /api/v1/specs/index — auto-generated markdown index",
            "detail": "The only mechanically generated spec index is this runtime API handler, built from the manifest-driven ledger — a different surface from the repo's specs/index.md. Inspected scripts/ for any index-completeness or cross-link validation (orphan/reachable/link-check): none exists, and manifest-based registration enforcement (spec_registry.rs find_unregistered_specs, 'Excludes `index.md`, `prior-art/`, `milestones/`') is policy-gated per repo and does not validate index.md rows, so nothing keeps the hand-maintained entry point complete."
          }
        ]
      },
      {
        "id": "fd6e133d1621c46338a657b0",
        "decision": "gap",
        "requirement_quote": "- Encode **golden principles** in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs.\n- Technical debt is paid continuously in small increments, not in painful bursts.\n- Human taste is captured once, then enforced continuously on every line.",
        "detail": "The principle-encoding and continuous-enforcement halves are partially real; the recurring-cleanup-agents-opening-fix-up-MRs half is not. Real: the meta-spec system encodes principles machine-actionably — seed_builtin_meta_specs seeds Principle/Standard/Persona meta-specs, prompt_assembly injects all REQUIRED meta-specs into every agent prompt ordered persona → principle → standard → process and records them in provenance attestations, and the daily meta_spec_conformance_sweep job (registered in jobs.rs) compares the active set SHA against provenance and auto-creates drift-review tasks on drift. Missing: (1) only one Principle meta-spec is seeded ('conventional-commits') — the mechanism exists but the golden-principles corpus is minimal. (2) The spec patrols (spec_patrol.rs run_patrol with five real spec-graph checks; spec_lifecycle_patrol.rs with three more) run only via manually-invoked HTTP endpoints — their sole production callers are the patrol_spec_links/patrol_spec_lifecycle handlers; jobs.rs registers no patrol job, so no recurring cleanup scan over spec-graph deviations exists (the repo's own task-292 records this as not-started). (3) No fix-up MR path: grep for 'fixup'/'fix-up' across crates/ returns zero matches; the only automatic MR creation is create_mr_for_completion, tied to agent completion of an existing task — no code path opens a fix-up MR from a patrol or sweep finding (all MergeRequest::new uses in reconciliation.rs are test fixtures). A drift task can eventually flow task → scheduler-spawned agent → completion MR, but that is task-driven indirection, not cleanup agents opening fix-up MRs, and the patrol path creates only notifications/messages, never tasks or MRs.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "\"conventional-commits\",",
            "detail": "seed_builtin_meta_specs seeds meta-specs including this single MetaSpecKind::Principle entry ('All commits must follow Conventional Commits specification.') plus one Standard and personas. The machine-actionable encoding of golden principles exists as a mechanism, but the seeded Principle corpus is one entry — human taste is captured only for commit style."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "//! The assembled set is also the attestation source: `meta_specs_used` records",
            "detail": "Continuous per-line enforcement is real: every agent prompt is assembled from all REQUIRED meta-specs (kind-ordered persona → principle → standard → process) at their pinned approved versions, and the injected set is recorded in provenance attestations — 'captured once, enforced continuously' is realized for whatever principles are registered."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.labels = vec![DRIFT_REVIEW_LABEL.to_string(), \"auto-created\".to_string()];",
            "detail": "The daily meta_spec_conformance_sweep (registered in jobs.rs as a recurring job) detects drift between the active meta-spec set and provenance-recorded SHAs and auto-creates drift-review tasks — recurring deviation detection with durable task creation is real production behavior, the strongest satisfied fragment of this section."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::spec_patrol::run_patrol(&state, now, threshold).await;",
            "detail": "The spec-graph patrol's only production callers are the manually-invoked HTTP handlers patrol_spec_links and patrol_spec_lifecycle (grep for run_patrol/run_and_escalate across crates/gyre-server/src shows only these handlers and tests). jobs.rs registers many recurring jobs (retention_cleanup, staleness checks, conformance sweep, pr_hygiene_check) but no patrol job — the patrol never runs on a schedule, so there is no recurring cleanup scan over spec-graph deviations."
          },
          {
            "file": "specs/tasks/task-292.md",
            "quote": "the patrol exists only as a manually invoked HTTP endpoint",
            "detail": "The repo's own task ledger records this exact gap as progress: not-started, requiring a scheduled spec-links patrol job, ABAC gating of the endpoint, and persona patrol inputs — confirming the recurring-cleanup integration is a known open gap, not an implemented behavior."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "/// Create the completion MR for an agent (agent-runtime.md §1 Phase 6 step 1).",
            "detail": "The only automatic MR creation path is agent-completion-driven (shared by POST /agents/:id/complete and the MCP gyre_agent_complete tool). Grep for 'fixup'/'fix-up' across crates/ returns zero matches, and all MergeRequest::new uses in reconciliation.rs are inside test fixtures — no code path opens a fix-up MR from a patrol or conformance-sweep finding."
          }
        ]
      },
      {
        "id": "3a373eea0ef641d10d664170",
        "decision": "satisfied",
        "requirement_quote": "- The codebase is optimized for **agent legibility first** - if an agent can't see it in context, it doesn't exist.\n- Favor boring, composable, stable dependencies that agents can fully internalize.\n- Custom lint error messages should include **remediation instructions** - they become agent context.",
        "detail": "The concrete, checkable requirements of this section are realized in production. (1) Custom lint remediation instructions: the ~90 scripts/check-*.sh custom lints (run via .pre-commit-config.yaml and CI's arch-lint job) pair every failure with a concrete remediation and a spec/flaw-class pointer — e.g. check-arch.sh prints 'Remediation: Move infrastructure code to gyre-adapters.' plus the architecture-spec reference; check-byte-slice-truncation.sh, check-fail-open-ref-resolution.sh and check-hardcoded-defaults.sh follow the same failure + fix + escape-hatch pattern, and several cite the specs/reviews/task-NNN.md flaw class that motivated them. These messages are exactly the 'agent context' the spec asks for. (2) Boring, composable, stable dependencies: the workspace dependency table is mainstream and stable — tokio 1, serde 1, axum 0.7, tower, rusqlite/diesel 2, clap 4, git2, ring, tracing — centrally declared and fully internalizable. (Note honestly: this is satisfied de facto by the dependency choices; it is not mechanically enforced — there is no cargo-deny/deny.toml or advisory gating in CI — but the spec states a preference to favor such dependencies, not a requirement to enforce it mechanically.) (3) Agent legibility first: AGENTS.md is deliberately a map ('This file is the map. Follow the links below for detail.'), docs/ is a flat, predictably-named, task-indexed set (api-reference, server-config, agent-protocol, development, ui, cli) matched to the AGENTS.md index table, and legibility-adjacent lints exist (dead-component, stale-stub-marker, unwired-port-method checks). The section is design-goal prose, and its concrete obligations are met by shipped behavior.",
        "evidence": [
          {
            "file": "scripts/check-arch.sh",
            "quote": "echo \"  Remediation: Move infrastructure code to gyre-adapters.\"",
            "detail": "Custom lint error messages include remediation instructions as required: this failure message pairs the violation with a concrete fix and the next line cites 'specs/development/architecture.md - Hexagonal Architecture Invariants'. The same pattern (failure + concrete remediation + escape hatch + flaw-class provenance) is followed across the custom lint fleet, e.g. check-fail-open-ref-resolution.sh ('Fail closed: propagate the None as an error/404...') and check-hardcoded-defaults.sh ('Fix: Store a structured policy value ... or add // hardcoded-default:ok if genuinely intentional')."
          },
          {
            "file": "Cargo.toml",
            "quote": "axum = { version = \"0.7\", features = [\"ws\"] }",
            "detail": "The workspace dependency table is a boring, composable, stable stack — tokio = { version = \"1\", features = [\"full\"] }, serde = { version = \"1\", ... }, axum 0.7, tower, rusqlite/diesel 2, clap, git2 — all mainstream crates an agent can fully internalize, centrally declared for the whole workspace. The preference is satisfied de facto by these choices (no cargo-deny enforcement exists, but the spec's literal obligation is to favor such dependencies, which the manifest does)."
          },
          {
            "file": "scripts/check-stale-stub-markers.sh",
            "quote": "# Architecture lint: detect stale \"(stubbed)\" markers in doc comments.",
            "detail": "Agent-legibility-first is realized as shipped structure and checks: AGENTS.md (inspected) is deliberately a map ('**This file is the map. Follow the links below for detail.**') with a Documentation Index routing every task type to a predictable flat docs/ file (api-reference, server-config, agent-protocol, development, ui, cli), and legibility-adjacent mechanical checks exist — this one (wired via .pre-commit-config.yaml) fails when a doc comment still claims a function is stubbed after implementation ('A stale marker misleads readers into thinking the function is not yet implemented'), alongside check-dead-components.sh and check-unwired-port-methods.sh. The codebase's entry surfaces are built for first-lookup context, which is what the section's design goal asks for."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "17fa0c307f57c2691d7eb0af",
        "decision": "gap",
        "requirement_quote": "- Agents must be able to **boot, drive, and validate** the application per worktree/branch.\n- Ephemeral, isolated environments per agent task - including their own observability stack.\n- **Full sensory access** - every component must be testable by agents with the same fidelity a human would have:\n  - **TUI:** visual snapshot testing (rendered terminal output comparison).\n  - **Web UI:** agents see the UI at multiple screen sizes/scales, access console logs, DOM state, network requests (via Chrome DevTools Protocol or equivalent).\n  - **API:** agents can drive and inspect every endpoint.",
        "detail": "Partially implemented; the load-bearing 'full sensory access' requirements are missing. What exists: spawn provisions a per-agent working directory (jj workspace, falling back to a plain-git worktree keyed to the requested branch, recorded in the DB linked to agent/repo/task), containers are ephemeral (--rm) and hardened (--network=none, memory/pids limits, non-root), and gates run test commands inside a detached git worktree at head_sha, so boot/drive/validate per worktree is substantially real, and agents receive GYRE_SERVER_URL + a pre-minted JWT letting them drive REST endpoints with their own credentials. What is missing: (1) 'including their own observability stack' — the only per-run observability machinery in the repo is a gate-scoped OTLP trace receiver that explicitly disclaims being a general observability backend; there is no Loki/Prometheus/Tempo/Grafana anywhere, and the container default --network=none denies the egress an observability stack would need. (2) TUI visual snapshot testing — the TUI is a plain ratatui status dashboard; no rendered-terminal-output comparison facility exists (no insta/vhs/snapshot tooling in gyre-cli or Cargo.lock). (3) Web UI sensory access — the Playwright suite does visual regression (toHaveScreenshot) but at exactly one fixed 1280x720 viewport with no multiple screen sizes/scales, no deviceScaleFactor/setViewportSize anywhere in web/tests, no Chrome DevTools Protocol usage (only route-mocking), console capture limited to a single pageerror guard test, and the whole harness is a dev/CI fixture that boots the server itself (webServer in playwright.config.js), not machinery an agent can invoke per worktree/branch. (4) 'agents can drive and inspect every endpoint' — no generic API-driving mechanism exists; the MCP tool catalog covers platform operations (tasks, MRs, messages, graph) with no endpoint-driving or console/network-inspection tool. The sibling spec specs/development/frontend-testing.md layers (vitest components, Playwright E2E) are likewise developer/CI harnesses, not agent-facing sensory access.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// Plain-git worktree: created for every agent (the plain-git",
            "detail": "Spawn provisions a per-agent working directory (jj workspace preferred, plain-git worktree on the requested branch as fallback), recorded in the DB linked to agent, repo, task and branch — the per-worktree/branch isolation half of the first bullet is real production behavior."
          },
          {
            "file": "crates/gyre-adapters/src/compute/container.rs",
            "quote": "let network = self.network.as_deref().unwrap_or(\"none\");",
            "detail": "Agent containers are ephemeral (--rm, --detach) and isolated with hardened defaults (network none, 2g memory, 512 pids, uid 65534). Isolation/ephemerality is satisfied, but the default denies network egress and nothing provisions a per-environment observability stack — no collector, metrics, or log store is started per agent task."
          },
          {
            "file": "crates/gyre-server/src/otlp_receiver.rs",
            "quote": "This is NOT a general-purpose observability backend. It is scoped to",
            "detail": "The only per-run observability machinery in the repo is this gate-time OTLP trace receiver, started per gate run and stopped after test execution. Repo-wide search finds no Loki/Prometheus/Tempo/Grafana or any other per-agent-task observability stack, so the 'including their own observability stack' requirement is unimplemented."
          },
          {
            "file": "crates/gyre-cli/src/tui.rs",
            "quote": "use ratatui::{",
            "detail": "The TUI is a plain ratatui/crossterm status dashboard (connection status, ping). Inspected crates/gyre-cli and Cargo.lock for insta/vhs/screenshot/snapshot testing of rendered terminal output: none exists — the 'TUI: visual snapshot testing' bullet has no implementation at all."
          },
          {
            "file": "web/tests/e2e/explorer-visual.spec.js",
            "quote": "viewport: { width: 1280, height: 720 },",
            "detail": "The only visual-regression suite runs at one fixed viewport. Searched web/tests and web/src for deviceScaleFactor and setViewportSize: zero matches; no test exercises multiple screen sizes/scales, there is no Chrome DevTools Protocol usage (network interaction is route-mocking via page.route), and console-log capture is a single pageerror guard in app.spec.js. Additionally this is a dev/CI harness — web/playwright.config.js webServer boots gyre-server itself with GYRE_DATABASE_URL=memory — not machinery an agent can invoke against its own worktree, so it does not deliver 'full sensory access ... with the same fidelity a human would have' to agents."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "container_env.insert(\"GYRE_SERVER_URL\".to_string(), state.base_url.clone());",
            "detail": "Agents do receive the server URL and a pre-minted auth token (GYRE_AUTH_TOKEN) at spawn, so they can drive REST endpoints with their own credentials — a partial realization of the API bullet. But no generic mechanism lets an agent drive and inspect every endpoint: the MCP tool catalog in crates/gyre-server/src/mcp.rs (gyre_create_task, gyre_list_mrs, gyre_search, graph tools, etc.) contains no API-driving or request/network-inspection tool, and no per-worktree application-boot facility exists for agents."
          }
        ]
      },
      {
        "id": "c5b26c825b5cd17bd8cd4b61",
        "decision": "gap",
        "requirement_quote": "- Agents query their own **logs, metrics, and traces** (LogQL, PromQL, TraceQL or equivalent).\n- Agents can reason about performance, errors, and behavior from instrumentation - not just code.",
        "detail": "No query language or equivalent exists for agents over their own telemetry. (1) Logs: GET /api/v1/agents/:id/logs is a plain limit/offset tail (default 100, max 1000) over an in-memory AgentLogBuffer that a restart orphans — no filtering, search, time range, or query language; the reference agent runner only ever appends logs, never queries them. (2) Metrics: /metrics (registered on the root router, outside the auth-layered api_router) exposes server-process Prometheus text (http request totals, active agents, merge queue depth) as an operator scrape endpoint — no per-agent metrics dimension and no PromQL. (3) Traces: server OTel spans export to an external operator-side collector (default http://localhost:4317); no collector, storage, or query surface ships in-repo. The only real trace data an agent can reach is the gate-run trace for an MR (GET /api/v1/merge-requests/:id/trace and the MCP trace://{mr_id} resource), which covers test-command spans, not the agent's own runtime behavior. No MCP tool exists for querying logs, metrics, or traces. The closest realized behavior for 'reason about performance, errors, and behavior from instrumentation' is gate-trace inspection and gate-failure feedback, which is MR-scoped, not agent-self-scoped.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/agent_logs.rs",
            "quote": "let limit = params.limit.unwrap_or(100).min(1000);",
            "detail": "The entire agent log query surface is a limit/offset tail returning Vec<String> lines. Combined with crates/gyre-server/src/lib.rs:368 'pub agent_logs: Arc<log_compression::AgentLogBuffer>,' (in-memory buffer), this is a write-mostly plain tail, not a LogQL-or-equivalent query capability; the reference runner (docker/gyre-agent/agent-runner.mjs) only POSTs logs."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": ".route(\"/metrics\", get(metrics::metrics_handler))",
            "detail": "The metrics endpoint is registered on the root router (the auth/ABAC layers apply to api::api_router() only) and returns server-process Prometheus text — an operator scrape surface with no per-agent dimension and no query language. Repo-wide search for promql/logql/traceql/loki/tempo/grafana finds zero query-language implementations."
          },
          {
            "file": "crates/gyre-server/src/telemetry.rs",
            "quote": "Exports spans via OTLP/gRPC to the collector at `OTEL_EXPORTER_OTLP_ENDPOINT`",
            "detail": "Server spans export to an external operator-side collector ('The server works fine without a collector'); no collector, trace storage, or agent-facing trace query surface ships in the repository, so agents cannot query their own runtime traces."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "\"description\": \"SDLC system trace for a merge request (HSI §3a). URI: trace://{mr_id}\",",
            "detail": "The only trace access exposed to agents is this MR gate-run trace resource (spans captured during a gate's test command). It is real instrumentation an agent can reason over, but it is scoped to MR gate runs — not the agent's own logs, metrics, or runtime traces — and the MCP tool catalog contains no log/metrics/trace query tool."
          }
        ]
      },
      {
        "id": "07a6866179016f8676c1640d",
        "decision": "gap",
        "requirement_quote": "- Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo.\n- **Mechanical enforcement** that docs stay current: linters, CI, doc-gardening agents.",
        "detail": "The artifact half is largely realized; the enforcement half is not. Realized: AGENTS.md is exactly the specced 'map, not a manual' entry point ('This file is the map. Follow the links below for detail.'), and design docs (specs/), execution plans (specs/tasks/ with enforced commits: attribution), architecture docs, and coverage matrices are all versioned in-repo. Missing: (1) quality grades — grep for 'quality grade'/'quality_grade' across crates/ returns zero matches; the concept exists only in spec prose, and its other source (specs/development/manager-agent.md) is marked Superseded in specs/index.md, so no production surface versions or tracks quality grades. (2) Mechanical enforcement that docs stay current — CI's arch-lint job runs ~25 scripts/check-*.sh lints, but they enforce code invariants (architecture boundaries, ABAC registry, byte-slice safety, etc.), not documentation currency; the one doc/spec-parity gate (scripts/check-cli-spec-parity.sh, header claiming 'Run during pre-commit and CI') is wired only into .pre-commit-config.yaml and appears nowhere in .github/workflows/ci.yml. (3) Doc-gardening agents — grep for 'garden'/'gardening' across scripts/, crates/ and .github/ returns zero matches; no production code path spawns an agent to detect or fix stale documentation. The recurring jobs that exist target spec-link staleness and meta-spec pins, not docs generally.",
        "evidence": [
          {
            "file": "scripts/check-task-commit-attribution.sh",
            "quote": "# check-task-commit-attribution.sh — Every task-labeled product-surface",
            "detail": "The artifact half of the section is realized: design docs, architecture docs and execution plans all live versioned in-repo (specs/, specs/tasks/, specs/reviews/, specs/coverage/), and this lint mechanically ties every task-labeled product-surface commit to its task's versioned `commits:` frontmatter. AGENTS.md (inspected) is exactly the specced 'map, not a manual' entry point ('**This file is the map. Follow the links below for detail.**') with progressive disclosure via Documentation Index and Codebase Map tables. What is missing is the quality-grades artifact and the doc-currency enforcement shown by the other evidence."
          },
          {
            "file": ".github/workflows/ci.yml",
            "quote": "run: bash scripts/check-arch.sh",
            "detail": "CI's arch-lint job runs the check-*.sh fleet, but inspected script purposes show they enforce code invariants (hexagonal boundaries, ABAC route registry, migration numbering, UTF-8 slicing, etc.) — none checks that documentation files stay current. No markdownlint, doc-link checker, or api-reference parity check exists in scripts/."
          },
          {
            "file": "scripts/check-cli-spec-parity.sh",
            "quote": "# Run during pre-commit and CI. Not exhaustive — supplements the",
            "detail": "The closest doc/spec-parity gate claims CI wiring in its header, but grep shows cli-spec-parity only in .pre-commit-config.yaml:35 — it is absent from .github/workflows/, so the doc-parity enforcement that does exist is pre-commit-only and bypassable (pre-commit is local)."
          },
          {
            "file": "specs/index.md",
            "quote": "| Manager Agent (superseded) | [`development/manager-agent.md`](development/manager-agent.md) | Superseded -- split into [`system/platform-model.md`](system/platform-model.md) + [`system/agent-runtime.md`](system/agent-runtime.md) §1 |",
            "detail": "Quality grades have no production support: grep for 'quality grade'/'quality_grade' in crates/ returns zero matches, and the only specs mentioning them are this section and the superseded manager-agent.md, whose Manager Agent (periodic quality-grade updates) does not exist in code. Combined with the zero matches for 'garden'/'gardening' across scripts/, crates/ and .github/ (inspected for any doc-gardening agent spawn path), both the quality-grades artifact and the doc-gardening-agent enforcement the section requires are missing."
          }
        ]
      },
      {
        "id": "fff96bf529e90ad943501101",
        "decision": "gap",
        "requirement_quote": "- All specs live in `specs/` with a `specs/index.md` entry point.\n- Think about spec structure in terms of **cache hits vs. misses**: if an agent tool call (search, read, etc.) finds what it needs on the first try, that's a cache hit. If it fails and requires additional lookups, that's a cache miss.\n- Specs should be structured and indexed to **maximize cache hits** - clear naming, cross-linking, predictable paths.",
        "detail": "Structurally strong but the entry point does not actually cover all specs, and nothing keeps it current. Realized: all specs live under specs/ in consistently kebab-case-named topic directories (system/, development/, personas/, milestones/, prior-art/), specs/index.md exists as the entry point with topic tables, cross-links and summaries, and pervasive cross-linking between specs (e.g. AGENTS.md's Key Architecture Specs table). Missing: the index is hand-maintained and already incomplete — active canonical specs specs/system/ui-navigation.md and specs/system/repo-lifecycle.md (and specs/system/ralph-loop.md) exist on disk but have no row in specs/index.md (grep of index.md for these filenames returns nothing; the only ralph row is the different, superseded development/ralph-loops.md). An agent whose first lookup goes through the specced entry point for UI navigation or repo lifecycle misses and must fall back to search — exactly the cache miss the section defines and says to avoid. Nothing mechanically validates index completeness or cross-link integrity: no script in scripts/ checks that every specs/**/*.md is reachable from specs/index.md. The runtime GET /api/v1/specs/index is a separate auto-generated surface built from the manifest ledger, not a validation of specs/index.md, and manifest registration enforcement (find_unregistered_specs, used to reject pushes adding unregistered specs) is policy-gated per repo and explicitly excludes index.md.",
        "evidence": [
          {
            "file": "specs/index.md",
            "quote": "This is the entry point for all Gyre specs. Read this first.",
            "detail": "The specced entry point exists, organized into topic tables (System / Development / Prior Art / Personas / Milestones / Open Questions) with cross-links and one-line summaries, and naming across specs/ is consistent and descriptive — the structure/naming/cross-linking practice the section asks for is genuinely present."
          },
          {
            "file": "specs/system/ui-navigation.md",
            "quote": "# UI Navigation Model",
            "detail": "This active canonical spec (it supersedes human-system-interface.md §1 and is linked from AGENTS.md's Key Architecture Specs table) has no row in specs/index.md — grep of index.md for 'ui-navigation' returns nothing. The same holds for specs/system/repo-lifecycle.md ('# Repo Lifecycle', defines repository CRUD lifecycle) and specs/system/ralph-loop.md. These are first-lookup cache misses through the specced entry point, the precise failure mode the section says to design against."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "// GET /api/v1/specs/index — auto-generated markdown index",
            "detail": "The only mechanically generated spec index is this runtime API handler, built from the manifest-driven ledger — a different surface from the repo's specs/index.md. Inspected scripts/ for any index-completeness or cross-link validation (orphan/reachable/link-check): none exists, and manifest-based registration enforcement (spec_registry.rs find_unregistered_specs, 'Excludes `index.md`, `prior-art/`, `milestones/`') is policy-gated per repo and does not validate index.md rows, so nothing keeps the hand-maintained entry point complete."
          }
        ]
      },
      {
        "id": "fd6e133d1621c46338a657b0",
        "decision": "gap",
        "requirement_quote": "- Encode **golden principles** in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs.\n- Technical debt is paid continuously in small increments, not in painful bursts.\n- Human taste is captured once, then enforced continuously on every line.",
        "detail": "The principle-encoding and continuous-enforcement halves are partially real; the recurring-cleanup-agents-opening-fix-up-MRs half is not. Real: the meta-spec system encodes principles machine-actionably — seed_builtin_meta_specs seeds Principle/Standard/Persona meta-specs, prompt_assembly injects all REQUIRED meta-specs into every agent prompt ordered persona → principle → standard → process and records them in provenance attestations, and the daily meta_spec_conformance_sweep job (registered in jobs.rs) compares the active set SHA against provenance and auto-creates drift-review tasks on drift. Missing: (1) only one Principle meta-spec is seeded ('conventional-commits') — the mechanism exists but the golden-principles corpus is minimal. (2) The spec patrols (spec_patrol.rs run_patrol with five real spec-graph checks; spec_lifecycle_patrol.rs with three more) run only via manually-invoked HTTP endpoints — their sole production callers are the patrol_spec_links/patrol_spec_lifecycle handlers; jobs.rs registers no patrol job, so no recurring cleanup scan over spec-graph deviations exists (the repo's own task-292 records this as not-started). (3) No fix-up MR path: grep for 'fixup'/'fix-up' across crates/ returns zero matches; the only automatic MR creation is create_mr_for_completion, tied to agent completion of an existing task — no code path opens a fix-up MR from a patrol or sweep finding (all MergeRequest::new uses in reconciliation.rs are test fixtures). A drift task can eventually flow task → scheduler-spawned agent → completion MR, but that is task-driven indirection, not cleanup agents opening fix-up MRs, and the patrol path creates only notifications/messages, never tasks or MRs.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "\"conventional-commits\",",
            "detail": "seed_builtin_meta_specs seeds meta-specs including this single MetaSpecKind::Principle entry ('All commits must follow Conventional Commits specification.') plus one Standard and personas. The machine-actionable encoding of golden principles exists as a mechanism, but the seeded Principle corpus is one entry — human taste is captured only for commit style."
          },
          {
            "file": "crates/gyre-server/src/prompt_assembly.rs",
            "quote": "//! The assembled set is also the attestation source: `meta_specs_used` records",
            "detail": "Continuous per-line enforcement is real: every agent prompt is assembled from all REQUIRED meta-specs (kind-ordered persona → principle → standard → process) at their pinned approved versions, and the injected set is recorded in provenance attestations — 'captured once, enforced continuously' is realized for whatever principles are registered."
          },
          {
            "file": "crates/gyre-server/src/reconciliation.rs",
            "quote": "task.labels = vec![DRIFT_REVIEW_LABEL.to_string(), \"auto-created\".to_string()];",
            "detail": "The daily meta_spec_conformance_sweep (registered in jobs.rs as a recurring job) detects drift between the active meta-spec set and provenance-recorded SHAs and auto-creates drift-review tasks — recurring deviation detection with durable task creation is real production behavior, the strongest satisfied fragment of this section."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "crate::spec_patrol::run_patrol(&state, now, threshold).await;",
            "detail": "The spec-graph patrol's only production callers are the manually-invoked HTTP handlers patrol_spec_links and patrol_spec_lifecycle (grep for run_patrol/run_and_escalate across crates/gyre-server/src shows only these handlers and tests). jobs.rs registers many recurring jobs (retention_cleanup, staleness checks, conformance sweep, pr_hygiene_check) but no patrol job — the patrol never runs on a schedule, so there is no recurring cleanup scan over spec-graph deviations."
          },
          {
            "file": "specs/tasks/task-292.md",
            "quote": "the patrol exists only as a manually invoked HTTP endpoint",
            "detail": "The repo's own task ledger records this exact gap as progress: not-started, requiring a scheduled spec-links patrol job, ABAC gating of the endpoint, and persona patrol inputs — confirming the recurring-cleanup integration is a known open gap, not an implemented behavior."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "/// Create the completion MR for an agent (agent-runtime.md §1 Phase 6 step 1).",
            "detail": "The only automatic MR creation path is agent-completion-driven (shared by POST /agents/:id/complete and the MCP gyre_agent_complete tool). Grep for 'fixup'/'fix-up' across crates/ returns zero matches, and all MergeRequest::new uses in reconciliation.rs are inside test fixtures — no code path opens a fix-up MR from a patrol or conformance-sweep finding."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "4dc88c1516f2135573d94cf255c246ef1b73ce7b3849683669a6543b21823e60"
      },
      {
        "file": "crates/gyre-adapters/src/compute/container.rs",
        "sha256": "d1d3728d3e77f9a4668a4d3cc10a7500d0fcf8e86408b0b84c5a1bbd6b3c6f84"
      },
      {
        "file": "crates/gyre-server/src/otlp_receiver.rs",
        "sha256": "5bb1373dd02c5662dd87a63119f969339c845252c2654be6160f1ab84d3cfde1"
      },
      {
        "file": "crates/gyre-cli/src/tui.rs",
        "sha256": "4399043949490ce626af85860ec59dd7aff0d441991ae8cbb924e74987021edc"
      },
      {
        "file": "web/tests/e2e/explorer-visual.spec.js",
        "sha256": "9cc60e2d707622ab5c8917a9a53930cdeb1f3ef38313e7f04d1002604dcf7d37"
      },
      {
        "file": "crates/gyre-server/src/api/agent_logs.rs",
        "sha256": "ca0ad410639963b93495c27d003243544301ddae448d13040f5b3896b70105e5"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "46d398754fa7d7921c396160262899d430e24de71bdd53c32e0e7f89c0329afb"
      },
      {
        "file": "crates/gyre-server/src/telemetry.rs",
        "sha256": "f3e284d763bd0fb382a6fdf7e5633737977a3f2f60ea5b62ff3e2c36f1165fd3"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "0b268bd680b9e5429ca191debfb2451f2841beb7b383cd1719a59dd70683dd68"
      },
      {
        "file": "scripts/check-task-commit-attribution.sh",
        "sha256": "ecbd0be553736f433bc7f99ec3eb8f96825c4d1779b8946a1f3dc8140d1436b6"
      },
      {
        "file": ".github/workflows/ci.yml",
        "sha256": "a58652ba210d38bd13b8ae0171c0deb7fad593b916d9e2977078a9962404b97a"
      },
      {
        "file": "scripts/check-cli-spec-parity.sh",
        "sha256": "3d85d8bdb17f1a6eef503835140e616871c8f3ddc36ca42f85c71d2376a73bd8"
      },
      {
        "file": "specs/index.md",
        "sha256": "27097f39e902ed477ea8d622035e2ace0e2117a501100adc41cda1f0572ac0fe"
      },
      {
        "file": "specs/system/ui-navigation.md",
        "sha256": "b031fda677818c313d459891259f0112dfa366f7c1b82b723acec2ea581f787b"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "701749a457338c0bff05f4c0da748f94759958d1140c9a68edfcf26aa5e0ab41"
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "sha256": "f02c2ec7b852145682f0e9c356de0be3098e336954d48aa7c368cfb98b6b8000"
      },
      {
        "file": "crates/gyre-server/src/reconciliation.rs",
        "sha256": "09ceb5008680b3517763d5b365e4b547d0e0118d10e078421a8d535c6a50ccdf"
      },
      {
        "file": "specs/tasks/task-292.md",
        "sha256": "4b0ddd4ccbd8038f90619c297cf1078d75a061b657d567de714a54d8edc600d6"
      },
      {
        "file": "scripts/check-arch.sh",
        "sha256": "cdc29bf13ee7fe088d4053e37b7364f71824329453f58d1357af41191f528d0c"
      },
      {
        "file": "Cargo.toml",
        "sha256": "2298e83b544633e37c0ce6f857ee4879924d6f18efcf798ab1a8179e373b1a90"
      },
      {
        "file": "scripts/check-stale-stub-markers.sh",
        "sha256": "6c00f7772408ebcdd159b4e320d9512d3d78edd9ed6e37195962a90b6dd43f06"
      }
    ]
  },
  "plan": {
    "audit": "e465f33e656971616345613f585e63ed4cf8fb5b2a5aacde6de1ac23ab970259",
    "tasks": [
      {
        "key": "agent-telemetry-query",
        "title": "Agent self-observability: durable per-agent logs, metrics, and traces with real query surfaces (REST + MCP)",
        "covers": [
          "c5b26c825b5cd17bd8cd4b61"
        ],
        "required_behavior": "Per specs/development/agent-experience.md §Observability for Agents: \"Agents query their own logs, metrics, and traces (LogQL, PromQL, TraceQL or equivalent)\" and \"Agents can reason about performance, errors, and behavior from instrumentation - not just code.\" Current state (verified at base 87377fe1): agent logs are a plain limit/offset tail over an in-memory AgentLogBuffer (crates/gyre-server/src/log_compression.rs, wired at lib.rs:2147) that a restart orphans — no filtering, search, or time range; GET /metrics (lib.rs:1014) exposes only server-process Prometheus totals with no per-agent dimension; server OTel spans export to an external operator-side collector (default http://localhost:4317) with no in-repo storage or query surface; the only agent-reachable trace data is the MR-scoped gate trace (GET /api/v1/merge-requests/:id/trace, MCP trace://{mr_id}); no MCP tool queries logs, metrics, or traces. Deliver: (1) Durable agent logs: a port-backed agent-log store (gyre-ports trait + mem and SQLite adapters via the existing store! wiring pattern; every adapter implementing the port enforces the trait's failure constraints per check-mem-port-contracts.sh; new migration uses the next unused 6-digit sequence number and portable SQL per check-migration-sql-portability.sh) that the existing append/get/stream handlers read and write — lines survive a server restart against durable storage, and the unparameterized query preserves today's limit/offset response shape. Never add the durable state as a bare Arc<Mutex<HashMap>> (check-in-memory-state-stores.sh). The store records structured lines (timestamp, level, message); the append API gains an optional level field (absent keeps today's raw-message behavior; legacy unclassified lines still return). (2) Structured log query (\"LogQL or equivalent\"): GET /api/v1/agents/:id/logs gains real query parameters — substring search and since/until time range (plus the level field) — executed in the repository against stored lines, not post-fetch truncation of a fixed tail. (3) Per-agent metrics (\"PromQL or equivalent\"): record real per-agent runtime series from events the platform already observes (heartbeats, gate runs with pass/fail outcomes, error signals, completions) in an agent-dimensioned store, and expose a per-agent metrics query returning named series with values; /metrics operator totals remain unchanged. (4) Agent-scoped traces (\"TraceQL or equivalent\"): capture per-agent runtime spans (session phases from spawn through heartbeat, gate run, and completion, with status and duration) into durable per-agent trace storage, queryable via GET /api/v1/agents/:id/trace with filtering by operation/status/time range — distinct from the existing MR-scoped gate trace. (5) MCP tools (e.g. gyre_query_logs, gyre_query_metrics, gyre_query_traces): read-only, NOT in the needs_write RBAC gate, returning the same data as the REST queries; any new route gets a RouteResourceMapping resolver entry (no exemption-file entries, no duplicates). Authorization scoping matches the REST routes: a caller sees telemetry only for agents within its tenant/workspace scope. (6) The \"reason about performance, errors, and behavior\" half is carried by the data: series and spans carry status/error/duration attributes derived from real recorded events, never hardcoded values standing in for measured ones.",
        "acceptance": [
          "After appending log lines and rebuilding AppState from the same durable database (restart equivalence), GET /api/v1/agents/:id/logs returns the exact lines; on current code the in-memory buffer returns none.",
          "With lines \"ERROR boom\" and \"INFO ok\" stored, a filtered query (level/substring) returns only the matching line, and a time-range query excludes lines outside the range; the unfiltered query returns today's shape.",
          "The per-agent metrics query returns series derived from recorded events: a heartbeat increments the heartbeat series, a failed gate run increments the failure series — values change with real events, and /metrics process totals are unchanged.",
          "GET /api/v1/agents/:id/trace returns spans with operation, status, and duration for the agent's runtime phases; a status=error filter returns only error spans.",
          "The MCP query tools are callable by an Agent-role principal for its own telemetry, a caller outside the telemetry's tenant/workspace scope is denied, and a tool call creates zero durable rows (row counts identical before/after).",
          "check-arch.sh, check-mem-port-contracts.sh, check-migration-versions.sh, check-migration-sql-portability.sh, check-in-memory-state-stores.sh, check-abac-route-registry.sh, and check-abac-exempt-handlers.sh all pass; no new exemption entries."
        ],
        "negative_probe": "Durability arm: POST /api/v1/agents/:id/logs with \"ERROR probe-line\", then recreate AppState against the same SQLite file and GET the logs — on current code the AgentLogBuffer is in-memory and the response is empty, so the probe fails before the task; after it, the line returns from storage. Query arm: store \"ERROR boom\" and \"INFO ok\" and request with the error filter — on current code LogsQuery (api/agent_logs.rs) has only limit/offset, so the filter parameter does not exist and cannot narrow the result; after the task only the error line returns. Metrics arm: query the per-agent metrics for an agent that has sent heartbeats — on current code metrics.rs contains only process-level gauges/counters with no agent dimension, so no per-agent heartbeat count can be returned; after the task it must. Trace arm: GET /api/v1/agents/:id/trace — on current code no such route exists (only /merge-requests/:id/trace), so the probe 404s; after the task it returns the agent's own spans.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "agent-sensory-access",
        "title": "Full sensory access: per-task observability stack, TUI visual snapshot testing, multi-viewport Web UI inspection (console/DOM/network), and a generic API-driving MCP tool",
        "covers": [
          "17fa0c307f57c2691d7eb0af"
        ],
        "required_behavior": "Per specs/development/agent-experience.md §Testing & Validation: boot/drive/validate per worktree is substantially real at base 87377fe1 (per-agent jj/git worktree recorded in agent_worktrees, ephemeral hardened containers, gates run test commands in a detached worktree at head_sha, agents receive GYRE_SERVER_URL + a pre-minted JWT), but the load-bearing full-sensory-access requirements are missing. Deliver: (1) Per-task observability stack (\"Ephemeral, isolated environments per agent task - including their own observability stack\"): each spawned agent task gets its own isolated telemetry scope built on the agent-telemetry-query task's per-agent log/metric/trace store — provisioned at spawn, reached over the agent's existing authenticated server channel (GYRE_SERVER_URL + JWT; containers keep the default --network=none, no new egress), with the reference runner (docker/gyre-agent/agent-runner.mjs) emitting its runtime telemetry (session phases, errors, durations) into its own scope; isolation is enforced by authorization — an agent's telemetry queries return only its own scope's data. (2) TUI visual snapshot testing: refactor crates/gyre-cli/src/tui.rs so view rendering is a pure function of app state onto a ratatui frame, and add snapshot tests that render via ratatui's TestBackend at a fixed terminal size and compare the rendered terminal output against committed baselines; the tests run under cargo test -p gyre-cli with no TTY, so agents validate TUI rendering per worktree/branch inside ordinary gate commands. (3) Web UI sensory access at multiple screen sizes/scales with console/DOM/network fidelity: (a) a read-only MCP tool (e.g. gyre_ui_inspect) that drives the headless Chromium the platform provisions for the e2e suite (path env-configurable) against the forge's web UI, authenticating the browser session as the calling principal, at caller-specified viewport sizes and device scale factors, and returns per-viewport screenshot data, captured console messages (errors and warnings included), serialized DOM state, and the network requests the page issued; the browser session is latency-bounded with a hard timeout and fails closed with an explicit error when Chromium is unavailable — never fabricated or empty captures; the tool performs navigation and capture only (no state-mutating interactions), so it stays out of the needs_write gate — pin that boundary explicitly. (b) Make the Playwright harness agent-invokable per worktree at multiple sizes: today web/playwright.config.js hardcodes baseURL http://localhost:2222 with the e2e token and webServer boots its own server, and no setViewportSize/deviceScaleFactor appears anywhere in web/tests — add env-overridable targeting of an already-running server (documented invocation; today's behavior unchanged when the env is unset) and viewport/scale configuration covering at least two screen sizes plus a device scale factor. (4) API driving/inspection (\"agents can drive and inspect every endpoint\"): a generic MCP tool (e.g. gyre_api_request) taking method, path, query, and body that executes the request against the forge's own API as the calling agent's principal — same authorization evaluation as a direct request (forward the caller's credentials on a loopback request or dispatch in-process with the caller's auth context), never elevated — and returns status, headers, and body; because the tool can perform writes via POST/PUT/DELETE, it MUST be added to the needs_write RBAC gate in mcp.rs (membership derived from the handler's effects per check-mcp-write-tools.sh). Any added HTTP client is latency-bounded (check-unbounded-external-http.sh).",
        "acceptance": [
          "Two agents having emitted disjoint telemetry, each agent's own-scope query returns only its data and excludes the other's — per-task isolation is enforced, not nominal.",
          "cargo test -p gyre-cli runs the TUI snapshot tests headlessly (no TTY); changing a rendered string in tui.rs makes the comparison fail with a rendered-output diff, and reverting makes it pass; baselines are committed to the repo.",
          "The MCP UI-inspect tool invoked with two viewport sizes and a scale factor returns per-viewport screenshot data plus console messages, DOM state, and network requests; a page that emits a console error surfaces that error in the captured console output; with the Chromium binary unavailable the tool returns an explicit failure, not empty captures.",
          "The Playwright suite runs against an externally started server when the new env var is set (and behaves exactly as today when unset), with at least two viewport sizes and a device scale factor exercised.",
          "gyre_api_request GET on an existing endpoint returns the real status/body; a ReadOnly-role caller is rejected by the needs_write gate for this tool; a request the principal is unauthorized for returns the same denial the direct REST call returns.",
          "check-mcp-write-tools.sh, check-abac-route-registry.sh, check-unbounded-external-http.sh, and check-arch.sh pass; no new exemption entries."
        ],
        "negative_probe": "TUI arm: edit a visible label in crates/gyre-cli/src/tui.rs and run cargo test -p gyre-cli — on current code no snapshot facility exists (tui.rs renders only to a live terminal via ratatui::init; TestBackend is used nowhere in the repo and no snapshot tooling is in Cargo.lock), so there is nothing that can fail; after the task the snapshot test must fail with the rendered diff and pass on revert. Web UI arm: call the MCP UI-inspect tool with sizes [1280x720, 375x667] and deviceScaleFactor 2 against a page that logs a console error — on current code the MCP catalog (mcp.rs tools list) has no UI-inspection tool and web/tests contains no setViewportSize/deviceScaleFactor usage, so the probe cannot pass; after the task the response must contain two per-size captures AND the console error entry, proving real browser capture rather than fabricated output. API arm: as a ReadOnly-role principal call gyre_api_request with a POST — on current code no such tool exists; after the task the needs_write gate must reject it, while an Agent-role caller's POST creates the real resource (row count changes), proving the tool drives real endpoints with the caller's own authorization rather than returning canned responses.",
        "dependencies": [
          "agent-telemetry-query"
        ],
        "existing_task": null
      },
      {
        "key": "spec-index-cache-hits",
        "title": "specs/index.md completeness and mechanical cache-hit enforcement: every spec reachable from the entry point, validated in CI and pre-commit",
        "covers": [
          "fff96bf529e90ad943501101"
        ],
        "required_behavior": "Per specs/development/agent-experience.md §Spec Organization & Cache Hit Optimization: \"All specs live in specs/ with a specs/index.md entry point\" and specs should be indexed to \"maximize cache hits - clear naming, cross-linking, predictable paths.\" The structure is real at base 87377fe1 (kebab-case topic directories, pervasive cross-linking), but the hand-maintained index is already incomplete — active canonical specs specs/system/ui-navigation.md, specs/system/repo-lifecycle.md, and specs/system/ralph-loop.md exist on disk with no row in specs/index.md — and nothing validates index completeness or cross-link integrity (no script in scripts/ checks that specs are reachable from the index). Deliver: (1) Complete specs/index.md: add rows in the existing table format with accurate summaries for every spec file under specs/ that lacks one, with the full missing set mechanically derived by scanning the tree (not just the three known misses); superseded and reference-only material (prior-art/) keeps the classification treatment the index already applies. (2) scripts/check-spec-index.sh — real, blocking validation with two arms: (a) completeness — every specs/**/*.md file has a row in specs/index.md (honoring the index's own documented classification for reference-only material); (b) integrity — every relative file link in specs/index.md resolves to an existing file. Violations print the file and the missing row or dead link and exit 1; the completed tree exits 0; no exemption entries in any frozen scripts/*-exemptions.txt. (3) Wire the script as a mandatory CI step in .github/workflows/ci.yml following the arch-lint job's conventions (never advisory, no `|| true`; additions only — coordinate with the ci-docs-release and architecture plans that also add ci.yml steps) and into .pre-commit-config.yaml per the existing hook conventions. The runtime GET /api/v1/specs/index manifest-ledger surface and manifest registration policy are out of scope (separate generated surface; do not change find_unregistered_specs behavior).",
        "acceptance": [
          "scripts/check-spec-index.sh exits 0 on the completed tree.",
          "Adding a new file specs/system/probe-spec.md with no index row makes the script exit 1 naming it; removing a file referenced by an index row makes it exit 1 naming the dead link.",
          "specs/index.md contains rows for specs/system/ui-navigation.md, specs/system/repo-lifecycle.md, and specs/system/ralph-loop.md with accurate summaries, and every other on-disk spec is likewise listed (mechanically derivable, not hand-audited).",
          "ci.yml runs the check as a mandatory step and the pre-commit wiring follows the existing hook pattern.",
          "No new entries in any frozen scripts/*-exemptions.txt."
        ],
        "negative_probe": "Run the completeness check against the current tree — on current code no such script exists, and the tree itself fails the requirement (grep of specs/index.md for ui-navigation, repo-lifecycle, and system/ralph-loop returns nothing, so an agent whose first lookup goes through the specced entry point for UI navigation or repo lifecycle misses and must fall back to search — exactly the cache miss the section says to avoid); after the task the script must exit 1 on that exact unmodified-tree condition until the rows are added, then 0. Integrity arm: delete the file behind any index link and run the script — it must exit 1 naming the dead link, proving it validates cross-link integrity against the filesystem rather than asserting that specs/index.md exists.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "fixup-mr-pipeline",
        "title": "Entropy management: golden-principles corpus plus an authorized fix-up pipeline converting patrol findings into fix-up tasks and fix-up MRs",
        "covers": [
          "fd6e133d1621c46338a657b0"
        ],
        "required_behavior": "Per specs/development/agent-experience.md §Entropy Management / Garbage Collection: \"Encode golden principles in the repo and run recurring cleanup agents that scan for deviations and open fix-up MRs\" and \"Human taste is captured once, then enforced continuously on every line.\" At base 87377fe1 the meta-spec mechanism is real (REQUIRED meta-specs injected into every prompt and recorded in provenance; daily meta_spec_conformance_sweep compares the active-set SHA against provenance and creates drift-review tasks) but the corpus is minimal — seed_builtin_meta_specs (lib.rs:2726) seeds exactly one Principle (conventional-commits) — and no fix-up MR path exists (grep fixup/fix-up across crates/ returns zero matches; the only automatic MR creation is create_mr_for_completion tied to task completion; the patrols create notifications/messages only). Deliver: (1) Golden-principles corpus: extend the builtin seed set with additional Principle meta-specs encoding golden principles as they are already normatively stated in canonical specs — at minimum the hexagonal boundary invariant (specs/development/architecture.md: gyre-domain depends only on ports + common) and the real-work/no-fakery rule (specs/GOAL.md) — each seeded REQUIRED following the existing conventional-commits pattern, with prompt text derived from the canonical spec wording (never invented), so prompt_assembly injects them into every agent prompt and provenance records them; the existing conformance sweep then continuously enforces the active set. (2) Fix-up pipeline: a production conversion function (e.g. ensure_fixup_task) that takes a patrol/sweep finding and idempotently creates a durable fix-up Task scoped to the finding's real workspace/repo — mirroring run_conformance_sweep's dedup (skip when an open task with the same title and scope exists), skipping and logging (never fabricating a \"default\" scope) when the finding's workspace cannot be resolved (check-fabricated-scope-defaults / check-scope-literal-defaults invariants) — labeled (e.g. \"fixup\", \"auto-created\") so the existing scheduler spawns a cleanup agent whose completion flows through the existing create_mr_for_completion: the fix-up MR path. (3) Producers, with authorization pinned per controller review feedback fd6e133d1621c46338a657b0:unauthorized-cross-tenant-fixup-write-path, correction option (b): the conversion call sites in the manual patrol HTTP handlers must establish caller identity and enforce per-handler authorization. patrol_spec_links (api/specs.rs:2137) currently declares only State and Json extractors — no caller identity at all — while the route sits in scripts/abac-route-registry-exemptions.txt (line 29) and so runs with NO middleware ABAC evaluation: add the auth extractor and, before creating any fix-up task, resolve the finding's scope (workspace, and repo via the spec ledger where the finding carries a spec path) and require the caller's tenant/workspace to contain it, returning Forbidden on mismatch; findings whose scope cannot be resolved are skipped and logged. This per-handler containment is additive to task-292's middleware-ABAC work on the same route (when task-292 lands, its resolver entry and exemption shrink provide middleware policy evaluation and the per-handler containment remains valid defense in depth); do not remove or add exemption-file entries here and do not add duplicate resolver entries. patrol_spec_lifecycle (already covered by middleware ABAC via the resolver entry at abac_middleware.rs:458) gains the conversion with fix-up creation scoped to the finding's resolved workspace — never a scope outside the finding's real one. System-principal producers (the doc-gardening job delivered by the doc-garden task, and the conformance sweep's existing drift-review tasks) create fix-up work as the system principal and need no caller containment. Coordination: task-292's scheduled spec-links patrol job is its own frozen contract (notification fan-out only) — do not modify it here; task-292 is not a dependency because the per-handler containment makes this task's wiring safe to land in either order.",
        "acceptance": [
          "Precondition on this criterion per the review feedback: the caller is authorized (within the finding's tenant/workspace scope) or is the system principal. Such a caller POSTing /api/v1/patrol/spec-links against state containing an error-severity finding gets exactly one durable fix-up task created for it — scoped to the finding's workspace/repo and labeled fixup — and a repeated invocation creates no duplicate (dedup by open-task title + scope); the fix-up task is spawnable by the existing scheduler and its completion produces a fix-up MR via create_mr_for_completion.",
          "A principal authenticated in a DIFFERENT tenant POSTing /api/v1/patrol/spec-links against the same state causes zero fix-up task creation (task row count unchanged before/after) and receives Forbidden.",
          "Findings with no resolvable workspace are skipped with a log line naming the finding; no task is created under a fabricated \"default\" scope.",
          "A fresh server boot seeds more than one REQUIRED Principle meta-spec (hexagonal boundary and no-fakery alongside conventional-commits), and an assembled agent prompt or its provenance attestation records all of them.",
          "scripts/check-abac-exempt-handlers.sh, check-abac-route-registry.sh, check-fabricated-scope-defaults.sh, check-scope-literal-defaults.sh, and check-arch.sh pass; the ABAC exemption file is byte-identical to its committed baseline after this task (no additions, no removals — task-292 owns the shrink)."
        ],
        "negative_probe": "Cross-tenant arm (the exact review-feedback probe): seed state with an error-severity stale_link in tenant A, authenticate a principal in tenant B, and POST /api/v1/patrol/spec-links comparing task row counts before/after — on current code there is no fix-up path at all (grep fixup in crates/ is empty; the handler's only side effect is notification fan-out), so the fix-up half of the probe cannot even execute; after this task the count must be unchanged and the response Forbidden, proving the per-handler containment gates the new durable, compute-spawning write. Corpus arm: inspect the seeded meta-specs on a fresh boot and the REQUIRED principles injected into an assembled prompt — on current code exactly one Principle (conventional-commits) exists, so an assertion over the corpus fails; after the task the hexagonal-boundary and no-fakery principles must appear in both the seed and the injected prompt/provenance. Dedup arm: invoke the patrol twice against the same error finding and count fix-up tasks — the second invocation must not create another row, proving idempotent conversion rather than unbounded task creation.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "doc-garden-quality-grades",
        "title": "Repository as system of record: in-repo quality grades, CI-enforced CLI/doc parity, and a recurring doc-gardening agent that opens fix-up work",
        "covers": [
          "07a6866179016f8676c1640d"
        ],
        "required_behavior": "Per specs/development/agent-experience.md §Repository as System of Record: \"Design docs, execution plans, architecture docs, quality grades - all versioned, all in-repo\" and \"Mechanical enforcement that docs stay current: linters, CI, doc-gardening agents.\" The artifact half is real at base 87377fe1 (AGENTS.md is the specced map-not-manual entry point; specs/, specs/tasks/, architecture docs and coverage matrices are versioned in-repo); three obligations are missing. Deliver: (1) Quality grades: a production surface that computes and versions quality grades in-repo — at merge completion the merge path computes a deterministic quality grade for the merged work from real recorded outcomes (gate results, review outcomes, verification status — data the MR record and gate executor already hold) and records it as a versioned in-repo artifact committed on the default branch (e.g. a structured per-repo quality ledger at a predictable path, appended/updated by the merge), retrievable via API and present in the repo tree; the grade is a function of the recorded outcomes — differing inputs yield differing grades, never a constant standing in for a computed value. (2) Mechanical docs-current enforcement in CI: scripts/check-cli-spec-parity.sh's header claims \"Run during pre-commit and CI\" but it appears only in .pre-commit-config.yaml and nowhere in .github/workflows/ci.yml — wire it into ci.yml as a mandatory step (never advisory, no `|| true`; additions only — coordinate with the ci-docs-release plan's check-doc-freshness.sh/check-i18n-coverage.sh steps and the architecture plan's ci.yml additions; do not modify or fork their scripts; exactly one CLI-parity script exists in scripts/). (3) Doc-gardening agent: a recurring job registered in jobs.rs (following the meta_spec_conformance_sweep registration pattern) running as the server-side system principal that scans the forge's stored repositories (default-branch tree content read via the existing git operations, for repos under spec governance) for documentation drift with mechanically checkable signals — at minimum: (a) broken cross-references: relative file links in tracked *.md files (AGENTS.md, docs/, specs/) pointing at paths that do not exist in the tree; (b) coverage-ledger drift: specs/coverage rows still marked not-started for sections whose owning task document (specs/tasks/*.md frontmatter) records terminal completion — and converts each finding into a fix-up task via the fixup-mr-pipeline's ensure_fixup_task (system principal, deduped, scoped to the workspace/repo the drifted artifact belongs to; unscopable findings are skipped and logged), so a recurring cleanup agent opens fix-up MRs for stale documentation. The scan evaluates real tree state, never a hardcoded file list.",
        "acceptance": [
          "A merge whose recorded gate/review outcomes are all green produces a quality-grade entry versioned in the repo (present in the default-branch tree and retrievable via the API), and a merge with materially different recorded outcomes produces a correspondingly different grade — the grade tracks the recorded outcomes.",
          "ci.yml runs scripts/check-cli-spec-parity.sh as a mandatory step; introducing a CLI-spec parity violation makes the step fail (verifiable by running the script directly).",
          "The doc-garden job is registered in the job registry and runs as the system principal; invoked against repo state containing a tracked *.md file with a relative link to a nonexistent path (and/or a coverage row still not-started for a section whose task document records complete), it creates exactly one scoped, deduped fix-up task via ensure_fixup_task; a second run creates no duplicate; clean state creates none.",
          "Doc-garden findings that cannot be scoped to a workspace/repo are skipped and logged, never created under a fabricated scope.",
          "check-arch.sh, check-in-memory-state-stores.sh (any durable job state goes through ports + an adapter), check-fabricated-scope-defaults.sh, and check-migration-versions.sh (if a migration is needed) pass; no new exemption entries."
        ],
        "negative_probe": "Quality-grades arm: complete two merges with materially different recorded gate/review outcomes and compare the recorded grades — on current code no quality-grade surface exists anywhere (grep for quality grade/quality_grade across crates/ returns zero matches), so there is nothing to compare; after the task the grades must differ consistently with the differing outcomes — a constant or input-independent grade fails this probe. Doc-garden arm: plant a tracked markdown file referencing ./nonexistent-target.md in a governed repo's default branch and run the job once — on current code no gardening code exists (grep garden/gardening across scripts/, crates/, and .github/ returns zero matches), so no fix-up task can appear; after the task exactly one fix-up task referencing the broken link must exist, and removing the link and re-running must not recreate it (proving evaluation of current tree state, not a remembered list). CI arm: temporarily introduce a CLI-spec parity violation and run the ci.yml step's command — on current code CI never invokes check-cli-spec-parity.sh (it appears only in .pre-commit-config.yaml), so a violating tree passes CI today; after the task the step must fail, proving the docs-current enforcement actually runs in CI.",
        "dependencies": [
          "fixup-mr-pipeline"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "agent-telemetry-query": "task-343",
    "agent-sensory-access": "task-344",
    "spec-index-cache-hits": "task-345",
    "fixup-mr-pipeline": "task-346",
    "doc-garden-quality-grades": "task-347",
    "_publication": "task-348",
    "_base": "973068a86ab3154b6cba448fda9369125326cab7"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed. The reviewed backlog (task-343 through task-347, each carrying its assigned section binding, required behavior, acceptance criteria, and negative probe) is published unchanged from the independently reviewed plan.

### Merge reconciliation

The previously published candidate conflicted with current main (main merged the specs/system/dependency-graph.md backlog as task-341/task-342, which also edits specs/task-spec-inputs.json). Resolved as a verbatim union: all five agent-experience `backlog/<section-id>` entries and their `bindings` keys from this branch, plus the dependency-graph entries from main, with every entry byte-identical to its parent and no existing binding rewritten (registry sections 329 shared + 5 + 1 = 335; bindings 272 + 5 + 1 = 278). The canonical spec specs/development/agent-experience.md and specs/coverage/ are untouched relative to both parents; no product file, gate, coverage status, or existing task changed. New tasks remain not-started.

### Verification evidence

- Resolved specs/task-spec-inputs.json parses as valid JSON and is exactly the union of both parents' entries (programmatic comparison: no missing, no extra, no rewritten entries).
- Each of the five task documents matches the reviewed plan verbatim: title, mapped depends_on (task-344 → task-343; task-347 → task-346), not-started progress, required_behavior, every acceptance criterion, negative_probe, and spec_ref binding.
- No duplicate active assignment: each of the five backlog section ids is referenced by exactly one binding in the registry.
- scripts/test_pipeline_spec_backlog.py (the durable stage chain that validates backlog publication manifests, including `validate_manifest`'s no-rewrite rules): 40 tests, all pass (`python3 -m unittest test_pipeline_spec_backlog` in scripts/, exit 0).
