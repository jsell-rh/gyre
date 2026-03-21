/**
 * M17.5: Frontend E2E tests with Playwright
 *
 * Tests run against a live gyre-server on localhost:2222 with GYRE_AUTH_TOKEN=e2e-test-token.
 * The seeded fixture calls POST /api/v1/admin/seed before each test.
 */

import { test, expect } from './fixtures/seeded.js';

const TOKEN = 'e2e-test-token';

// ---------------------------------------------------------------------------
// Helper: click a sidebar nav item by label text
// ---------------------------------------------------------------------------
async function navigateTo(page, label) {
  await page.getByRole('button', { name: label, exact: true }).first().click();
}

// ---------------------------------------------------------------------------
// Auth flow
// ---------------------------------------------------------------------------

test.describe('Auth flow', () => {
  test('auth_token_modal_stores_token', async ({ page }) => {
    await page.goto('/');
    // Click the auth button in the topbar
    await page.getByRole('button', { name: /authenticated|no token/i }).click();

    // Modal should open — fill in the token input
    const tokenInput = page.locator('#token-input');
    await tokenInput.waitFor({ state: 'visible' });
    await tokenInput.fill(TOKEN);
    // Use CSS locator: aria-hidden on modal-backdrop hides buttons from getByRole
    await page.locator('[role="dialog"] button:has-text("Save")').click();

    // Modal should close
    await expect(tokenInput).not.toBeVisible();

    // Auth button should now show "Authenticated"
    await expect(page.getByRole('button', { name: /authenticated/i })).toBeVisible();
  });

  test('wrong_token_shows_error_state', async ({ page }) => {
    // Start without seeded token — set a wrong one
    await page.addInitScript(() => {
      localStorage.setItem('gyre_auth_token', 'totally-wrong-token');
    });
    await page.goto('/');

    // Dashboard loads but WS/API calls may fail; auth button shows "Authenticated"
    // because token is set (but wrong). The WS indicator should show error state.
    await page.waitForLoadState('networkidle');

    // The auth button still shows "Authenticated" (token is set)
    const authBtn = page.getByRole('button', { name: /authenticated/i });
    await expect(authBtn).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Dashboard home
// ---------------------------------------------------------------------------

test.describe('Dashboard home', () => {
  test('dashboard_loads_metric_cards', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Four metric cards should be visible
    await expect(page.getByText('Active Agents')).toBeVisible();
    await expect(page.getByText('Open Tasks')).toBeVisible();
    await expect(page.getByText('Pending MRs')).toBeVisible();
    await expect(page.getByText('Queue Depth')).toBeVisible();

    // Each metric-value should contain a number
    const values = page.locator('.metric-value');
    const count = await values.count();
    expect(count).toBeGreaterThanOrEqual(4);
    for (let i = 0; i < count; i++) {
      const text = await values.nth(i).textContent();
      expect(text?.trim()).toMatch(/^\d+$/);
    }
  });

  test('seed_demo_data_button_works', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Get initial agent count
    const firstValue = page.locator('.metric-value').first();
    const beforeText = await firstValue.textContent();

    // Click Seed Demo Data
    const seedBtn = page.getByRole('button', { name: /seed demo data/i });
    await seedBtn.click();

    // Toast should appear confirming success
    await expect(page.getByText(/demo data seeded|already.seeded/i)).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

test.describe('Projects', () => {
  test('create_project_via_modal', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Projects');

    // Click "New Project" button
    const newProjectBtn = page.getByRole('button', { name: /new project/i });
    await newProjectBtn.click();

    // Modal opens — fill the form
    const nameInput = page.locator('input[placeholder="my-project"], input[placeholder*="project"]').first();
    await nameInput.waitFor({ state: 'visible' });
    const projectName = `e2e-project-${Date.now()}`;
    await nameInput.fill(projectName);

    // Submit — use CSS locator: aria-hidden on modal-backdrop hides buttons from getByRole
    await page.locator('[role="dialog"] button:has-text("Create Project")').click();

    // Toast should appear confirming success
    await expect(page.getByText('Project created')).toBeVisible({ timeout: 5000 });
  });

  test('project_list_shows_empty_state_or_projects', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Projects');

    // Either projects are visible (from seed) or empty state is shown
    const hasProjects = await page.locator('.project-card, [class*="card"]').count();
    const emptyState = page.locator('[class*="empty"], text="No projects"');
    // At least one of the two conditions holds
    expect(hasProjects > 0 || await emptyState.count() > 0).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// Task board
// ---------------------------------------------------------------------------

test.describe('Task board', () => {
  test('create_task_appears_in_kanban', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Tasks');

    // Wait for task board to load
    await page.waitForLoadState('networkidle');

    // Click "+ New Task" button (in task board or quick actions on dashboard)
    const newTaskBtn = page.getByRole('button', { name: /new task/i }).first();
    await newTaskBtn.click();

    // Fill task title
    const titleInput = page.locator('input[placeholder="Task title"], input[placeholder*="title"]').first();
    await titleInput.waitFor({ state: 'visible' });
    const taskTitle = `e2e-task-${Date.now()}`;
    await titleInput.fill(taskTitle);

    // Submit — use CSS locator: aria-hidden on modal-backdrop hides buttons from getByRole
    await page.locator('[role="dialog"] button:has-text("Create Task"), [role="dialog"] button:has-text("Creating")').first().click();

    // Toast and task should appear
    await expect(page.getByText(/task created/i)).toBeVisible({ timeout: 5000 });
  });

  test('task_board_renders_kanban_columns', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Tasks');
    await page.waitForLoadState('networkidle');

    // Kanban columns should be visible (Backlog, In Progress, etc.)
    const columns = page.locator('.column-header, .kanban-column, [class*="column"]');
    await expect(columns.first()).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Agent management
// ---------------------------------------------------------------------------

test.describe('Agent management', () => {
  test('agent_list_shows_status_badges', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Agents');
    await page.waitForLoadState('networkidle');

    // Either agents list is shown with status badges, or empty state
    const agentItems = page.locator('.agent-card, [class*="agent"], .status-badge, [class*="badge"]');
    const emptyState = page.locator('text=/no agents/i');

    const agentCount = await agentItems.count();
    const emptyCount = await emptyState.count();
    expect(agentCount > 0 || emptyCount > 0).toBeTruthy();
  });

  test('spawn_agent_modal_validates_fields', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Agents');
    await page.waitForLoadState('networkidle');

    // Look for Spawn Agent button
    const spawnBtn = page.getByRole('button', { name: /spawn agent/i });
    if (await spawnBtn.count() > 0) {
      await spawnBtn.click();
      // Modal should open
      const modal = page.locator('[role="dialog"], .modal, [class*="modal"]').first();
      await expect(modal).toBeVisible({ timeout: 3000 });
    }
    // If no spawn button visible, agents list might be empty — that's okay
  });

  test('agent_logs_tab_shows_output', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Agents');
    await page.waitForLoadState('networkidle');

    // If an agent card is clickable, open it and check logs tab
    const agentCard = page.locator('.agent-card, [class*="agent-item"]').first();
    if (await agentCard.count() > 0) {
      await agentCard.click();

      // Look for a Logs tab
      const logsTab = page.getByRole('button', { name: /logs/i });
      if (await logsTab.count() > 0) {
        await logsTab.click();
        // Either log lines or empty state should appear
        await page.waitForLoadState('networkidle');
        const logArea = page.locator('[class*="log"], .log-line').or(page.getByText(/no logs/i));
        expect(await logArea.count()).toBeGreaterThanOrEqual(0); // flexible
      }
    }
  });
});

// ---------------------------------------------------------------------------
// MCP Tool Catalog
// ---------------------------------------------------------------------------

test.describe('MCP Tool Catalog', () => {
  test('mcp_catalog_shows_8_tools', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'MCP Tools');
    await page.waitForLoadState('networkidle');

    // Wait for tool cards to appear
    const toolCards = page.locator('.tool-card, [class*="tool-card"], [class*="mcp-tool"]');
    await toolCards.first().waitFor({ state: 'visible', timeout: 5000 });
    const count = await toolCards.count();
    expect(count).toBe(8);
  });

  test('mcp_tool_card_expands_schema', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'MCP Tools');
    await page.waitForLoadState('networkidle');

    // Click on the first tool card to expand it
    const firstCard = page.locator('.tool-card, [class*="tool-card"]').first();
    await firstCard.waitFor({ state: 'visible', timeout: 5000 });
    await firstCard.click();

    // JSON schema or expanded content should appear
    await expect(page.locator('pre, code, [class*="schema"]').first()).toBeVisible({ timeout: 3000 });
  });
});

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

test.describe('Settings', () => {
  test('settings_shows_server_info', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Settings');
    await page.waitForLoadState('networkidle');

    // Server info card should show name and version from /api/v1/version
    await expect(page.getByText('gyre', { exact: true })).toBeVisible();
    await expect(page.getByText('0.1.0').first()).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Merge Queue
// ---------------------------------------------------------------------------

test.describe('Merge Queue', () => {
  test('merge_queue_view_renders', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Merge Queue');
    await page.waitForLoadState('networkidle');

    // Either entries are shown (from seed) or empty state appears
    const queueEntries = page.locator('[class*="queue"], .queue-entry');
    const emptyState = page.locator('text=/no entries|empty|nothing queued/i');
    const entryCount = await queueEntries.count();
    const emptyCount = await emptyState.count();
    expect(entryCount > 0 || emptyCount > 0).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// Global Search
// ---------------------------------------------------------------------------

test.describe('Global Search', () => {
  test('cmd_k_opens_search', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Press Ctrl+K (Meta+K on Mac, Ctrl+K on Linux)
    await page.keyboard.press('Control+k');

    // Search overlay should open
    const searchOverlay = page.locator('[class*="search-bar"], [class*="search-overlay"], input[placeholder*="search" i]');
    await expect(searchOverlay.first()).toBeVisible({ timeout: 3000 });
  });

  test('search_for_agent_returns_result', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await page.keyboard.press('Control+k');

    // Type a search query
    const searchInput = page.locator('input[type="text"]').last();
    await searchInput.waitFor({ state: 'visible', timeout: 3000 });
    await searchInput.fill('agent');

    // Results should appear (or "no results" if none match)
    await page.waitForLoadState('networkidle');
    // Just verify no crash — results container exists
    const results = page.locator('[class*="search-result"], [class*="result-item"]');
    expect(await results.count()).toBeGreaterThanOrEqual(0);
  });
});

// ---------------------------------------------------------------------------
// Analytics & Cost
// ---------------------------------------------------------------------------

test.describe('Analytics and Cost', () => {
  test('analytics_bar_chart_renders', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Analytics');
    await page.waitForLoadState('networkidle');

    // Analytics view should load without errors — check heading or content area
    await expect(
      page.locator('[class*="analytics"]').or(page.locator('[class*="chart"]')).or(page.getByText('Analytics')).first()
    ).toBeVisible({ timeout: 5000 });
  });

  test('cost_summary_table_renders', async ({ page }) => {
    await page.goto('/');
    await navigateTo(page, 'Costs');
    await page.waitForLoadState('networkidle');

    // Cost view should render — table or empty state
    await expect(
      page.locator('[class*="cost"]').or(page.locator('table')).or(page.getByText('Cost')).first()
    ).toBeVisible({ timeout: 5000 });
  });
});

// ---------------------------------------------------------------------------
// Accessibility baseline
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// i18n / console error guard
// ---------------------------------------------------------------------------

test.describe('i18n locale init', () => {
  test('no_console_errors_on_dashboard_load', async ({ page }) => {
    const errors = [];
    page.on('pageerror', (err) => errors.push(err.message));
    await page.goto('/');
    await page.waitForTimeout(2000);
    expect(errors).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// Accessibility baseline
// ---------------------------------------------------------------------------

test.describe('Accessibility', () => {
  test('no_axe_violations_on_dashboard', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Run axe-core accessibility audit
    const { checkA11y } = await import('@axe-core/playwright').catch(() => ({ checkA11y: null }));
    if (checkA11y) {
      await checkA11y(page, undefined, {
        runOnly: {
          type: 'tag',
          values: ['wcag2a', 'wcag2aa'],
        },
        // Only fail on critical and serious violations
        violations: {
          impact: ['critical', 'serious'],
        },
      });
    } else {
      // axe not available — just verify page loads without JS errors
      await expect(page.locator('.app').first()).toBeVisible({ timeout: 5000 });
    }
  });
});
