/**
 * ExplorerViewNavigableArchitecture.test.js — task-178 (system-explorer.md
 * §1 "The Navigable Architecture" + ui-navigation.md §3 Architecture tab):
 *
 * - Lens selector is a checkbox group; the Evaluative overlay fetches
 *   GET /repos/:id/graph/risks and exposes the Risk heat metric.
 * - In-view filter panel: filter icon in the control bar toggles the 200px
 *   panel; category checkboxes and concept chips toggle node visibility on
 *   the canvas (asserted against actual canvas draw output).
 * - View selector dropdown: saved views (delivered by the explorer chat
 *   WebSocket's `views` message) are listed and loadable; "Generate
 *   view..." focuses the Ask/generate chat.
 * - View spec editor: "Edit view spec" opens a collapsible JSON editor;
 *   Apply translates the ViewSpec to a canvas view query (annotation
 *   title rendered by the canvas), invalid JSON shows an inline error.
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

// ── Mock WebSocket (saved views arrive via the explorer chat socket) ──────
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
  simulateMessage(obj) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
}

const GRAPH = {
  nodes: [
    { id: 'pkg-1', node_type: 'package', name: 'payment-domain', qualified_name: 'payment-domain', visibility: 'public', spec_confidence: 'none' },
    { id: 'mod-1', node_type: 'module', name: 'retry', qualified_name: 'payment-domain::retry', visibility: 'public', spec_confidence: 'none' },
    { id: 'n-1', node_type: 'type', name: 'RetryPolicy', qualified_name: 'payment-domain::retry::RetryPolicy', spec_path: 'specs/system/payment-retry.md', spec_confidence: 'high', visibility: 'public' },
  ],
  edges: [
    { id: 'c1', source_id: 'pkg-1', target_id: 'mod-1', edge_type: 'contains' },
    { id: 'c2', source_id: 'mod-1', target_id: 'n-1', edge_type: 'contains' },
  ],
};

// Captured canvas draw output, attributed to the MAIN canvas
// (.treemap-canvas) and bucketed per frame: drawFrame() begins with
// ctx.setTransform, so each setTransform starts a new frame and every
// fillText accumulates into the current frame's label list. Unlike a
// snapshot-and-clear list — which reads empty whenever the canvas is idle
// between redraws, making `not.toContain` assertions vacuous — the latest
// frame's labels persist, so a negative assertion can only pass when a
// frame drawn AFTER the interaction really omits the label.
let frameCount;
let frameLabels;

beforeEach(() => {
  fns.clear();
  // Lens selection persists in the URL (?lens=evaluative) — start every
  // test from a clean URL so a previous test's lens deep link cannot leak
  // into the next render's initial lens.
  window.history.pushState({}, '', '/workspaces/ws-1/explorer');
  fn('repoGraph').mockResolvedValue(GRAPH);
  fn('allRepos').mockResolvedValue([{ id: 'repo-1', name: 'payment-api' }]);
  fn('repos').mockResolvedValue([{ id: 'repo-1', name: 'payment-api' }]);
  fn('mergeRequests').mockResolvedValue([]);
  fn('mrTrace').mockResolvedValue({ spans: [] });
  fn('getSpecs').mockResolvedValue([
    { path: 'specs/system/payment-retry.md', title: 'Payment Retry', approval_status: 'approved' },
  ]);
  fn('getSpecProgress').mockResolvedValue({ total_tasks: 2, completed_tasks: 1 });
  fn('repoGraphTimeline').mockResolvedValue([]);
  fn('mrTimeline').mockResolvedValue({ mr_id: 'mr-1', events: [] });
  fn('mrAttestation').mockResolvedValue(null);
  fn('mrDiff').mockResolvedValue({ diff: '' });
  fn('repoGraphRisks').mockResolvedValue([
    { node_id: 'n-1', name: 'RetryPolicy', churn_rate: 3, complexity: 5, fan_in: 2, fan_out: 1, spec_covered: true },
  ]);

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
  // One stable 2d context per canvas element (the main canvas and the
  // minimap each get their own) so draw calls can be attributed per canvas.
  HTMLCanvasElement.prototype.getContext = vi.fn(function () {
    if (this.__mockCtx) return this.__mockCtx;
    const isMain = this.classList?.contains('treemap-canvas');
    const ctx = {
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
      stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn((text) => { if (isMain) frameLabels.push(String(text)); }),
      strokeText: vi.fn(),
      measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(),
      setTransform: vi.fn(() => { if (isMain) { frameCount++; frameLabels = []; } }),
      save: vi.fn(), restore: vi.fn(),
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
      roundRect: vi.fn(),
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

function renderRepoScope() {
  return render(ExplorerView, {
    props: { scope: { type: 'repo', repoId: 'repo-1', workspaceId: 'ws-1' } },
  });
}

async function waitForCanvas() {
  await waitFor(() => {
    expect(document.querySelector('.explorer-canvas-area canvas')).toBeTruthy();
  }, { timeout: 5000 });
}

function drawnLabels() {
  // Labels of the latest drawn frame. Redraws are synchronous (rAF mock),
  // and the frame's labels persist until the next frame replaces them, so
  // this reads the canvas's current rendered state — never an empty
  // snapshot of an idle canvas.
  return [...frameLabels];
}

/** Wait until the render loop settles (no new frames for two consecutive
 *  reads) — interactions below capture a frame baseline afterwards. */
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

