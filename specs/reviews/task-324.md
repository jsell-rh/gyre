# Review — task-324 (Publish reviewed spec backlog for specs/system/lsp-call-graph.md)

Spec: `specs/system/lsp-call-graph.md` (plan `a8e88983c3060cd3af4d90d16e8709208b9361fa1dece0940873c9e8d32bc60d`, audit base `8546a781`, publication base `52d7b66b`).
Candidate under review: `68caa9d7` (branch `pipeline/task-324/a64b596904324cd9985a5aa007b24f2b-1`).
Verdict: **approved** — no findings. Task creation only; no product implementation claimed or needed.

## Probes run (checkout at candidate `68caa9d`)

- `git diff --stat 52d7b66b..68caa9d` → exactly 4 files, 737 insertions, 0 deletions: `specs/tasks/task-322.md`, `specs/tasks/task-323.md`, `specs/tasks/task-324.md`, `specs/task-spec-inputs.json`. No product code, spec, coverage matrix, registry exemption, or gate touched.
- Plan-fidelity comparison (python3, exact equality): the JSON embedded in `task-324.md` equals the assigned plan object; `task-322.md` / `task-323.md` required_behavior, acceptance criteria (6 / 5), and negative_probe equal the plan's task entries verbatim; the discovery-evidence blocks equal the audit's two gap entries.
- Registry integrity: `specs/task-spec-inputs.json` is valid JSON; vs base it is purely additive (sections 310→312, bindings 256→258, zero removed/modified). The two new `backlog/<section-id>` section keys point at real headings in the canonical spec (`LSP-Powered Call Graph Extraction` → `Edge Types Produced` / `Prerequisites`; multi-element heading paths match the registry's dominant convention), and the two new binding keys follow the established `Spec gap <hash> / <task-key> / <requirements-hash>` format, mapping each task to exactly the gap section its plan entry `covers`.
- Mechanical checks (all pass): `scripts/check-task-commit-attribution.sh`, `scripts/check-hierarchy.sh`, `scripts/check-commit-msg.sh` on the candidate message.

## Audit challenge (gap decisions re-verified against code)

Both `gap` decisions are accurate at the audited base and at HEAD: `EdgeType::Implements` is constructed only in `rust_extractor.rs:1160` (impl blocks) — Python `extract_class` has zero superclass handling and TypeScript `emit_class` has zero heritage/`extends` handling; `EdgeType::Returns` only in `rust_extractor.rs:845`; `EdgeType::RoutesTo` in rust/typescript/go extractors but never in `python_extractor.rs`; the Dockerfile's only rust-analyzer/typescript-language-server mention is the stale "task-073/075 scope" comment (line 73) and line 91 copies only the Python drivers (no `ts-callgraph.mjs`). Spot-checked `satisfied`-decision evidence also holds (fire-and-forget Pass 2 at `graph_extraction.rs:503`, per-language dispatch at `call_graph.rs:66/:73`, `cha` import in `scripts/go-callgraph/main.go:17`).

## Decomposition, dependencies, duplicate-work

- Two planned tasks are independent and correctly split along the hexagonal boundary (task-322: gyre-domain extractors; task-323: Dockerfile provisioning); both `depends_on: []` is correct.
- No active task duplicates the work: every task mentioning Implements/RoutesTo/rust-analyzer/typescript-language-server is complete except task-289 (unrelated spec-links work). The TypeScript `implements` clause is explicitly carved out of task-322's scope (cross-plan ownership boundary with the realized-model staged plan), preventing duplicate edge emission.
- Task numbering (322/323/324) matches the plan's `assigned_names`; no pre-existing files with those names were overwritten.
- New tasks are `not-started`; the publication task itself is `ready-for-review` per the assignment header. Commit message and attribution conventions match the task-313 publication precedent.

Evidence: `/tmp/stage/review-evidence/` (summary.txt, edgetype-constructions.txt, python-superclass.txt, attribution-check.txt, hierarchy-check.txt, commit-msg.txt).
