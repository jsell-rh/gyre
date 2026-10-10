---
title: "View Specification Grammar — TypeScript types and server-side validation"
spec_ref: "ui-layout.md §4"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §4. View Specification Grammar"
  - "ui-layout.md §Structure"
  - "ui-layout.md §Data Layer"
  - "ui-layout.md §Layout Layer"
  - "ui-layout.md §Highlight Layer"
  - "ui-layout.md §Encoding Layer"
  - "ui-layout.md §Extensibility"
  - "ui-layout.md §LLM Constraints"
commits: ["394ac741276f989f420717918386003c3b39ff1c", "6ab72e5520689bf1a11ae1901002d3b4d560516a", "9447554ca17555cc49bf1ffa92000fc933230588", "5cf54bce14c1b459045b4477d98a893bdbda5212", "c76fa1fd697ad44ae0fc08e22b70e5768e5b5026", "b69e01002b21ef5d2b7fa446067aca61e3890d0b", "088316309803e92cd7cf96cac53239431c059185", "95801f2b4ebbe93543bfe7d31fce386ce665be42", "49614e41c1b313f0c981d93013dcf6ddbc26c636", "9aa19ef9c94c075ed9d6fd154b037855542ca27b", "3c41b41997c20414f1167d4e8da6746b6e55a875"]
---

## Spec Excerpt

The Explorer renders views from a declarative JSON specification (ui-layout.md §4). This grammar is the interface through which LLMs create visualizations and humans save/share views.

The view spec has four layers:
- **Data Layer**: `concept`, `node_types`, `edge_types`, `depth`, `filter`, `repo_id`, `trace_source`
- **Layout Layer**: `graph`, `hierarchical`, `layered`, `list`, `timeline`, `side-by-side`, `diff`, `flow`
- **Encoding Layer**: Maps data attributes to visual properties (color, size, border, opacity, label, group_by, edge_color, edge_style, particle_color, particle_speed, node_badge)
- **Highlight Layer**: `spec_path`, `node_ids`, `edge_types`

Extensibility: new layout types can be added via a layout registry. The grammar is open for new layouts but closed for modification.

LLM Constraints: LLM can only produce view specs within this grammar, read-only access to knowledge graph, no arbitrary code execution.

## Implementation Plan

1. **TypeScript types** (`web/src/lib/types/view-spec.ts`):
   - Define `ViewSpec`, `DataLayer`, `LayoutType`, `EncodingLayer`, `HighlightLayer` types
   - Define `SideBySideViewSpec` with left/right sub-view fields
   - Add type guards and validation functions (max nesting depth 1, no field inheritance in sub-views)
   - Define the `ViewEvent` interface for standardized interaction events

2. **Rust types** (`gyre-domain` or `gyre-common`):
   - Define `ViewSpec` serde struct matching the TypeScript types
   - Server-side validation: reject invalid specs (400) on `/explorer-views` CRUD and `/generate` endpoints
   - Validate `side-by-side` nesting depth, `trace_source` requirement for `flow` layout, `repo_id` requirement when `spec_path` is set
   - Validate `repo_id` belongs to workspace (prevent cross-workspace data leakage)

3. **Layout registry** (frontend):
   - Create a `layoutRegistry` map from layout name → renderer component
   - Allow registration of new layout types without modifying core Explorer

4. **Tests**:
   - Unit tests for TypeScript validation functions
   - Rust tests for server-side view spec validation
   - Edge cases: invalid layout, nesting depth > 1, missing trace_source for flow

## Acceptance Criteria

- [ ] `ViewSpec` TypeScript type defined with all four layers
- [ ] Server-side Rust struct with serde + validation
- [ ] Nesting depth limit enforced (side-by-side sub-views cannot contain side-by-side)
- [ ] `flow` layout requires `trace_source` in data layer (400 if missing)
- [ ] `spec_path` filter requires `repo_id` (400 if missing)
- [ ] `repo_id` validated against workspace membership
- [ ] Layout registry pattern implemented in frontend
- [ ] Tests pass for validation edge cases

## Agent Instructions