/** Wait for at least one frame drawn after `count`, then return that
 *  frame's labels — the canvas state as actually rendered after the
 *  interaction that triggered the redraw. */
async function labelsOfNextRedraw(count) {
  await waitFor(() => {
    expect(frameCount).toBeGreaterThan(count);
  }, { timeout: 5000 });
  return drawnLabels();
}

// ── Lens selector: checkbox group + risk overlay ──────────────────────────

describe('ExplorerView — lens checkbox group (ui-navigation.md §3)', () => {
  it('renders Structural (locked on), Evaluative (toggle), Observable (future) checkboxes', async () => {
    renderRepoScope();
    await waitForCanvas();

    const structural = document.querySelector('[data-testid="lens-checkbox-structural"] input');
    const evaluative = document.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    const observable = document.querySelector('[data-testid="lens-checkbox-observable"] input');
    expect(structural && evaluative && observable).toBeTruthy();
    // Structural is always the base layer — checked and locked.
    expect(structural.checked).toBe(true);
    expect(structural.disabled).toBe(true);
    // Evaluative is an additive overlay toggle.
    expect(evaluative.checked).toBe(false);
    expect(evaluative.disabled).toBe(false);
    // Observable is future — locked off (role=checkbox wrapper around the
    // disabled visual input; keyboard-operable per the a11y repair).
    expect(observable.checked).toBe(false);
    expect(observable.disabled).toBe(true);
    const observableBox = document.querySelector('[data-testid="lens-checkbox-observable"]');
    expect(observableBox.getAttribute('aria-checked')).toBe('false');
    expect(observableBox.getAttribute('aria-disabled')).toBe('true');
    // Keyboard operation surfaces the telemetry notice (same as click).
    expect(document.querySelector('.observable-banner')).toBeNull();
    observableBox.focus();
    await fireEvent.keyDown(observableBox, { key: 'Enter' });
    expect(document.querySelector('.observable-banner')).toBeTruthy();
  });

  it('checking Evaluative fetches GET /repos/:id/graph/risks and shows the Risk metric', async () => {
    renderRepoScope();
    await waitForCanvas();

    const evaluative = document.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    await fireEvent.click(evaluative);
    expect(evaluative.checked).toBe(true);

    // The Evaluative lens fetches risk data from the graph/risks endpoint.
    await waitFor(() => {
      expect(fn('repoGraphRisks')).toHaveBeenCalledWith('repo-1');
    });
    // The fetched risk data lands as a selectable Risk heat metric.
    await waitFor(() => {
      expect(document.querySelector('[data-testid="evaluative-metric-risk"]')).toBeTruthy();
    });
  });

  it('unchecking Evaluative drops the URL lens param and the risk metric', async () => {
    renderRepoScope();
    await waitForCanvas();

    const evaluative = document.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    await fireEvent.click(evaluative);
    await waitFor(() => {
      expect(document.querySelector('[data-testid="evaluative-metric-risk"]')).toBeTruthy();
    });
    await fireEvent.click(evaluative);
    expect(evaluative.checked).toBe(false);
    await waitFor(() => {
      expect(document.querySelector('[data-testid="evaluative-metric-risk"]')).toBeNull();
    });
  });
});

