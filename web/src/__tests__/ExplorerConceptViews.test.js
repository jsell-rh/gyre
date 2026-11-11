/**
 * ExplorerConceptViews.test.js — task-182 (system-explorer.md §4
 * "Concept Views"): cross-cutting architecture views by concept.
 *
 * - Concept browser: the in-view filter panel lists predefined concepts
 *   from the repo's spec manifest (GET /repos/:id/graph/concepts).
 * - Selecting a manifest concept fetches its projection
 *   (GET /repos/:id/graph/concept/:name — the manifest-based mechanism,
 *   distinct from the view spec grammar's substring `concept` field) and
 *   fades non-concept nodes to background (drawn at ~15% opacity, NOT
 *   removed), with the active concept as a clearable control-bar chip.
 * - Ad-hoc concepts: multi-select (Shift+Click) + right-click →
 *   "Create concept" opens a naming dialog; saving persists the grouping
 *   as an explorer view (POST /workspaces/:id/explorer-views) whose
 *   ViewQuery concept scope seeds exactly the selected nodes.
 * - Edges between concept nodes are highlighted (canvas draw path).
 *
 * Fade assertions read ctx.globalAlpha at each label fillText — the same
 * draw-path pinning pattern as the filter-preset dimming tests, so a
 * regression that reverts the fade to node removal (label disappears) or
 * to no-op (alpha unchanged) fails here.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';

// Shared mock: every api method is a cached vi.fn() defaulting to an empty
// result; tests override the ones they exercise.
const fns = new Map();
function fn(name) {
  if (!fns.has(name)) fns.set(name, vi.fn().mockResolvedValue(undefined));
  return fns.get(name);
}

vi.mock('../lib/api.js', () => ({
  api: new Proxy(
    {},
    {
      get(_target, prop) {
        if (typeof prop !== 'string') return undefined;
        return fn(prop);
      },
    }
  ),
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// ── Mock WebSocket (the explorer chat socket connects on mount) ──────────
let sockets;

class MockWebSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSING = 2;
  static CLOSED = 3;

  constructor(url) {
    this.url = url;
    this.readyState = MockWebSocket.CONNECTING;
    this.sent = [];
    this.send = vi.fn((data) => this.sent.push(JSON.parse(data)));
    this.close = vi.fn(() => { this.readyState = MockWebSocket.CLOSED; });
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    sockets.push(this);
  }
  simulateOpen() {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }
}

// Two top-level packages: auth-domain holds the Authentication concept's
// nodes, shipping-domain is unrelated and must fade when the concept is
// active. Asserted through actual canvas draw output (label alphas).
const GRAPH = {
  nodes: [
    { id: 'pkg-a', node_type: 'package', name: 'auth-domain', qualified_name: 'auth-domain', visibility: 'public', spec_confidence: 'none' },
    { id: 'mod-a', node_type: 'module', name: 'tokens', qualified_name: 'auth-domain::tokens', visibility: 'public', spec_confidence: 'none' },
    { id: 'n-a', node_type: 'type', name: 'JwtAuthProvider', qualified_name: 'auth-domain::tokens::JwtAuthProvider', spec_path: 'specs/system/identity-security.md', spec_confidence: 'high', visibility: 'public' },
    { id: 'n-a2', node_type: 'type', name: 'TokenStore', qualified_name: 'auth-domain::tokens::TokenStore', spec_path: 'specs/system/identity-security.md', spec_confidence: 'high', visibility: 'public' },
    { id: 'pkg-s', node_type: 'package', name: 'shipping-domain', qualified_name: 'shipping-domain', visibility: 'public', spec_confidence: 'none' },
    { id: 'mod-s', node_type: 'module', name: 'logistics', qualified_name: 'shipping-domain::logistics', visibility: 'public', spec_confidence: 'none' },
  ],
  edges: [
    { id: 'c1', source_id: 'pkg-a', target_id: 'mod-a', edge_type: 'contains' },
    { id: 'c2', source_id: 'mod-a', target_id: 'n-a', edge_type: 'contains' },
    { id: 'c3', source_id: 'mod-a', target_id: 'n-a2', edge_type: 'contains' },
    { id: 'c4', source_id: 'pkg-s', target_id: 'mod-s', edge_type: 'contains' },
    { id: 'x1', source_id: 'n-a', target_id: 'n-a2', edge_type: 'calls' },
  ],
};

// Two root type nodes, no containers — a tiny layout whose leaves are
// visible and hit-testable at the canvas center region (same shape as the
// scope-drill fixture), for the multi-select → Create concept flow.
const TWO_LEAF_GRAPH = {
  nodes: [
    { id: 't-1', node_type: 'type', name: 'JwtAuthProvider', qualified_name: 'JwtAuthProvider', visibility: 'public', spec_confidence: 'high' },
    { id: 't-2', node_type: 'type', name: 'TokenStore', qualified_name: 'TokenStore', visibility: 'public', spec_confidence: 'high' },
  ],
  edges: [
    { id: 'x-1', source_id: 't-1', target_id: 't-2', edge_type: 'calls' },
  ],
};

// Captured canvas draw output, attributed to the MAIN canvas
// (.treemap-canvas), bucketed per frame (setTransform starts a frame):
// labels + their draw opacity at the fillText call, stroke colors + alpha
// at the stroke() call, and the first screen position of each leaf label
// (the click target for hit-tested interactions).
let frameCount;
let frameLabels;
let frameLabelAlphas;
let frameStrokes;
let labelPositions;

beforeEach(() => {
  fns.clear();
  window.history.pushState({}, '', '/workspaces/ws-1/explorer');
  fn('repoGraph').mockResolvedValue(GRAPH);
  fn('allRepos').mockResolvedValue([{ id: 'repo-1', name: 'auth-api' }]);
  fn('repos').mockResolvedValue([{ id: 'repo-1', name: 'auth-api' }]);
  fn('mergeRequests').mockResolvedValue([]);
  fn('mrTrace').mockResolvedValue({ spans: [] });
  fn('getSpecs').mockResolvedValue([
    { path: 'specs/system/identity-security.md', title: 'Identity Security', approval_status: 'approved' },
  ]);
  fn('getSpecProgress').mockResolvedValue({ total_tasks: 1, completed_tasks: 1 });
  fn('repoGraphTimeline').mockResolvedValue([]);
  fn('mrTimeline').mockResolvedValue({ mr_id: 'mr-1', events: [] });
  fn('mrAttestation').mockResolvedValue(null);
  fn('mrDiff').mockResolvedValue({ diff: '' });
  fn('repoGraphRisks').mockResolvedValue([]);
  fn('explorerViews').mockResolvedValue([]);

  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  global.ResizeObserver = class ResizeObserver {
    observe() {} disconnect() {} unobserve() {}
  };
  global.requestAnimationFrame = vi.fn((cb) => { cb(16); return 1; });
  global.cancelAnimationFrame = vi.fn();

  frameCount = 0;
  frameLabels = [];
  frameLabelAlphas = [];
  frameStrokes = [];
  labelPositions = new Map();
  HTMLCanvasElement.prototype.getContext = vi.fn(function () {
    if (this.__mockCtx) return this.__mockCtx;
    const isMain = this.classList?.contains('treemap-canvas');
    const ctx = {
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
      stroke: vi.fn(function () {
        if (isMain) frameStrokes.push({ color: ctx.strokeStyle, alpha: ctx.globalAlpha, width: ctx.lineWidth });
      }),
      moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn((text, x, y) => {
        if (!isMain) return;
        const t = String(text);
        frameLabels.push(t);
        frameLabelAlphas.push({ text: t, alpha: ctx.globalAlpha });
        // First position per label — the node's screen center (click target).
        if (!labelPositions.has(t)) labelPositions.set(t, { x, y });
      }),
      strokeText: vi.fn(),
      measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(),
      setTransform: vi.fn(() => {
        if (isMain) {
          frameCount++;
          frameLabels = [];
          frameLabelAlphas = [];
          frameStrokes = [];
        }
      }),
      save: vi.fn(), restore: vi.fn(),
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
      roundRect: vi.fn(),
      clip: vi.fn(),
      ellipse: vi.fn(),
      createLinearGradient: vi.fn(() => ({ addColorStop: vi.fn() })),
      filter: '',
    };
    this.__mockCtx = ctx;
    return ctx;
  });

  sockets = [];
  global.WebSocket = MockWebSocket;
  localStorage.setItem('gyre_auth_token', 'bearer-token-1');
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ ticket: 'ticket-abc-123' }),
    })
  );
});

function renderRepoScope(graph = GRAPH) {
  fn('repoGraph').mockResolvedValue(graph);
  return render(ExplorerView, {
    props: { scope: { type: 'repo', repoId: 'repo-1', workspaceId: 'ws-1' } },
  });
}

async function waitForCanvas() {
  await waitFor(() => {
    expect(document.querySelector('.explorer-canvas-area canvas')).toBeTruthy();
  }, { timeout: 5000 });
}

async function waitForCanvasSettled() {
  let prev = -1;
  await waitFor(
    async () => {
      const c = frameCount;
      if (c === prev) return;
      prev = c;
      await new Promise((r) => setTimeout(r, 30));
      throw new Error('canvas still drawing');
    },
    { timeout: 5000 }
  );
}

async function labelsOfNextRedraw(count) {
  await waitFor(() => {
    expect(frameCount).toBeGreaterThan(count);
  }, { timeout: 5000 });
  return [...frameLabels];
}

/** Label → latest draw opacity within the current frame. */
function labelAlphaMap() {
  const m = new Map();
  for (const { text, alpha } of frameLabelAlphas) m.set(text, alpha);
  return m;
}

