---
title: "Manifest-driven Concept Views for the knowledge graph"
spec_ref: "realized-model.md §4 Concept Views"
depends_on: []
progress: complete
coverage_sections:
  - "realized-model.md §4. Concept Views"
commits: ["5d2c217b691ca6a6f3aea11205bad0fc8396a7f9", "a95c9df6f51de1d9b7d9424e091ca2e16f2d1954", "9aea8b653eb82b01f6cc715d3c964908410de98f", "e475fd802eb7eb68441b09eb5e630ad35aaa65a8", "8597bbec8fd1e7d2120736cb6e798373b2d32084", "0779ae15d6dcb17307d52e9cf694b38f75106c67", "e8b09b48e0ce4660918e22e0fdf392f993fae888", "3501ed421966653f03a3323d49967afcbbcd0a52", "d38ed2f9641ea29abda0ed80a1c157b70cd14722", "e4b55cdab1db1460297a327fac313e18efe0a712", "b88725880feef95d020e3e115a93064adbc95043", "d75afe7c1e3d163f64d5ad42a1edf8588cbb7a07", "2076ba76e29b8474c009daaa4147451ba49aa31f", "4052081908cf27a9d7abec9923fe24ee850f1a88", "860d06502d94bea226facb13620055e8f30b433a", "fcd83271fbb7b8dacaadf7709e4e67a892c26a1d"]
review: specs/reviews/task-205.md
---

## Spec Excerpt

> ### 4. Concept Views
>
> A concept view is a named, saved projection of the knowledge graph that cuts
> across modules. Concepts are defined in the spec manifest:
>
> ```yaml
> concepts:
>   - name: Authentication
>     description: "Token validation, RBAC, ABAC, JWT handling"
>     include:
>       - types: ["*Auth*", "*Token*", "*Rbac*", "*Abac*", "*Jwt*"]
>       - traits: ["*Auth*"]
>       - modules: ["*::auth*", "*::identity*"]
>       - endpoints: ["/api/v1/auth/*", "/.well-known/*"]
>       - specs: ["identity-security.md", "abac-policy-engine.md"]
>
>   - name: Merge Pipeline
>     description: "Merge queue, gates, MR lifecycle"
>     include:
>       - types: ["MergeRequest", "MergeQueueEntry", "Gate*", "QueueProcessor"]
>       - modules: ["*::merge*", "*::gates*"]
>       - specs: ["source-control.md", "agent-gates.md", "merge-dependencies.md"]
> ```

Spec API table (realized-model.md §7) further disambiguates the two concept
surfaces:

> | `GET /api/v1/repos/{id}/graph` | GET | Full knowledge graph for a repo (nodes + edges). Optional `?concept=` query param for **case-insensitive substring** filtering on node name/qualified_name (distinct from manifest-based `/graph/concept/:name`). |
> | `GET /api/v1/repos/{id}/graph/concept/{name}` | GET | **Concept view projection** |
> | `GET /api/v1/workspaces/{id}/graph/concept/{name}` | GET | Workspace-scoped concept search |

## Why This Is Open (current hollow state)

`crates/gyre-common/src/graph.rs:196` defines `ConceptView { name, description,
include_types, include_traits, include_modules, include_endpoints,
include_specs }` — but it is **never instantiated**. `get_graph_concept`
(`crates/gyre-server/src/api/graph.rs:657`) and its helper
`assemble_concept_results` (graph.rs:587) self-document:

> "In the full implementation this would use ConceptView definitions from the
> spec manifest. For now, it matches nodes whose `name` or `qualified_name`
> contains the concept name."

i.e. `/graph/concept/:name` currently does case-insensitive **substring** matching
identical to the `?concept=` query param — the manifest `concepts:` block is
never parsed and cross-module include-pattern projection does not exist. Coverage
matrix flags this section `not-started` (hollow).

## Implementation Plan

Real work only — no substring stand-in for the manifest projection.