// ── In-view filter panel ──────────────────────────────────────────────────

describe('ExplorerView — in-view filter panel (ui-navigation.md §3)', () => {
  it('is hidden until the control-bar filter icon is clicked, then shows categories with counts', async () => {
    renderRepoScope();
    await waitForCanvas();

    expect(document.querySelector('[data-testid="explorer-filter-panel"]')).toBeNull();
    await fireEvent.click(screen.getByTestId('filter-panel-toggle'));
    const panel = document.querySelector('[data-testid="explorer-filter-panel"]');
    expect(panel).toBeTruthy();

    // Category checkboxes with per-category node counts.
    const boundaries = within(panel).getByTestId('filter-category-boundaries');
    expect(boundaries.querySelector('input').checked).toBe(true);
    expect(boundaries.textContent).toContain('Boundaries');
    expect(boundaries.textContent).toContain('2'); // package + module
    for (const id of ['interfaces', 'data', 'specs']) {
      expect(within(panel).getByTestId(`filter-category-${id}`)).toBeTruthy();
    }

    // Collapsible via the panel's own collapse button.
    await fireEvent.click(within(panel).getByRole('button', { name: /collapse filter panel/i }));
    expect(document.querySelector('[data-testid="explorer-filter-panel"]')).toBeNull();
  });

  it('unchecking Boundaries hides boundary nodes from the canvas draw', async () => {
    renderRepoScope();
    await waitForCanvas();

    // Default zoom draws the boundary labels (semantic zoom: boundaries first).
    await waitFor(() => {
      expect(frameCount).toBeGreaterThan(0);
      expect(drawnLabels()).toContain('payment-domain');
    }, { timeout: 5000 });
    await waitForCanvasSettled();

    await fireEvent.click(screen.getByTestId('filter-panel-toggle'));
    const panel = document.querySelector('[data-testid="explorer-filter-panel"]');
    const boundaries = within(panel).getByTestId('filter-category-boundaries');
    const framesBefore = frameCount;
    await fireEvent.click(boundaries.querySelector('input'));

    // The frame drawn AFTER the filter change omits the boundary nodes —
    // including the synthetic package/module tree-group boxes, which ARE
    // the boundaries at default zoom (task-178 repair: the draw loop used
    // to exempt tree-groups, making the Boundaries checkbox a user-visible
    // no-op in the default containment layout).
    const labels = await labelsOfNextRedraw(framesBefore);
    expect(labels).not.toContain('payment-domain');
    expect(labels).not.toContain('retry');

    // Round trip: re-checking Boundaries brings the nodes back — proves the
    // absence above was a real redraw, not a stalled canvas.
    const framesBeforeRestore = frameCount;
    await fireEvent.click(boundaries.querySelector('input'));
    const restored = await labelsOfNextRedraw(framesBeforeRestore);
    expect(restored).toContain('payment-domain');
  });

  it('selecting a discovered concept focuses the canvas on matching nodes', async () => {
    renderRepoScope();
    await waitForCanvas();

    // The unfiltered canvas draws the top-level boundary summary.
    await waitFor(() => {
      expect(drawnLabels()).toContain('payment-domain');
    }, { timeout: 5000 });
    await waitForCanvasSettled();

    await fireEvent.click(screen.getByTestId('filter-panel-toggle'));
    const panel = document.querySelector('[data-testid="explorer-filter-panel"]');

    // Concepts are discovered from the graph: "retry" spans the retry module
    // and RetryPolicy; "payment" spans all three nodes.
    const retryChip = within(panel).getByTestId('filter-concept-retry');
    expect(retryChip).toBeTruthy();
    expect(within(panel).getByTestId('filter-concept-payment')).toBeTruthy();

    const framesBefore = frameCount;
    await fireEvent.click(retryChip);
    expect(retryChip.getAttribute('aria-pressed')).toBe('true');

    // Nodes unrelated to the concept (the payment-domain package, whose
    // name matches no "retry") disappear from the frame drawn after the
    // selection — the concept filter reaches the tree-group containers,
    // not just the leaf nodes.
    const labels = await labelsOfNextRedraw(framesBefore);
    expect(labels).not.toContain('payment-domain');

    // Clearing the concepts restores the full graph.
    const framesBeforeClear = frameCount;
    await fireEvent.click(within(panel).getByRole('button', { name: /clear concepts/i }));
    const restored = await labelsOfNextRedraw(framesBeforeClear);
    expect(restored).toContain('payment-domain');
  });
});

