/**
 * Explorer visual regression tests (TASK-057).
 *
 * These tests capture screenshots of the Explorer canvas at various states
 * and compare them against committed baselines using Playwright's
 * `toHaveScreenshot()`.
 *
 * Because the Explorer's graph data comes from server-side code analysis
 * (not seeded demo data), we use Playwright route interception to provide
 * deterministic mock graph data. This ensures:
 * - Layout is reproducible (same nodes/edges → same treemap positions)
 * - Tests don't depend on a real git repo being analyzed
 * - Screenshots are stable across runs
 *
 * All view queries are applied via the manual View Query Editor (real UI
 * interaction): open editor, fill JSON textarea, click "Run Query".
 *
 * Tested scenarios (from explorer-implementation.md §Testing > Visual Tests):
 * 1. Semantic zoom at different zoom levels
 * 2. View query rendering (groups, callouts, narrative markers)
 * 3. Filter presets show correct subsets
 * 4. Blast radius interactive mode
 */

import { test, expect } from './fixtures/seeded.js';
import { MOCK_GRAPH, VIEW_QUERY_WITH_ANNOTATIONS, BLAST_RADIUS_QUERY } from './fixtures/mock-graph.js';

const SEED_SLUG = 'default';
const SEED_REPO = 'gyre-core';
const REPO_ID = 'seed-repo-1';

// Mock workspace and repo data for deterministic rendering.
// The global auth token resolves as tenant_id="default", so workspace API
// responses must use tenant_id="default" to match.
const MOCK_WORKSPACE = {
  id: 'ws-visual-test',
  tenant_id: 'default',
  name: 'Visual Test Workspace',
  slug: SEED_SLUG,
  created_at: 1711324800,
  updated_at: 1711324800,
};

const MOCK_REPO = {
  id: REPO_ID,
  workspace_id: MOCK_WORKSPACE.id,
  name: SEED_REPO,
  description: null,
  default_branch: 'main',
  status: 'Active',
  created_at: 1711324800,
  updated_at: 1711324800,
  is_mirror: false,
  mirror_url: null,
  mirror_interval_secs: null,
  last_mirror_sync: null,
  clone_url: `http://localhost:2222/git/${SEED_SLUG}/${SEED_REPO}`,
};

/**
 * Set up route interception to provide deterministic data.
 *
 * IMPORTANT: This function MUST be called BEFORE any page navigation
 * (page.goto / navigateToExplorer). Playwright's page.route() only
 * intercepts requests made AFTER the handler is registered. Registering
 * after navigation means the initial API calls are missed.
 */
async function setupGraphIntercept(page) {
  // ── Workspace & repo APIs ──────────────────────────────────────────
  await page.route('**/api/v1/workspaces', (route) => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([MOCK_WORKSPACE]),
      });
    } else {
      route.continue();
    }
  });

  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(MOCK_WORKSPACE),
    });
  });

  await page.route(`**/api/v1/workspaces?slug=${SEED_SLUG}`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([MOCK_WORKSPACE]),
    });
  });

  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}/repos`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([MOCK_REPO]),
    });
  });

  await page.route('**/api/v1/repos', (route) => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([MOCK_REPO]),
      });
    } else {
      route.continue();
    }
  });

  await page.route(`**/api/v1/repos/${REPO_ID}`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(MOCK_REPO),
    });
  });

  // ── Graph endpoints ────────────────────────────────────────────────
  await page.route(`**/api/v1/repos/${REPO_ID}/graph`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(MOCK_GRAPH),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/types`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ nodes: [] }),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/modules`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ nodes: [] }),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/risks`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([]),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/timeline**`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([]),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/diff**`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ nodes: [], edges: [] }),
    });
  });

  // ── Saved views ────────────────────────────────────────────────────
  await page.route(`**/api/v1/repos/${REPO_ID}/views`, (route) => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([]),
      });
    } else {
      route.continue();
    }
  });

  // ── Graph predict/preview endpoints ────────────────────────────────
  await page.route(`**/api/v1/repos/${REPO_ID}/graph/predict`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ nodes: [], edges: [] }),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/graph/thorough-preview`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ nodes: [], edges: [] }),
    });
  });

  // ── Catch-all for other workspace-scoped endpoints ─────────────────
  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}/explorer-views**`, (route) => {
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([]),
    });
  });

  await page.route(`**/api/v1/repos/${REPO_ID}/specs**`, (route) => {
    if (route.request().method() === 'GET') {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([]),
      });
    } else {
      route.continue();
    }
  });
}

