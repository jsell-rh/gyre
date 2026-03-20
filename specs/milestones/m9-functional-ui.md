# M9: Functional User Interface

## Goal

Make the Gyre web UI a fully functional application with working user journeys,
not just rendered components. Every view must be interactive, data must be
available on first load, and authentication must work end-to-end.

## Deliverables

### M9.1 Backend Seed Data
- `POST /api/v1/admin/seed` — idempotent endpoint that populates demo data
- Projects, repos, agents (various statuses), tasks (all kanban columns), MRs, activity events, merge queue entries
- Enables immediate dashboard engagement without manual API calls

### M9.2 Frontend CRUD
- Create modals for Projects, Repositories, Tasks from their respective views
- Dashboard quick-action buttons (New Project, New Task, Seed Demo Data)
- All mutations use toast notifications for feedback
- Full user journey: create project -> add repo -> create task -> create MR -> enqueue

### M9.3 Auth Integration
- All API calls include `Authorization: Bearer {token}` header
- Token stored in localStorage, configurable from UI
- Dev mode auto-authenticates with system token
- WebSocket and REST use the same token source
- Auth status indicator in topbar

## Acceptance Criteria

- [ ] User can load dashboard and see populated data (via seed or manual creation)
- [ ] User can create projects, repos, and tasks from the UI
- [ ] Admin panel loads without 401 errors
- [ ] Agent spawn works from the UI
- [ ] All API calls are authenticated
- [ ] `cargo test --all` passes
- [ ] `npx vite build` succeeds