// ── Concept browser + manifest concept selection ──────────────────────────

describe('ExplorerView — concept views (system-explorer.md §4)', () => {
  it('concept browser lists predefined concepts from the manifest', async () => {
    fn('repoGraphConcepts').mockResolvedValue({
      repo_id: 'repo-1',
      concepts: [
        { name: 'Authentication', description: 'Token validation, RBAC, ABAC, JWT handling' },
        { name: 'Billing', description: 'Invoicing and payments' },
      ],
    });
    renderRepoScope();
    await waitForCanvas();

    await fireEvent.click(screen.getByTestId('filter-panel-toggle'));
    const panel = document.querySelector('[data-testid="explorer-filter-panel"]');
    expect(panel).toBeTruthy();

    // The panel opened → the manifest listing was fetched.
    await waitFor(() => expect(fn('repoGraphConcepts')).toHaveBeenCalledWith('repo-1'));

    // Predefined (manifest) concepts render, distinct from the discovered
    // (substring) section.
    const auth = await within(panel).findByTestId('filter-manifest-concept-Authentication');
    expect(auth).toBeTruthy();
    expect(within(panel).getByTestId('filter-manifest-concept-Billing')).toBeTruthy();
    expect(within(panel).getByText('Predefined')).toBeTruthy();
    // Discovered concepts (substring mechanism) are still listed — both
    // data sources are supported per the spec's distinction.
    expect(within(panel).getByText('Discovered')).toBeTruthy();
    expect(within(panel).getByTestId('filter-concept-tokens')).toBeTruthy();
  });

  it('selecting a predefined concept fetches the manifest projection and fades non-concept nodes (not removed)', async () => {
    fn('repoGraphConcepts').mockResolvedValue({
      repo_id: 'repo-1',
      concepts: [{ name: 'Authentication', description: 'Token validation and JWT handling' }],
    });
    // Manifest-based projection: exactly the concept's nodes (the
    // manifest mechanism — NOT a substring search).
    fn('getGraphConcept').mockResolvedValue({
      repo_id: 'repo-1',
      nodes: [
        { id: 'mod-a', name: 'tokens' },
        { id: 'n-a', name: 'JwtAuthProvider' },
        { id: 'n-a2', name: 'TokenStore' },
      ],
      edges: [],
    });
    renderRepoScope();
    await waitForCanvas();

    await waitFor(() => {
      expect(labelAlphaMap().get('shipping-domain')).toBeGreaterThan(0);
    }, { timeout: 5000 });
    await waitForCanvasSettled();
    const baseline = labelAlphaMap();

    await fireEvent.click(screen.getByTestId('filter-panel-toggle'));
    const panel = document.querySelector('[data-testid="explorer-filter-panel"]');
    const chip = await within(panel).findByTestId('filter-manifest-concept-Authentication');

    const framesBefore = frameCount;
    await fireEvent.click(chip);
    expect(chip.getAttribute('aria-pressed')).toBe('true');

    // The manifest projection endpoint resolved the concept's node set.
    await waitFor(() => expect(fn('getGraphConcept')).toHaveBeenCalledWith('repo-1', 'Authentication'));
    await labelsOfNextRedraw(framesBefore);

    // Active concept shows as a control-bar filter chip (click to clear).
    const barChip = document.querySelector('[data-testid="concept-filter-chip"]');
    expect(barChip).toBeTruthy();
    expect(barChip.textContent).toContain('Authentication');

    // Fade, not removal: the unrelated shipping package stays drawn at
    // ~15% of its baseline opacity; the auth package (concept nodes
    // inside) keeps its full draw opacity.
    const faded = labelAlphaMap();
    expect(faded.get('shipping-domain'), 'non-concept node must stay drawn').toBeDefined();
    expect(faded.get('shipping-domain') / baseline.get('shipping-domain')).toBeCloseTo(0.15, 5);
    expect(faded.get('auth-domain') / baseline.get('auth-domain')).toBeCloseTo(1.0, 5);

    // Clearing via the control-bar chip restores the full-opacity graph.
    const framesBeforeClear = frameCount;
    await fireEvent.click(document.querySelector('[data-testid="concept-filter-chip-clear"]'));
    await labelsOfNextRedraw(framesBeforeClear);
    const restored = labelAlphaMap();
    expect(restored.get('shipping-domain') / baseline.get('shipping-domain')).toBeCloseTo(1.0, 5);
    expect(document.querySelector('[data-testid="concept-filter-chip"]')).toBeNull();
  });

  it('multi-select + right-click "Create concept" saves an ad-hoc concept as an explorer view', async () => {
    renderRepoScope(TWO_LEAF_GRAPH);
    await waitForCanvas();

    await waitFor(() => {
      expect(labelPositions.get('JwtAuthProvider')).toBeDefined();
      expect(labelPositions.get('TokenStore')).toBeDefined();
    }, { timeout: 5000 });
    await waitForCanvasSettled();

    const canvas = document.querySelector('canvas.treemap-canvas');
    const clickAt = (pos, shiftKey) => {
      // onClick rejects clicks that moved >4px from mousedown (pan guard).
      canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: pos.x, clientY: pos.y, bubbles: true }));
      canvas.dispatchEvent(new MouseEvent('mouseup', { clientX: pos.x, clientY: pos.y, bubbles: true }));
      canvas.dispatchEvent(new MouseEvent('click', { clientX: pos.x, clientY: pos.y, shiftKey, bubbles: true }));
    };

    // Shift+Click both leaves → multi-select (concept creation selection).
    clickAt(labelPositions.get('JwtAuthProvider'), true);
    clickAt(labelPositions.get('TokenStore'), true);
    await waitFor(() => {
      expect(document.querySelector('.concept-creation-bar')).toBeTruthy();
    }, { timeout: 5000 });

    // Right-click on the selection → context menu offers "Create concept".
    const pos = labelPositions.get('JwtAuthProvider');
    canvas.dispatchEvent(new MouseEvent('contextmenu', { clientX: pos.x, clientY: pos.y, bubbles: true }));
    const menuItem = await screen.findByTestId('ctx-create-concept');
    expect(menuItem.textContent).toContain('Create concept');
    await fireEvent.click(menuItem);

    await fireEvent.click(menuItem);
    const dialog = await screen.findByTestId('concept-dialog');
    expect(dialog.textContent).toContain('2 selected nodes');
    await fireEvent.input(screen.getByTestId('concept-dialog-name'), {
      target: { value: 'Auth core' },
    });
    await fireEvent.click(screen.getByTestId('concept-dialog-save'));

    // The grouping is persisted as an explorer view: a ViewQuery concept
    // scope seeded with exactly the selected nodes (no edge expansion —
    // the ad-hoc concept IS the grouped nodes).
    await waitFor(() => {
      expect(fn('saveExplorerView')).toHaveBeenCalledWith(
        'ws-1',
        expect.objectContaining({
          name: 'Auth core',
          spec: expect.objectContaining({
            scope: expect.objectContaining({
              type: 'concept',
              seed_nodes: ['JwtAuthProvider', 'TokenStore'],
              expand_depth: 0,
            }),
          }),
        })
      );
    }, { timeout: 5000 });

    // Success closes the dialog.
    await waitFor(() => {
      expect(screen.queryByTestId('concept-dialog')).toBeNull();
    }, { timeout: 5000 });
  });

  it('create-concept dialog requires a name and surfaces save errors', async () => {
    renderRepoScope(TWO_LEAF_GRAPH);
    await waitForCanvas();

    await waitFor(() => {
      expect(labelPositions.get('JwtAuthProvider')).toBeDefined();
      expect(labelPositions.get('TokenStore')).toBeDefined();
    }, { timeout: 5000 });
    await waitForCanvasSettled();

    const canvas = document.querySelector('canvas.treemap-canvas');
    const clickAt = (pos, shiftKey) => {
      canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: pos.x, clientY: pos.y, bubbles: true }));
      canvas.dispatchEvent(new MouseEvent('mouseup', { clientX: pos.x, clientY: pos.y, bubbles: true }));
      canvas.dispatchEvent(new MouseEvent('click', { clientX: pos.x, clientY: pos.y, shiftKey, bubbles: true }));
    };
    clickAt(labelPositions.get('JwtAuthProvider'), true);
    clickAt(labelPositions.get('TokenStore'), true);
    await waitFor(() => {
      expect(document.querySelector('.concept-creation-bar')).toBeTruthy();
    }, { timeout: 5000 });

    const pos = labelPositions.get('TokenStore');
    canvas.dispatchEvent(new MouseEvent('contextmenu', { clientX: pos.x, clientY: pos.y, bubbles: true }));
    await fireEvent.click(await screen.findByTestId('ctx-create-concept'));

    // Empty name → inline error, no save call.
    await fireEvent.click(await screen.findByTestId('concept-dialog-save'));
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(fn('saveExplorerView')).not.toHaveBeenCalled();

    // Server rejection → the error is surfaced and the dialog stays open.
    fn('saveExplorerView').mockRejectedValue(new Error('view name already exists'));
    await fireEvent.input(screen.getByTestId('concept-dialog-name'), {
      target: { value: 'Auth core' },
    });
    await fireEvent.click(screen.getByTestId('concept-dialog-save'));
    expect(await waitFor(() => screen.getByRole('alert'))).toBeTruthy();
    expect(screen.getByRole('alert').textContent).toContain('already exists');
    expect(screen.queryByTestId('concept-dialog')).toBeTruthy();
  });
});

