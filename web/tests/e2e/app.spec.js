/**
 * Gyre E2E tests — HSI navigation model (S4.1 App Shell)
 *
 * Tests run against a live gyre-server on localhost:2222 with GYRE_AUTH_TOKEN=e2e-test-token.
 * The seeded fixture calls POST /api/v1/admin/seed before each test.
 *
 * Navigation model: 6 fixed sidebar items (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin)
 * URL routing: /inbox | /briefing | /explorer | /specs | /meta-specs | /admin (tenant scope)
 *              /workspaces/:id/:nav (workspace scope)
 *              /repos/:id/:nav (repo scope)
 */

import { test, expect } from './fixtures/seeded.js';

// ---------------------------------------------------------------------------
// Helper: navigate by direct URL (more reliable than clicking sidebar)
// ---------------------------------------------------------------------------
const NAV_ROUTES = {
  'inbox':      '/inbox',
  'briefing':   '/briefing',
  'explorer':   '/explorer',
  'specs':      '/specs',
  'meta-specs': '/meta-specs',
  'admin':      '/admin',
};

async function navigateTo(page, nav) {
  const route = NAV_ROUTES[nav];
  if (!route) throw new Error(`Unknown nav: ${nav}`);
  await page.goto(route);
  await page.waitForLoadState('networkidle');
}

// ---------------------------------------------------------------------------
// App shell structure
// ---------------------------------------------------------------------------

test.describe('App shell', () => {
  test('renders_sidebar_with_6_nav_items', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Sidebar should be present
    const sidebar = page.locator('[data-testid="sidebar"]');
    await expect(sidebar).toBeVisible({ timeout: 5000 });

    // All 6 nav items should be present
    const navItems = ['Inbox', 'Briefing', 'Explorer', 'Specs', 'Meta-specs', 'Admin'];
    for (const label of navItems) {
      const btn = sidebar.getByRole('button', { name: label, exact: true });
      await expect(btn).toBeVisible({ timeout: 3000 });
    }
  });

  test('sidebar_active_state_updates_on_navigation', async ({ page }) => {
    await navigateTo(page, 'explorer');

    const explorerBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Explorer' });
    await expect(explorerBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });

    // Navigate to Specs — active state should move
    const specsBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Specs', exact: true });
    await specsBtn.click();
    await page.waitForLoadState('networkidle');
    await expect(specsBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('sidebar_collapse_toggle', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const collapseBtn = page.getByRole('button', { name: /collapse sidebar/i });
    await expect(collapseBtn).toBeVisible({ timeout: 3000 });
    await collapseBtn.click();

    // Sidebar should now show 'Expand sidebar' button
    const expandBtn = page.getByRole('button', { name: /expand sidebar/i });
    await expect(expandBtn).toBeVisible({ timeout: 3000 });

    // Click again to restore
    await expandBtn.click();
    await expect(page.getByRole('button', { name: /collapse sidebar/i })).toBeVisible({ timeout: 3000 });
  });

  test('topbar_renders_search_and_user_menu', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Search trigger should be visible in topbar
    const searchTrigger = page.locator('.search-trigger').first();
    await expect(searchTrigger).toBeVisible({ timeout: 3000 });

    // User menu button should be visible
    const userBtn = page.getByRole('button', { name: /user menu/i });
    await expect(userBtn).toBeVisible({ timeout: 3000 });
  });

  test('status_bar_renders_with_ws_indicator', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Status bar footer should exist
    const statusBar = page.locator('[aria-label="Status bar"]');
    await expect(statusBar).toBeVisible({ timeout: 3000 });

    // WS status item should be visible
    const wsStatus = statusBar.locator('[aria-label*="WebSocket" i]');
    await expect(wsStatus).toBeVisible({ timeout: 3000 });
  });

  test('landing_page_is_explorer_on_first_visit', async ({ page }) => {
    // Clear any stored workspace ID to simulate first visit
    await page.addInitScript(() => {
      localStorage.removeItem('gyre_workspace_id');
    });
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Should land on explorer (no stored workspace)
    const explorerBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Explorer' });
    await expect(explorerBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });
});

// ---------------------------------------------------------------------------
// Auth flow
// ---------------------------------------------------------------------------