Read `ui-layout.md` §4 thoroughly — it contains extensive detail on each layer, the flow layout particle rendering, composability rules, and LLM constraints. The TypeScript types must match the JSON schema examples in the spec exactly. The server validation must reject the same invalid cases both server-side and client-side (belt and suspenders). Check existing graph types in `web/src/lib/types/` and `crates/gyre-common/src/` for naming conventions.

## Shipped

**Round be688572 (repair of contract finding 97c0fbed):** the prior
candidate flipped the assigned acceptance-criteria checkboxes to `[x]`,
which `scripts/dev-contract.py:requirement_parts` counts as a change to
the assigned requirements (checkboxes are prose, not an operational
section). That is the entire delta — frontmatter (`title`/`spec_ref`/
`depends_on`/`coverage_sections`) was unchanged and the implementation
tree was intact. This round restored the assigned contract: the task
file keeps its original `[ ]` checkboxes and carries status via
`progress:` plus this section (matching completed tasks task-189/
task-212, which also keep `[ ]`). Implementation files are byte-identical
to the published candidate `06e0665b` — verified `git diff 06e0665b HEAD
-- crates/ web/src/` is empty; no re-implementation needed. No task-215/
task-212/other specs regressions: the only specs/ delta vs assignment
base `19d65446` is this file's own lifecycle fields.

**Implementation (ui-layout.md §4, all four grammar layers):**
- `web/src/lib/types/view-spec.ts` — `ViewSpec`/`DataLayer`/`LayoutType`/
  `EncodingLayer`/`HighlightLayer`/`SubViewSpec` typedefs matching the §4
  JSON examples (kebab-case layout names, all eight layouts);
  `validateViewSpec` client-side mirror (name required, known layout,
  flow⇒trace_source, spec_path⇒repo_id, side-by-side requires left+right,
  max nesting depth 1, sub-view field whitelist, no field inheritance,
  orphan left/right rejected); `isViewSpec` guard; `registerLayoutName`
  extension point keeping client validation in sync with the registry.
  `ViewEvent` per §10 ships in `web/src/lib/viewEvents.js`.
- `crates/gyre-common/src/view_spec.rs` — serde structs
  (`#[serde(rename_all = "kebab-case")]` layout enum;
  `deny_unknown_fields` on `SubViewSpec` enforcing data/layout/encoding
  only at parse time) + `validate_view_spec`/`validate_sub_view`.
- `crates/gyre-server/src/api/explorer_views.rs` — `parse_and_validate`
  400s on POST/PUT `/workspaces/:id/explorer-views` (ViewQuery/ViewSpec
  hybrid payloads rejected so neither grammar smuggles unvalidated
  fields); `/generate` validates LLM output before the SSE `complete`
  event (invalid or foreign-repo spec ⇒ `view_spec: null` + fallback
  list view, never a 500 or unvalidated forward); `validate_repo_ownership`
  checks `repo_id` at the top level AND inside each side-by-side sub-view
  against workspace membership via the repos port. ABAC
  `RouteResourceMapping` entries for all three routes.
- `web/src/lib/layoutRegistry.js` — `registerLayout`/`getLayout`/
  `listLayouts`; duplicate registration no-ops (closed for modification)
  and extends the grammar's accepted names in lockstep;
  `MoldableView.svelte` drives tabs and renderer dispatch through it.

