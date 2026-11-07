---
title: "HSI Policies ↔ Trust Level Integration UI"
spec_ref: "human-system-interface.md §2a"
depends_on:
  - task-077
progress: complete
coverage_sections:
  - "human-system-interface.md §2a Policies ↔ Trust Level Integration"
commits: ["7271313c9bbec615e6ec5baad90715eab261a0a4", "98612a2a112c74c007e8f7587afa90ffe29d3667"]
---

## Spec Excerpt

The Trust Level and Policies tabs in Admin are conceptually linked — changing trust level changes which policies are active. The UI must make this relationship visible:

**Trust Level tab shows implied policies:**
- Below the trust level radio buttons, a read-only summary lists the `trust:` policies that the selected preset creates (or would create if switching).
- Format: compact list showing policy name, effect (Allow/Deny), and target.
- A "View all policies" link navigates to the Policies tab.

**Policies tab shows trust origin:**
- Policies with `trust:` prefix display a badge indicating which trust preset created them (e.g., "From: Supervised").
- When trust level is NOT Custom, banner: "Trust level: {level} — policies are preset-managed. Switch to Custom to edit."
- When trust level IS Custom, banner: "Custom trust — full policy editor enabled."

**Cross-linking:**
- Trust Level tab → "View all policies" link → Policies tab
- Policies tab → "Change trust level" link → Trust Level tab
- Both links stay within the Admin view (tab switch, not navigation change)

## Implementation Plan

1. **Trust Level tab enhancement** (Admin > workspace scope):
   - Below the radio buttons (Supervised/Guided/Autonomous/Custom), add a read-only policy summary section
   - For each preset, show the policies it creates/removes:
     - Supervised: `trust:require-human-mr-review` (Deny merge by system)
     - Guided: no trust: policies (relies on built-ins)
     - Autonomous: no trust: policies
   - Show what would change on switching (diff view: "Will add: ...", "Will remove: ...")
   - Add "View all policies →" link that switches to Policies tab

2. **Policies tab enhancement** (Admin > workspace scope):
   - Group policies by prefix: `builtin:`, `trust:`, user-created
   - `trust:` policies show a badge: "From: {current_trust_level}"
   - Add a preset banner at the top:
     - If trust_level != Custom: "Trust level: {level} — policies are preset-managed. Switch to Custom to edit." with link to Trust Level tab
     - If trust_level == Custom: "Custom trust — full policy editor enabled."
   - Immutable policies (`builtin:require-human-spec-approval`) greyed out with tooltip "This policy cannot be removed or overridden."
   - Add "← Change trust level" link that switches to Trust Level tab

3. **Tab switching within Admin view:**
   - Trust Level and Policies are sibling tabs in the workspace Admin view
   - Cross-links perform tab switch, not full navigation

## Acceptance Criteria

- [ ] Trust Level tab shows read-only policy summary below radio buttons
- [ ] Policy summary shows name, effect, and target for each trust: policy
- [ ] "View all policies" link navigates to Policies tab within Admin
- [ ] Policies tab groups policies by prefix (builtin:/trust:/user-created)
- [ ] trust: policies show "From: {level}" badge
- [ ] Preset-managed banner appears when trust_level != Custom
- [ ] Custom banner appears when trust_level == Custom
- [ ] Immutable policies greyed out with tooltip
- [ ] "Change trust level" link navigates back to Trust Level tab
- [ ] Tab switching is within Admin view (no full page navigation)
- [ ] `npm test` passes in `web/`

## Shipped

Production commit `7271313c9` implements HSI §2a on the workspace Admin surface (`WorkspaceSettings.svelte` — the workspace-scope Admin view reached via the gear / status-bar trust item). The former combined "Trust & Policies" tab is split into sibling **Trust Level** and **Policies** tabs so the conceptual link is a visible, in-place tab relationship.

### Trust Level tab

- Below the radio buttons: a read-only "Policies implied by {level}" summary mirroring `gyre-domain`'s `trust_policies_for_level` (Supervised → `trust:require-human-mr-review` — Deny — merge by system; Guided/Autonomous → relies on built-ins; Custom → user-managed). Rows show name, effect, and target per the spec's compact format.
- When the form selection differs from the persisted level, a diff block shows "Will add: …" / "Will remove: …" lines for the trust: policy set a save would rewrite.
- "View all policies →" link switches to the Policies tab within the same Admin view (no navigation change).
- Saving a trust level now records the level from the `PUT /workspaces/:id` response and drops the cached policy list, so the Policies tab reloads the server-rewritten trust: set.

### Policies tab (new sibling)

- Queries the real policy set (`GET /api/v1/policies?scope=Workspace&scope_id=…` via `api.workspaceAbacPolicies`) and groups rows by origin: `builtin:` (name prefix or `built_in` flag), `trust:`, and user-created — each group with its own header and count.
- `trust:` rows show a "From: {level}" badge derived at display time from the persisted trust level.
- Banner per spec: "Trust level: {level} — policies are preset-managed. Switch to Custom to edit." when not Custom; "Custom trust — full policy editor enabled." when Custom.
- Immutable / built-in policies are greyed out (`.policy-locked`) with tooltip "This policy cannot be removed or overridden." and a disabled delete. `trust:` deletes are locked while preset-managed and unlocked under Custom — matching the backend, where the to-Custom transition preserves trust: policies as the user-managed starting point.
- "← Change trust level" link switches back to the Trust Level tab in-place.

### Test evidence

`npm ci && npm test` in `web/`: 64 files / 1644 tests passed, 41 skipped. Focused suite `vitest run src/__tests__/WorkspaceSettings.test.js`: 68 passed, including 18 new HSI §2a tests + 2 updated tab-structure tests (8 tabs: Trust Level and Policies as siblings). New tests assert rendered DOM text/classes/attributes (banner strings, badge "From: Guided", tooltip, disabled deletes, group membership, diff lines, in-place tab switches), so removing any of the shipped behaviors fails them. Evidence files with commands and exit codes: `/tmp/stage/review-evidence/task-084-web-npm-test.txt` and `task-084-vitest-workspacesettings.txt`.

## Agent Instructions

Read `specs/system/human-system-interface.md` §2a (Policies ↔ Trust Level Integration) for the full spec. This depends on task-077 (Trust Gradient domain + UI) which implements the trust level radio buttons and ABAC policy presets. The Admin workspace view likely exists at `/workspaces/:id/admin`. Check the current Admin component structure. The Policies tab should query `GET /api/v1/policies?scope=Workspace&scope_id=<workspace_id>`. The Trust Level tab is part of workspace settings (`PUT /api/v1/workspaces/:id` with `trust_level` field). The ABAC policy engine spec (`abac-policy-engine.md`) defines the policy model used here.