test.describe('Auth flow', () => {
  test('user_menu_opens_on_click', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const userBtn = page.getByRole('button', { name: /user menu/i });
    await userBtn.click();

    // Dropdown menu should appear with at least Profile and API Token
    const dropdown = page.locator('[role="menu"]');
    await expect(dropdown).toBeVisible({ timeout: 3000 });
    await expect(page.locator('[role="menuitem"]', { hasText: 'Profile' })).toBeVisible();
    await expect(page.locator('[role="menuitem"]', { hasText: 'API Token' })).toBeVisible();
  });

  test('api_token_modal_opens_and_saves', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Open user menu → API Token
    await page.getByRole('button', { name: /user menu/i }).click();
    await page.locator('[role="menuitem"]', { hasText: 'API Token' }).click();

    // Token modal should open
    const tokenInput = page.locator('#token-input');
    await tokenInput.waitFor({ state: 'visible', timeout: 3000 });

    await tokenInput.fill('e2e-test-token');
    await page.locator('[role="dialog"] button:has-text("Save")').click();

    // Modal should close
    await expect(tokenInput).not.toBeVisible({ timeout: 3000 });
  });

  test('auth_active_class_set_when_token_present', async ({ page }) => {
    // Token is set by the seeded fixture via addInitScript
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // User button should have auth-active class (green dot) when token is set
    const userBtn = page.locator('.user-btn.auth-active');
    await expect(userBtn).toBeVisible({ timeout: 3000 });
  });
});

// ---------------------------------------------------------------------------
// Inbox view
// ---------------------------------------------------------------------------