1. **Parse the `concepts:` block from the manifest.**
   - In `crates/gyre-server/src/spec_registry.rs`, add a `ConceptDef` struct and
     a `#[serde(default)] pub concepts: Vec<ConceptDef>` field to `SpecManifest`
     (struct at spec_registry.rs:25). The manifest `include` value is a YAML
     sequence of single-key maps (`- types: [...]`, `- traits: [...]`, etc.);
     model it so each of `types`/`traits`/`modules`/`endpoints`/`specs` is
     collected into the corresponding `Vec<String>` (flatten the sequence of
     single-key maps into one `Include` aggregate). A concept with no matching
     keys yields empty vectors.
   - Provide a conversion `ConceptDef -> gyre_common::graph::ConceptView`
     (or populate `ConceptView` directly) so downstream code operates on the
     domain type.
   - Add unit tests in `spec_registry.rs` proving the two spec-example concepts
     parse into the exact expected include-pattern vectors, and that a manifest
     with **no** `concepts:` key parses (empty vec) rather than erroring.

2. **Implement glob matching.**
   - The include patterns use `*` wildcards only (`*Auth*`, `Gate*`,
     `*::merge*`, `/api/v1/auth/*`). Implement a small case-sensitive glob
     matcher (translate `*` → match-any, escape other regex metacharacters) OR
     reuse an already-vendored crate if one is in the workspace `Cargo.lock`
     (check before adding a dependency — do NOT add a new crate if `globset`
     or `glob` is already a transitive dep and usable). Unit-test the matcher:
     `Gate*` matches `GateApprovals` not `LateGate`; `*::merge*` matches
     `gyre_domain::merge::queue`; `MergeRequest` matches exactly and not
     `MergeRequestDependency` (no implicit wildcard).

3. **Project the knowledge graph through a ConceptView.**
   - Add a function (in `api/graph.rs` or a new `concept.rs` module) that, given
     the repo's `GraphNode`s/`GraphEdge`s and a `ConceptView`, returns the
     matched node/edge subgraph. Matching rules, keyed by `NodeType`
     (`crates/gyre-common/src/graph.rs:9`):
     - `include_types` → nodes with `node_type` in {`Type`, `Class`, `Enum`,
       `Struct`-equivalent} matched by glob against `name` (and/or
       `qualified_name`).
     - `include_traits` → nodes with `node_type` in {`Trait`, `Interface`}.
     - `include_modules` → nodes with `node_type` in {`Module`, `Package`}
       matched against `qualified_name`.
     - `include_endpoints` → nodes with `node_type == Endpoint` matched against
       `name`/`qualified_name` (the route path).
     - `include_specs` → nodes whose `spec_path`/`spec_paths` (or `GovernedBy`
       edge target) matches one of the listed spec paths.
     - A node is included if it matches **any** include rule (union). Edges are
       included when both endpoints are in the matched node set (reuse the
       existing edge-filtering logic in `assemble_concept_results`).

4. **Rewire the endpoints.**
   - `get_graph_concept` (`GET /api/v1/repos/:id/graph/concept/:concept_name`,
     registered at `api/mod.rs:853`): resolve the repo via
     `state.repos.find_by_id`, read `specs/manifest.yaml` at the repo's default
     branch via `spec_registry::read_git_file(&git_bin, &repo.path,
     &repo.default_branch, "specs/manifest.yaml")` (pattern used at
     `api/specs_assist.rs:159`), parse concepts, find the concept whose `name`
     matches `:concept_name` (case-insensitive exact match on the concept name),
     and return the projection from step 3. If no manifest concept matches the
     name, return **404** (a named concept view that isn't defined does not
     exist) — do NOT silently fall back to substring matching.
   - Keep the `?concept=` substring behavior on `GET /api/v1/repos/:id/graph`
     (`get_repo_graph`) and update the stale doc-comments on
     `assemble_concept_results`/`get_graph_concept` to reflect the new reality.
   - `get_workspace_graph_concept`
     (`GET /api/v1/workspaces/:id/graph/concept/:concept_name`, registered at
     `api/mod.rs:884`): apply the same manifest-driven resolution per repo in the
     workspace and union the results (each repo carries its own manifest; a repo
     lacking the named concept contributes nothing). This keeps the workspace
     endpoint a real concept-view projection rather than substring search.

