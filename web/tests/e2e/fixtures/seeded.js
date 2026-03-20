import { test as base } from '@playwright/test';

const BASE = process.env.PLAYWRIGHT_BASE_URL || 'http://localhost:2222';
// In CI: GYRE_AUTH_TOKEN=e2e-test-token. Locally: use the dev server token.
const TOKEN = process.env.GYRE_AUTH_TOKEN || process.env.GYRE_E2E_TOKEN || 'test-token';

/**
 * Playwright fixture that seeds demo data before the test and sets the auth
 * token in localStorage so all API calls succeed.
 */
export const test = base.extend({
  page: async ({ page }, use) => {
    // Seed demo data via API (idempotent — safe to call multiple times)
    try {
      await fetch(`${BASE}/api/v1/admin/seed`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${TOKEN}` },
      });
    } catch {
      // Server may not support seed — continue anyway
    }

    // Set auth token in localStorage before any page load
    await page.addInitScript((token) => {
      localStorage.setItem('gyre_auth_token', token);
    }, TOKEN);

    await use(page);
  },
});

export { expect } from '@playwright/test';
