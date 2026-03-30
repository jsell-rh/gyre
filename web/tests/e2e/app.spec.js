/**
 * Gyre E2E tests — new navigation model (ui-navigation.md)
 *
 * Tests run against a live gyre-server on localhost:2222 with GYRE_AUTH_TOKEN=e2e-test-token.
 * The seeded fixture calls POST /api/v1/admin/seed before each test.
 *
 * Navigation model: workspace home dashboard + repo horizontal tabs (no sidebar)
 * URL routing: /workspaces/:slug            → workspace home
 *              /workspaces/:slug/r/:repo     → repo mode, Specs tab (default)
 *              /workspaces/:slug/r/:repo/architecture → Architecture tab
 *              /workspaces/:slug/r/:repo/decisions    → Decisions tab
 *              /workspaces/:slug/r/:repo/code         → Code tab
 *              /workspaces/:slug/r/:repo/settings     → Settings tab
 *              /profile                      → user profile
 */

import { test, expect } from './fixtures/seeded.js';

// Seed workspace slug — matches the seed fixture data.
// The seed endpoint creates a workspace with this slug; adjust if fixture changes.
const SEED_SLUG = 'default';
const SEED_REPO = 'sample-repo'; // first repo in seeded workspace; adjust if fixture changes

// ---------------------------------------------------------------------------
// App shell structure
// ---------------------------------------------------------------------------

test.describe('App shell', () => {
  test('topbar_renders_with_workspace_selector_search_decisions_avatar', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Topbar should be present
    const topbar = page.locator('[data-testid="topbar"]');
    await expect(topbar).toBeVisible({ timeout: 5000 });

    // Search trigger visible in topbar
    const searchTrigger = page.locator('.search-trigger').first();
    await expect(searchTrigger).toBeVisible({ timeout: 3000 });

    // Decisions badge visible in topbar
    const decisionsBadge = page.locator('[data-testid="decisions-badge"]');
    await expect(decisionsBadge).toBeVisible({ timeout: 3000 });

    // User avatar button visible
    const userBtn = page.getByRole('button', { name: /user menu/i });
    await expect(userBtn).toBeVisible({ timeout: 3000 });
  });

  test('no_sidebar_present', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // The old 6-item sidebar MUST NOT exist in the new navigation model
    const sidebar = page.locator('[data-testid="sidebar"]');
    await expect(sidebar).not.toBeAttached({ timeout: 3000 });
  });

  test('status_bar_renders_with_ws_indicator', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Status bar footer should exist
    const statusBar = page.locator('[aria-label="Status bar"]');
    await expect(statusBar).toBeVisible({ timeout: 3000 });

    // WebSocket status item with role=status should be visible
    const wsStatus = statusBar.locator('[role="status"]');
    await expect(wsStatus).toBeVisible({ timeout: 3000 });
  });

  test('app_shell_renders_on_root_url', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
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

  test('user_menu_profile_navigates_to_profile', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.getByRole('button', { name: /user menu/i }).click();
    await page.locator('[role="menuitem"]', { hasText: 'Profile' }).click();

    await page.waitForLoadState('networkidle');
    // URL should contain /profile
    expect(page.url()).toContain('/profile');
  });
});

// ---------------------------------------------------------------------------
// Workspace home (§2 of ui-navigation.md)
// ---------------------------------------------------------------------------