5. **Do not regress** the existing `?concept=` substring path or the
   node→spec mapping used by `/graph/spec/:path`.

## Acceptance Criteria

- `SpecManifest` parses a `concepts:` block into `ConceptView`-shaped data;
  `ConceptView` is instantiated from real manifest content in production code
  (grep proves it is no longer dead).
- `GET /api/v1/repos/:id/graph/concept/:name` returns the union projection of
  nodes/edges matching the named concept's include patterns (types, traits,
  modules, endpoints, specs) — NOT a substring match — and returns 404 for an
  undefined concept name.
- `GET /api/v1/workspaces/:id/graph/concept/:name` returns the same projection
  unioned across the workspace's repos.
- The glob matcher honors anchoring (`MergeRequest` ≠ `MergeRequestDependency`;
  `Gate*` ≠ `LateGate`).
- A hard integration test seeds a repo with a `specs/manifest.yaml` containing
  the spec's `Authentication` concept plus graph nodes (an `Auth`-named `Type`,
  an unrelated `Type`, a module under `*::auth*`, an endpoint `/api/v1/auth/x`,
  and a node governed by `identity-security.md`) and asserts the endpoint
  returns exactly the concept-matching nodes and excludes the unrelated node.
  This test MUST fail if the endpoint reverts to substring matching.
- `cargo build --all` and the new/affected tests pass. Hexagonal boundary
  (`scripts/check-arch.sh`) unchanged — glob/projection logic lives in server or
  domain, not adapters; domain does not import adapters.

## Agent Instructions

- Read `specs/system/realized-model.md` §4 and §7 in full before coding.
- Confirm the route registrations at `crates/gyre-server/src/api/mod.rs:853` and
  `:884` before wiring handlers.
- Reuse `spec_registry::read_git_file` and `state.repos.find_by_id(...).path /
  .default_branch` — do not invent a new repo-path resolution.
- Do not add a new crate dependency if the workspace already vendors a usable
  glob matcher; otherwise a minimal hand-rolled `*`-glob is acceptable and
  preferred over pulling a heavy dependency.
- Skip formatters/linters and project-wide suites; run only `cargo build --all`
  and the tests you add/touch. Conventional commits; author unchanged.
- When done, set this task's `progress: complete`, record commit SHAs, and the
  reviewer will re-audit the coverage row.

## Shipped

- `specs/manifest.yaml` `concepts:` blocks parse into domain `ConceptView`s
  (five include-rule vectors, repeated-key aggregation) via
  `SpecManifest.concepts` + `From<&ConceptDef>`; repos without a concepts
  block parse cleanly.
- `GET /api/v1/repos/:id/graph/concept/:name` and
  `GET /api/v1/workspaces/:id/graph/concept/:name` return the
  manifest-driven union projection (types/traits/modules/endpoints/specs,
  anchored case-sensitive `*`-globs, edge-metadata route paths, GovernedBy
  spec linkage) with 404/no-contribution for undefined concept names —
  substring fallback removed; the `?concept=` substring query param on
  `/graph` is preserved as a distinct surface per spec §7.
- MCP `graph_concept` tool shares the same projection assembly (HSI §11
  parity) for repo and workspace scopes, with explicit errors for undefined
  concepts and unknown repos.
- Glob matcher + projection live in `gyre-common` (`glob_match`,
  `ConceptView::project`); no new crate dependencies; 16 new tests including
  integration tests a mutation probe confirms fail under substring revert.

### Rebase repair (assignment 5bdf4967)

