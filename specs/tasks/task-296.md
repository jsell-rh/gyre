---
title: "Publish reviewed spec backlog for specs/system/activity-dashboard.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/activity-dashboard.md",
  "plan_id": "edd4a0642e19668210bc61fe5c2e76afff5bd214e3cf964cb837ce1442604859",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "f0cbfae7cd02647d6b5042920573f4c2f7133ad835f8a42bff9af3cec3cb7696",
    "sections": [
      {
        "id": "dba01522c9fb7fb9aa3be898",
        "decision": "gap",
        "requirement_quote": "- **Real-time activity feed** showing what agents and users have been working on over any time period.\n- Each activity entry links to the relevant artifact - PR, commit, branch, test results, etc.\n- **\"Try it\" links:** For completed work, provide a way for users to spin up an ephemeral environment (NixOS + Tailscale) running that version of the code, so they can interact with and test what was built - without touching production.\n- This is the human window into the Ralph loop: see what's happening, attach to a TTY if needed, spin up a preview, give feedback.\n- Feeds the analytics system: what did users actually try? What did they approve or reject?",
        "detail": "The spec section (the whole document — one heading, five normative bullets) requires: (1) a real-time activity feed over any time period; (2) each entry linking to the relevant artifact (PR, commit, branch, test results); (3) 'Try it' links that spin up an ephemeral NixOS + Tailscale environment running the completed work's code version, isolated from production; (4) the dashboard as the human window into the Ralph loop — see what's happening, attach to a TTY, spin up a preview, give feedback; (5) feeding analytics with what users tried and what they approved or rejected. What exists: a feed endpoint (GET /api/v1/activity) synthesizing events from durable notifications, a workspace telemetry endpoint (GET /api/v1/workspaces/:id/activity) with membership enforcement and a `since` filter, and dashboard activity sidebars (WorkspaceHome, CrossWorkspaceHome) whose items navigate to MR/agent/task/spec entities — so a feed of agent/user work with artifact navigation partially exists, and approve/reject decisions do feed analytics (spec.approved, mr.merged, mr.closed, agent.completed/failed, task.status_changed). What fails the contract: (a) NOT real-time — the UI feed is a one-shot fetch per workspace-scope change with no polling and no push subscription; (b) 'any time period' unsupported — the unified handler ignores its `since` parameter (list_recent is limit-only), the client passes only limit, and the one since-filtered surface reads an in-memory evicting ring buffer and is never called by the SPA; (c) 'Try it' links entirely absent — no ephemeral-environment provisioning code exists anywhere ('Tailscale' appears nowhere in production code; the only 'NixOS' occurrence is a seed task title); (d) 'spin up a preview' absent for the same reason; (e) TTY attach exists server-side (/ws/agents/:id/tty) but the frontend helper agentTtyUrl is dead code with zero consumers, so the human window cannot attach to a TTY; (f) 'what did users actually try?' has no analytics data source because the try-it feature does not exist. Bullets (1)-(2) are partially implemented; bullets (3) and the preview/TTY-attach/analytics-of-trials parts of (4)-(5) are unimplemented, so the section is a gap.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "    let lim = params.limit.unwrap_or(30);\n\n    let notifs = state\n        .notifications\n        .list_recent(lim)\n        .await\n        .unwrap_or_default();",
            "detail": "The unified feed endpoint GET /api/v1/activity (crates/gyre-server/src/api/mod.rs:140) backs the dashboard feed from durable notifications, so a feed of agent/user work exists. But the handler's ActivityQueryParams declares `since: Option<u64>` and never reads it: only `limit` is used and NotificationRepository::list_recent is limit-only, so the feed cannot be queried over an arbitrary time period. The sibling workspace endpoint (workspace_activity_handler) does honor `since`, but it reads the in-memory TelemetryBuffer and is never called by the SPA."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "    nextLoadGen();\n    loadDecisions();\n    loadRepos();\n    loadSpecs();\n    loadRules();\n    loadTasks();\n    loadMrs();\n    loadAgents();\n    loadActivity();",
            "detail": "The dashboard's activity feed is NOT real-time. loadActivity() (defined at web/src/components/WorkspaceHome.svelte:1007) has exactly one call site: this $effect that fires once per workspace-scope change. There is no polling timer (no setInterval refreshes activityEvents) and no WebSocket subscription writes activityEvents; the shared /ws realtime channel (web/src/lib/notifications.svelte.js) updates only the notification bell/drawer, not the activity sidebar. CrossWorkspaceHome.svelte (lines 472-479) uses the same one-shot pattern. The feed is a snapshot taken at navigation time; new agent/MR/spec events do not appear until the user changes scope or reloads."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "  activity: (limit = 100) => request(`/activity?limit=${limit}`),",
            "detail": "The SPA's only activity-feed call passes just `limit` — no since/until time-period parameter exists on the client, matching the server handler that ignores its `since` field. Additionally there is no api.js function targeting /workspaces/:id/activity (the workspace telemetry endpoint with a working `since` filter), so the time-filtered surface is unreachable from the dashboard."
          },
          {
            "file": "crates/gyre-server/src/api/admin.rs",
            "quote": "        \"Set up NixOS flake for CI\",",
            "detail": "The 'Try it' links requirement is entirely unimplemented. This seed-data task title is the ONLY occurrence of 'NixOS' in production code (repo-wide grep of crates/ and web/src); 'Tailscale' appears nowhere in crates/ or web/src at all. Repo-wide search for 'try it', 'spin up', 'ephemeral environment', and 'preview environment' finds no feature code: the api/mod.rs route table registers no endpoint that provisions an environment running a given version of the code, and WorkspaceHome activity items navigate only to entity detail views. The WireGuard mesh (crates/gyre-server/src/api/network.rs, POST /api/v1/network/peers) registers peer public keys and allocates mesh IPs but is not connected to activity entries or code versions, and is not Tailscale/NixOS environment provisioning."
          },
          {
            "file": "crates/gyre-server/src/tty.rs",
            "quote": "//! GET /ws/agents/:id/tty — WebSocket terminal viewer for agent log output.",
            "detail": "A real server-side TTY stream exists (registered at crates/gyre-server/src/lib.rs:1077): it authenticates, replays buffered agent log lines, and streams new lines live. So 'attach to a TTY' is implemented at the transport layer — but not exposed in the human dashboard (see the dead frontend helper evidence), so the 'human window into the Ralph loop' cannot actually attach to a TTY from the UI."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "  // Agent TTY WebSocket URL\n  agentTtyUrl: (id) =>",
            "detail": "agentTtyUrl is dead code: a repo-wide grep of web/src for 'agentTtyUrl' returns only this definition (web/src/lib/api.js:292-293) and no component imports or calls it. No Svelte component opens the /ws/agents/:id/tty socket, so the dashboard provides no TTY-attach affordance; the server endpoint is unreachable from the product UI."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "                        nav(primaryType, primaryId, data);",
            "detail": "Artifact linking is partially satisfied: each sidebar activity item is a button that navigates to the referenced entity (mr / agent / task / spec, with repo_id context) via nav(), and notification rows (lines ~1481-1486) similarly deep-link to MR/agent/task/spec detail views where commits/branches/gates are shown. This covers PR/MR and indirect commit/branch/test-results navigation, but entries carry no direct commit/branch/test-result links of their own."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                let ev = AnalyticsEvent::new(\n                    new_id(),\n                    \"spec.approved\",",
            "detail": "The approve/reject half of the analytics feed exists: spec approval auto-emits the `spec.approved` AnalyticsEvent (with spec_path, approver_type, approval_mode), MR paths emit mr.merged / mr.closed (spec rejection closes spec-edit MRs with reason spec_rejected), and agent.completed / agent.failed / task.status_changed are auto-emitted elsewhere — human decisions land in the analytics store. However the 'what did users actually try?' half has NO data source: since the 'Try it' ephemeral-environment feature does not exist, no code records any try/preview/trial analytics event (no such event name exists at any state.analytics.record call site), so the analytics system cannot answer what users tried."
          },
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "    let messages = state.telemetry_buffer.list_since(&ws_id, since_ms, limit);",
            "detail": "The workspace-scoped activity endpoint does implement a `since` time filter (epoch-milliseconds, entries with created_at <= since excluded, plus limit), which is the only surface honoring 'over any time period' — but it queries the TelemetryBuffer, an in-memory bounded ring buffer that evicts oldest entries (per-workspace cap and workspace-count cap), so it cannot serve arbitrary historical periods after restart or eviction, and the SPA never calls this endpoint (no api.js client function exists for /workspaces/:id/activity)."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "/// In-memory ring buffer for Telemetry-tier messages.",
            "detail": "Confirms the telemetry-backed activity source is an in-memory ring buffer (per-workspace FIFO eviction, largest-workspace eviction), not durable storage — historical activity 'over any time period' beyond the buffer window is unavailable on this surface, and the buffer is lost on restart."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "dba01522c9fb7fb9aa3be898",
        "decision": "gap",
        "requirement_quote": "- **Real-time activity feed** showing what agents and users have been working on over any time period.\n- Each activity entry links to the relevant artifact - PR, commit, branch, test results, etc.\n- **\"Try it\" links:** For completed work, provide a way for users to spin up an ephemeral environment (NixOS + Tailscale) running that version of the code, so they can interact with and test what was built - without touching production.\n- This is the human window into the Ralph loop: see what's happening, attach to a TTY if needed, spin up a preview, give feedback.\n- Feeds the analytics system: what did users actually try? What did they approve or reject?",
        "detail": "The spec section (the whole document — one heading, five normative bullets) requires: (1) a real-time activity feed over any time period; (2) each entry linking to the relevant artifact (PR, commit, branch, test results); (3) 'Try it' links that spin up an ephemeral NixOS + Tailscale environment running the completed work's code version, isolated from production; (4) the dashboard as the human window into the Ralph loop — see what's happening, attach to a TTY, spin up a preview, give feedback; (5) feeding analytics with what users tried and what they approved or rejected. What exists: a feed endpoint (GET /api/v1/activity) synthesizing events from durable notifications, a workspace telemetry endpoint (GET /api/v1/workspaces/:id/activity) with membership enforcement and a `since` filter, and dashboard activity sidebars (WorkspaceHome, CrossWorkspaceHome) whose items navigate to MR/agent/task/spec entities — so a feed of agent/user work with artifact navigation partially exists, and approve/reject decisions do feed analytics (spec.approved, mr.merged, mr.closed, agent.completed/failed, task.status_changed). What fails the contract: (a) NOT real-time — the UI feed is a one-shot fetch per workspace-scope change with no polling and no push subscription; (b) 'any time period' unsupported — the unified handler ignores its `since` parameter (list_recent is limit-only), the client passes only limit, and the one since-filtered surface reads an in-memory evicting ring buffer and is never called by the SPA; (c) 'Try it' links entirely absent — no ephemeral-environment provisioning code exists anywhere ('Tailscale' appears nowhere in production code; the only 'NixOS' occurrence is a seed task title); (d) 'spin up a preview' absent for the same reason; (e) TTY attach exists server-side (/ws/agents/:id/tty) but the frontend helper agentTtyUrl is dead code with zero consumers, so the human window cannot attach to a TTY; (f) 'what did users actually try?' has no analytics data source because the try-it feature does not exist. Bullets (1)-(2) are partially implemented; bullets (3) and the preview/TTY-attach/analytics-of-trials parts of (4)-(5) are unimplemented, so the section is a gap.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "    let lim = params.limit.unwrap_or(30);\n\n    let notifs = state\n        .notifications\n        .list_recent(lim)\n        .await\n        .unwrap_or_default();",
            "detail": "The unified feed endpoint GET /api/v1/activity (crates/gyre-server/src/api/mod.rs:140) backs the dashboard feed from durable notifications, so a feed of agent/user work exists. But the handler's ActivityQueryParams declares `since: Option<u64>` and never reads it: only `limit` is used and NotificationRepository::list_recent is limit-only, so the feed cannot be queried over an arbitrary time period. The sibling workspace endpoint (workspace_activity_handler) does honor `since`, but it reads the in-memory TelemetryBuffer and is never called by the SPA."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "    nextLoadGen();\n    loadDecisions();\n    loadRepos();\n    loadSpecs();\n    loadRules();\n    loadTasks();\n    loadMrs();\n    loadAgents();\n    loadActivity();",
            "detail": "The dashboard's activity feed is NOT real-time. loadActivity() (defined at web/src/components/WorkspaceHome.svelte:1007) has exactly one call site: this $effect that fires once per workspace-scope change. There is no polling timer (no setInterval refreshes activityEvents) and no WebSocket subscription writes activityEvents; the shared /ws realtime channel (web/src/lib/notifications.svelte.js) updates only the notification bell/drawer, not the activity sidebar. CrossWorkspaceHome.svelte (lines 472-479) uses the same one-shot pattern. The feed is a snapshot taken at navigation time; new agent/MR/spec events do not appear until the user changes scope or reloads."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "  activity: (limit = 100) => request(`/activity?limit=${limit}`),",
            "detail": "The SPA's only activity-feed call passes just `limit` — no since/until time-period parameter exists on the client, matching the server handler that ignores its `since` field. Additionally there is no api.js function targeting /workspaces/:id/activity (the workspace telemetry endpoint with a working `since` filter), so the time-filtered surface is unreachable from the dashboard."
          },
          {
            "file": "crates/gyre-server/src/api/admin.rs",
            "quote": "        \"Set up NixOS flake for CI\",",
            "detail": "The 'Try it' links requirement is entirely unimplemented. This seed-data task title is the ONLY occurrence of 'NixOS' in production code (repo-wide grep of crates/ and web/src); 'Tailscale' appears nowhere in crates/ or web/src at all. Repo-wide search for 'try it', 'spin up', 'ephemeral environment', and 'preview environment' finds no feature code: the api/mod.rs route table registers no endpoint that provisions an environment running a given version of the code, and WorkspaceHome activity items navigate only to entity detail views. The WireGuard mesh (crates/gyre-server/src/api/network.rs, POST /api/v1/network/peers) registers peer public keys and allocates mesh IPs but is not connected to activity entries or code versions, and is not Tailscale/NixOS environment provisioning."
          },
          {
            "file": "crates/gyre-server/src/tty.rs",
            "quote": "//! GET /ws/agents/:id/tty — WebSocket terminal viewer for agent log output.",
            "detail": "A real server-side TTY stream exists (registered at crates/gyre-server/src/lib.rs:1077): it authenticates, replays buffered agent log lines, and streams new lines live. So 'attach to a TTY' is implemented at the transport layer — but not exposed in the human dashboard (see the dead frontend helper evidence), so the 'human window into the Ralph loop' cannot actually attach to a TTY from the UI."
          },
          {
            "file": "web/src/lib/api.js",
            "quote": "  // Agent TTY WebSocket URL\n  agentTtyUrl: (id) =>",
            "detail": "agentTtyUrl is dead code: a repo-wide grep of web/src for 'agentTtyUrl' returns only this definition (web/src/lib/api.js:292-293) and no component imports or calls it. No Svelte component opens the /ws/agents/:id/tty socket, so the dashboard provides no TTY-attach affordance; the server endpoint is unreachable from the product UI."
          },
          {
            "file": "web/src/components/WorkspaceHome.svelte",
            "quote": "                        nav(primaryType, primaryId, data);",
            "detail": "Artifact linking is partially satisfied: each sidebar activity item is a button that navigates to the referenced entity (mr / agent / task / spec, with repo_id context) via nav(), and notification rows (lines ~1481-1486) similarly deep-link to MR/agent/task/spec detail views where commits/branches/gates are shown. This covers PR/MR and indirect commit/branch/test-results navigation, but entries carry no direct commit/branch/test-result links of their own."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "                let ev = AnalyticsEvent::new(\n                    new_id(),\n                    \"spec.approved\",",
            "detail": "The approve/reject half of the analytics feed exists: spec approval auto-emits the `spec.approved` AnalyticsEvent (with spec_path, approver_type, approval_mode), MR paths emit mr.merged / mr.closed (spec rejection closes spec-edit MRs with reason spec_rejected), and agent.completed / agent.failed / task.status_changed are auto-emitted elsewhere — human decisions land in the analytics store. However the 'what did users actually try?' half has NO data source: since the 'Try it' ephemeral-environment feature does not exist, no code records any try/preview/trial analytics event (no such event name exists at any state.analytics.record call site), so the analytics system cannot answer what users tried."
          },
          {
            "file": "crates/gyre-server/src/api/activity.rs",
            "quote": "    let messages = state.telemetry_buffer.list_since(&ws_id, since_ms, limit);",
            "detail": "The workspace-scoped activity endpoint does implement a `since` time filter (epoch-milliseconds, entries with created_at <= since excluded, plus limit), which is the only surface honoring 'over any time period' — but it queries the TelemetryBuffer, an in-memory bounded ring buffer that evicts oldest entries (per-workspace cap and workspace-count cap), so it cannot serve arbitrary historical periods after restart or eviction, and the SPA never calls this endpoint (no api.js client function exists for /workspaces/:id/activity)."
          },
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "/// In-memory ring buffer for Telemetry-tier messages.",
            "detail": "Confirms the telemetry-backed activity source is an in-memory ring buffer (per-workspace FIFO eviction, largest-workspace eviction), not durable storage — historical activity 'over any time period' beyond the buffer window is unavailable on this surface, and the buffer is lost on restart."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/api/activity.rs",
        "sha256": "3224c9ccbfa6f00f3eec0c26a37b5fa7f28cb32b07c017d859e7aad3b705fc1b"
      },
      {
        "file": "web/src/components/WorkspaceHome.svelte",
        "sha256": "b589439a0c7061a509163fe09fa3e3e6a8b4b33e57b5aefcd997f6560f4cf0c3"
      },
      {
        "file": "web/src/lib/api.js",
        "sha256": "de3d8147c3ebaad027847809ebcf72a5ad6286e6983544f715931c307c927740"
      },
      {
        "file": "crates/gyre-server/src/api/admin.rs",
        "sha256": "4e9d39b02e035233018476d50af6543fff8f88aca74c5804114d1db37302225f"
      },
      {
        "file": "crates/gyre-server/src/tty.rs",
        "sha256": "e92d8c401c7ec86554fda1bcb64488bb093949daac44c01e181919b72ed81c89"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "b6391c5208b1cfe511c0d41b69f7e14e08472d2b597655e299dfcb7e0eac8ca0"
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "sha256": "dea344a7ab04b3f2bd754e1335f9143184c5a02508f0763ebd7638f17d735558"
      }
    ]
  },
  "plan": {
    "audit": "4f13bb362a55ec02b4150e6b8f53e395470b80b380c6aa77d9cd2c42afd81211",
    "tasks": [
      {
        "key": "activity-dashboard-completion",
        "title": "Activity Dashboard completion: real-time durable feed with time-period queries, artifact links, Try-it ephemeral preview environments, TTY attach, and trial analytics",
        "covers": [
          "dba01522c9fb7fb9aa3be898"
        ],
        "required_behavior": "Close specs/system/activity-dashboard.md (single section, audit gap dba01522c9fb7fb9aa3be898, failure modes (a)-(f)). All five bullets must hold in production code. (1) Real-time feed over ANY time period: (a) time period — the unified GET /api/v1/activity handler (crates/gyre-server/src/api/activity.rs) must honor its declared-but-ignored `since` query parameter (epoch seconds) against DURABLE storage: extend NotificationRepository (crates/gyre-ports/src/notification.rs) with a general time-filtered feed query (e.g. list_since(since, limit): created_at >= since, ordered created_at DESC — distinct from the existing type-specific list_by_workspace_type_since) implemented by EVERY adapter implementing the port (mem in crates/gyre-server/src/mem.rs, SQLite in crates/gyre-adapters/src/sqlite/notification.rs, PostgreSQL in crates/gyre-adapters/src/postgres/notification.rs — port-contract invariant), and make the handler execute the filter in the repository, not by post-fetch truncation; web/src/lib/api.js activity() forwards the since parameter. (b) real-time — the SPA activity sidebars (web/src/components/WorkspaceHome.svelte and CrossWorkspaceHome.svelte) must stop being one-shot snapshots per workspace-scope change: subscribe through the app's single shared /ws connection (web/src/lib/ws.js onMessage, the channel notifications.svelte.js already rides) and live-prepend bus Messages delivered for the subscribed workspace (the server already broadcasts telemetry/event-tier Messages via message_broadcast_tx, and ws.rs delivers them only to subscribed same-tenant members with per-workspace membership enforcement — that server-side isolation is existing behavior and must be preserved); map incoming messages to the same entry shape the /activity handler returns, dedupe by id, and on reconnect re-sync via GET /api/v1/activity?since=<newest entry timestamp>. (2) Artifact links: every activity entry referencing an artifact navigates to it — MR/agent/task/spec entries (existing nav()) plus gate-failure/test-result entries landing on the MR's gate output, with entries surfacing the artifact refs they carry (mr_id, repo_id, agent_id, task_id, spec_path, and any commit/branch/gate refs present in the notification body) as deep links; no feed entry with a resolvable artifact may be a dead end. (3) 'Try it' links — ephemeral preview environments for completed work: a durable PreviewEnvironment entity through gyre-ports (new port trait) + SQLite migration (next unused 6-digit sequence number; latest committed is 2026-11-20-000069_scim_group_member_display, so the next is 000070) + mem adapter, wired with the store! pattern — NEVER an in-memory Arc<Mutex<HashMap>> AppState field (durable-state invariant). REST surface: POST provisioning (MR-scoped, e.g. /api/v1/mrs/:id/try-it, for the MR's merged commit; a repo+ref form is acceptable) plus GET (list/status) and DELETE (teardown). Provisioning is real: dispatch through the existing compute-target machinery (spawn.rs dispatch_agent_launch / crates/gyre-ports/src/compute.rs ComputeTarget) to run a container built from the NixOS flake-based image path with a checkout of the artifact's commit, using isolated scratch state (its own temp data dir — it must NOT touch the production database/storage), and register the environment as a WireGuard mesh peer (network.rs register_peer, mesh IP allocation) so the requesting user can reach it over the platform's encrypted overlay network — the platform's realization of the specced 'NixOS + Tailscale' ephemeral environment; the behavioral contract is: ephemeral, isolated from production, running that exact version of the code, reachable by the user over the mesh. Lifecycle: status provisioning -> active -> expired/failed with a TTL expiry/teardown job on the existing jobs.rs scheduler that terminates the environment and removes its mesh peer. Every new route gets an ABAC RouteResourceMapping entry in abac_middleware.rs (registry invariant — no exemption-file entries) and per-handler workspace-membership/tenant scoping. UI: merged-MR activity entries and the MR detail surface render a 'Try it' action that provisions, shows live status, and tears down. (4) Human window into the Ralph loop — TTY attach must be reachable from the product UI: a TTY viewer component that opens the existing authenticated /ws/agents/:id/tty WebSocket (crates/gyre-server/src/tty.rs already replays buffered agent log lines then streams live output), mounted from agent surfaces (agent detail panel and/or activity entries referencing a running agent) via the currently-dead web/src/lib/api.js agentTtyUrl helper; 'see what's happening' = (1), 'spin up a preview' = (3), 'give feedback' already exists (notification inbox + approve/reject decisions) and must be preserved unchanged. (5) Feeds analytics — what users actually TRIED: provisioning records a real AnalyticsEvent via state.analytics.record (e.g. 'preview.started' carrying user_id, workspace_id, repo_id, mr_id/ref in properties) and teardown/expiry records 'preview.ended'/'preview.expired', making the trial half of the analytics feed real; the approve/reject half (spec.approved, mr.merged, mr.closed, agent.completed/failed, task.status_changed) already exists and must be preserved. Note on concurrent work: the assigned ui-layout plan (spec-72f70a7108d07f5ab2fb9e3e) also touches WorkspaceHome.svelte for the specs table/explorer surfaces — scope frontend edits in this task to the activity sidebar, TTY viewer, and try-it affordances to avoid collision; this is file-hygiene, not a behavioral dependency.",
        "acceptance": [
          "GET /api/v1/activity?since=T excludes durable notifications with created_at < T and includes newer ones, newest-first; the filter runs in the repository layer of every adapter implementing NotificationRepository (mem + SQLite + PostgreSQL), and web/src/lib/api.js activity() forwards since.",
          "With the dashboard open on workspace W, a bus message emitted for W (emit_telemetry / event emit) appears in the activity sidebar without reload or scope change, while a message for a different workspace does not (vitest feeding the shared ws store).",
          "POST try-it for a merged MR creates a durable PreviewEnvironment row (queryable after a server restart against SQLite) carrying the resolved commit SHA, drives provisioning through the compute-target machinery, registers a WireGuard mesh peer for the environment, and exposes status; DELETE and TTL expiry terminate it and remove the peer.",
          "A same-tenant non-member caller receives 403 on the try-it routes with no preview row and no mesh peer created; a request naming an unresolvable MR/ref receives 4xx with no side effects.",
          "A 'preview.started' AnalyticsEvent with user/workspace/repo/mr properties is recorded on provisioning and queryable through the analytics query API; teardown records the corresponding end event.",
          "An agent surface renders an Attach-TTY control that opens the /ws/agents/:id/tty socket and displays replayed then live lines (vitest asserts the socket URL from agentTtyUrl is actually consumed); agentTtyUrl is no longer dead code.",
          "Every activity entry with a resolvable artifact deep-links to that artifact's detail, including gate-failure/test-result entries landing on the MR's gate output."
        ],
        "negative_probe": "Primary probe (enforcement): seed a merged MR in workspace W; as a same-tenant user with NO membership in W, POST /api/v1/mrs/:id/try-it — the response must be 403 and a direct repository query must find zero preview_environment rows and no newly registered mesh peer. On the current tree the route does not exist (404 — probe fails outright); an implementation that provisions before scoping, or keeps environments in an in-memory map the repository query cannot see, also fails the probe. Companion probe (time period, real defect (b)): seed one durable notification older than T, then GET /api/v1/activity?since=T — the stale entry must be absent; the current handler ignores `since` (list_recent is limit-only), so the stale entry is returned and the probe fails until the repository-level time filter exists.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "activity-dashboard-completion": "task-295",
    "_publication": "task-296",
    "_base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Publication of the reviewed activity-dashboard backlog, rebased onto the updated main base (9e7cf62e) that carried the dependency-graph backlog (task-278..285).

### What was done

- Resolved the `specs/task-spec-inputs.json` merge conflict by keeping both sides: the base's dependency-graph backlog entries/bindings and this plan's `backlog/dba01522c9fb7fb9aa3be898` entry plus its `activity-dashboard-completion` binding. The resolved tree's diff vs 9e7cf62e is byte-identical in content to the original publication commit d8f37938 (+9 registry lines, task-295.md +92, task-296.md +211); `python3 -m json.tool` validates the registry.
- Created `specs/tasks/task-295.md` exactly per the reviewed plan: title, `depends_on: []`, `progress: not-started`, required behavior covering audit gap `dba01522c9fb7fb9aa3be898` (all five spec bullets, failure modes (a)-(f)), the seven acceptance criteria, and the verification-intent probes, unchanged.
- No product file, spec, existing task, coverage status, or gate was touched; no new coverage rows were marked.

### Audit challenge (per required behavior)

Re-read `specs/system/activity-dashboard.md` and spot-checked the audit's evidence against the current tree: the unified `activity_handler` declares `since: Option<u64>` and never reads it (only `limit` is used; `list_recent` is limit-only); `agentTtyUrl` in `web/src/lib/api.js:292` has zero consumers; "Tailscale" appears nowhere in `crates/` or `web/src`; no try-it/preview/ephemeral-environment route exists in `api/mod.rs`; the workspace `since`-filtered surface reads the in-memory `TelemetryBuffer`. The gap decision holds. The decomposition (one task covering the single-section spec) and empty dependencies are sound — the noted ui-layout overlap is file hygiene only. No duplicate active task targets this spec (only task-295 and this publication task reference it).

Task definitions and explicit bindings only; no product implementation claimed.
