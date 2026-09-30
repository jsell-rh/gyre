# Coverage: User Management & Notifications

**Spec:** [`system/user-management.md`](../../system/user-management.md)
**Last audited:** 2026-04-13
**Coverage:** 0/36 (9 n/a)

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
| 22 | My Dashboard (Landing Page After Login) | 3 | task-assigned | task-208 | SUPERSEDED (2026-09-30 PM cycle, conflict resolution with HSI §12): landing-page role taken by ui-navigation.md §2 workspace home; My Tasks/MRs/Agents rows forbidden in /profile by HSI §12 "What the Profile Is NOT". task-208 amends user-management.md §"My Stuff" Views to record the supersession and removes the /users/me/{agents,tasks,mrs} endpoints + profile tabs; this row should be reclassified n/a when that amendment lands. task-114 (original assignee) rescoped to /@{username} only. |
| 23 | User Profile Page (`/@{username}`) | 3 | task-assigned | task-114 | Rescoped 2026-09-30: task-114 now covers only the public /@{username} profile (My Dashboard scope removed as superseded). Depends on task-208 (spec amendment keeps /@{username} text unchanged). |
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
