---
title: "Dependency auto-detection: full extraction forms (git refs, structured contracts, nested files) and every-push trigger"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / dep-detection-extraction-forms / ed9f34316476"
depends_on: []
progress: ready-for-review
commits: ["423769ddb227634b5b5fb30909fcaac22b47e159", "05ba66134f77df6cffd99aa4720fd5d92764fe5a"]
---

## Required behavior

Narrow delta on the existing push-time detection pipeline (task-048's detect_dependencies_on_push and its helper parsers in crates/gyre-server/src/git_http.rs) — the reconciliation store logic (§4, already real) is unchanged. (A) TRIGGER (§Automatic Detection parent, gap 12ff1c4b): detection runs on EVERY push to any repo — move the detect_dependencies_on_push call out of the `applied_updates.iter().filter(|u| u.refname == default_ref)` loop so it runs for every applied non-delete ref update on ANY branch, and remove the changed-dependency-file early return: all detectors run on every push, parsing the manifests/contracts present at the pushed tip. Reconcile is idempotent for no-change pushes; ran_methods must cover all methods that ran (with all detectors running every push, orphaning stays correct: an edge from a branch whose declarations vanish on a later push is marked Orphaned, never deleted; Manual edges are never touched). This also fixes the real failure where a repo created after a source repo's push is never matched until another dependency-file-touching push happens. (B) CODE MANIFESTS (§1, gap e07d32e2): Cargo.toml — parse [dependencies] entries with BOTH `path = "..."` (existing) and `git = "<url>"` inline-table keys: extract the Gyre repo name from the git URL (last path segment, `.git` stripped, handling `<workspace>/<repo>.git` forms), match against known tenant repos (state.repos.list()), version_pinned from `version`/`tag`/`rev` keys when present; enumerate ALL Cargo.toml files in the tree at the pushed SHA (git ls-tree — the list_files_by_extension pattern already used for .proto), not only the root manifest, with source_artifact carrying the manifest's tree path. package.json — in addition to `file:`/`workspace:` forms, extract dependency values that are git URLs (`"repo-b": "git+https://gyre.example.com/<ws>/repo-b.git#v1.2"`, git+ssh forms), matched by URL last segment. pyproject.toml — in addition to `file:` forms, extract PEP 508 direct references `pkg @ git+https://...`. go.mod stays as-is (already faithful under the generic wording). External registry deps (crates.io/npm/PyPI) remain ignored — only Gyre-to-Gyre dependencies tracked. (C) API CONTRACTS (§3, gap 8244b7d9): replace the whole-document `content.contains(repo_name)` scans with structured extraction: OpenAPI/Swagger — parse the contract document (YAML or JSON) and extract server URLs only (OpenAPI 3.x `servers[].url`; Swagger 2.0 `host` + `basePath`), matching known repo names within the extracted URLs; a repo name appearing only in a description, title, or arbitrary string creates NO edge. mcp.json — parse the JSON structure and extract server URL fields (e.g. `mcpServers.<name>.url`), not arbitrary document text. Enumerate contract files tree-wide (any openapi.yaml/openapi.yml/swagger.json/mcp.json path, e.g. api/openapi.yaml — the spec's own entity example), not only root-level names. Proto import detection (already real, structured) is unchanged. All parsers must use find-based slicing that cannot panic on multibyte UTF-8 (scripts/check-byte-slice-truncation). COORDINATION: the staged spec-lifecycle and agent-workflow plans also edit git_http.rs but in unrelated functions (process_spec_lifecycle, gates); keep changes scoped to the detection helpers and the post-receive call site.

## Acceptance Criteria

