# Coverage: User Management & Notifications

**Spec:** [`system/user-management.md`](../../system/user-management.md)
**Last audited:** 2026-10-07 (task-208 landed — spec §"My Stuff" Views amended to record supersession: `ui-navigation.md` §2 workspace home is the landing page, and My Tasks / My MRs / My Agents have no per-user surface per `human-system-interface.md` §12 "What the Profile Is NOT"; §UI Pages "My Dashboard" row removed with a supersession note; the M22.8 `/users/me/{agents,tasks,mrs}` "✅ Implemented" row struck to **Removed**. Row 22 reclassified `n/a` (no implementable requirement survives the amendment); row 23's task-208 dependency satisfied. Counts: 10→11 n/a, 26→25 task-assigned. Prior — 2026-09-29 full audit: all 10 `n/a` rows spot-checked correct: Problem/Notifications-Problem are context, Notifications/"My Stuff"/API are heading-only, Remaining-Gaps subsections explicitly "Not Yet Specced" per spec, Relationship is cross-reference, M22.8 Baseline is a status assessment with no implementable requirement. All 26 task-assigned rows verified against existing not-started task files (task-110/111/112/113/114/120/121/122/123/124/125/126/127/208). Row 22/23 supersession notes confirmed current at that time (spec amendment was still pending). Header n/a count corrected 9→10. No status changes: 0 implemented, 0 verified rows in file.)
**Coverage:** 0/36 (11 n/a, 0 not-started, 25 task-assigned, 0 implemented, 0 verified)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | User Entity | 2 | task-assigned | task-120 | |
| 3 | Username vs Display Name | 3 | task-assigned | task-120 | |
| 4 | Workspace Membership | 2 | task-assigned | task-121 | |
| 5 | Tenant-Level User Onboarding | 3 | task-assigned | task-110 | |
| 6 | Workspace Invitation Flow | 3 | task-assigned | task-110 | |
| 7 | Invitation Expiry | 3 | task-assigned | task-110 | |
| 8 | Ownership Transfer & Reclamation | 3 | task-assigned | task-121 | |
| 9 | Repo Access | 3 | task-assigned | task-121 | |
| 10 | Team Management | 2 | task-assigned | task-122 | |
| 11 | User Preferences | 2 | task-assigned | task-120 | |
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