- Resolved the rebase of this branch onto base fefde2946 (landed task-089
  HSI Explorer Generated Views and task-083 briefing repo-filter work). The
  single content conflict — `crates/gyre-server/src/api/graph.rs`, both sides
  appending distinct test sections at the end of the tests module — was
  resolved as a union: task-205's concept-view tests and the base's briefing
  repo-filter tests both kept; no code from either side dropped. All task-205
  production code (ConceptDef parsing, `glob_match`, `ConceptView::project`,
  `get_graph_concept`/`get_workspace_graph_concept`, MCP `graph_concept`)
  auto-merged intact; verified present and green at the resolved tree.
- Re-verification at the resolved HEAD (d099607f6):
  - `cargo build -p gyre-server` (SKIP_WEB_BUILD=1) → OK.
  - `cargo test -p gyre-server --lib -- api::graph::tests mcp::tests
    spec_registry::tests` → 176 passed, 0 failed (all 12 concept tests
    included).
  - `cargo test -p gyre-common --lib` → 120 passed, 0 failed.
  - `cargo test -p gyre-server --test graph_integration` → 38 passed, 0
    failed on a real TCP listener (base added one test).
  - Evidence: /tmp/stage/review-evidence/task205-rebase-resolution.md.
- Historical attribution for the earlier contract/scope repairs (re-owned
  realized-model §4 coverage flip commits da4ad02a5/c88fc0962, exemption-anchor
  fix 5d2c217b6) remains recorded in `commits:` above.
- No production behavior changed this round; `web/dist` untouched by this
  commit (staged dist changes come from the merge of the base and are restored
  by the executor from the assignment base).

## Repair round (checkpoint d415416e, exit 130)

The interrupted assignment left the implementation complete and an R1 review
on record (`specs/reviews/task-205.md`, verdict `complete`); what died mid-run
was investigation of a 35/35 `graph_integration` failure. Root-caused this
round as sandbox infrastructure, not code: `accept(2)` returns `EOPNOTSUPP`
(errno 95) in this runtime for every stack (Python `socket.accept` and a
minimal Rust/tokio `TcpListener::accept` probe both fail identically; `bind`
and `connect` succeed). Every test in that binary serves HTTP over a real
listener, so all 35 fail uniformly — including pre-existing tests untouched
by this task. Evidence: `/tmp/stage/review-evidence/task205-sandbox-tcp-accept-restriction.txt`
(probe source, outputs, and the exact host/CI commands required).

Repairs shipped in `2076ba76`:

- `scripts/check-task-commit-attribution.sh` failed at the merged HEAD
  because the merge base itself (`f4acb4eb`, landed task-189) is a
  task-labeled product-surface commit absent from task-189's frontmatter
  (which listed only the pre-squash pipeline SHAs). Recorded `f4acb4eb` in
  `specs/tasks/task-189.md` `commits:` — the check's documented remedy; no
  exemptions added, exemption file untouched. Gate now passes.
- `web/dist` churn reverted to the merge-base state: the hashed-file swap
  came from the embedded `npm run build` during checkpoint compiles, not
  from any web change (branch has zero `web/src`/`web/tests`/`package.json`
  delta; bundle string-literals are identical — only minifier-renamed
  identifiers differ). Task branches do not ship dist rebuilds.

Re-verification at repaired HEAD (`2076ba76`):

- `cargo build --all` OK.
- `cargo test -p gyre-server --lib -- api::graph::tests mcp::tests
  spec_registry::tests` → 142 passed (all 12 concept tests included).
- `cargo test -p gyre-common --lib` → 98 passed (4 concept_tests included).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`,
  `check-dead-message-kinds.sh`, `check-task-commit-attribution.sh` — all pass.
- TCP-listener integration tests (`cargo test -p gyre-server --test
  graph_integration`) cannot run in this sandbox (accept(2) restriction
  above); the exact commands are recorded in the evidence file for host
  verification and GitHub CI.
