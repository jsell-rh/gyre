# Review — task-110: Implement tenant & workspace invitation flow

- **Candidate:** `a7799ac0566b011872179b8655c110156420a609`
- **Base:** `349a32ac6f5557786f82d17c13f8815de600e837`
- **Round:** 15 (re-review of the round-14 repair)
- **Verdict:** Approved

## Scope of this round

The only durable finding from the round-14 review (checkpoint `d05c2cee`) was F1:
`invite_to_workspace` typed the invitee's in-app notification as
`NotificationType::TrustSuggestion`, misclassifying every workspace invitation in the
invitee's Inbox. This round verifies the repair commit `ea366b15` and re-spot-checks
the task contract. Evidence: `/tmp/stage/review-evidence/` (summary.md + logs).

## F1 repair verification (approved)

- `crates/gyre-server/src/api/invitations.rs:1262` now constructs the notification
  with `NotificationType::InvitationReceived`, matching user-management.md
  §Notification Entity ("A user was invited to a workspace") and the sibling
  `invite_member` handler.
- The variant is fully wired: `gyre-common/src/notification.rs` (serde, severity),
  and the frontend Inbox maps it to the `invitation_received` kind with info
  severity (`web/src/components/Inbox.svelte:76,141`), so the invitee's inbox now
  classifies workspace invitations correctly.
- The test `invite_notification_carries_accept_path` was hardened with
  `assert_eq!(notif.notification_type, InvitationReceived)` and an `assert_ne!`
  against `TrustSuggestion`. The test is a real round-trip (authenticated POST via
  the router, notification read back through `list_for_user`), so a regression of
  the typing fails the test. The prior version asserted only the body `kind` field
  and could not catch the mis-typing; that gap is closed.
- Line 1262 is the only production `NotificationType::` construction site in
  `invitations.rs`; no other mis-typed notification sites remain.

## Independent checks run

- `cargo check -p gyre-server --lib --tests` → exit 0 (8m07s; build.rs also ran
  `npm ci` + web build successfully).
- `scripts/check-abac-route-registry.sh` → exit 0.
- `scripts/check-dead-message-kinds.sh` → exit 0.
- Acceptance-criteria spot checks (entities, port-backed AppState stores,
  migrations 000058/000061, all 5 tenant routes + workspace invite lifecycle routes
  with ABAC mappings, ring CSPRNG tokens hashed with SHA-256, expiry sweeper spawned
  from `main.rs:136`): all confirmed as real implementations, not labels.
- `grep -c '| not-started |' specs/coverage/system/user-management.md` → 0.

## Verifier-required probes (not run here — executor wrapper blocks gyre-server
test compilation; infrastructure restriction, not a code defect)

- `cargo test -p gyre-server --lib api::invitations::tests::invite_notification_carries_accept_path`
- `cargo test -p gyre-server --lib api::invitations`
- Full workspace `cargo test --all` per task acceptance criteria.

## Notes

- `web/dist/` changes in the delta are generated output owned by the deterministic
  verifier; no authored frontend source changed in this round's delta.
- The working tree contained a pre-existing unstaged modification to
  `specs/tasks/task-110.md` (task-template reset) before review began; left
  untouched — a stage artifact, not a code defect.
- Prior rounds (1–14) reviewed the full invitation implementation; their findings
  are all repaired in the candidate history (SSO linking, username contract,
  fail-closed policy, `allow_re_invite` enforcement, pending-scope unique index,
  non-atomic-create markers, notification typing).
