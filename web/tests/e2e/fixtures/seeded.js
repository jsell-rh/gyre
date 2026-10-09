import { test as base } from '@playwright/test';

const BASE = process.env.PLAYWRIGHT_BASE_URL || 'http://localhost:2222';
// In CI: GYRE_AUTH_TOKEN=e2e-test-token. Locally: use the dev server token.
const TOKEN = process.env.GYRE_AUTH_TOKEN || process.env.GYRE_E2E_TOKEN || 'test-token';

/** Real seed fixture identities (crates/gyre-server/src/api/admin.rs admin_seed):
 *  workspace slug "default", first repo "gyre-core" (seed-repo-1). Tests must
 *  use these constants, never invented names — a test that navigates to a
 *  nonexistent repo silently exercises the fallback path instead of repo mode. */
export const SEED_WORKSPACE_SLUG = 'default';
export const SEED_REPO_NAME = 'gyre-core';

/**
 * Playwright fixture that seeds demo data before the test and sets the auth
 * token in localStorage so all API calls succeed.
 *
 * Seed failures THROW instead of being swallowed: every test that depends on
 * seeded data fails fast with the real cause, rather than timing out on a
 * missing-workspace workspace home five assertions later.
 */
export const test = base.extend({
  page: async ({ page }, use) => {
    // Seed demo data via API (idempotent — safe to call multiple times).
    // The static auth token resolves as tenant "default" with Admin role;
    // the seed handler creates the demo workspace in the caller's tenant so
    // the SPA's workspace list (filtered by the same tenant) sees it.
    const resp = await fetch(`${BASE}/api/v1/admin/seed`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${TOKEN}` },
    });
    if (!resp.ok) {
      throw new Error(`e2e seed failed: POST ${BASE}/api/v1/admin/seed → ${resp.status} ${await resp.text().catch(() => '')}`);
    }

    // Verify the seeded workspace is actually visible to this caller before
    // running any test: a 200 from the seed endpoint with zero visible
    // workspaces means every workspace-scoped test is about to fail — fail
    // here with the scope mismatch named.
    const wsResp = await fetch(`${BASE}/api/v1/workspaces`, {
      headers: { Authorization: `Bearer ${TOKEN}` },
    });
    if (!wsResp.ok) {
      throw new Error(`e2e workspace list failed: GET ${BASE}/api/v1/workspaces → ${wsResp.status}`);
    }
    const workspaces = await wsResp.json();
    if (!Array.isArray(workspaces) || !workspaces.some(w => w.slug === SEED_WORKSPACE_SLUG)) {
      throw new Error(
        `e2e seed produced no visible workspace with slug "${SEED_WORKSPACE_SLUG}" for this caller ` +
        `(got ${JSON.stringify(workspaces?.map(w => ({ id: w.id, slug: w.slug, tenant_id: w.tenant_id })))}) — ` +
        'tenant-scope mismatch between seed and caller'
      );
    }

    // Set auth token in localStorage before any page load
    await page.addInitScript((token) => {
      localStorage.setItem('gyre_auth_token', token);
    }, TOKEN);

    await use(page);
  },
});

export { expect } from '@playwright/test';
