# Frontend UI Testing Strategy

## Problem

The Gyre frontend has shipped multiple critical bugs that should have been caught by automated testing:
- `$state` rune used in plain `.js` file (runtime ReferenceError)
- Empty data on all views (no seed data, no create actions)
- Auth headers missing from API calls (401 on admin endpoints)
- Components rendering but user journey non-functional

These are environment failures. The Ralph loop must include frontend validation gates
that catch these categories before code reaches main.

## Testing Layers

### Layer 1: Build-Time Validation (vite build)

Already implemented:
- `svelteRuneLint` plugin catches `$state`/`$derived` in plain `.js` files
- Svelte compiler warnings (a11y, state_referenced_locally)

Needs addition:
- **TypeScript checking** for `.svelte` and `.js` files (add `svelte-check`)
- **Unused import detection**
- **Build must fail on warnings** (currently warnings are logged but don't fail)

### Layer 2: Component Unit Tests (vitest + @testing-library/svelte)

Test each component in isolation:
- Renders without errors
- Displays loading skeleton, then data or empty state
- API calls include auth headers
- Create modals open, validate, submit, show toast
- Navigation between views works

Framework: `vitest` (Vite-native test runner) + `@testing-library/svelte`

### Layer 3: Integration Tests (Playwright)

End-to-end browser tests against a running Gyre server:
- Load dashboard, verify metric cards render
- Seed demo data via UI button, verify views populate
- Create project -> create repo -> create task (full user journey)
- Admin panel loads without 401
- WebSocket connection indicator shows connected
- Auth token modal works

Framework: `playwright` with chromium

### Layer 4: Visual Regression (optional, future)

Screenshot comparison for design system compliance.

## Implementation Plan

### Phase 1: vitest + component tests (immediate)

```
web/
  vitest.config.js
  src/
    __tests__/
      DashboardHome.test.js
      ProjectList.test.js
      TaskBoard.test.js
      AgentList.test.js
      api.test.js
```

Install: `npm install -D vitest @testing-library/svelte jsdom`

Minimum test coverage:
- Every page component renders without throwing
- `api.js` request() includes Authorization header
- Toast notifications fire on success/error
- Modal open/close works
- Empty state renders when data is []

### Phase 2: Playwright E2E (follow-up)

```
web/
  e2e/
    dashboard.spec.js
    user-journey.spec.js
    auth.spec.js
```

Install: `npx playwright install`

Minimum scenarios:
- Dashboard loads and shows metric cards
- Seed button populates all views
- Full create project -> task -> MR flow
- Auth token change updates API calls

## CI Integration

Frontend tests run as part of the build gate:
1. `npx vitest run` -- component tests (fast, no browser needed)
2. `npx vite build` -- build validation (rune lint, compiler warnings)
3. `npx playwright test` -- E2E (requires running server, slower)

All three must pass before merge.

## Relationship to Existing Specs

- **Agent Runtime** (`system/agent-runtime.md` §1): Frontend tests are a quality gate in every agent's loop
- **Speed & Backpressure** (`development/speed-backpressure.md`): vitest is sub-second for component tests
- **Agent Experience** (`development/agent-experience.md`): Agents get immediate feedback on UI breaks
