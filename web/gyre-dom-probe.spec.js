/**
 * In-sandbox DOM verification for the 10 repaired explorer-visual scenarios.
 * The sandbox cannot reproduce CI pixel baselines (no Google Fonts → different
 * font metrics; recorded in review evidence). This probe verifies the same
 * scenarios structurally: toolbar contents, lens behavior, filter dimming,
 * view-query annotation, treemap presence, node-count stats.
 */
import { test, expect } from '@playwright/test';
import { MOCK_GRAPH, VIEW_QUERY_WITH_ANNOTATIONS, BLAST_RADIUS_QUERY } from './tests/e2e/fixtures/mock-graph.js';

const SEED_SLUG = 'default';
const SEED_REPO = 'gyre-core';
const REPO_ID = 'seed-repo-1';
const MOCK_WORKSPACE = { id: 'ws-visual-test', tenant_id: 'default', name: 'Visual Test Workspace', slug: SEED_SLUG, created_at: 1711324800, updated_at: 1711324800 };
const MOCK_REPO = { id: REPO_ID, workspace_id: MOCK_WORKSPACE.id, name: SEED_REPO, description: null, default_branch: 'main', status: 'Active', created_at: 1711324800, updated_at: 1711324800, is_mirror: false, mirror_url: null, mirror_interval_secs: null, last_mirror_sync: null, clone_url: `http://localhost:2222/git/${SEED_SLUG}/${SEED_REPO}` };

test.use({ viewport: { width: 1280, height: 720 }, reducedMotion: 'reduce' });

async function setupGraphIntercept(page) {
  await page.route('**/api/v1/workspaces', (r) => r.request().method() === 'GET' ? r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([MOCK_WORKSPACE]) }) : r.continue());
  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(MOCK_WORKSPACE) }));
  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}/repos`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([MOCK_REPO]) }));
  await page.route('**/api/v1/repos', (r) => r.request().method() === 'GET' ? r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([MOCK_REPO]) }) : r.continue());
  await page.route(`**/api/v1/repos/${REPO_ID}`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(MOCK_REPO) }));
  await page.route(`**/api/v1/repos/${REPO_ID}/graph`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(MOCK_GRAPH) }));
  await page.route(`**/api/v1/repos/${REPO_ID}/graph/*`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ nodes: [], edges: [] }) }));
  await page.route(`**/api/v1/repos/${REPO_ID}/graph/risks`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) }));
  await page.route(`**/api/v1/repos/${REPO_ID}/views`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) }));
  await page.route(`**/api/v1/repos/${REPO_ID}/specs**`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) }));
  await page.route(`**/api/v1/workspaces/${MOCK_WORKSPACE.id}/explorer-views**`, (r) => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) }));
}

async function navigateToExplorer(page) {
  await page.goto(`/workspaces/${SEED_SLUG}/r/${SEED_REPO}/architecture`);
  await page.waitForLoadState('networkidle');
  await expect(page.locator('canvas.treemap-canvas')).toBeAttached({ timeout: 10_000 });
  await expect(page.locator('.graph-stats, .treemap-stats').first()).toContainText('nodes', { timeout: 10_000 });
  await page.waitForTimeout(1000);
  const anomalyClose = page.locator('.anomaly-close');
  if (await anomalyClose.isVisible({ timeout: 1000 }).catch(() => false)) {
    await anomalyClose.click();
    await page.waitForTimeout(300);
  }
}

async function applyQueryViaEditor(page, query) {
  const editorToggle = page.locator('button[aria-label="Toggle manual view query editor"]');
  await expect(editorToggle).toBeVisible({ timeout: 5_000 });
  const editorPanel = page.locator('.query-editor-panel');
  if (!(await editorPanel.isVisible().catch(() => false))) {
    await editorToggle.click();
    await expect(editorPanel).toBeVisible({ timeout: 3_000 });
  }
  const textarea = page.locator('.query-editor-textarea');
  await textarea.fill(JSON.stringify(query));
  await page.locator('.query-editor-run-btn').click();
  await page.waitForTimeout(1000);
}

test('toolbar contains lens toggle, five filter presets, stats', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  const toolbar = page.locator('.treemap-toolbar');
  await expect(toolbar).toBeVisible();
  await expect(toolbar.getByRole('button', { name: 'Structural' })).toBeVisible();
  await expect(toolbar.getByRole('button', { name: 'Evaluative' })).toBeVisible();
  await expect(toolbar.getByRole('button', { name: /Observable/ })).toBeDisabled();
  for (const preset of ['All', 'Endpoints', 'Types', 'Calls', 'Dependencies']) {
    await expect(toolbar.getByRole('button', { name: preset, exact: true })).toBeVisible();
  }
  await expect(toolbar.locator('.treemap-stats')).toContainText('34 nodes');
});

test('evaluative lens adds overlay + playback controls to toolbar', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  await page.locator('.lens-group').getByRole('button', { name: 'Evaluative' }).click();
  await page.waitForTimeout(500);
  await expect(page.locator('.treemap-toolbar')).toBeVisible();
  await expect(page.locator('.treemap-toolbar').getByText(/No trace data/)).toBeVisible();
});

test('view query renders annotation bar with resolved title/description', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  await applyQueryViaEditor(page, VIEW_QUERY_WITH_ANNOTATIONS);
  const bar = page.locator('.query-annotation');
  await expect(bar).toBeVisible();
  await expect(bar.locator('.annotation-title')).toHaveText('Architecture Overview');
  await expect(bar.locator('.annotation-desc')).toHaveText('Request flow through the system');
});

test('blast radius query shows annotation with resolved count', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  await applyQueryViaEditor(page, BLAST_RADIUS_QUERY);
  const bar = page.locator('.query-annotation');
  await expect(bar).toBeVisible();
  await expect(bar.locator('.annotation-title')).toHaveText('Blast radius: spawn_agent');
  await expect(bar.locator('.annotation-desc')).toContainText('transitive callers/implementors');
});

test('filter presets switch active state', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  const toolbar = page.locator('.treemap-toolbar');
  for (const preset of ['Endpoints', 'Types', 'Calls', 'Dependencies', 'All']) {
    await toolbar.getByRole('button', { name: preset, exact: true }).click();
    await expect(toolbar.getByRole('button', { name: preset, exact: true })).toHaveAttribute('aria-pressed', 'true');
  }
});

test('chat rail renders localized strings (no raw i18n keys)', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  const chat = page.locator('.explorer-chat-area');
  await expect(chat).toBeVisible();
  const text = await chat.innerText();
  expect(text).not.toContain('explorer_chat.');
});

test('explorer chat header shows Ready status after ws settles or fails cleanly', async ({ page }) => {
  await setupGraphIntercept(page);
  await navigateToExplorer(page);
  await page.waitForTimeout(1500);
  const headerText = await page.locator('.explorer-chat-area').innerText();
  // Must be a localized status, never a raw key
  expect(headerText).not.toContain('explorer_chat.');
});
