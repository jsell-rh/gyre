# Review — task-282 (Spec-literal /api/v1/deps/* API surface)

Spec: `specs/system/dependency-graph.md` §API (gap `spec-cab97078efffe0fbfb7339be` / deps-spec-route-paths).
Candidate under review: `6448974234c683f56958a4eb5e22d0dbb0e65ff5` (branch `pipeline/task-282/d7581433f6cd4ece81261b056c7c32dd-1`, base `c42bfe033949ef04f8c4c41681126f116ab6a9de`; product commits 88309116, 53b8b419, f5703622, 2ae4dbed; process head 64489742).
Verdict: **needs-revision** — one blocking finding (F1: cross-tenant read via `?workspace=` on the graph endpoint) and one secondary finding (F2). Everything else verified clean by inspection, mechanical checks, `cargo check`, and a standalone routing probe.

## Round 1

Probes run (evidence: `/tmp/stage/review-evidence/`, summary in `task282-review.md`):

- `bash scripts/check-abac-route-registry.sh` → exit 0 (all ten `/api/v1/deps/*` routes resolve; no exemption-file growth — the diff touches no `scripts/` file).
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0 (86 handlers).
- `bash scripts/check-forged-scope-fields.sh` → exit 0 (`AddManualDependencyRequest` carries only repo identities and edge attributes).
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0 (9m34s).
- Standalone axum-0.7.9 routing probe replicating the task-282 route block verbatim: all ten spec-literal rows match (zero 404s), `?workspace=` query accepted, all pre-existing dependency routes still match (additions, not a rename), no matchit conflict panic, `DELETE /api/v1/deps/breaking` → 405 (static GET-only route shadows the flat `:id` param — documented, acceptable).
- Full diff read of all seven touched files.

Verified working (no findings):

- **Route registration.** All ten spec rows are registered in `api/mod.rs` backed by the real handler cores; existing routes (`/api/v1/dependencies/*`, `/api/v1/repos/:id/*`, `/api/v1/workspaces/:id/dependency-graph`) are untouched — the CLI/UI consumers keep working (routing probe confirms both surfaces answer).
- **ABAC resolver.** Ten new `RouteResourceMapping` entries, no duplicates, no exemption growth. The per-repo routes use `:repo_id`, which `extract_path_attributes` lifts into `resource.repo_id` → `set_workspace_and_tenant_from_repo` resolves the tenant, so the builtin tenant-isolation policy governs them exactly like the `/repos/:id/*` siblings.
- **`POST /api/v1/deps/manual`.** No caller-supplied tenant/workspace scoping fields; source repo validated by `require_repo_tenant_containment` (loads repo → workspace → tenant, Forbidden on mismatch, fail-closed NotFound on an unresolvable scope chain) and against repo/workspace-scoped token claims. Probe test covers same-tenant create, cross-tenant Forbidden with no edge created, repo-scoped-token Forbidden, and unknown-source 404.
- **`DELETE /api/v1/deps/{id}`.** Tenant containment on the edge's source repo runs BEFORE the manual-only refusal (no detection-method oracle for foreign tenants); auto-detected edge refused with InvalidInput and verified to survive; manual edge deleted for a same-tenant caller. This is the task's second required probe, proven at the handler level.
- **`POST /api/v1/deps/breaking/{id}/acknowledge`.** Per-handler tenant containment against the change's source repo (the old flat `/api/v1/dependencies/breaking/:id/acknowledge` core had none — the new path is strictly stronger); idempotent; probe test covers cross-tenant Forbidden with the change left unacknowledged.
- **Test repair round (2ae4dbed)** is sound: the workspace-scoped graph core serves the dedicated route's committed semantics (edges with at least one endpoint in the workspace, in place since d7940e85, pre-task-282); the repaired test now also asserts `?workspace=` ≡ dedicated route on identical fixtures, which is exactly the task contract ("the SAME graph").
- **Docs and coverage.** `docs/api-reference.md` documents all ten paths with accurate shapes; coverage row 15 re-evidenced with literal-path source locations and honestly notes test-execution deferral.
- **Attribution.** All four product commits are in the task's `commits:` frontmatter; the head process commit touches only the task file.

Findings:

- [-] **F1 (blocking — unsafe scope handling / cross-tenant disclosure): `GET /api/v1/deps/graph?workspace={id}` has no tenant containment.** `get_graph` (crates/gyre-server/src/api/dependencies.rs, this diff) gained a `GraphQuery` extractor and delegates to `workspace_scoped_graph`, but the handler takes NO auth extractor and performs NO tenant comparison — it loads any workspace by id (404 only when missing) and returns its graph. The ABAC middleware cannot close this: the matched patterns `/api/v1/deps/graph` and `/api/v1/dependencies/graph` carry no path params, there is no "dependency" arm in `extract_entity_attributes`, and the query string is never consulted for scope attributes, so `resource.tenant_id` stays unset; the builtin `tenant-isolation` policy (950, Deny) explicitly "fires only when BOTH tenant ids are known" and falls through, after which the role bands (`developer-write-access` 800, `readonly-get-only` 600) Allow the read. The dedicated `/api/v1/workspaces/:id/dependency-graph` route IS protected — its path segment yields `resource.workspace_id` → `set_tenant_from_workspace` → tenant-isolation Deny on cross-tenant callers — so the new query-parameter form delivers the same data while bypassing exactly that containment. Reproduction: authenticated tenant-A Developer token, `GET /api/v1/deps/graph?workspace=<tenant-B-workspace-id>` → 200 with tenant B's dependency graph (repo names, edges, versions, breaking changes); the dedicated route returns 403 for the same caller. Because both paths share `get_graph`, the leak also applies to `/api/v1/dependencies/graph?workspace=` (the Query extractor is new in this diff). The task's own contract requires per-handler authorization wherever the middleware cannot resolve the scoped entity ("load the scoped entity, compare tenant/workspace against the caller, Forbidden on mismatch") — the handler must take the caller, load the workspace, and compare `workspace.tenant_id` (mirroring `require_repo_tenant_containment`). The probe test `deps_graph_endpoint_workspace_query_scopes_the_graph` uses only tenant-1 fixtures, so it passes with the containment absent — a cross-tenant kill probe (expect 403, currently 200) must be added.
- [-] **F2 (secondary — scope handling): `POST /api/v1/deps/manual` validates the source repo's tenant but not the target's.** A tenant-A caller can declare an edge into a tenant-B repo (`target_repo_id` is checked for existence only), which then enters tenant-B's dependents, blast-radius, and cascade-test surface. This mirrors the pre-existing `add_dependency` core semantics that the task instructed the implementer to reuse (its source is path-protected, its target equally unvalidated), so it is not a regression introduced by the reuse — but the new flat handler already loads both repos and enforcing containment on both ends is a one-line strengthening that fits the check-forged-scope-fields rationale (the edge's scoping is only half-enforced). Fix together with F1.

Verifier must run (gyre-server test compilation exceeds the review sandbox's 4 GiB budget):

- `cargo test -p gyre-server --lib api::dependencies::tests` (four task-282 probe tests + dependencies regression cover);
- `cargo test -p gyre-server --test task164_dependency_graph_detail` (dedicated-route and tenant-wide graph cover);
- `cargo test -p gyre-server --test tenant_isolation` (regression);
- the F1 kill probe: router-level request, two-tenant fixtures, tenant-A token, `GET /api/v1/deps/graph?workspace=<tenant-B-ws>` must 403 and return no tenant-B edges — fails on this candidate (200 + cross-tenant graph).
