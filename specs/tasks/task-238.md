---
title: "Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
Environment fingerprint: `host-178659320de621791dba7f0aa9660a64bbadb1bc690b33f14cf78b2343318cbd`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.3GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/tools/checks.sh
rustfmt: crates/gyre-server/tests/explorer_ws_integration.rs: changed lines need formatting: 464, 465, 466, 469, 471
crates/gyre-server/tests/explorer_ws_integration.rs:412: clippy::useless_conversion: useless conversion to the same type: `std::string::String`
    Checking gyre-common v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-common)
   Compiling gyre-server v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-server)
warning: gyre-server@0.1.0: SKIP_WEB_BUILD=1 set, skipping web build
    Checking gyre-domain v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-domain)
    Checking gyre-cli v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-cli)
    Checking gyre-ports v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-ports)
    Checking gyre-adapters v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/crates/gyre-adapters)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 34s

Architecture lint passed: gyre-domain has no forbidden dependencies or I/O.
Hierarchy lint passed: all hierarchy fields are non-optional.
OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries).
check-abac-exempt-handlers: OK (89 handler(s) checked)
check-mcp-write-tools: OK (8 write-capable tool(s) checked, all gated)
OK: no duplicate Diesel migration versions.
OK: no dialect-only SQL in shared migrations.
OK: every MessageKind variant has an emitter (or documented exemption).
check-byte-slice-truncation: OK
check-relative-path-defaults: OK
OK: no fail-open .unwrap_or_default()/.unwrap_or("") on resolve_ref() results.
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  27bd585c  task-155  feat(task-155): Implement gyre search CLI command

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/dfa3886638c041b7a25ebbac1115e861/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-nhr105n1/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Reproduction:** at clean base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
(HEAD = origin/main, tree clean), `bash scripts/check-task-commit-attribution.sh`
exits 1 with the recorded violation: `27bd585c task-155 feat(task-155):
Implement gyre search CLI command`. This is the sole probe named in the
machine-readable `GYRE_BASELINE_FAILURE_JSON`. Evidence:
`/tmp/stage/review-evidence/task-238-attribution-repro.txt`.

**Repair (1 line, `specs/tasks/task-155.md:8`):** appended the landing commit
SHA `27bd585ca7eb429905ccbded1f48b4d0167c0c20` to task-155's existing
`commits:` list, preserving the 7 recorded branch SHAs (wip/checkpoint/fix/feat
lineage of the squashed PR). Same flaw class and remedy as task-228's
`05709c24` (task-196 drift), task-224's `a11ba8d3` (task-068 drift), and
task-219's `e96d25ab` (task-200 drift): the squash landing commit cannot
contain its own SHA, so the recorded list stayed one entry short and the
landing surface (+837 lines across gyre-cli/client.rs, main.rs, docs/cli.md)
was invisible to review scoping (task-095 R3-F4). No exemptions added —
`scripts/task-commit-attribution-exemptions.txt` remains frozen at its 3-entry
baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`);
no gate, skip, or check weakened; no Rust/JS product source changed
(`git diff <base> HEAD -- scripts/ crates/ web/` is empty).

**Note on `dev-attribution.py`:** running `python3 scripts/dev-attribution.py
task-155` derives `["27bd585c..."]` alone because the 7 branch SHAs are not
ancestors of main (squash-merged) and the script rewrites rather than appends.
Replacing the 7 SHAs would erase recorded review-scoping history, so this
repair appends instead — same choice the approved task-228 round made
(documented in its Shipped section).

**Probe after repair:** exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `/tmp/stage/review-evidence/task-238-attribution-after-repair.txt`.

**Mutation check (test-the-repair):** landing SHA removed via sed → identical
FAIL exit 1 (`27bd585c task-155 ...`); restored → exit 0. The pass is
attributable to the recorded SHA, not gate drift. Evidence:
`/tmp/stage/review-evidence/task-238-attribution-mutated.txt`.

**Full static gate suite on the repaired tree:** all 21 deterministic gates
pass (`check-arch`, `check-hierarchy`, `check-abac-route-registry`,
`check-abac-exempt-handlers`, `check-mcp-write-tools`, `check-migration-versions`,
`check-migration-sql-portability`, `check-dead-message-kinds`,
`check-byte-slice-truncation`, `check-relative-path-defaults`,
`check-fail-open-ref-resolution`, `check-task-commit-attribution`,
`check-mem-port-contracts`, `check-fabricated-scope-defaults`,
`check-lossy-secret-conversion`, `check-scope-literal-defaults`,
`check-inert-enforcement`, `check-forged-scope-fields`,
`check-forwarded-header-trust`, `check-in-memory-state-stores`,
`check-unbounded-external-http`). Per-gate logs:
`/tmp/stage/review-evidence/task-238-check-*.txt`.

**Diff-scoped lint gates vs base:** `check-rustfmt-diff.py <base>` exit 0
(0 Rust files changed); `check-clippy-diff.py <base>` exit 0 ("changed lines
clean (0 Rust files, 1141 existing warnings outside changes)"). Evidence:
`/tmp/stage/review-evidence/task-238-rustfmt-diff-gate.txt`,
`/tmp/stage/review-evidence/task-238-clippy-diff-gate.txt`.

**Baseline-log rustfmt/clippy lines are not main-state failures:** the log's
`explorer_ws_integration.rs:464,465,466,469,471 need formatting` and
`:412 clippy::useless_conversion` reference a file state that does not exist
at base — the file is 420 lines at `27bd585c` (verified across all refs; only
the unmerged `origin/pipeline/task-069` branch ever had 498). Those lines came
from the interrupted attempt `dfa38866`'s checkout (base + discarded candidate
working tree), not from clean main: `check-rustfmt-diff.py` and
`check-clippy-diff.py` compare base..HEAD, and at clean HEAD the diff is
empty. The baseline environment's own `GYRE_BASELINE_FAILURE_JSON` records
exactly one failing probe — the attribution check repaired above. Separately,
current stable clippy (1.99.0) reports a warning-level `never_loop` at
`crates/gyre-domain/src/rust_extractor.rs:1071` under the CI invocation
(`-W clippy::all` downgrades it from the correctness-group default); it is
pre-existing main debt on an unchanged line, outside every gate's scope, and
the CI-exact invocation exits 0 at clean HEAD. Evidence:
`/tmp/stage/review-evidence/task-238-clippy-ci-invocation.txt`.

**Attribution for this task:** this branch's only changes are
`specs/tasks/task-155.md` (the repair) and this task file — no product
surface — so `commits: []` is attribution-canonical (same as task-228's
specs-only rounds).