- [ ] Pushing `[dependencies] repo-b = { git = "https://gyre.example.com/<ws>/repo-b.git" }` in a repo where repo-b is a known tenant repo creates a Code/CargoToml DependencyEdge to repo-b (GET /api/v1/repos/:id/dependencies shows it, with version_pinned when a version/tag/rev key is present).
- [ ] A push to a NON-default branch that adds a dependency declaration updates the graph for that repo; a later push whose tip no longer contains the declaration marks the edge Orphaned (not deleted).
- [ ] An api/openapi.yaml (nested) whose servers[].url references repo-b creates an Api edge with source_artifact "api/openapi.yaml"; the same document mentioning repo-b only in a description creates no edge.
- [ ] An mcp.json whose mcpServers entry url references repo-b's server creates an Api/McpToolRef edge; a repo name appearing only in an unrelated string field creates no edge.
- [ ] package.json `"repo-b": "git+https://gyre.example.com/<ws>/repo-b.git"` and pyproject.toml `repo-b @ git+https://...` produce Code edges matched to the known repo.
- [ ] After a new repo named repo-c is created, the next push to a repo whose committed manifest already referenced repo-c by name creates the edge (no dependency-file-change short-circuit).
- [ ] A Cargo.toml in a workspace-member subdirectory (not the root) declaring a path/git dependency on a known repo is detected with the manifest path as source_artifact.

## Verification intent

Push a Cargo.toml whose only repo-b reference is `repo-b = { git = "https://gyre.example.com/<ws>/repo-b.git" }` (no `path` key) on a feature branch, then GET the repo's dependencies: pre-fix this fails twice over — detect_cargo_path_deps/extract_path_value parse only `path = "..."` values so a git reference produces no edge, and detection only runs for `u.refname == default_ref` updates so a feature-branch push never runs detection at all — the probe must fail until both the git-reference extraction and the every-push trigger land. Companion over-match probe: an openapi.yaml whose description text contains "repo-b" but whose servers[].url does not must produce NO Api edge — pre-fix `content.contains(repo_name)` creates one, so the probe fails until server-URL-structured extraction replaces the substring scan.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Shipped

All three gap areas are implemented in `crates/gyre-server/src/git_http.rs`, scoped strictly to the detection helpers and the post-receive call site (no changes to `process_spec_lifecycle` or gates). Coverage rows 5/6/8 in `specs/coverage/system/dependency-graph.md` re-evidenced with current function names and line numbers; no other rows touched.

### Behavior

- **Every-push trigger (gap 12ff1c4b):** the `detect_dependencies_on_push` call moved out of the `filter(|u| u.refname == default_ref)` loop — it now runs for every applied (non-delete) ref update on any branch (`git_http.rs` post-receive, `for update in applied_updates.iter()`). The changed-dependency-file early return is gone: `detect_dependencies_on_push` no longer takes a changed-file list; all eight detectors always run against the tree at the pushed tip, `ran_methods` covers every method each push, and reconcile orphans (never deletes) edges whose declarations vanish from the tip while leaving Manual edges untouched. A target repo created after the manifest was committed is matched by the next push, even one touching no dependency file.
- **Cargo.toml (gap e07d32e2):** `detect_cargo_deps` replaces `detect_cargo_path_deps`/`extract_path_value`/`extract_dep_version` — parses `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` inline-table entries with word-boundary key matching for BOTH `path = "\u2026"` and `git = "<url>"`, with `version`/`tag`/`rev` captured as the pinned version. `repo_name_from_reference` extracts the repo name from a reference (URL last path segment, `#fragment`/`?query` stripped, `.git` suffix removed, `git@host:` forms handled). ALL `Cargo.toml` files in the tree at the pushed SHA are enumerated via `git ls-tree` (`list_files_by_names`), with the manifest's tree path as `source_artifact`.
- **package.json / pyproject.toml:** `detect_package_json_deps` additionally extracts git URL values (`git+https://\u2026`, `git+ssh://\u2026`, `#fragment` as version); `detect_pyproject_deps` additionally extracts PEP 508 direct references `pkg @ git+https://\u2026`. go.mod unchanged. External registry deps still ignored — only references matching a known tenant repo (`state.repos.list()`, excluding self) create edges.
- **API contracts (gap 8244b7d9):** `detect_openapi_refs` parses the document (serde_yaml; JSON is valid YAML) and extracts server URLs only — OpenAPI 3.x `servers[].url`, Swagger 2.0 `host`+`basePath` — matching known repo names as whole URL labels (`repo_name_in_url`). `detect_mcp_refs` parses the mcp.json structure for `mcpServers.<name>.url`/`.server` and `tools[].url`/`.server` fields only. A repo name appearing only in a description/title/unrelated field creates NO edge. Contract files are enumerated tree-wide (`api/openapi.yaml` carries its tree path as `source_artifact`). Proto import detection unchanged.