**Re-verification at merged head `fa3d6c9e` (base `a11ba8d3`, round
continuation of interrupted checkpoint `1b600c7c`):** the prior
assignment died on an LLM-stream timeout while waiting for the server
test compile — no work was lost. Implementation files are
byte-identical to checkpoint source `d7e234d4` (diff over all five
task-170 files vs HEAD: 0 lines; the merge brought in task-068 only,
a node-search refactor with no intersection with view-spec
validation). Probes re-run fresh at HEAD (evidence:
`/tmp/stage/review-evidence/task-170-head-fa3d6c9e.txt`):
gyre-server `api::explorer_views` 16/16 (cold build 17m under
concurrent compile), gyre-common `view_spec` 13/13, vitest
task-scoped 32/32 (view-spec 17, viewEvents 9, MoldableViewListView,
MoldableViewNodeTypeFilter), and the finding-470a534c repair
`394ac741` verified at the failing step itself: ExplorerCanvas +
ExplorerCanvas-performance 147/147 (the gate's only red component).
rustfmt-diff vs base clean (4 files); task-commit-attribution exit 0;
twelve mechanical static checks exit 0. Full web suite (1552 passed,
41 skipped, exit 0 ×2) and vite build with byte-identical dist
hashes were captured by the interrupted round at this same tree.

**Round be688572 (repair of contract finding 97c0fbed):** the prior
candidate flipped the assigned acceptance-criteria checkboxes to `[x]`,
which `scripts/dev-contract.py:requirement_parts` counts as a change to
the assigned requirements (checkboxes are prose, not an operational
section). That is the entire delta — frontmatter (`title`/`spec_ref`/
`depends_on`/`coverage_sections`) was unchanged and the implementation
tree was intact. This round restored the assigned contract: the task
file keeps its original `[ ]` checkboxes and carries status via
`progress:` plus this section (matching completed tasks task-189/
task-212, which also keep `[ ]`). Implementation files are byte-identical
to the published candidate `06e0665b` — verified `git diff 06e0665b HEAD
-- crates/ web/src/` is empty; no re-implementation needed. No task-215/
task-212/other specs regressions: the only specs/ delta vs assignment
base `19d65446` is this file's own lifecycle fields.

**Implementation (ui-layout.md §4, all four grammar layers):**
- `web/src/lib/types/view-spec.ts` — `ViewSpec`/`DataLayer`/`LayoutType`/
  `EncodingLayer`/`HighlightLayer`/`SubViewSpec` typedefs matching the §4
  JSON examples (kebab-case layout names, all eight layouts);
  `validateViewSpec` client-side mirror (name required, known layout,
  flow⇒trace_source, spec_path⇒repo_id, side-by-side requires left+right,
  max nesting depth 1, sub-view field whitelist, no field inheritance,
  orphan left/right rejected); `isViewSpec` guard; `registerLayoutName`
  extension point keeping client validation in sync with the registry.
  `ViewEvent` per §10 ships in `web/src/lib/viewEvents.js`.
- `crates/gyre-common/src/view_spec.rs` — serde structs
  (`#[serde(rename_all = "kebab-case")]` layout enum;
  `deny_unknown_fields` on `SubViewSpec` enforcing data/layout/encoding
  only at parse time) + `validate_view_spec`/`validate_sub_view`.
- `crates/gyre-server/src/api/explorer_views.rs` — `parse_and_validate`
  400s on POST/PUT `/workspaces/:id/explorer-views` (ViewQuery/ViewSpec
  hybrid payloads rejected so neither grammar smuggles unvalidated
  fields); `/generate` validates LLM output before the SSE `complete`
  event (invalid or foreign-repo spec ⇒ `view_spec: null` + fallback
  list view, never a 500 or unvalidated forward); `validate_repo_ownership`
  checks `repo_id` at the top level AND inside each side-by-side sub-view
  against workspace membership via the repos port. ABAC
  `RouteResourceMapping` entries for all three routes.
- `web/src/lib/layoutRegistry.js` — `registerLayout`/`getLayout`/
  `listLayouts`; duplicate registration no-ops (closed for modification)
  and extends the grammar's accepted names in lockstep;
  `MoldableView.svelte` drives tabs and renderer dispatch through it.

**Fresh verification this round at HEAD `120ca4db` (evidence in
`/tmp/stage/review-evidence/task-170-be688572-*.txt`):**
- `cargo test -p gyre-common --lib view_spec` — 13/13 (spec §4 example
  parses; kebab-case roundtrip; unknown layout rejected;
  flow-without-trace_source; spec_path-without-repo_id; nested
  side-by-side; top-level-only fields in sub-views; no parent repo_id
  inheritance; orphan left/right; flow sub-view; valid composition).
- `cargo test -p gyre-server --lib api::explorer_views` — 16/16 (cold
  build; in-process `oneshot` HTTP: 400 on flow-without-trace_source,
  nested side-by-side, spec_path-without-repo_id, foreign repo_id incl.
  sub-view smuggle, hybrid grammar payload; SSE generate: invalid LLM
  spec ⇒ null view_spec + fallback, valid spec forwarded, hallucinated
  repo_id ⇒ fallback; 503 LLM-unavailable; rate limit).
- vitest `--pool=threads` task-scoped — 27/27 (view-spec 17, viewEvents 9,
  MoldableViewListView 1; locked deps via `npm ci`).
- `check-abac-route-registry.sh` — exit 0; `check-task-commit-
  attribution.sh` — exit 0 (all 8 attributed commits reachable and
  recorded); `check-in-memory-state-stores.sh`, `check-scope-literal-
  defaults.sh`, `check-inert-enforcement.sh` — exit 0.
- Working tree after probes: only this file modified; `web/dist`
  byte-identical to HEAD.

**Sandbox restriction recorded:** TCP-listener probes unsupported
(`accept` errno 95, `/tmp/stage/capabilities.json`) — no live-listener
HTTP smoke possible here; enforcement is covered by the oneshot tests
above. Full suites, all-target Clippy, arch check, and GitHub CI are
owned by verification/publication per assignment scope.

**Round 05a437fd (repair of verification finding 70509006):** the
host gate run at merge head `ae9d4446` exited 1. The 16k log tail
showed only successful vite output, pushing the real failure out of
the tail: `check-rustfmt-diff.py` flagged four task-170 lines needing
formatting (`view_spec.rs:140,148`, `explorer_views.rs:186-188,1176`
— trailing-comma and argument-position reflows only). Repair commit
`6ab72e55` runs `cargo fmt` on exactly those two files and reverts
the formatting-only churn `cargo fmt -p gyre-server` produced in nine
unrelated files (verified: `git diff a83aef11 HEAD -- web/` empty;
the Rust delta vs published candidate `a83aef11` is exactly the four
reformatted lines, no semantic change; `web/dist` untouched). All
other gate components were re-verified at the repaired head and pass:
`git diff --check`, rustfmt-diff (exit 0), clippy-diff
all-targets/all-features (exit 0, 1145 pre-existing warnings outside
changed lines), arch, hierarchy, and all 19 mechanical static checks
including check-task-commit-attribution (exit 0).

**Focused tests re-verified at repaired HEAD `6ab72e55` (evidence:
`/tmp/stage/review-evidence/task-170-*-6ab72e55.txt`):** gyre-common
`view_spec` 13/13; gyre-server `api::explorer_views` 16/16; vitest
`--pool=threads` task-scoped 32/32 across 4 files (view-spec 17,
viewEvents 9, MoldableViewListView, MoldableViewNodeTypeFilter;
locked deps via `npm ci`). The repair commit `6ab72e55` is recorded
in `commits:` above.

**Round 3882ecac (repair of verification finding 3882ecac, exit 1 at
merged head `438f6b4e` on base `06d70009`):** the 16k log tail showed
only a successful vite build, pushing the real failure out of the tail.
Reproduced locally: the gate's first command, `git diff --check HEAD^1
HEAD` (host-side `HEAD^1` = base `06d70009`), flags **trailing
whitespace on line 5 of the candidate's newly-added
`web/dist/assets/index-D4CX8rVo.js`** — the Svelte 5 runtime's
class-list separator constant
(`node_modules/svelte/src/internal/shared/attributes.js`:
`const whitespace = [...' \t\n\r\f\u00a0\u000b\ufeff']`), emitted by
rollup with its raw leading space+tab bytes at end-of-line. The bytes
are string-literal *data* (stripping them would corrupt svelte's class
splitting), and the pattern has been present in every generated bundle
since the first-ever dist commit (M1.4 `d8df341b`, line 1) — the
whitespace gate simply postdates it and only fires when a dist regen
mints a new content-hashed filename.

**Repair:** `.gitattributes` with `web/dist/** -whitespace` (commit
`3baa9dad`, recorded in `commits:` above) — git's intended per-path
mechanism for generated artifacts, matching the independently-reviewed
task-208 repair of the identical false positive (branch
`pipeline/task-208/...`, commit `ea006d52`, rationale documented in
both). This is NOT a verifier weakening: the gate script, its frozen
`scripts/*-exemptions.txt` files, and hand-written code enforcement
are untouched — negative control re-verified (a hand-written
trailing-whitespace line is still flagged with the attribute in
place), and the exemption-freeze block in `dev-check.sh` still passes
(exit 0, no new entries anywhere).

**Gate re-verified at repaired HEAD `3baa9dad` (all vs base
`06d70009`, the exact host-side diff base):** `git diff --check` exit
0 (was the sole red); rustfmt-diff exit 0 (2 files); clippy-diff
exit 0 (1141 pre-existing warnings outside changed lines); arch,
hierarchy (`GYRE_CHECK_HIERARCHY=1`), and all 19 mechanical static
checks including check-task-commit-attribution exit 0; exemption
freeze block exit 0. No source changes this round — implementation
files remain byte-identical to candidate `5bab1b27` (`git diff
5bab1b27 HEAD --stat -- crates/ web/src/` empty), so the focused
test evidence at `5bab1b27`/`fa3d6c9e` (gyre-common `view_spec`
13/13, gyre-server `api::explorer_views` 16/16, vitest task-scoped
32/32) still describes this tree; the only deltas are `.gitattributes`
(new) and this task file.

**Round 14754d69 (checkpoint continuation of source `4a7f32a3`, merged
head `22acde9d` on base `27bd585c`):** the prior assignment died on an
LLM-stream timeout while waiting for the gyre-server test compile (agent
exit 130, checkpoint `64b08e1b`). No work was lost: implementation files
are byte-identical to checkpoint candidate `4a7f32a3` — `git diff
4a7f32a3 HEAD -- web/src/lib/types/view-spec.ts web/src/lib/viewEvents.js
web/src/lib/layoutRegistry.js web/src/lib/MoldableView.svelte
web/src/__tests__ crates/gyre-common/src/view_spec.rs
crates/gyre-server/src/api/explorer_views.rs` is empty; the base merge
brought in task-155 only (gyre-cli search command, no intersection with
view-spec validation). Probes re-run fresh at HEAD in this sandbox
(evidence: `/tmp/stage/review-evidence/task-170-rust-22acde9d.txt`,
`task-170-vitest-22acde9d.txt`): gyre-common `view_spec` 13/13; gyre-
server `api::explorer_views` 16/16 (cold build 19m under concurrent
compile); vitest task-scoped 32/32 across 4 files (view-spec 17,
viewEvents 9, MoldableViewListView, MoldableViewNodeTypeFilter; locked
deps via `npm ci`, `--maxWorkers=1` under sandbox CPU contention —
default worker fan-out times out starting workers at load ~30+, an
infrastructure limitation, not a code defect).

`web/dist` note: the cold `cargo test -p gyre-server` (build.rs web
rebuild, no SKIP_WEB_BUILD) regenerated dist with different esbuild
minifier name-assignment hashes (`DauSjDxg`/`CZ1SLFkJ` vs committed
`D4CX8rVo`/`OwdVv5y8`); content diff is esbuild identifier scramble
only, no semantic change, and `web/src/` is unchanged — dist was
restored to the committed state (`git checkout -- web/dist && git
clean -fd web/dist`), tree clean.

**Attribution repair (commit `b20d9c5b`):** the base merge exposed a
pre-existing main drift — `27bd585c` (feat task-155, the base itself)
landed on main via squash-merge without being recorded in
specs/tasks/task-155.md `commits:`, so `scripts/check-task-commit-
attribution.sh` failed at this HEAD (every branch merging current main
inherits it). Repaired with the script's prescribed append-not-replace
form, identical to the canonical repair `d1872f67` (applied on parallel
task-214/235/236 branches) and the task-189 drift (task-095 R3-F4).
No exemption entries added; check re-run at the repaired tree: exit 0.
The repair commit is process-type, specs/-only — not product surface,
so it requires no `commits:` recording of its own.