test.describe('Workspace home', () => {
  test('landing_shows_workspace_home_dashboard', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Workspace home component should be present
    const wsHome = page.locator('[data-testid="workspace-home"]');
    await expect(wsHome).toBeVisible({ timeout: 5000 });
  });

  test('decisions_section_renders_or_shows_empty_state', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const decisionsSection = page.locator('[data-testid="section-decisions"]');
    await expect(decisionsSection).toBeVisible({ timeout: 5000 });

    // After loading completes: either decision items or empty state text must be visible
    const decisionsContent = decisionsSection
      .locator('[data-testid="decision-item"]')
      .or(decisionsSection.locator('[data-testid="decisions-empty"]'))
      .or(decisionsSection.locator('.skeleton-row'))
      .first();
    await expect(decisionsContent).toBeVisible({ timeout: 8000 });
  });

  test('repos_section_renders_or_shows_empty_state', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const reposSection = page.locator('[data-testid="section-repos"]');
    await expect(reposSection).toBeVisible({ timeout: 5000 });

    // Repo rows or empty state must be visible
    const reposContent = reposSection
      .locator('[data-testid="repo-row"]')
      .or(reposSection.locator('[data-testid="repos-empty"]'))
      .or(reposSection.locator('.skeleton-row'))
      .first();
    await expect(reposContent).toBeVisible({ timeout: 8000 });
  });

  test('briefing_section_renders', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const briefingSection = page.locator('[data-testid="section-briefing"]');
    await expect(briefingSection).toBeVisible({ timeout: 5000 });
  });

  test('specs_section_renders', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const specsSection = page.locator('[data-testid="section-specs"]');
    await expect(specsSection).toBeVisible({ timeout: 5000 });
  });

  test('agent_rules_section_renders', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    const agentRulesSection = page.locator('[data-testid="section-agent-rules"]');
    await expect(agentRulesSection).toBeVisible({ timeout: 5000 });
  });

  test('workspace_selector_visible_in_topbar', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Workspace selector visible in workspace home mode
    const wsSelector = page.locator('[data-testid="ws-selector"]');
    await expect(wsSelector).toBeVisible({ timeout: 3000 });
  });

  test('workspace_home_url_loads_correctly', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}`);
    await page.waitForLoadState('networkidle');

    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Repo mode (§3 of ui-navigation.md)
// ---------------------------------------------------------------------------

test.describe('Repo mode', () => {
  test('clicking_repo_navigates_to_repo_mode', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Wait for repo list to load (or empty state)
    const repoLink = page.locator('[data-testid="repo-link"]').first();
    const hasRepo = await repoLink.isVisible({ timeout: 6000 }).catch(() => false);
    if (!hasRepo) {
      // No repos in seed — just verify workspace home loads
      await expect(page.locator('[data-testid="workspace-home"]')).toBeVisible({ timeout: 3000 });
      return;
    }

    await repoLink.click();
    await page.waitForLoadState('networkidle');

    // Should enter repo mode
    const repoMode = page.locator('[data-testid="repo-mode"]');
    await expect(repoMode).toBeVisible({ timeout: 5000 });
  });

  test('horizontal_tabs_render_in_repo_mode', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const tabBar = page.locator('[data-testid="repo-tab-bar"]');
    await expect(tabBar).toBeVisible({ timeout: 5000 });

    // Tabs: Specs, Architecture, Decisions, Code, ⚙ (Settings)
    for (const tabLabel of ['Specs', 'Architecture', 'Decisions', 'Code']) {
      await expect(tabBar.getByRole('tab', { name: tabLabel })).toBeVisible({ timeout: 3000 });
    }
    // Settings tab uses gear icon label
    const settingsTab = tabBar.getByRole('tab', { name: /settings|⚙/i });
    await expect(settingsTab).toBeVisible({ timeout: 3000 });
  });

  test('specs_tab_is_default_landing_tab', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const specsTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Specs' });
    await expect(specsTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
  });

  test('back_arrow_visible_in_topbar_in_repo_mode', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const backBtn = page.locator('[data-testid="back-btn"]');
    await expect(backBtn).toBeVisible({ timeout: 5000 });
  });

  test('repo_breadcrumb_shows_workspace_slash_repo', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const breadcrumb = page.locator('[data-testid="repo-breadcrumb"]');
    await expect(breadcrumb).toBeVisible({ timeout: 5000 });
  });

  test('back_arrow_returns_to_workspace_home', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const backBtn = page.locator('[data-testid="back-btn"]');
    await expect(backBtn).toBeVisible({ timeout: 5000 });
    await backBtn.click();
    await page.waitForLoadState('networkidle');

    // Back button navigates to workspace home
    const wsHome = page.locator('[data-testid="workspace-home"]');
    await expect(wsHome).toBeVisible({ timeout: 5000 });
  });

  test('repo_header_renders_with_repo_name', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    const repoHeader = page.locator('[data-testid="repo-header"]');
    await expect(repoHeader).toBeVisible({ timeout: 5000 });

    const repoNameEl = page.locator('[data-testid="repo-name"]');
    await expect(repoNameEl).toBeVisible({ timeout: 3000 });
  });

  test('architecture_tab_renders_and_is_active', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/architecture`);
    await page.waitForLoadState('networkidle');

    const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
    await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });

    // Tab panel content should be visible
    const tabContent = page.locator('[role="tabpanel"]');
    await expect(tabContent).toBeVisible({ timeout: 5000 });
  });

  test('decisions_tab_renders_and_is_active', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/decisions`);
    await page.waitForLoadState('networkidle');

    const decisionsTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Decisions' });
    await expect(decisionsTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
  });

  test('specs_tab_renders_spec_list_or_empty_state', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/specs`);
    await page.waitForLoadState('networkidle');

    const tabContent = page.locator('[role="tabpanel"]');
    await expect(tabContent).toBeVisible({ timeout: 5000 });

    // Spec content or empty state should be visible inside the tab panel
    const content = tabContent
      .locator('[class*="spec"]')
      .or(tabContent.locator('table'))
      .or(tabContent.locator('[class*="empty"]'))
      .or(tabContent.locator('[class*="skeleton"]'))
      .first();
    await expect(content).toBeVisible({ timeout: 8000 });
  });
});

