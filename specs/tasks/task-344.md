---
title: "Full sensory access: per-task observability stack, TUI visual snapshot testing, multi-viewport Web UI inspection (console/DOM/network), and a generic API-driving MCP tool"
spec_ref: "Spec gap spec-8f4edcf188b12e2a350050e2 / agent-sensory-access / fd38a2fca0f8"
depends_on: [task-343]
progress: not-started
commits: []
---

## Required behavior

Per specs/development/agent-experience.md §Testing & Validation: boot/drive/validate per worktree is substantially real at base 87377fe1 (per-agent jj/git worktree recorded in agent_worktrees, ephemeral hardened containers, gates run test commands in a detached worktree at head_sha, agents receive GYRE_SERVER_URL + a pre-minted JWT), but the load-bearing full-sensory-access requirements are missing. Deliver: (1) Per-task observability stack ("Ephemeral, isolated environments per agent task - including their own observability stack"): each spawned agent task gets its own isolated telemetry scope built on the agent-telemetry-query task's per-agent log/metric/trace store — provisioned at spawn, reached over the agent's existing authenticated server channel (GYRE_SERVER_URL + JWT; containers keep the default --network=none, no new egress), with the reference runner (docker/gyre-agent/agent-runner.mjs) emitting its runtime telemetry (session phases, errors, durations) into its own scope; isolation is enforced by authorization — an agent's telemetry queries return only its own scope's data. (2) TUI visual snapshot testing: refactor crates/gyre-cli/src/tui.rs so view rendering is a pure function of app state onto a ratatui frame, and add snapshot tests that render via ratatui's TestBackend at a fixed terminal size and compare the rendered terminal output against committed baselines; the tests run under cargo test -p gyre-cli with no TTY, so agents validate TUI rendering per worktree/branch inside ordinary gate commands. (3) Web UI sensory access at multiple screen sizes/scales with console/DOM/network fidelity: (a) a read-only MCP tool (e.g. gyre_ui_inspect) that drives the headless Chromium the platform provisions for the e2e suite (path env-configurable) against the forge's web UI, authenticating the browser session as the calling principal, at caller-specified viewport sizes and device scale factors, and returns per-viewport screenshot data, captured console messages (errors and warnings included), serialized DOM state, and the network requests the page issued; the browser session is latency-bounded with a hard timeout and fails closed with an explicit error when Chromium is unavailable — never fabricated or empty captures; the tool performs navigation and capture only (no state-mutating interactions), so it stays out of the needs_write gate — pin that boundary explicitly. (b) Make the Playwright harness agent-invokable per worktree at multiple sizes: today web/playwright.config.js hardcodes baseURL http://localhost:2222 with the e2e token and webServer boots its own server, and no setViewportSize/deviceScaleFactor appears anywhere in web/tests — add env-overridable targeting of an already-running server (documented invocation; today's behavior unchanged when the env is unset) and viewport/scale configuration covering at least two screen sizes plus a device scale factor. (4) API driving/inspection ("agents can drive and inspect every endpoint"): a generic MCP tool (e.g. gyre_api_request) taking method, path, query, and body that executes the request against the forge's own API as the calling agent's principal — same authorization evaluation as a direct request (forward the caller's credentials on a loopback request or dispatch in-process with the caller's auth context), never elevated — and returns status, headers, and body; because the tool can perform writes via POST/PUT/DELETE, it MUST be added to the needs_write RBAC gate in mcp.rs (membership derived from the handler's effects per check-mcp-write-tools.sh). Any added HTTP client is latency-bounded (check-unbounded-external-http.sh).

## Acceptance Criteria

- [ ] Two agents having emitted disjoint telemetry, each agent's own-scope query returns only its data and excludes the other's — per-task isolation is enforced, not nominal.
- [ ] cargo test -p gyre-cli runs the TUI snapshot tests headlessly (no TTY); changing a rendered string in tui.rs makes the comparison fail with a rendered-output diff, and reverting makes it pass; baselines are committed to the repo.
- [ ] The MCP UI-inspect tool invoked with two viewport sizes and a scale factor returns per-viewport screenshot data plus console messages, DOM state, and network requests; a page that emits a console error surfaces that error in the captured console output; with the Chromium binary unavailable the tool returns an explicit failure, not empty captures.
- [ ] The Playwright suite runs against an externally started server when the new env var is set (and behaves exactly as today when unset), with at least two viewport sizes and a device scale factor exercised.
- [ ] gyre_api_request GET on an existing endpoint returns the real status/body; a ReadOnly-role caller is rejected by the needs_write gate for this tool; a request the principal is unauthorized for returns the same denial the direct REST call returns.
- [ ] check-mcp-write-tools.sh, check-abac-route-registry.sh, check-unbounded-external-http.sh, and check-arch.sh pass; no new exemption entries.

## Verification intent

TUI arm: edit a visible label in crates/gyre-cli/src/tui.rs and run cargo test -p gyre-cli — on current code no snapshot facility exists (tui.rs renders only to a live terminal via ratatui::init; TestBackend is used nowhere in the repo and no snapshot tooling is in Cargo.lock), so there is nothing that can fail; after the task the snapshot test must fail with the rendered diff and pass on revert. Web UI arm: call the MCP UI-inspect tool with sizes [1280x720, 375x667] and deviceScaleFactor 2 against a page that logs a console error — on current code the MCP catalog (mcp.rs tools list) has no UI-inspection tool and web/tests contains no setViewportSize/deviceScaleFactor usage, so the probe cannot pass; after the task the response must contain two per-size captures AND the console error entry, proving real browser capture rather than fabricated output. API arm: as a ReadOnly-role principal call gyre_api_request with a POST — on current code no such tool exists; after the task the needs_write gate must reject it, while an Agent-role caller's POST creates the real resource (row count changes), proving the tool drives real endpoints with the caller's own authorization rather than returning canned responses.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-experience.md`. Source: `87377fe145ebddf27ef975fba7829fb10f4bb136`; plan: `06fdb4a49c8ed342ae5f76200503e9e22b0b9b4ae513eb26ddd930d0be5ad861`.

[
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
  }
]