// ── View selector dropdown: saved views + generate option ─────────────────

describe('ExplorerView — view selector dropdown (ui-navigation.md §3)', () => {
  it('lists saved views delivered by the explorer chat socket and loads one on click', async () => {
    renderRepoScope();
    await waitForCanvas();

    // The explorer chat WebSocket connects and requests the saved views.
    await waitFor(() => expect(sockets.length).toBe(1), { timeout: 5000 });
    sockets[0].simulateOpen();
    sockets[0].simulateMessage({
      type: 'views',
      views: [
        { id: 'v-1', name: 'My Endpoints', description: 'endpoint surface', query: { scope: { type: 'filter', node_types: ['endpoint'] }, annotation: { title: 'My Endpoints' } } },
      ],
    });

    const selector = document.querySelector('[data-testid="explorer-view-selector"]');
    await fireEvent.click(within(selector).getByRole('button', { name: /view selector/i }));
    const menu = selector.querySelector('.view-select-menu');
    expect(menu).toBeTruthy();

    const saved = within(menu).getByTestId('view-option-saved');
    expect(saved.textContent).toContain('My Endpoints');
    await fireEvent.click(saved);

    // The saved view's query is applied to the canvas (annotation title)
    // and the collapsed selector shows the saved view's name.
    await waitFor(() => {
      expect(document.querySelector('.annotation-title')?.textContent).toBe('My Endpoints');
    });
    expect(document.querySelector('.view-select-btn').textContent).toContain('My Endpoints');
  });

  it('lists saved views from the REST endpoint when the dropdown opens (no chat socket required)', async () => {
    // GET /workspaces/:id/explorer-views (workspace-scoped records carry a
    // ViewSpec in `spec`, translated to a canvas view query on load).
    fn('explorerViews').mockResolvedValue([
      {
        id: 'wv-1',
        name: 'REST Surface',
        description: 'workspace view delivered by REST',
        is_builtin: false,
        spec: { name: 'REST Surface', layout: 'graph', data: { node_types: ['type'] }, encoding: {}, highlight: {} },
      },
    ]);
    renderRepoScope();
    await waitForCanvas();

    const selector = document.querySelector('[data-testid="explorer-view-selector"]');
    await fireEvent.click(within(selector).getByRole('button', { name: /view selector/i }));

    // Opening the dropdown fetches the saved-views list via REST — the
    // chat socket never delivers a `views` message in this test.
    await waitFor(() => {
      expect(fn('explorerViews')).toHaveBeenCalledWith('ws-1');
    });
    const menu = selector.querySelector('.view-select-menu');
    const saved = await within(menu).findByTestId('view-option-saved');
    expect(saved.textContent).toContain('REST Surface');

    await fireEvent.click(saved);
    // The workspace record's ViewSpec is translated (viewSpecToViewQuery)
    // and applied to the canvas — the annotation title comes from the spec.
    await waitFor(() => {
      expect(document.querySelector('.annotation-title')?.textContent).toBe('REST Surface');
    });
    expect(document.querySelector('.view-select-btn').textContent).toContain('REST Surface');
  });

  it('"Generate view..." focuses the natural-language view generator (Ask input)', async () => {
    renderRepoScope();
    await waitForCanvas();

    const selector = document.querySelector('[data-testid="explorer-view-selector"]');
    await fireEvent.click(within(selector).getByRole('button', { name: /view selector/i }));
    const menu = selector.querySelector('.view-select-menu');
    await fireEvent.click(within(menu).getByTestId('view-option-generate'));

    // The chat column switches to the generate mode (GeneratedViewChat).
    await waitFor(() => {
      expect(document.querySelector('.gen-chat-container')).toBeTruthy();
    });
    const genTab = document.querySelector('.chat-mode-tab:nth-child(2)');
    expect(genTab.getAttribute('aria-selected')).toBe('true');
  });
});