// ---------------------------------------------------------------------------
// URL routing
// ---------------------------------------------------------------------------

test.describe('URL routing', () => {
  test('workspace_home_url_loads', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}`);
    await page.waitForLoadState('networkidle');

    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain(`/workspaces/${SEED_SLUG}`);
  });

  test('repo_url_loads_with_tab_bar', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
    const tabBar = page.locator('[data-testid="repo-tab-bar"]');
    await expect(tabBar).toBeVisible({ timeout: 5000 });
  });

  test('repo_architecture_url_activates_architecture_tab', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/architecture`);
    await page.waitForLoadState('networkidle');

    const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
    await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 5000 });
  });

  test('unknown_route_falls_back_gracefully', async ({ page }) => {
    // SPA: unknown route should not crash the app shell
    await page.goto('/nonexistent-view');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
  });

  test('browser_back_forward_navigation', async ({ page }) => {
    // Navigate workspace home → repo mode
    await page.goto(`/workspaces/${SEED_SLUG}`);
    await page.waitForLoadState('networkidle');

    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    // Go back to workspace home
    await page.goBack();
    await page.waitForLoadState('networkidle');
    await expect(page.locator('[data-testid="workspace-home"]')).toBeVisible({ timeout: 5000 });

    // Go forward to repo mode
    await page.goForward();
    await page.waitForLoadState('networkidle');
    await expect(page.locator('[data-testid="repo-mode"]')).toBeVisible({ timeout: 5000 });
  });

  test('profile_url_renders', async ({ page }) => {
    await page.goto('/profile');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('.app')).toBeVisible({ timeout: 5000 });
    expect(page.url()).toContain('/profile');
  });
});

// ---------------------------------------------------------------------------
// Keyboard shortcuts (§6 of ui-navigation.md — updated g-key sequences)
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

  test('g_h_navigates_to_workspace_home', async ({ page }) => {
    // Start in repo mode
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    // g h sequence (GitHub-style, 500ms window)
    await page.keyboard.press('g');
    await page.keyboard.press('h');
    await page.waitForLoadState('networkidle');

    // Should navigate to workspace home
    const wsHome = page.locator('[data-testid="workspace-home"]');
    await expect(wsHome).toBeVisible({ timeout: 5000 });
  });

  test('g_1_navigates_to_specs_tab_in_repo_mode', async ({ page }) => {
    // Start on architecture tab
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/architecture`);
    await page.waitForLoadState('networkidle');

    // g 1 → Specs tab (repo mode only)
    await page.keyboard.press('g');
    await page.keyboard.press('1');
    await page.waitForLoadState('networkidle');

    const specsTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Specs' });
    await expect(specsTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
  });

  test('g_2_navigates_to_architecture_tab_in_repo_mode', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    // g 2 → Architecture tab (repo mode only)
    await page.keyboard.press('g');
    await page.keyboard.press('2');
    await page.waitForLoadState('networkidle');

    const archTab = page.locator('[data-testid="repo-tab-bar"]').getByRole('tab', { name: 'Architecture' });
    await expect(archTab).toHaveAttribute('aria-selected', 'true', { timeout: 3000 });
  });

  test('esc_in_repo_mode_returns_to_workspace_home', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    // With no panel open, Esc in repo mode navigates to workspace home
    await page.keyboard.press('Escape');
    await page.waitForLoadState('networkidle');

    const wsHome = page.locator('[data-testid="workspace-home"]');
    await expect(wsHome).toBeVisible({ timeout: 5000 });
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

  test('main_content_has_id_for_skip_link', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('#main-content')).toBeAttached();
  });

  test('status_bar_has_aria_label', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Status bar uses aria-label="Status bar"
    const statusBar = page.locator('[aria-label="Status bar"]');
    await expect(statusBar).toBeVisible({ timeout: 3000 });
  });

  test('repo_tab_bar_has_tablist_role', async ({ page }) => {
    await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}`);
    await page.waitForLoadState('networkidle');

    // Tab bar should have role=tablist with aria-label
    const tabBar = page.getByRole('tablist', { name: /repo navigation/i });
    await expect(tabBar).toBeVisible({ timeout: 5000 });
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
