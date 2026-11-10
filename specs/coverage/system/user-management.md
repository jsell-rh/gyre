# Coverage: User Management & Notifications

**Spec:** [`system/user-management.md`](../../system/user-management.md)
**Last audited:** 2026-11-04 (merged audit after task-120 + task-208 both landed on this branch). Task-208 (2026-10-07) — spec §"My Stuff" Views amended to record supersession: `ui-navigation.md` §2 workspace home is the landing page, and My Tasks / My MRs / My Agents have no per-user surface per `human-system-interface.md` §12 "What the Profile Is NOT"; §UI Pages "My Dashboard" row removed with a supersession note; the M22.8 `/users/me/{agents,tasks,mrs}` "✅ Implemented" row struck to **Removed**. Row 22 reclassified `n/a` (no implementable requirement survives the amendment); row 23's task-208 dependency satisfied. Task-120 (2026-10-09) — rows 2, 3, and 11 implemented (User Entity, Username vs Display Name, User Preferences: domain types, migration, `PUT /users/me` partial update, SCIM cutover). Merged counts: 10→11 n/a, 26→22 task-assigned, 0→3 implemented. Prior — 2026-09-29 full audit: all 10 `n/a` rows spot-checked correct: Problem/Notifications-Problem are context, Notifications/"My Stuff"/API are heading-only, Remaining-Gaps subsections explicitly "Not Yet Specced" per spec, Relationship is cross-reference, M22.8 Baseline is a status assessment with no implementable requirement. All 26 task-assigned rows verified against existing not-started task files (task-110/111/112/113/114/120/121/122/123/124/125/126/127/208). Header n/a count corrected 9→10.
**Coverage:** 3/36 (11 n/a, 0 not-started, 22 task-assigned, 3 implemented, 0 verified)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | User Entity | 2 | implemented | task-120 | Implemented (task-120, 2026-10-09) — `User` (gyre-domain/src/user.rs) carries every spec field: username, display_name, avatar_url, timezone, locale, tenant_id, global_role, preferences, last_login_at, created_at, updated_at, plus legacy roles/email. `UserPreferences` (7 fields) with Theme/UiDensity/DiffView/FeedScope enums matching spec variants exactly. Migration 000058 (`2026-10-08-000058_user_entity_profile_fields`; renumbered twice — 000056→000057 after main's task-180 landed `spec_assertion_results` under the same Diesel version, then 000057→000058 after main's task-193 landed `spec_approval_attestation` under `20261008000057`, which would have silently dropped this one — Diesel runs one migration per version) adds the persisted columns (username, avatar_url, preferences JSON, last_login_at, tenant_id, global_role) with defaults (timezone UTC, locale en-US) and backfills legacy rows; the F1 parser-overflow repair runs the sanitizer as 38 sequential depth-1 UPDATEs over a scratch column — verified on SQLite by `migration_000058_backfills_unique_url_safe_usernames` (gyre-adapters). `PUT /api/v1/users/me` returns and partially updates profile fields. Reviewed in specs/reviews/task-120.md Round 2 (PASS) and landed on main. |
| 3 | Username vs Display Name | 3 | implemented | task-120 | Implemented (task-120, 2026-10-09) — handle contract enforced across the full chain. `validate_username`: 1–64 chars, [a-z0-9_-], no edge/consecutive separators. `sanitize_username`: lowercases and maps separators to `-` (Jordan.Sell@example.com → jordan-sell-example-com). Auth flow (`find_or_create_user`, auth.rs): first login derives the handle from SSO `preferred_username` (sub fallback when unsanitizable), stamps `last_login_at`; re-login never renames when the IdP changes preferred_username. Immutability guarded at the adapter layer in all three implementations (sqlite/postgres user.rs, mem.rs) with rejection tests. Uniqueness via create-contract checks + `idx_users_username`. SCIM cutover (F2 repair): `scim_create_user` sanitizes userName (externalId fallback, 400 when neither yields a usable handle, 409 on duplicate); `scim_update_user` ignores changed userName/externalId — immutable identity after creation. |
| 4 | Workspace Membership | 2 | task-assigned | task-121 | |
| 5 | Tenant-Level User Onboarding | 3 | task-assigned | task-110 | |
| 6 | Workspace Invitation Flow | 3 | task-assigned | task-110 | |
| 7 | Invitation Expiry | 3 | task-assigned | task-110 | |
| 8 | Ownership Transfer & Reclamation | 3 | task-assigned | task-121 | |
| 9 | Repo Access | 3 | task-assigned | task-121 | |
| 10 | Team Management | 2 | task-assigned | task-122 | |
| 11 | User Preferences | 2 | implemented | task-120 | Implemented (task-120, 2026-10-09) — stored server-side as JSON in `users.preferences` (migration backfills `UserPreferences::default()`), returned by `GET /users/me`, updated by `PUT /users/me` with genuine partial-update semantics: `UserPreferencesPatch` + `apply_patch` merge omitted fields over stored values (omitted notification_channels/default_workspace_id keep stored values — kills replace-with-defaults), malformed theme → 400 with nothing applied. Reviewed in specs/reviews/task-120.md Round 2 (PASS) and landed on main. |
| 12 | Session Management | 2 | task-assigned | task-111 | |
| 13 | Notifications | 2 | n/a | - | Section heading only — no implementable requirement. |
| 14 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 15 | Notification Entity | 3 | task-assigned | task-123 | |
| 16 | Delivery Channels | 3 | task-assigned | task-112 | |
| 17 | Who Gets Notified | 3 | task-assigned | task-112 | |
| 18 | Notification Routing for Agent Escalations | 3 | task-assigned | task-112 | |
| 19 | In-App Notification UI | 3 | task-assigned | task-113 | |
| 20 | Email Notifications | 3 | task-assigned | task-113 | |
| 21 | "My Stuff" Views | 2 | n/a | - | Section heading only — no implementable requirement. |
| 22 | My Dashboard (Landing Page After Login) | 3 | n/a | - | Reclassified `n/a` (2026-10-07, task-208): no implementable requirement remains after the spec amendment. Landing-page role belongs to `ui-navigation.md` §2 workspace home; My Tasks/My MRs/My Agents are forbidden as per-user surfaces by HSI §12 "What the Profile Is NOT"; Pending Approvals / My Notifications / Recent Activity are served by the workspace home Decisions section, notification bell, and Briefing. Deletions: `users.rs` `get_my_{agents,tasks,mrs}` handlers, their routes (`api/mod.rs`), ABAC mappings (`abac_middleware.rs`), `api.js` `myAgents/myTasks/myMrs`, and the three `/profile` tabs. |
| 23 | User Profile Page (`/@{username}`) | 3 | task-assigned | task-114 | Rescoped 2026-09-30: task-114 now covers only the public /@{username} profile (My Dashboard scope removed as superseded). Depends on task-208 — dependency satisfied 2026-10-07: the amendment left the `/@{username}` section text unchanged, as required. |
| 24 | API | 2 | n/a | - | Section heading only — no implementable requirement. |
| 25 | Tenant Invitations | 3 | task-assigned | task-110 | |
| 26 | User Management | 3 | task-assigned | task-124 | |
| 27 | Workspace Membership | 3 | task-assigned | task-124 | |
| 28 | Teams | 3 | task-assigned | task-125 | |
| 29 | Notifications | 3 | task-assigned | task-125 | |
| 30 | CLI | 2 | task-assigned | task-126 | |
| 31 | UI Pages | 2 | task-assigned | task-127 | |
| 32 | Remaining Gaps for Enterprise Readiness | 2 | n/a | - | Future work overview — not yet specced. |
| 33 | External Integrations / Webhooks (Not Yet Specced) | 3 | n/a | - | Not yet specced — no implementable requirement. |
| 34 | Deployment / CD (Not Yet Specced) | 3 | n/a | - | Not yet specced — no implementable requirement. |
| 35 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
| 36 | Completeness Assessment (M22.8 Baseline) | 2 | n/a | - | Status assessment — no implementable requirement. |
