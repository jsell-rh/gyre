# TASK-048 Review — R1

**Reviewer:** Verifier
**Date:** 2026-04-10
**Commit under review:** `9db04e6a`
**Verdict:** `needs-revision` (3 findings)

---

## Findings

- [-] [process-revision-complete] **F1: `target_artifact` set to `source_artifact` in `reconcile_dependencies` — wrong data stored on all new edges.**
  In `git_http.rs:2091–2098`, when `reconcile_dependencies` creates a new edge, `source_artifact.as_str()` is passed for **both** the `source_artifact` and `target_artifact` parameters of `DependencyEdge::new()`. The `detected_edges` tuple type `(String, String, DependencyType, DetectionMethod, Option<String>)` has no field for `target_artifact` at all — the dependency name/identifier (crate name, package name, module path, spec link target) is discarded during the detection→collection step.
  **Consequences:**
  (a) All new edges store the file name (e.g., `"Cargo.toml"`, `"package.json"`) as both `source_artifact` AND `target_artifact`, violating the spec entity definition which says `target_artifact` should be `"crate name, spec path, API endpoint"`.
  (b) The post-reconciliation drift check at `git_http.rs:2469–2470` uses `edge.target_artifact.clone()` as the dependency name for `extract_dep_version(&toml_content, &target_name)`. Since `target_artifact` is `"Cargo.toml"` (the file name) instead of the crate name, this lookup will never find a match — version drift is never computed for any edge created through reconciliation.
  (c) The old pre-TASK-048 code correctly passed `basename.as_str()` (the crate name) for `target_artifact`.
  **Fix:** Add a `target_artifact: String` field to the `detected_edges` tuple. Populate it from each parser's output: `basename` for Cargo.toml, `candidate` (package/path name) for package.json, `module_path` for go.mod, the path/package name for pyproject, the spec link target for manifest, and the matched repo name for API contract detectors. Pass it as the 6th argument to `DependencyEdge::new()` instead of `source_artifact`.

- [-] [process-revision-complete] **F2: Reconciliation incorrectly orphans edges from dependency files not changed in the current push.**
  The detection code only scans dependency files that were modified in the push (e.g., `if has_cargo_toml { ... }`). But `reconcile_dependencies` compares **all** existing non-Manual edges for the repo against the detected set and marks any missing ones as `Orphaned`.
  **Scenario:**
  1. Push 1 changes `Cargo.toml` → detects dep on repo-B via `CargoToml` → creates edge.
  2. Push 2 changes `package.json` only (Cargo.toml untouched) → detects dep on repo-C via `PackageJson` → creates edge. BUT the CargoToml edge from Push 1 is NOT in `detected_edges` (Cargo.toml wasn't changed and wasn't scanned), so reconciliation marks it as `Orphaned` — even though the Cargo.toml still declares the dependency.
  This applies to all detection method combinations: any push that doesn't touch ALL dependency file types will incorrectly orphan edges from the untouched file types. The spec says "On every push to any repo, the forge: 1. Parse Dependency Files" — implying all files should be scanned, not just changed ones.
  **Fix:** Either (a) always scan all dependency files regardless of changes (matching the spec's literal wording), or (b) scope the orphan check in `reconcile_dependencies` to only consider edges whose `detection_method` matches a detection method that actually ran in this push (pass a `ran_methods: HashSet<DetectionMethod>` parameter and skip edges whose method is not in the set during the orphaning loop).

- [-] [process-revision-complete] **F3: `go.mod` detection uses `replace` directives instead of spec-required `require` directives.**
  The spec §1 "Parse Dependency Files" says: `go.mod -> extract require directives referencing Gyre modules`. The implementation (`detect_go_mod_deps`) only parses `replace` directives with local paths. `require` directives — which declare actual module dependencies (e.g., `require forge.internal/workspace/repo v1.0.0`) — are not parsed.
  The task description changed this to "parse `replace` directives with local paths" — a task-vs-spec transcription error. `replace` directives are development overrides to redirect imports to local filesystem paths; `require` directives are the actual dependency declarations. The spec's intent matches `require` (the forge matches dependency identifiers against known repos in the tenant, same as the Cargo.toml approach).
  **Fix:** Parse `require` directives (both single-line `require module/path v1.0.0` and block `require ( ... )` syntax) and match the module path against known Gyre repo names. The existing `replace` parsing may be kept as a supplementary detection path, but `require` parsing must be added to satisfy the spec.
