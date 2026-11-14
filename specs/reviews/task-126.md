# Review — task-126 (Implement user management CLI commands)

Spec: `user-management.md` §CLI. Candidate: `1233d892` (base `5984177c`).
Commits under review: `7d8f93d9` (feat: CLI commands + client methods + tests),
`dacb3b04` (review-repair: drop a fabricated invited-by column from the invitation
table). Docs/process commits (`cc71eaea`, `2cf8adac`, `23a914b9`, `1233d892`) touch
specs only. Verdict: **approved**.

## Round 1

Probes (evidence: /tmp/stage/review-evidence/summary.md and logs):

- `cargo check -p gyre-cli` → OK.
- `cargo test -p gyre-cli --bin gyre` → **189 passed, 0 failed** (39 new tests).
- Mutation probe: renaming the invite request body field `user_id`→`userId` in
  `client.rs` makes `workspace_invite_by_handle_resolves_user_then_posts_invite`
  FAIL on its exact-JSON body assertion (source restored; tree clean). The flow
  tests are not self-confirming.
- `check-task-commit-attribution.sh` → **passes on the candidate tree** (verified by
  temporarily restoring the committed task file; exit 0). The working-tree failure
  seen during review is the stage harness resetting `specs/tasks/task-126.md` to
  its assigned `commits: []` state, not a candidate defect. Both product commits
  (`7d8f93d9`, `dacb3b04`) are recorded in the candidate's frontmatter.
- `check-byte-slice-truncation.sh`, `check-arch.sh` → OK.

Verified working:

- **Every command in the spec's §CLI block exists and parses.** Parse tests cover
  each spec line: `whoami`, `profile set`, `sessions list/revoke/revoke-all`,
  `tenant invite` (bare / `--workspace`+`--role` / `--bulk`), `tenant invitations
  list/revoke`, `workspace members list/set-role/remove`, `workspace invite`
  (@handle and email), `workspace invitations list/revoke`, `team create/add/list/remove`,
  `notifications list` (`--unread --priority`), `notifications preferences set`
  (`--email-digest`, `--email-min-priority`, `--slack-url`, plus slack/webhook
  channels). No command-name collisions with pre-existing commands at base.
- **Wire conformance to the live server (task-124/125 endpoints), checked
  field-by-field.** Request bodies match the server request structs exactly
  (`InviteMemberRequest {user_id, role}`, `UpdateMemberRoleRequest {role, reason}`,
  `CreateTeamRequest {name, description, lead}`, `AddTeamMemberRequest {user_id}`,
  `UpdateNotifPrefsRequest.channels` → `NotificationChannels` nested shape with
  `Hourly`/`High` variant wire names); paths match `api/mod.rs` routes including the
  spec-path `GET /api/v1/notifications` with `?type=` (server `rename = "type"`);
  response parsing matches `UserProfileResponse`, `PublicUserProfileResponse`,
  `MembershipResponse`, `NotificationResponse` (incl. `read` for the `--unread`
  filter).
- **Real client behavior, not stubs.** `api_request` maps non-2xx to errors naming
  the operation; `@handle` resolution goes through `GET /users/:username`; teams
  resolve by slug then name against the real team list; notification preferences
  `set` is a read-merge-write over GET/PUT that preserves untouched channels
  (test-proven: a configured Slack channel survives an email-only update) and
  forces `in_app: true` per §Delivery Channels; `profile set` sends only requested
  fields (partial-update semantics, test-proven); validation (role set, digest,
  priority, CSV rows, `--transfer` requires Owner, `--role` requires `--workspace`,
  `--bulk`/positional exclusivity, empty-value rejection) happens before any
  request (zero-request tests).
- **Honest partial state.** `sessions` and `tenant invite`/`tenant invitations`
  command groups call exactly the spec'd §API paths (verified against
  user-management.md lines 560-563/575-577), but their server endpoints do not
  exist yet (grep of api/mod.rs confirms) — sessions are owned by task-111 and
  tenant invitations by task-110, both not-started; the spec's own §Remaining-Gaps
  lists both as "Specced above; not yet implemented". Coverage row 30 correctly
  stays task-assigned with a detailed note; matrix counts remain consistent
  (11 implemented / 11 n/a / 14 task-assigned = 36, 0 not-started). This mirrors
  the split task-124 recorded for rows 25/26 and is the correct behavior for a
  CLI task whose dependencies (task-124/125) do not include those servers.
- **Review-repair commit is real**: `dacb3b04` removed an `invited_by` column that
  echoed the invitation id (the membership response carries no such field) and
  replaced it with the actual `accepted` flag.

Non-blocking observations (no action required):

- `print_member_list`'s "INVITED" column shows the membership row id; for pending
  members that id is the invitation id (task-124's model: pending invitations are
  unaccepted membership rows), so the value is directly usable with
  `gyre workspace invitations revoke <id>`. Cosmetic labeling at most.
- `notifications list --unread` filters client-side over the server's max page
  (200) — documented in docs/cli.md.
- A few request-line assertions are prefix-based (`starts_with`), so a
  suffix-appending path mutation can evade them; mitigated by exact-JSON body
  assertions on the same captured requests (the mutation probe proves the body
  assertions catch real defects).

Deferred to the independent verifier (build-budget policy): `cargo test --all`
(task acceptance criterion), `cargo clippy --all-targets`, full mechanical suite.
No server-side, web, or script files were changed by this candidate.
