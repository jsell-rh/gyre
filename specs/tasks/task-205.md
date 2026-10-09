---
title: "Manifest-driven Concept Views for the knowledge graph"
spec_ref: "realized-model.md §4 Concept Views"
depends_on: []
progress: not-started
coverage_sections:
  - "realized-model.md §4. Concept Views"
commits: ["3d460b0e97c73b7fc0a8e94072b0484e366c2b93", "6c557e95652b6ed9f72fa8ee25a36f6e64df90ba"]
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