// ── Concept edge emphasis (canvas draw path) ──────────────────────────────

describe('ExplorerCanvas — concept view edge emphasis (draw path)', () => {
  // Functions with multi-segment qualified names land as leaf cells inside
  // one shared tree-group at a high initial fit zoom (same fixture shape
  // as the filter-dimming tests), so individual edges draw (zoom ≥ 0.5).
  const CN = (id) => ({
    id, node_type: 'function', name: id, qualified_name: `app.svc.${id}`,
    file_path: 'app/svc.py', line_start: 0, line_end: 0,
    visibility: 'public', spec_confidence: 'none', test_node: false,
  });
  const NODES = [CN('part1'), CN('part2'), CN('lone1'), CN('lone2')];
  const EDGES = [
    { id: 'e1', source_id: 'part1', target_id: 'part2', edge_type: 'calls' },
    { id: 'e2', source_id: 'lone1', target_id: 'lone2', edge_type: 'calls' },
  ];

  let ctxMock;
  let strokes;

  beforeEach(() => {
    strokes = [];
    ctxMock = {
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
      stroke: vi.fn(function () {
        strokes.push({ color: ctxMock.strokeStyle, alpha: ctxMock.globalAlpha, width: ctxMock.lineWidth });
      }),
      moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn(), strokeText: vi.fn(),
      measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
      roundRect: vi.fn(), clip: vi.fn(), ellipse: vi.fn(),
      createLinearGradient: vi.fn(() => ({ addColorStop: vi.fn() })),
      filter: '',
    };
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ctxMock);
    global.ResizeObserver = class ResizeObserver {
      observe() {} disconnect() {} unobserve() {}
    };
    global.requestAnimationFrame = vi.fn((cb) => { cb(16); return 1; });
    global.cancelAnimationFrame = vi.fn();
  });

  function renderCanvas(props) {
    return render(ExplorerCanvas, props);
  }

  it('edges between concept nodes are highlighted; other edges fade with their endpoints', async () => {
    renderCanvas({
      nodes: NODES,
      edges: EDGES,
      conceptNodeIds: new Set(['part1', 'part2']),
    });
    await new Promise((r) => setTimeout(r, 50));

    // Concept edge (part1→part2): recolored to the concept accent and
    // boosted to high alpha (≥ 0.8 — the emphasis floor). Concept node
    // borders share the accent color at the node's own (zoom-faded) draw
    // opacity, so the assertion is existence of the boosted stroke, not
    // a blanket claim about every accent stroke.
    const conceptStrokes = strokes.filter((s) => s.color === '#a78bfa');
    expect(conceptStrokes.length, 'concept edge/border must be drawn with the concept accent').toBeGreaterThan(0);
    expect(
      conceptStrokes.some((s) => s.alpha >= 0.79),
      'the edge between concept nodes must be highlighted (alpha ≥ 0.8)'
    ).toBe(true);

    // Non-concept edge (lone1→lone2): keeps the calls color but is faded
    // to background with its endpoints (zoom-faded base × 0.15).
    const callsStrokes = strokes.filter((s) => s.color === '#60a5fa');
    expect(callsStrokes.length, 'non-concept calls edge must still be drawn (faded)').toBeGreaterThan(0);
    for (const s of callsStrokes) {
      expect(s.alpha, `non-concept edge must fade to background: ${JSON.stringify(s)}`).toBeLessThanOrEqual(0.02);
    }
  });

  it('without an active concept, edges keep their base color and alpha (no concept emphasis leaks)', async () => {
    renderCanvas({ nodes: NODES, edges: EDGES, conceptNodeIds: null });
    await new Promise((r) => setTimeout(r, 50));

    expect(strokes.filter((s) => s.color === '#a78bfa')).toHaveLength(0);
    const callsStrokes = strokes.filter((s) => s.color === '#60a5fa');
    expect(callsStrokes.length).toBeGreaterThan(0);
    // Base (unfaded) draw alpha — the same edge in the concept test above
    // is drawn at ≤ 2% of this (the fade), so this pins the contrast.
    for (const s of callsStrokes) {
      expect(s.alpha, `unfaded edge must keep its base alpha: ${JSON.stringify(s)}`).toBeGreaterThanOrEqual(0.09);
    }
  });
});
