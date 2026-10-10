# Review — task-193: Mode-based spec approval status resolution (spec-registry.md §9)

Candidate: `c7e476678ef165caa704f4071e7b25a7ea681f7c`
Base: `8c2d177505852b3e39cd77f4f782fb355de245aa`
Reviewer: independent review, fresh model (retry after infrastructure non-completion)
Date: 2026-10-10

## Verdict

**NOT approved** — one defect (process/artifact, not functional). The §9
implementation itself is genuine and well-tested; see "What was verified".

## Finding R1 — accidental `web/dist` rebuild fails the whitespace gate

Commit `b65efd44` ("implement(task-193): pipeline checkpoint") ships a rebuilt
`web/dist` bundle with **zero** changes to `web/src`, `web/package.json`, or
`web/package-lock.json`:

- `git diff --stat 8c2d1775..c7e47667 -- web/src/ web/package.json web/package-lock.json` → empty.
- The rebuild replaces `index-fzyK9GaC.js`/`index-duCVht8R.css` with
  `index-D4CX8rVo.js`/`index-Bn8TWOVG.css`, renames
  `elk.bundled-BcuvlO8W.js` → `elk.bundled-OwdVv5y8.js`, and edits `index.html`.
- The new file `web/dist/assets/index-D4CX8rVo.js` line 5 contains a trailing
  tab (byte ~72511, vendored svelte-i18n runtime), so
  `git diff --check 8c2d1775 c7e47667` exits **2**
  (`/tmp/stage/review-evidence/git-diff-check.txt`).

This is the exact failure class `scripts/dev-check.sh:8` treats as a candidate
defect ("Git uses exit 2 for whitespace errors"), and the exact pattern the
task-210 review round 12 reverted with the recorded policy: "task branches
don't ship dist rebuilds — CI and the integration gate build from source"
(dev-check.sh lines 84–86 themselves rebuild from source and then restore the
committed dist). The shipped section in `specs/tasks/task-193.md` does not
disclose the rebuild.

**Fix:** `git restore --source=8c2d177505852b3e39cd77f4f782fb355de245aa -- web/dist`
(and drop any now-untracked new bundle files), commit as a process-type
commit, re-run `git diff --check <base> HEAD` (must exit 0). No code changes
needed — `git diff 32c94f62 c7e47667 -- crates/` is empty, so the crates
surface is unaffected.

## What was verified (independent evidence, logs in /tmp/stage/review-evidence/)

- **Diff scoping:** net base→candidate crates/ diff is task-193 work only.
  The admin.rs delta vs the earlier reviewed checkpoint `3e467f37` enters via
  merge `468af4da` whose second parent `4b9d61c4` is an ancestor of the
  assigned base (inherited, not task work).
- **Resolver unit tests:** 13/13 pass (`resolver-unit-tests.log`, exit 0).
- **E2E API tests:** 55/55 pass through the real axum router with real git
  repos, real minted EdDSA agent JWTs (`mint_with_workload`), real KV workload
  attestations, and a real API-key human (`specs-api-tests.log`,
  `pristine-restore-verify.log`, exit 0).
- **Negative control 1 (agent validity hollowed):** patching
  `is_valid_agent_approval` back to "any active agent approval" makes
  `agent_only_requires_exact_stack_hash_match_e2e` and
  `agent_only_rejects_attestation_below_minimum_e2e` FAIL
  (`negative-control-agent.log`) — the tests genuinely gate
  persona/attestation_level/stack_hash.
- **Negative control 2 (mode logic reverted):** patching the mode match to
  "human OR agent" makes `human_and_agent_requires_both_approvals_e2e` FAIL
  (`negative-control-mode.log`) — the mode semantics are genuinely gated.
  All patches were restored; worktree verified clean and suite re-run green.
- **Persistence:** SQLite adapter round-trip 3/3 including
  `attestation_level`/`stack_hash` (`adapter-roundtrip.log`); Postgres row
  mapping and schema updated; migration `2026-10-08-000056` is the next
  unused sequence number, portable `ALTER TABLE ADD COLUMN` with a down.sql;
  no data dropped. gyre-domain 363/363.
- **Real values, not invented:** `stack_hash` from the verified JWT
  `wl_stack_hash` claim; `attestation_level` derived via the same
  `derive_attestation_level` (constraint_check.rs) from the agent's real
  workload-attestation KV record — the canonical supply-chain level
  derivation; humans record None/None.
- **Signal chain:** `SpecApproved` now fires only on transition into
  Approved (prior_status != Approved && new == Approved).
- **Gates:** check-arch.sh, check-task-commit-attribution.sh,
  check-migration-versions.sh, check-migration-sql-portability.sh all exit 0
  at the candidate. `git diff --check` exits 2 (R1 above).
- **Sandbox limitation:** no TCP listener support (capabilities.json errno
  95), so no live-server HTTP probe; the in-process router tests are the
  strongest available transport-level verification here. GitHub CI on the
  exact candidate SHA remains mandatory.

## Acceptance criteria status

- SpecApprovalEvent persists attestation_level/stack_hash (JWT → repo → read
  back): **met** (adapter + E2E round-trip tests).
- human_and_agent not Approved on either half alone: **met** (unit + E2E +
  negative control 2).
- agent_only min_attestation_level enforced: **met** (unit + E2E + negative
  control 1).
- stack_hash exact match enforced: **met** (unit + E2E + negative control 1).
- Unit tests that fail under a reverted "any valid approval" resolver:
  **met** (both negative controls observed FAIL then restored to green).
- cargo build / touched-crate tests / check-arch.sh pass: **met**.
- Clean diff (no unrelated artifacts): **NOT met** — R1.