/**
 * Navigate to the Explorer (architecture tab) and wait for the canvas to render.
 * MUST be called AFTER setupGraphIntercept() — route handlers must be
 * registered before navigation so they intercept the initial API calls.
 */
async function navigateToExplorer(page) {
  await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/architecture`);
  await page.waitForLoadState('networkidle');

  // Wait for the canvas element to be attached
  const canvas = page.locator('canvas.treemap-canvas');
  await expect(canvas).toBeAttached({ timeout: 10_000 });

  // Wait for the graph stats to show node count (confirms data loaded and rendering)
  const stats = page.locator('.graph-stats, .treemap-stats');
  await expect(stats.first()).toContainText('nodes', { timeout: 10_000 });

  // Allow rendering to stabilize (animations, layout)
  await page.waitForTimeout(1000);

  // Dismiss the anomaly panel if it overlays the canvas — it obscures the
  // treemap and makes screenshots identical regardless of filter/query state.
  const anomalyClose = page.locator('.anomaly-close');
  if (await anomalyClose.isVisible({ timeout: 1000 }).catch(() => false)) {
    await anomalyClose.click();
    await page.waitForTimeout(300);
  }
}

/**
 * Apply a view query via the manual View Query Editor UI.
 *
 * Opens the query editor panel, fills the JSON textarea, and clicks
 * "Run Query". This is a real user interaction — no custom events
 * or window globals.
 */
async function applyQueryViaEditor(page, query) {
  // Open the query editor if not already open
  const editorToggle = page.locator('button[aria-label="Toggle manual view query editor"]');
  await expect(editorToggle).toBeVisible({ timeout: 5_000 });

  // Check if editor is already open
  const editorPanel = page.locator('.query-editor-panel');
  const isOpen = await editorPanel.isVisible().catch(() => false);
  if (!isOpen) {
    await editorToggle.click();
    await expect(editorPanel).toBeVisible({ timeout: 3_000 });
  }

  // Fill the query JSON into the textarea
  const textarea = page.locator('.query-editor-textarea');
  await expect(textarea).toBeVisible({ timeout: 3_000 });
  await textarea.fill(JSON.stringify(query));

  // Click "Run Query" to apply
  const runBtn = page.locator('.query-editor-run-btn');
  await expect(runBtn).toBeEnabled({ timeout: 3_000 });
  await runBtn.click();

  // Allow canvas to re-render with the new query
  await page.waitForTimeout(1000);
}

// ---------------------------------------------------------------------------
// Test configuration — fixed viewport + reduced motion for determinism
// ---------------------------------------------------------------------------

test.use({
  viewport: { width: 1280, height: 720 },
  reducedMotion: 'reduce',
});

// ---------------------------------------------------------------------------
// 1. Semantic zoom at different zoom levels
// ---------------------------------------------------------------------------

test.describe('Semantic zoom visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('zoom_level_0_packages_overview', async ({ page }) => {
    await navigateToExplorer(page);

    // Zoom out to see all packages — dispatch wheel events on the canvas
    await page.evaluate(() => {
      const canvas = document.querySelector('canvas.treemap-canvas');
      if (canvas) {
        for (let i = 0; i < 15; i++) {
          canvas.dispatchEvent(new WheelEvent('wheel', {
            deltaY: 100, clientX: 640, clientY: 360, bubbles: true,
          }));
        }
      }
    });
    await page.waitForTimeout(800);

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('zoom-level-0-packages.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('zoom_level_1_modules', async ({ page }) => {
    await navigateToExplorer(page);

    // Default zoom shows the full graph — no zoom adjustment needed
    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('zoom-level-1-modules.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('zoom_level_2_types_and_functions', async ({ page }) => {
    await navigateToExplorer(page);

    // Zoom in to see individual types and functions
    await page.evaluate(() => {
      const canvas = document.querySelector('canvas.treemap-canvas');
      if (canvas) {
        for (let i = 0; i < 10; i++) {
          canvas.dispatchEvent(new WheelEvent('wheel', {
            deltaY: -100, clientX: 640, clientY: 360, bubbles: true,
          }));
        }
      }
    });
    await page.waitForTimeout(800);

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('zoom-level-2-types.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});

// ---------------------------------------------------------------------------
// 2. View query rendering (groups, callouts, narrative markers)
// ---------------------------------------------------------------------------

test.describe('View query rendering visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('view_query_with_groups_callouts_narrative', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply the annotated view query via the query editor UI
    await applyQueryViaEditor(page, VIEW_QUERY_WITH_ANNOTATIONS);

    // Capture the canvas area with the view query applied —
    // groups, callouts, and narrative markers should be visible
    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('view-query-annotated.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('view_query_container_with_annotation_bar', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply the annotated view query
    await applyQueryViaEditor(page, VIEW_QUERY_WITH_ANNOTATIONS);

    // Capture the full container including toolbar and annotation bar
    const container = page.locator('.treemap-container');
    await expect(container).toHaveScreenshot('view-query-container.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});

// ---------------------------------------------------------------------------
// 3. Filter presets show correct subsets
// ---------------------------------------------------------------------------

test.describe('Filter presets visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('filter_all_shows_complete_graph', async ({ page }) => {
    await navigateToExplorer(page);

    // Default filter is 'all' — all nodes visible, no query applied
    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-all.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('filter_endpoints_shows_only_endpoints', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply filter query for endpoints via the query editor
    await applyQueryViaEditor(page, {
      scope: { type: 'filter', node_types: ['endpoint'] },
      emphasis: { highlight: { matched: { color: '#3b82f6' } }, dim_unmatched: 0.1 },
      edges: { filter: ['calls', 'routes_to'] },
      zoom: 'fit',
    });

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-endpoints.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('filter_types_shows_only_type_nodes', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply filter for types/interfaces — highlight data model nodes
    await applyQueryViaEditor(page, {
      scope: { type: 'filter', node_types: ['type', 'interface', 'trait', 'enum'] },
      emphasis: { highlight: { matched: { color: '#10b981' } }, dim_unmatched: 0.1 },
      edges: { filter: ['field_of', 'depends_on', 'implements'] },
      zoom: 'fit',
    });

    // Zoom in so individual type nodes with dimming are visible
    await page.evaluate(() => {
      const canvas = document.querySelector('canvas.treemap-canvas');
      if (canvas) {
        for (let i = 0; i < 5; i++) {
          canvas.dispatchEvent(new WheelEvent('wheel', {
            deltaY: -100, clientX: 640, clientY: 360, bubbles: true,
          }));
        }
      }
    });
    await page.waitForTimeout(500);

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-types.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('filter_calls_shows_call_graph', async ({ page }) => {
    await navigateToExplorer(page);

    // Focus on spawn_agent and its callers — shows call graph structure
    await applyQueryViaEditor(page, {
      scope: {
        type: 'focus',
        node: 'fn-spawn-agent',
        edges: ['calls', 'routes_to'],
        direction: 'incoming',
        depth: 3,
      },
      emphasis: { highlight: { matched: { color: '#8b5cf6' } }, dim_unmatched: 0.1 },
      edges: { filter: ['calls', 'routes_to'] },
      zoom: 'fit',
    });

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-calls.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('filter_dependencies_shows_dependency_edges', async ({ page }) => {
    await navigateToExplorer(page);

    // Focus on RepositoryPort trait and its implementations/dependents
    await applyQueryViaEditor(page, {
      scope: {
        type: 'focus',
        node: 'trait-repo-port',
        edges: ['depends_on', 'implements'],
        direction: 'incoming',
        depth: 3,
      },
      emphasis: { highlight: { matched: { color: '#f59e0b' } }, dim_unmatched: 0.1 },
      edges: { filter: ['depends_on', 'implements'] },
      zoom: 'fit',
    });

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-dependencies.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});

// ---------------------------------------------------------------------------
// 4. Blast radius interactive mode
// ---------------------------------------------------------------------------

test.describe('Blast radius visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('blast_radius_tiered_coloring_on_node_click', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply the blast radius query via the query editor.
    // The blast radius query uses $clicked as scope.node, which makes
    // ExplorerCanvas store it as an interactive query template.
    // The tiered coloring only activates after clicking a node.
    await applyQueryViaEditor(page, BLAST_RADIUS_QUERY);

    // Click a node on the canvas to trigger blast radius BFS coloring.
    // Since the canvas is Canvas 2D (not DOM), we click at a position
    // where nodes are likely rendered by the treemap layout.
    const canvas = page.locator('canvas.treemap-canvas');
    const box = await canvas.boundingBox();
    expect(box).toBeTruthy();

    // Click in the upper-left quadrant to avoid any overlay panels
    await canvas.click({ position: { x: box.width * 0.25, y: box.height * 0.25 }, force: true });
    await page.waitForTimeout(1000);

    // Capture the canvas after blast radius activation — should show
    // tiered coloring (red → orange → yellow → gray) with dimmed unmatched nodes
    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('blast-radius-tiered.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('blast_radius_dimmed_unmatched_nodes', async ({ page }) => {
    await navigateToExplorer(page);

    // Apply a focus query with a fixed node (not $clicked) to show
    // the blast radius from spawn_agent without requiring a click.
    // This produces deterministic tiered coloring from a known node.
    await applyQueryViaEditor(page, {
      scope: {
        type: 'focus',
        node: 'fn-spawn-agent',
        edges: ['calls', 'implements', 'depends_on'],
        direction: 'incoming',
        depth: 10,
      },
      emphasis: {
        tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'],
        dim_unmatched: 0.12,
      },
      edges: { filter: ['calls', 'implements', 'depends_on'] },
      zoom: 'fit',
      annotation: {
        title: 'Blast radius: spawn_agent',
        description: '{{count}} transitive callers/implementors',
      },
    });

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('blast-radius-fixed-node.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});

// ---------------------------------------------------------------------------
// 5. Toolbar and UI chrome visual regression
// ---------------------------------------------------------------------------

test.describe('Explorer toolbar visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('toolbar_renders_with_lens_toggle_and_stats', async ({ page }) => {
    await navigateToExplorer(page);

    const toolbar = page.locator('.treemap-toolbar');
    await expect(toolbar).toBeVisible({ timeout: 5_000 });

    await expect(toolbar).toHaveScreenshot('toolbar-default.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });

  test('evaluative_lens_toggle_changes_toolbar', async ({ page }) => {
    await navigateToExplorer(page);

    // Click the Evaluative lens button
    const evalBtn = page.locator('.lens-group').getByRole('button', { name: 'Evaluative' });
    await evalBtn.click();
    await page.waitForTimeout(500);

    const toolbar = page.locator('.treemap-toolbar');
    await expect(toolbar).toHaveScreenshot('toolbar-evaluative.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});

// ---------------------------------------------------------------------------
// 6. Full page composite screenshot
// ---------------------------------------------------------------------------

test.describe('Explorer full page visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('full_explorer_page_default_state', async ({ page }) => {
    await navigateToExplorer(page);

    const tabPanel = page.locator('[role="tabpanel"]');
    await expect(tabPanel).toHaveScreenshot('explorer-full-page.png', {
      maxDiffPixelRatio: 0.02,
      timeout: 10_000,
    });
  });
});