// ── View spec editor ──────────────────────────────────────────────────────

describe('ExplorerView — view spec editor (ui-navigation.md §3)', () => {
  it('"Edit view spec" opens a collapsible JSON editor prefilled with the current spec', async () => {
    renderRepoScope();
    await waitForCanvas();

    expect(document.querySelector('[data-testid="view-spec-editor"]')).toBeNull();
    await fireEvent.click(screen.getByTestId('view-spec-editor-toggle'));
    const editor = document.querySelector('[data-testid="view-spec-editor"]');
    expect(editor).toBeTruthy();

    const textarea = within(editor).getByTestId('view-spec-textarea');
    const parsed = JSON.parse(textarea.value);
    expect(parsed).toHaveProperty('data');
    expect(parsed).toHaveProperty('layout');

    // Collapsible via the toggle.
    await fireEvent.click(screen.getByTestId('view-spec-editor-toggle'));
    expect(document.querySelector('[data-testid="view-spec-editor"]')).toBeNull();
  });

  it('Apply translates the edited ViewSpec to a canvas view query', async () => {
    renderRepoScope();
    await waitForCanvas();

    await fireEvent.click(screen.getByTestId('view-spec-editor-toggle'));
    const editor = document.querySelector('[data-testid="view-spec-editor"]');
    const textarea = within(editor).getByTestId('view-spec-textarea');
    const apply = within(editor).getByRole('button', { name: 'Apply' });

    await fireEvent.input(textarea, {
      target: { value: JSON.stringify({ name: 'Payment types', layout: 'graph', data: { node_types: ['type'] }, encoding: {}, highlight: {} }) },
    });
    await fireEvent.click(apply);

    // No error, and the canvas renders the applied view's annotation title.
    await waitFor(() => {
      expect(document.querySelector('.annotation-title')?.textContent).toBe('Payment types');
    });
    expect(within(editor).queryByRole('alert')).toBeNull();
  });

  it('invalid JSON shows an inline error and applies nothing', async () => {
    renderRepoScope();
    await waitForCanvas();

    await fireEvent.click(screen.getByTestId('view-spec-editor-toggle'));
    const editor = document.querySelector('[data-testid="view-spec-editor"]');
    const textarea = within(editor).getByTestId('view-spec-textarea');
    const apply = within(editor).getByRole('button', { name: 'Apply' });

    await fireEvent.input(textarea, { target: { value: '{not json' } });
    await fireEvent.click(apply);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Invalid JSON');
    // The canvas keeps the previous state — no annotation was applied.
    expect(document.querySelector('.annotation-title')).toBeNull();
  });
});