### Test evidence

`cargo check -p gyre-server --tests` passes with zero warnings (build wrapper used; the finalizer removed one unused closure and applied rustfmt to the new test code only). New tests: HTTP-level `push_to_non_default_branch_detects_git_dependency_then_orphans` (feature-branch git push through git-receive-pack creates the CargoToml git-ref edge with `version_pinned` from `tag`, then a declaration-removing push marks it Orphaned, still present), and `detect_dependencies_on_push_full_extraction_forms` (nested `crates/member/Cargo.toml` source_artifact, `api/openapi.yaml` servers[].url edge + description-mention non-edge, mcp.json url-field edge + unrelated-field non-edge, package.json/pyproject git forms, repo-c created after commit matched by a README-only push, idempotent no-change pushes), plus parser unit tests for git/tag/rev keys, word-boundary key matching, URL last-segment name extraction, Swagger 2.0 host+basePath, JSON documents, and invalid-document tolerance. Full gyre-server test execution is deferred to the independent verifier (model-phase 4 GiB budget; the cargo wrapper blocks server test builds mechanically) — the exact test list is recorded in `/tmp/stage/review-evidence/task-278-evidence.md`.

### Source checkpoints

- `423769ddb227634b5b5fb30909fcaac22b47e159` — pre-lease-expiry implementation checkpoint (recovered verbatim as `dcdb28f1eb469d2ab0cf694d975a90deaec628ad`).
- This finalizer round (`05ba66134f77df6cffd99aa4720fd5d92764fe5a`): warning cleanup + rustfmt on the new tests, coverage rows 5/6/8, task bookkeeping. Recorded in `commits:` frontmatter.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "12ff1c4b10f99712ceaf0c8e",
    "decision": "gap",
    "requirement_quote": "On every push to any repo, the forge:",
    "detail": "Umbrella section whose four numbered steps are accounted under their child headings (§1 gap: Cargo.toml git-reference extraction missing; §2 satisfied; §3 gap: OpenAPI/mcp.json detection is a substring scan and only root-level contract files are read; §4 satisfied) — the child gaps are not re-counted here. The parent's own content is the push-time trigger: detection is genuinely wired into the git post-receive path, but only for ref updates on the repo's default branch (filtered by `u.refname == default_ref`) and only when a dependency-related file changed in the push. Pushes to non-default branches do not touch the graph (the graph does update when such branches later land on the default branch), and the extraction forms named by the children are only partially implemented, so the parent's statement is not fully true at this base.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            // Dependency graph: auto-detect Cargo.toml path deps (M22.4).\n            detect_dependencies_on_push(\n                &state_clone,\n                &repo_id_clone,\n                &repo_path_clone,\n                &update.new_sha,\n            )\n            .await;",
        "detail": "The trigger wiring is real: this call site sits inside `for update in applied_updates.iter().filter(|u| u.refname == default_ref)` — the post-receive loop scoped to default-branch ref updates — and detect_dependencies_on_push early-returns unless a dependency file changed in the push."
      }
    ]
  },
  {
    "id": "e07d32e2ebc10c9a36f6343c",
    "decision": "gap",
    "requirement_quote": "Cargo.toml       -> extract [dependencies] with git/path references to other Gyre repos",
    "detail": "Per the cycle-3 review correction (cargo-git-references-unparsed), this section is a gap: the spec names BOTH git and path reference forms as the Cargo.toml extraction contract, and the implementation parses only `path = \"...\"` inline-table values (detect_cargo_path_deps / extract_path_value). A `[dependencies]` entry like `repo-b = { git = \"https://gyre.example.com/<ws>/repo-b.git\" }` — exactly the Gyre-to-Gyre reference the spec names — produces no DependencyEdge; no code in the Cargo detection path handles `git =` (or `rev =`) keys. On the other three files, whose spec wordings are generic ('dependencies pointing to Gyre-hosted packages', 'require directives referencing Gyre modules', 'dependencies referencing Gyre packages'): go.mod is faithful under the generic wording because module paths are matched by last path segment, which covers both bare names and Gyre-URL module paths; package.json and pyproject.toml handle only `file:`/`workspace:` and path/`file:` forms respectively — a Gyre-hosted package referenced by git URL (e.g. `\"repo-b\": \"git+https://...\"` or `pkg @ git+https://...`) is a natural reading of the generic wording and is not detected. Secondary: only root-level Cargo.toml is read (`read_git_file(..., \"Cargo.toml\")`) although the changed-file trigger matches nested `*/Cargo.toml` paths, so path deps declared in workspace-member manifests are never detected. Matching against known tenant repos (state.repos.list()) and ignoring external registry deps are both correctly implemented for the forms that are parsed.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "fn extract_path_value(line: &str) -> Option<String> {\n    let path_idx = line.find(\"path\")?;",
        "detail": "The only Cargo.toml dependency-reference extractor: it locates the literal `path` key in a dependency line and reads its quoted value. There is no corresponding git= handling — grep for git-reference parsing in the Cargo detection path finds nothing, so `repo-b = { git = \"https://gyre.example.com/ws/repo-b.git\" }` yields no edge."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            let path_deps = detect_cargo_path_deps(&content);\n            for path_dep in &path_deps {",
        "detail": "The Cargo.toml branch of detect_dependencies_on_push feeds only detect_cargo_path_deps output into DetectedEdge (DependencyType::Code, DetectionMethod::CargoToml); git-reference values never enter the flow."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "                    if let Some(path) = ver_str.strip_prefix(\"file:\") {",
        "detail": "package.json detection handles only `file:` and `workspace:` reference forms; a Gyre-hosted package referenced by git URL is not extracted, though the spec's wording ('dependencies pointing to Gyre-hosted packages') naturally covers it."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        // Single-line: `require module/path v1.0.0`",
        "detail": "go.mod require (and supplementary local replace) directives are parsed and matched by last path segment, which covers URL-form Gyre module paths — this file's reading is faithful to its generic spec wording."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        // PEP 621: [project] dependencies = [...] with `pkg @ file:///path`",
        "detail": "pyproject.toml detection handles Poetry `{path = ...}` and PEP 621 `@ file:` forms only; PEP 508 direct git references (`pkg @ git+https://...`) to Gyre packages are not extracted."
      }
    ]
  },
  {
    "id": "8244b7d9d6d350af456c6492",
    "decision": "gap",
    "requirement_quote": "openapi.yaml / swagger.json  -> extract server URLs matching other Gyre repos",
    "detail": "Only one of the three contract forms is implemented as specified. Proto import detection is real structured parsing (`import \"...\"` directives, first path segment matched against known repo names, all .proto files in the tree scanned via git ls-tree). But OpenAPI/Swagger and mcp.json detection are coarse whole-document substring scans — `content.contains(repo_name)` — not the specced extraction of server URLs from openapi.yaml/swagger.json or tool references from mcp.json; any mention of a repo name anywhere in the document (e.g. in a description) creates an Api edge, which both over-matches beyond server URLs and never inspects the servers[] structure the spec names. Additionally, only the three root-level file names are read (`openapi.yaml`, `openapi.yml`, `swagger.json` at the repo root) even though the change trigger matches nested paths — the spec's own entity example uses `api/openapi.yaml` as a source_artifact — so nested API contracts are never parsed. The code's own comment ('Best-effort: many API contracts don't explicitly reference repo names') acknowledges the divergence; a code comment is not a spec amendment.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "    for repo_name in known_repos {\n        // Match repo names in server URLs, $ref paths, and other string values.\n        if content.contains(repo_name) {",
        "detail": "detect_openapi_refs: instead of parsing servers[].url (or $ref values) from the YAML/JSON document, it scans the raw document text for each known repo name — a substring stand-in for the specced server-URL extraction."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        for api_file in &[\"openapi.yaml\", \"openapi.yml\", \"swagger.json\"] {",
        "detail": "Only the three root-level contract files are read via read_git_file, while the has_openapi trigger also matches nested `*/openapi.yaml` etc. — nested contracts (the spec's own `api/openapi.yaml` example artifact) are never parsed."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "        if let Some(rest) = trimmed.strip_prefix(\"import \") {",
        "detail": "Proto detection is the satisfied third of the section: structured import-directive parsing with first-segment matching against known repos, scanned across all .proto files at the pushed SHA."
      }
    ]
  }
]
