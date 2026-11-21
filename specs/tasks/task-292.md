---
title: "Spec Links — accountability integration: scheduled patrol job, ABAC-gated endpoint (exemption shrink), persona patrol inputs, and a strictly read-only MCP patrol tool"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-accountability-patrol / 5a915155d9bc"
depends_on: []
progress: ready-for-review
commits: ["aa1b2c3a6fe63d10641fb3cb8530ade3d56b8630", "d710d60a20fa7ccba16c208232588de4cfb65e74"]
---

## Required behavior

spec-links.md §Accountability Agent Integration: the five patrol checks are genuinely implemented over the real links store and ledger (spec_patrol.rs run_patrol: stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep depends_on chains), but the integration the heading requires is missing — the patrol exists only as a manually invoked HTTP endpoint, the accountability persona's documented patrol inputs list no spec-graph items, and the endpoint sits in the frozen ABAC exemption file (line 29) with a handler that performs no per-handler authorization at all (any authenticated principal in any tenant can trigger the forge-wide patrol and receive cross-tenant findings plus notification fan-out). Per controller review feedback, the MCP tool's side-effect boundary must be pinned exactly: run_patrol itself is side-effect-free (it only snapshots the links store and spec ledger and returns findings); the notification fan-out (create_notifications_for_error_findings → durable notification rows) lives in the HTTP handler patrol_spec_links (api/specs.rs:1915), not in run_patrol. Deliver: (1) Scheduled job: register a recurring spec-links patrol job in jobs.rs (mirroring the staleness job's registration at :493) that runs run_patrol and performs the notification fan-out for error findings, so the patrol runs on the accountability cycle without manual invocation. (2) Authorization: register /api/v1/patrol/spec-links in the ABAC resolver (RouteResourceMapping, mirroring /api/v1/patrol/spec-lifecycle at abac_middleware.rs:439) and remove the route from scripts/abac-route-registry-exemptions.txt — the exemption file must shrink; no new exemptions, no duplicate resolver entries — so the endpoint gets middleware policy evaluation instead of no authorization decision at all. (3) Persona patrol inputs: update specs/personas/accountability.md "What You Scan" to include the spec-graph surfaces (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep depends_on chains) so the accountability agent's documented patrol covers spec-graph awareness. (4) MCP tool (e.g. gyre_spec_patrol) exposing the patrol to the accountability agent: the tool wraps the pure run_patrol/findings computation ONLY — its handler must NOT call create_notifications_for_error_findings and must not create notifications, tasks, or any durable rows; notification fan-out remains exclusively with the scheduled background job and the ABAC-gated HTTP endpoint. Because the tool performs no repository writes it legitimately stays out of the needs_write RBAC gate — note check-mcp-write-tools.sh cannot catch a handler that fans out notifications via a helper (it scans only direct state.<repo>.<write> calls in mcp.rs), so the boundary is pinned here and must be asserted by test: read-only callers may invoke the tool.

## Acceptance Criteria

- [ ] The spec-links patrol job is registered in the job registry and, when due, runs the patrol and creates notifications for error findings without any manual invocation.
- [ ] /api/v1/patrol/spec-links is removed from scripts/abac-route-registry-exemptions.txt and has a RouteResourceMapping resolver entry; scripts/check-abac-route-registry.sh and scripts/check-abac-exempt-handlers.sh pass.
- [ ] The MCP tool returns the same findings run_patrol returns for the same state, is callable by a ReadOnly-role principal, and a tool call creates zero durable rows (notification and task row counts identical before/after).
- [ ] specs/personas/accountability.md "What You Scan" lists the spec-graph patrol items.

## Verification intent

As a ReadOnly-role principal, call the MCP patrol tool against state containing a stale link beyond threshold: the tool must return the finding AND leave the durable notification row count unchanged (before/after counts equal). If the handler were wired to the HTTP endpoint's fan-out — the exact hazard the review feedback flags, an ungated durable write invisible to check-mcp-write-tools.sh because the write happens inside create_notifications_for_error_findings — the row-count assertion fails while CI stays green. Second probe: a principal without spec access in another tenant calling POST /api/v1/patrol/spec-links receives a policy-denied response; on current code the route is exempt from ABAC and the handler has no auth extractor, so the call succeeds and returns cross-tenant findings.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Shipped

All four integration surfaces from the required behavior are implemented in production code. Source checkpoint `d710d60a` (retained from the prior assignment) carried the implementation; this finalization round applied `cargo fmt` to the newly added Rust code (whitespace only, no behavior change), extended the coverage row, and recorded attribution.

### Scheduled patrol job

`jobs.rs:497` registers `spec_links_patrol` (daily interval, enabled, mirrors the staleness job's registration) whose handler runs `spec_patrol::run_job_once` (`spec_patrol.rs:89`): the full five-check `run_patrol` with the shared `DEFAULT_STALE_THRESHOLD_SECS` (7 days) plus the `create_notifications_for_error_findings` fan-out — so error findings (unresolved conflicts, dangling implementations) reach Admin/Developer members on the accountability cycle with no manual invocation. Test `jobs.rs::spec_links_patrol_job_registered_runs_patrol_and_notifies` seeds a dangling `implements` link with resolvable workspace context, triggers the job through the registry's own run path, and asserts the Admin member received the error finding as a durable notification row.

### ABAC-gated endpoint (exemption shrink)

`/api/v1/patrol/spec-links` is registered in the ABAC policy resolver (`abac_middleware.rs:448`, `"spec"` resource — POST derives the `write` action), mirroring the spec-lifecycle patrol at `:439`, and removed from `scripts/abac-route-registry-exemptions.txt` (file 42→41 entries; `FROZEN_EXEMPTION_COUNT` tightened 46→41 to match — shrink only, no new exemptions, no duplicate resolver entries). A principal without spec write access is now policy-denied instead of receiving forge-wide findings plus notification fan-out. Test `abac_middleware.rs::spec_links_patrol_route_gets_policy_evaluation`: foreign-tenant ReadOnly principal → FORBIDDEN (policy decision), Admin → OK (not a blanket deny).

### Read-only MCP tool `gyre_spec_patrol`

Tool definition (`mcp.rs:373`), dispatch (`:5298`), handler `handle_spec_patrol` wraps ONLY the pure `run_patrol` computation — it never calls `create_notifications_for_error_findings` and creates no notifications, tasks, or durable rows; because it performs no repository writes it is legitimately outside the `needs_write` RBAC gate, so ReadOnly-role principals may invoke it. The side-effect boundary is asserted by test `mcp.rs::spec_patrol_tool_readonly_returns_findings_without_durable_writes`: a ReadOnly JWT calls the tool against state holding a stale link beyond threshold plus a dangling implementation with a seeded Admin fan-out recipient; the test asserts the tool returns exactly `run_patrol`'s findings for that state, that notification and task row counts are identical before/after, and that the Admin recipient received zero rows (the exact hazard `check-mcp-write-tools.sh` cannot see). `mcp_tools_list_includes_spec_patrol` asserts discovery via `tools/list`.

### Persona patrol inputs and docs

`specs/personas/accountability.md` "What You Scan" item 6 lists the five spec-graph surfaces (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep `depends_on` chains) and their delivery surfaces. `docs/api-reference.md` updated for the ABAC-gated endpoint + scheduled job and the new MCP tool. Coverage row 16 in `specs/coverage/system/spec-links.md` updated with the integration source evidence.

### Verification evidence and deferred tests

Probes run in this sandbox via the build wrapper (`/tmp/stage/bin/cargo`), log at `/tmp/stage/review-evidence/probe-log.md`:

- `cargo fmt --check -p gyre-server` — failed on the newly added test code, fixed with `cargo fmt -p gyre-server` (whitespace only), re-check exit 0.
- `cargo check -p gyre-server` — exit 0 (no warnings in new code).
- `cargo check -p gyre-server --tests` — exit 0 (new test modules type-check).
- `scripts/check-abac-route-registry.sh` — exit 0; `scripts/check-abac-exempt-handlers.sh` — exit 0 (89 handlers); `scripts/check-mcp-write-tools.sh` — exit 0 (7 write tools gated; `gyre_spec_patrol` correctly absent).

Deferred to the independent verifier (hard 4 GiB budget — gyre-server test-binary builds must not run in this sandbox):

    cargo test -p gyre-server spec_links_patrol_route_gets_policy_evaluation -- --nocapture
    cargo test -p gyre-server spec_links_patrol_job_registered_runs_patrol_and_notifies -- --nocapture
    cargo test -p gyre-server spec_patrol_tool_readonly_returns_findings_without_durable_writes -- --nocapture
    cargo test -p gyre-server mcp_tools_list_includes_spec_patrol -- --nocapture

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "ac4a8552cbca5b1333030655",
    "decision": "gap",
    "requirement_quote": "The Accountability agent's patrol gains spec-graph awareness:",
    "detail": "All five checks are implemented as real logic over the links store and spec ledger (stale links beyond a threshold, orphaned supersessions -- links targeting deprecated specs, unresolved conflicts -- both specs of a conflicts_with link approved, dangling implementations -- implements target absent from the ledger, and deep depends_on chains via memoized longest-path depth > 5). The gap is the integration the heading requires: the patrol exists only as a manually invoked HTTP endpoint. It is not part of any scheduled job (jobs.rs registers the staleness job and many others, but no spec patrol), the accountability persona's documented patrol inputs list no spec-graph items (specs/personas/accountability.md \"What You Scan\" covers specs, code, MRs, AGENTS.md, milestones only), and the endpoint is listed in the frozen ABAC route-registry exemption file while its handler performs no per-handler authorization at all -- patrol_spec_links declares no auth extractor, so any authenticated principal in any tenant can trigger the forge-wide patrol and receive cross-tenant findings plus notification fan-out (check-abac-exempt-handlers.sh passes only because its Rule 1 matches handlers that name an auth extractor; this handler names none).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/spec_patrol.rs",
        "quote": "\n    // 1. Stale links beyond threshold\n    check_stale_links(&links, now_secs, stale_threshold_secs, &mut findings);\n\n    // 2. Orphaned supersessions\n    check_orphaned_supersessions(&links, &ledger_map, &mut findings);\n\n    // 3. Unresolved conflicts\n    check_unresolved_conflicts(&links, &ledger_map, &mut findings);\n\n    // 4. Dangling implementations\n    check_dangling_implementations(&links, &ledger_map, &mut findings);",
        "detail": "The five specced patrol checks are genuinely implemented over the real links store and ledger snapshot."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "pub async fn patrol_spec_links(\n    State(state): State<Arc<AppState>>,\n    Json(req): Json<crate::spec_patrol::PatrolRequest>,\n) -> Json<crate::spec_patrol::PatrolResponse> {",
        "detail": "The only delivery surface is this handler: no auth extractor, no scope comparison -- combined with the ABAC exemption this means no authorization decision at all on the patrol endpoint."
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "quote": "/api/v1/patrol/spec-links",
        "detail": "/api/v1/patrol/spec-links is in the frozen ABAC exemption file, so it runs without middleware policy evaluation; per the repo invariant an exempt handler must enforce per-handler authorization, which the handler above does not."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "            |state| async move { crate::spec_link_staleness::run_once(&state).await },",
        "detail": "The job scheduler registers the staleness checker but no spec patrol job -- the patrol never runs on the accountability agent's cycle unless manually invoked."
      },
      {
        "file": "specs/personas/accountability.md",
        "quote": "On each patrol cycle, read and cross-reference:",
        "detail": "The accountability persona's patrol inputs enumerate specs, code, MRs, AGENTS.md and milestones -- no spec-graph items, so the agent's patrol prompt has no awareness of the spec-link checks."
      }
    ]
  }
]
