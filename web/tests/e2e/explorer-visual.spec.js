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

import { test, expect, SEED_WORKSPACE_SLUG, SEED_REPO_NAME } from './fixtures/seeded.js';
import { MOCK_GRAPH, VIEW_QUERY_WITH_ANNOTATIONS, BLAST_RADIUS_QUERY } from './fixtures/mock-graph.js';
import { readFileSync } from 'node:fs';

// Deterministic webfont fixtures (see fixtures/forced-fonts.css for rationale).
// Served via route interception so sandbox and CI rasterize identical font
// binaries — the app's system-ui/'SF Mono' stacks and the fonts.googleapis.com
// link never reach fontconfig or the network from these tests.
const FIXTURES_DIR = new URL('./fixtures/', import.meta.url);
const FORCED_FONTS_CSS = new URL('./fixtures/forced-fonts.css', import.meta.url);
const FONTS_DIR = new URL('./fixtures/fonts/', import.meta.url);

const SEED_SLUG = SEED_WORKSPACE_SLUG;
const SEED_REPO = SEED_REPO_NAME;
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
  // ── Deterministic webfonts (font-truth parity) ────────────────────
  // Every glyph in every baseline must come from the same font binary in
  // every environment. Three sources are pinned:
  //   a) fonts.googleapis.com is served an empty stylesheet so the network
  //      never decides which Red Hat faces load (sandbox: host blocked;
  //      CI: real webfonts — different subsetting/rasterization).
  //   b) The forced-fonts.css fixture (committed) declares @font-face for
  //      the Red Hat families from committed woff2 binaries and overrides
  //      every literal font stack the Explorer surface uses. It is injected
  //      at document_start, before the app boots.
  //   c) Canvas ctx.font strings patch 'system-ui' → 'Red Hat Text' and
  //      "'SF Mono', Menlo, monospace" → 'Red Hat Mono' through the context
  //      prototype so canvas labels also bypass fontconfig.
  const forcedFontsCss = readFileSync(FORCED_FONTS_CSS, 'utf8');
  await page.route('**/fonts.googleapis.com/**', (route) => {
    route.fulfill({ status: 200, contentType: 'text/css', body: '' });
  });
  await page.route('**/test-assets/fonts/**', (route) => {
    const name = route.request().url().split('/test-assets/fonts/')[1].split(/[?#]/)[0];
    try {
      const body = readFileSync(new URL(name, FONTS_DIR));
      route.fulfill({
        status: 200,
        contentType: 'font/woff2',
        body,
      });
    } catch {
      route.fulfill({ status: 404, body: '' });
    }
  });
  await page.addInitScript(
    ({ css }) => {
      // Inject before any app script runs: <style> with @font-face + stack
      // overrides (!important — later app styles cannot re-enable fontconfig
      // stacks), rewritten so font urls hit the route above.
      const style = document.createElement('style');
      style.textContent = css.replace(
        /url\('fonts\//g,
        "url('/test-assets/fonts/",
      );
      document.addEventListener('DOMContentLoaded', () => {
        document.head.appendChild(style);
      });
      // document_start: head may not exist yet — also append immediately when
      // possible so @font-face exist before first fonts.load()/layout.
      (document.head || document.documentElement).appendChild(style);
      // Patch canvas font shorthand so ctx.font strings resolve to the
      // committed webfonts instead of fontconfig generics.
      const proto = CanvasRenderingContext2D.prototype;
      const desc = Object.getOwnPropertyDescriptor(proto, 'font');
      if (desc && desc.set) {
        Object.defineProperty(proto, 'font', {
          ...desc,
          set(value) {
            desc.set.call(
              this,
              String(value)
                .replace(/system-ui/g, "'Red Hat Text'")
                .replace(/'SF Mono', Menlo, monospace/g, "'Red Hat Mono'"),
            );
          },
        });
      }
      // Eagerly decode every face at document_start; by the time the app
      // fetches graph data and runs its first drawFrame, fonts are loaded.
      for (const family of ['Red Hat Text', 'Red Hat Mono', 'Red Hat Display']) {
        for (const weight of [400, 500, 600, 700]) {
          document.fonts.load(`${weight} 13px '${family}'`, 'A').catch(() => {});
        }
      }
    },
    { css: forcedFontsCss },
  );
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

    // Zoom in slightly to show module-level detail (mid zoom between
    // fully zoomed out packages and fully zoomed in types/functions)
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
    await page.waitForTimeout(800);

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

// Screenshot comparison options for the filter-preset tests.
//
// Playwright's default pixelmatch threshold (0.2, perceptual YIQ) discounts
// this dark theme's preset dimming: dimming a leaf from full opacity to 0.1
// over the dark background moves the affected pixels by only ~0.05–0.15
// normalized YIQ — under the default threshold — so a render with preset
// dimming entirely disabled in production code still matched the committed
// baselines (task-080 review finding: nodeFilterOpacity() forced to 1.0 kept
// every filter-preset visual test green). The tighter threshold makes those
// dimming shifts count: the Endpoints-state canvas differs from a
// dimming-disabled render by ~83k pixels at threshold 0.05, far above the
// 2% (7,296 px) allowance.
const PRESET_SNAPSHOT_OPTIONS = {
  maxDiffPixelRatio: 0.02,
  threshold: 0.05,
  timeout: 10_000,
};

// Canvas-pixel change probing — behavioral assertions on the live canvas,
// independent of the committed baselines (so the test still fails if the
// baselines are re-recorded from a build with broken dimming).
//
// The canvas renders deterministically in this suite (forced webfonts, fixed
// viewport, reduced motion), so identical states produce identical pixels; a
// visible-change count therefore measures real rendering changes, not noise.
// The epsilon matches the screenshot threshold above (5% of 255 per channel);
// preset dimming shifts pixels by ~25–40 per channel, well above it.
const VISIBLE_CHANGE_EPSILON = 13;

// Minimum canvas pixels that must visibly change for a UI action to count
// as having re-rendered the graph. Calibrated on the deterministic mock
// graph: 5 wheel-zoom steps repaint ~239k of 363,924 canvas pixels (66%),
// clicking the Endpoints preset ~100k (27%), and the Endpoints→Types subset
// change ~127k (35%). With preset dimming disabled in production code the
// Endpoints→Types transition repaints only ~40 pixels — three orders of
// magnitude below any of these — so 20k sits safely between the healthy and
// broken behavior.
const MIN_VISIBLE_CANVAS_CHANGES = 20_000;

async function storeCanvasSnapshot(page) {
  await page.evaluate(() => {
    const canvas = document.querySelector('canvas.treemap-canvas');
    const ctx = canvas.getContext('2d');
    // getImageData() copies the pixels; the snapshot stays valid after the
    // evaluate call returns.
    window.__canvasSnapshot = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  });
}

// Count canvas pixels that changed by more than VISIBLE_CHANGE_EPSILON in
// any RGBA channel since the last storeCanvasSnapshot()/call, then re-arm
// the snapshot to the current frame so consecutive calls measure
// consecutive transitions.
async function countVisibleCanvasChanges(page) {
  return page.evaluate((epsilon) => {
    const canvas = document.querySelector('canvas.treemap-canvas');
    const ctx = canvas.getContext('2d');
    const now = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    const before = window.__canvasSnapshot;
    if (!before) throw new Error('countVisibleCanvasChanges: no canvas snapshot stored');
    let changed = 0;
    for (let i = 0; i < now.length; i += 4) {
      const d = Math.max(
        Math.abs(now[i] - before[i]),
        Math.abs(now[i + 1] - before[i + 1]),
        Math.abs(now[i + 2] - before[i + 2]),
        Math.abs(now[i + 3] - before[i + 3]),
      );
      if (d > epsilon) changed++;
    }
    window.__canvasSnapshot = now;
    return changed;
  }, VISIBLE_CHANGE_EPSILON);
}

test.describe('Filter presets visual regression', () => {
  test.beforeEach(async ({ page }) => {
    await setupGraphIntercept(page);
  });

  test('filter_all_shows_complete_graph', async ({ page }) => {
    await navigateToExplorer(page);

    // Default filter is 'all' — all nodes visible, no query applied
    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-all.png', PRESET_SNAPSHOT_OPTIONS);
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
    await expect(canvasArea).toHaveScreenshot('filter-endpoints.png', PRESET_SNAPSHOT_OPTIONS);
  });

  test('filter_preset_buttons_show_correct_subsets', async ({ page }) => {
    await navigateToExplorer(page);

    // Spec §32 visual test: "Filter presets show correct subsets" — click the
    // actual toolbar preset buttons (not the query editor) and verify the
    // canvas re-renders with the preset's subset.
    //
    // Two independent layers of verification (task-080 review repair):
    //  1. Behavioral — canvas-pixel change counts between preset states
    //     (see countVisibleCanvasChanges). The Endpoints→Types transition
    //     repaints ~35% of the canvas because a different node subset stays
    //     lit; with preset dimming disabled in production code it repaints
    //     ~40 pixels, so a broken subset filter fails here no matter what
    //     the baselines say.
    //  2. Visual — screenshot comparison at threshold 0.05
    //     (PRESET_SNAPSHOT_OPTIONS). The committed baselines record the
    //     dimmed subsets; the tighter threshold makes the dimming shifts
    //     count against the comparator's default 0.2 perceptual discount.

    // Snapshot the settled pre-zoom canvas, then zoom in so individual leaf
    // nodes (and their per-preset dimming) are visible — at the fully
    // zoomed-out package overview the leaves are too small to distinguish
    // presets.
    await storeCanvasSnapshot(page);
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

    // The zoom must actually take effect (it repaints ~66% of the canvas).
    // If the wheel-zoom path breaks, the preset screenshots below would
    // silently record the unzoomed package overview where per-leaf dimming
    // is invisible — the original review found exactly that recorded state
    // in a stale baseline.
    const zoomChanges = await countVisibleCanvasChanges(page);
    expect(zoomChanges).toBeGreaterThan(MIN_VISIBLE_CANVAS_CHANGES);

    // Click the "Endpoints" preset button — endpoint nodes stay at full
    // opacity, everything else dims.
    const endpointsBtn = page.locator('.filter-preset-group button', {
      hasText: 'Endpoints',
    });
    await expect(endpointsBtn).toBeVisible({ timeout: 5_000 });
    await endpointsBtn.click();
    await expect(endpointsBtn).toHaveAttribute('aria-pressed', 'true');
    await page.waitForTimeout(500);

    // The preset click must visibly re-render the canvas (~27% of it changes:
    // dimmed non-endpoint leaves + the filtered edge set).
    const endpointsChanges = await countVisibleCanvasChanges(page);
    expect(endpointsChanges).toBeGreaterThan(MIN_VISIBLE_CANVAS_CHANGES);

    const canvasArea = page.locator('.treemap-canvas-area');
    await expect(canvasArea).toHaveScreenshot('filter-preset-endpoints.png', PRESET_SNAPSHOT_OPTIONS);

    // Click the "Types" preset button — the subset must change (type/
    // interface/field nodes emphasized instead of endpoints).
    const typesBtn = page.locator('.filter-preset-group button', {
      hasText: 'Types',
    });
    await typesBtn.click();
    await expect(typesBtn).toHaveAttribute('aria-pressed', 'true');
    await expect(endpointsBtn).toHaveAttribute('aria-pressed', 'false');
    await page.waitForTimeout(500);

    // The subset change must be visible on the canvas: Endpoints→Types lights
    // a different node set (~35% of the canvas repaints). This is the
    // assertion that fails when preset dimming breaks — without node dimming
    // the two presets render nearly identical canvases (~40 changed pixels,
    // all of them edges).
    const typesChanges = await countVisibleCanvasChanges(page);
    expect(typesChanges).toBeGreaterThan(MIN_VISIBLE_CANVAS_CHANGES);

    await expect(canvasArea).toHaveScreenshot('filter-preset-types.png', PRESET_SNAPSHOT_OPTIONS);
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
    await expect(canvasArea).toHaveScreenshot('filter-types.png', PRESET_SNAPSHOT_OPTIONS);
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
    await expect(canvasArea).toHaveScreenshot('filter-calls.png', PRESET_SNAPSHOT_OPTIONS);
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
    await expect(canvasArea).toHaveScreenshot('filter-dependencies.png', PRESET_SNAPSHOT_OPTIONS);
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
    // BLAST_RADIUS_QUERY uses a fixed focus node ('fn-spawn-agent'),
    // so tiered coloring activates immediately from the focus query
    // without requiring a click. The click below tests node selection
    // on top of the already-active tiered coloring.
    await applyQueryViaEditor(page, BLAST_RADIUS_QUERY);

    // Click a node on the canvas to test node selection overlay on top
    // of the tiered coloring. Since the canvas is Canvas 2D (not DOM),
    // we click at a position where nodes are rendered by the treemap.
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

    // Check the Evaluative lens checkbox (ui-navigation.md §3: the lens
    // selector is a checkbox group, not buttons)
    const evalCheck = page.locator('[data-testid="lens-checkbox-evaluative"] input');
    await evalCheck.click();
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