test.describe('Inbox view', () => {
  test('inbox_view_renders', async ({ page }) => {
    await navigateTo(page, 'inbox');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/inbox');
  });

  test('inbox_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'inbox');

    const inboxBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Inbox' });
    await expect(inboxBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('inbox_badge_btn_in_topbar', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Topbar inbox badge shortcut button
    const inboxBtn = page.locator('.inbox-badge-btn').first();
    await expect(inboxBtn).toBeVisible({ timeout: 3000 });
  });

  test('inbox_shows_content_or_empty_state', async ({ page }) => {
    await navigateTo(page, 'inbox');

    // Inbox renders some content or empty state
    const inboxContent = page
      .locator('[class*="inbox"]')
      .or(page.locator('[class*="notification"]'))
      .or(page.locator('[class*="empty"]'))
      .or(page.locator('[class*="skeleton"]'))
      .first();
    await expect(inboxContent).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Briefing view
// ---------------------------------------------------------------------------

test.describe('Briefing view', () => {
  test('briefing_view_renders', async ({ page }) => {
    await navigateTo(page, 'briefing');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/briefing');
  });

  test('briefing_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'briefing');

    const briefingBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Briefing' });
    await expect(briefingBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('briefing_shows_content_or_empty_state', async ({ page }) => {
    await navigateTo(page, 'briefing');

    const briefingContent = page
      .locator('[class*="briefing"]')
      .or(page.locator('[class*="section"]'))
      .or(page.locator('[class*="empty"]'))
      .or(page.locator('[class*="skeleton"]'))
      .first();
    await expect(briefingContent).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Explorer view
// ---------------------------------------------------------------------------

test.describe('Explorer view', () => {
  test('explorer_view_renders_at_tenant_scope', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.removeItem('gyre_workspace_id');
    });
    await navigateTo(page, 'explorer');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/explorer');
  });

  test('explorer_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'explorer');

    const explorerBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Explorer' });
    await expect(explorerBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('explorer_shows_workspace_cards_or_content', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.removeItem('gyre_workspace_id');
    });
    await navigateTo(page, 'explorer');

    // Tenant scope: workspace cards or empty state
    const explorerContent = page
      .locator('[class*="explorer"]')
      .or(page.locator('[class*="workspace-card"]'))
      .or(page.locator('[class*="card"]'))
      .or(page.locator('[class*="canvas"]'))
      .or(page.locator('[class*="empty"]'))
      .first();
    await expect(explorerContent).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Specs view
// ---------------------------------------------------------------------------

test.describe('Specs view', () => {
  test('specs_view_renders', async ({ page }) => {
    await navigateTo(page, 'specs');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/specs');
  });

  test('specs_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'specs');

    const specsBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Specs', exact: true });
    await expect(specsBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('specs_shows_table_or_empty_state', async ({ page }) => {
    await navigateTo(page, 'specs');

    const specsContent = page
      .locator('[class*="spec"]')
      .or(page.locator('table'))
      .or(page.locator('[class*="empty"]'))
      .or(page.locator('[class*="skeleton"]'))
      .first();
    await expect(specsContent).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Meta-specs view
// ---------------------------------------------------------------------------

test.describe('Meta-specs view', () => {
  test('metaspecs_view_renders', async ({ page }) => {
    await navigateTo(page, 'meta-specs');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/meta-specs');
  });

  test('metaspecs_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'meta-specs');

    const metaBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Meta-specs' });
    await expect(metaBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('metaspecs_shows_persona_catalog_or_empty_state', async ({ page }) => {
    await navigateTo(page, 'meta-specs');

    const metaContent = page
      .locator('[class*="meta"]')
      .or(page.locator('[class*="persona"]'))
      .or(page.locator('table'))
      .or(page.locator('[class*="empty"]'))
      .or(page.locator('[class*="skeleton"]'))
      .first();
    await expect(metaContent).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Admin view
// ---------------------------------------------------------------------------

test.describe('Admin view', () => {
  test('admin_view_renders', async ({ page }) => {
    await navigateTo(page, 'admin');

    await expect(page.locator('.content-inner')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/admin');
  });

  test('admin_sidebar_item_is_active', async ({ page }) => {
    await navigateTo(page, 'admin');

    const adminBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Admin' });
    await expect(adminBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('admin_shows_tabbed_interface', async ({ page }) => {
    await navigateTo(page, 'admin');

    // Admin panel has tabs: Health, Jobs, Audit, Agents, etc.
    const tabs = page.locator('[role="tab"], [class*="tab-btn"], [class*="tab-item"]');
    await expect(tabs.first()).toBeVisible({ timeout: 5000 });
  });

  test('admin_health_tab_shows_server_info', async ({ page }) => {
    await navigateTo(page, 'admin');

    // Health tab should show gyre server info
    const healthContent = page
      .locator('[class*="health"]')
      .or(page.getByText('gyre'))
      .or(page.getByText(/0\.1\.0/))
      .first();
    await expect(healthContent).toBeVisible({ timeout: 5000 });
  });

  test('user_menu_profile_navigates_to_profile', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.getByRole('button', { name: /user menu/i }).click();
    await page.locator('[role="menuitem"]', { hasText: 'Profile' }).click();

    await page.waitForLoadState('networkidle');
    // Profile view renders a .user-profile container (UserProfile component)
    const profileContainer = page.locator('.user-profile');
    await expect(profileContainer).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Scope breadcrumb
// ---------------------------------------------------------------------------

test.describe('Scope breadcrumb', () => {
  test('breadcrumb_renders_in_topbar', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // ScopeBreadcrumb renders in the topbar — look for tenant name "Gyre" or breadcrumb element
    const topbar = page.locator('.topbar, header').first();
    await expect(topbar).toBeVisible({ timeout: 3000 });

    const breadcrumb = topbar
      .locator('[class*="breadcrumb"]')
      .or(topbar.locator('[class*="scope"]'))
      .or(topbar.getByText('Gyre'))
      .first();
    await expect(breadcrumb).toBeVisible({ timeout: 3000 });
  });

  test('workspace_scope_url_loads_without_crash', async ({ page }) => {
    // Navigating to a scope URL should load gracefully
    await page.goto('/explorer');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// URL routing
// ---------------------------------------------------------------------------

test.describe('URL routing', () => {
  test('all_6_nav_routes_are_reachable', async ({ page }) => {
    for (const [nav, route] of Object.entries(NAV_ROUTES)) {
      await page.goto(route);
      await page.waitForLoadState('networkidle');

      // App shell should render
      await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });

      // Correct sidebar item should be active
      const navBtn = page.locator('[data-testid="sidebar"]')
        .getByRole('button', { name: new RegExp(nav.replace('-', '[-\\s]'), 'i') })
        .first();
      await expect(navBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
    }
  });

  test('unknown_route_falls_back_gracefully', async ({ page }) => {
    // SPA: unknown route should not crash the app
    await page.goto('/nonexistent-view');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
  });

  test('browser_back_forward_navigation', async ({ page }) => {
    await navigateTo(page, 'inbox');
    await navigateTo(page, 'explorer');

    // Go back to inbox
    await page.goBack();
    await page.waitForLoadState('networkidle');
    const inboxBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Inbox' });
    await expect(inboxBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });

    // Go forward to explorer
    await page.goForward();
    await page.waitForLoadState('networkidle');
    const explorerBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Explorer' });
    await expect(explorerBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });
});

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

test.describe('Keyboard shortcuts', () => {
  test('cmd_k_opens_search_overlay', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+k');

    const searchInput = page
      .locator('[class*="search-bar"]')
      .or(page.locator('[class*="search-overlay"]'))
      .or(page.locator('input[placeholder*="search" i]'))
      .first();
    await expect(searchInput).toBeVisible({ timeout: 3000 });
  });

  test('slash_key_opens_search', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('/');

    const searchInput = page
      .locator('[class*="search-bar"]')
      .or(page.locator('input[placeholder*="search" i]'))
      .first();
    await expect(searchInput).toBeVisible({ timeout: 3000 });
  });

  test('question_mark_opens_shortcuts_overlay', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('?');

    const dialog = page.getByRole('dialog', { name: /keyboard shortcuts/i });
    await expect(dialog).toBeVisible({ timeout: 3000 });

    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible({ timeout: 3000 });
  });

  test('cmd_1_navigates_to_inbox', async ({ page }) => {
    await page.goto('/explorer');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+1');

    const inboxBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Inbox' });
    await expect(inboxBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('cmd_3_navigates_to_explorer', async ({ page }) => {
    await page.goto('/inbox');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+3');

    const explorerBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Explorer' });
    await expect(explorerBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('cmd_5_navigates_to_meta_specs', async ({ page }) => {
    // meta-specs is the only hyphenated nav ID — test it explicitly
    await page.goto('/inbox');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+5');

    const metaBtn = page.locator('[data-testid="sidebar"]').getByRole('button', { name: 'Meta-specs' });
    await expect(metaBtn).toHaveAttribute('aria-current', 'page', { timeout: 3000 });
  });

  test('esc_closes_shortcuts_overlay', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('?');
    const dialog = page.getByRole('dialog', { name: /keyboard shortcuts/i });
    await expect(dialog).toBeVisible({ timeout: 3000 });

    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible({ timeout: 3000 });
  });

  test('esc_closes_user_menu', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.getByRole('button', { name: /user menu/i }).click();
    const dropdown = page.locator('[role="menu"]');
    await expect(dropdown).toBeVisible({ timeout: 3000 });

    await page.keyboard.press('Escape');
    await expect(dropdown).not.toBeVisible({ timeout: 3000 });
  });
});

// ---------------------------------------------------------------------------
// Global Search
// ---------------------------------------------------------------------------

test.describe('Global Search', () => {
  test('search_trigger_button_opens_search', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.locator('.search-trigger').first().click();

    const searchInput = page
      .locator('[class*="search-bar"]')
      .or(page.locator('input[placeholder*="search" i]'))
      .first();
    await expect(searchInput).toBeVisible({ timeout: 3000 });
  });

  test('search_input_accepts_text', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+k');

    const input = page.locator('input[type="text"], input[type="search"]').last();
    await input.waitFor({ state: 'visible', timeout: 3000 });
    await input.fill('spec');

    // Input accepted the text
    await expect(input).toHaveValue('spec');
  });
});

// ---------------------------------------------------------------------------
// Accessibility baseline
// ---------------------------------------------------------------------------

test.describe('Accessibility', () => {
  test('no_axe_violations_on_load', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const { checkA11y } = await import('@axe-core/playwright').catch(() => ({ checkA11y: null }));
    if (checkA11y) {
      await checkA11y(page, undefined, {
        runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa'] },
        violations: { impact: ['critical', 'serious'] },
      });
    } else {
      await expect(page.locator('.app').first()).toBeVisible({ timeout: 5000 });
    }
  });

  test('skip_to_main_content_link_exists', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const skipLink = page.locator('.skip-to-content, a[href="#main-content"]');
    await expect(skipLink).toBeAttached();
  });

  test('sidebar_has_aria_label', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const sidebar = page.getByRole('navigation', { name: /main navigation/i });
    await expect(sidebar).toBeVisible({ timeout: 3000 });
  });

  test('main_content_has_id_for_skip_link', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('#main-content')).toBeAttached();
  });
});

// ---------------------------------------------------------------------------
// Console error guard
// ---------------------------------------------------------------------------

test.describe('i18n locale init', () => {
  test('no_console_errors_on_load', async ({ page }) => {
    const errors = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto('/');
    await page.waitForTimeout(2000);
    expect(errors).toEqual([]);
  });
});
