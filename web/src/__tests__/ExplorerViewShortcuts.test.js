/**
 * ExplorerViewShortcuts.test.js — explorer-implementation.md §Phase 4
 * ("Keyboard shortcuts (Escape to clear query, / to focus chat)") and
 * system-explorer.md §1 / ui-layout.md §2.
 *
 * Verifies the ExplorerView window-level keyboard handler (onWindowKeydown,
 * wired via <svelte:window onkeydown>):
 *   - Escape clears the active view query (which is also how trace mode —
 *     an interactive view query with _trace: true, produced by "Trace from
 *     here" — is exited) and returns the canvas to its default state
 *   - Escape closes the detail panel when one is open
 *   - Escape closes overlays in cascade order (most recent first)
 *   - '/' focuses the chat input and preventDefaults so no "/" is typed
 *   - none of the shortcuts fire while a text input is focused
 *
 * The full ExplorerView tree is rendered (graph load, ExplorerCanvas,
 * ExplorerChat with a mocked WebSocket) so the assertions exercise the real
 * event wiring, not an extracted copy of the handler.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// ── API mock ─────────────────────────────────────────────────────────────
// Must be before the ExplorerView import so vi.mock runs first.
vi.mock('../lib/api.js', () => {
  const nodes = [
    { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
    { id: 'fn1', node_type: 'function', name: 'create_user', qualified_name: 'api.create_user', file_path: 'api/handlers.py', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
    { id: 'type1', node_type: 'type', name: 'User', qualified_name: 'domain.User', file_path: 'domain/models.py', line_start: 1, line_end: 20, visibility: 'public', spec_confidence: 'high', test_node: false },
  ];
  const edges = [
    { id: 'e1', source_id: 'pkg1', target_id: 'fn1', edge_type: 'contains' },
    { id: 'e2', source_id: 'pkg1', target_id: 'type1', edge_type: 'contains' },
    { id: 'e3', source_id: 'fn1', target_id: 'type1', edge_type: 'calls' },
  ];
  return {
    api: {
      workspaces: vi.fn().mockResolvedValue([]),
      workspaceBudget: vi.fn().mockResolvedValue(null),
      repos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
      workspaceRepos: vi.fn().mockResolvedValue([]),
      allRepos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
      repoGraph: vi.fn().mockResolvedValue({ nodes, edges }),
      getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
      generateExplorerView: vi.fn().mockResolvedValue({ ok: false }),
      mergeRequests: vi.fn().mockResolvedValue({ merge_requests: [] }),
      mrTrace: vi.fn().mockResolvedValue(null),
      graphQueryDryrun: vi.fn().mockResolvedValue(null),
    },
  };
});

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

// ── Shared mocks ─────────────────────────────────────────────────────────

let mockCtx;
let sockets; // WebSocket instances created by ExplorerChat
let rafQueue; // pending requestAnimationFrame callbacks

beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(),
    setTransform: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
    font: '',
    textAlign: '',
    textBaseline: '',
    shadowColor: '',
    shadowBlur: 0,
    setLineDash: vi.fn(),
    getLineDash: vi.fn(() => []),
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  // rAF callbacks are queued and only run when flushRaf() is called — the
  // same "after the current task" semantics as a real browser. (A naive
  // synchronous cb() mock breaks ExplorerCanvas's animation loop: the nested
  // synchronous execution inside `animFrame = requestAnimationFrame(animLoop)`
  // leaves a stale non-null animFrame, so later scheduleRedraw() calls never
  // schedule and the camera stops animating.)
  rafQueue = [];
  global.requestAnimationFrame = vi.fn(cb => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();

  sockets = [];
  global.WebSocket = class MockWebSocket {
    constructor(url) {
      this.url = url;
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
      sockets.push(this);
    }
  };

  global.window.matchMedia = vi.fn((query) => ({
    matches: false,
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));

  // ExplorerChat's ticket-based WS auth reads the auth token from
  // localStorage; without it the chat input stays disabled and cannot be
  // focused by the '/' shortcut.
  localStorage.setItem('gyre_auth_token', 'test-token');
});

afterEach(() => {
  vi.restoreAllMocks();
  sockets = [];
  rafQueue = [];
});

/** Run queued rAF frames until the animation settles (camera lerp, fades). */
function flushRaf(maxFrames = 3000) {
  let batches = 0;
  while (rafQueue.length > 0 && batches < maxFrames) {
    const batch = rafQueue;
    rafQueue = [];
    for (const cb of batch) cb(performance.now());
    batches++;
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────

const REPO_SCOPE = { type: 'repo', repoId: 'test-repo-1', workspaceId: 'ws1' };

function renderExplorerView(props = {}) {
  return render(ExplorerView, {
    props: { scope: REPO_SCOPE, ...props },
    context: new Map([
      ['navigate', vi.fn()],
      ['goToWorkspaceSettings', vi.fn()],
    ]),
  });
}

/** Wait until the graph has loaded and the canvas + control bar are up. */
async function renderLoadedExplorerView() {
  const result = renderExplorerView();
  await waitFor(() => {
    expect(result.container.querySelector('.treemap-canvas')).toBeTruthy();
  });
  // Concept search input starts disabled while loading; enabled == loaded.
  await waitFor(() => {
    expect(result.container.querySelector('.concept-input').disabled).toBe(false);
  });
  return result;
}

/** Bring the ExplorerChat session to 'ready' so its input is focusable. */
async function connectChat(container) {
  await waitFor(() => expect(sockets.length).toBeGreaterThan(0));
  sockets[0].onopen();
  const chatInput = container.querySelector('.chat-input');
  await waitFor(() => expect(chatInput.disabled).toBe(false));
  return chatInput;
}

// The exact view-query shape produced by the canvas context menu's
// "Trace from here" action (ExplorerCanvas.contextMenuAction('trace')).
const TRACE_QUERY = {
  scope: { type: 'focus', node: 'create_user', edges: ['calls', 'routes_to'], direction: 'outgoing', depth: 15 },
  emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#22c55e', '#94a3b8'], dim_unmatched: 0.12 },
  edges: { filter: ['calls', 'routes_to'] },
  zoom: 'fit',
  annotation: { title: 'Trace from: create_user', description: 'Causal flow showing execution order' },
  _trace: true,
};

// ── Tests ────────────────────────────────────────────────────────────────

describe('ExplorerView — keyboard shortcuts (explorer-implementation.md §Phase 4)', () => {
  it("'/' focuses the chat input and preventDefaults so no '/' is typed into it", async () => {
    const { container } = await renderLoadedExplorerView();
    const chatInput = await connectChat(container);

    // Nothing focused: dispatch on window (target = window, not an input).
    const ev = new KeyboardEvent('keydown', { key: '/', bubbles: true, cancelable: true });
    window.dispatchEvent(ev);

    // preventDefault is what keeps the '/' character out of the input.
    expect(ev.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(chatInput);
    expect(chatInput.value).toBe('');
  });

  it("'/' does not focus the chat input while a text input is focused", async () => {
    const { container } = await renderLoadedExplorerView();
    const chatInput = await connectChat(container);
    const conceptInput = container.querySelector('.concept-input');
    conceptInput.focus();
    expect(document.activeElement).toBe(conceptInput);

    const ev = new KeyboardEvent('keydown', { key: '/', bubbles: true, cancelable: true });
    conceptInput.dispatchEvent(ev);

    // The isTyping guard (INPUT/TEXTAREA/SELECT/contentEditable target) must
    // suppress the shortcut entirely: no preventDefault, no focus steal.
    expect(ev.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(conceptInput);
    expect(document.activeElement).not.toBe(chatInput);
  });

  it('Escape clears the active view query and returns the canvas to its default state', async () => {
    const { container } = await renderLoadedExplorerView();

    // Activate an interactive view query through a real UI control (the Risk
    // heat map toolbar button routes through onInteractiveQuery, the same
    // path "Trace from here" and the chat view queries use).
    const riskBtn = container.querySelector('button[title^="Toggle risk heat map"]');
    expect(riskBtn).toBeTruthy();
    fireEvent.click(riskBtn);

    const annotation = await waitFor(() => {
      const el = container.querySelector('.query-annotation');
      expect(el).toBeTruthy();
      return el;
    });
    expect(container.querySelector('.annotation-title').textContent).toContain('Risk Map');
    expect(riskBtn.classList.contains('active')).toBe(true);

    fireEvent.keyDown(window, { key: 'Escape' });

    // Query cleared: annotation gone and the canvas overlay flag reset.
    await waitFor(() => {
      expect(container.querySelector('.query-annotation')).toBeFalsy();
    });
    await waitFor(() => {
      expect(riskBtn.classList.contains('active')).toBe(false);
    });
    expect(annotation).toBeTruthy(); // (was the element we saw before Escape)
  });

  it('Escape exits trace mode (active view query with _trace: true)', async () => {
    const { container } = await renderLoadedExplorerView();

    // Open the manual view query editor and run the trace query — the same
    // query object the canvas context menu's "Trace from here" produces.
    fireEvent.click(container.querySelector('button[aria-label="Toggle manual view query editor"]'));
    const textarea = await waitFor(() => {
      const el = container.querySelector('.query-editor-textarea');
      expect(el).toBeTruthy();
      return el;
    });
    fireEvent.input(textarea, { target: { value: JSON.stringify(TRACE_QUERY) } });
    fireEvent.click(container.querySelector('.query-editor-run-btn'));

    await waitFor(() => {
      expect(container.querySelector('.query-annotation')).toBeTruthy();
    });
    expect(container.querySelector('.annotation-title').textContent).toContain('Trace from: create_user');

    // Trace mode is genuinely active, not just annotated: run the animation
    // frames (the query's zoom:'fit' converges the camera onto the matched
    // nodes, which lifts the tree-group LOD so the numbered step badges are
    // drawn) — ①=create_user, ②=User via the calls edge.
    flushRaf();
    const drawnTexts = mockCtx.fillText.mock.calls.map(c => c[0]);
    expect(drawnTexts).toContain('1');
    expect(drawnTexts).toContain('2');

    // Escape cascade: the query editor (most recent overlay) closes first,
    // the trace query stays active until the second Escape.
    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(container.querySelector('.query-editor-panel')).toBeFalsy();
    });
    expect(container.querySelector('.query-annotation')).toBeTruthy();

    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(container.querySelector('.query-annotation')).toBeFalsy();
    });

    // Trace mode is really exited: the post-Escape redraw still draws the
    // graph (labels) but no longer draws the trace step badges.
    mockCtx.fillText.mockClear();
    flushRaf();
    const postEscapeTexts = mockCtx.fillText.mock.calls.map(c => c[0]);
    expect(postEscapeTexts).toContain('create_user'); // redraw happened
    expect(postEscapeTexts).not.toContain('1');
    expect(postEscapeTexts).not.toContain('2');
  });

  it('Escape closes the detail panel when one is open', async () => {
    const { container } = await renderLoadedExplorerView();

    // Open the detail panel through the canvas-scoped search: focus the
    // canvas, press '/' to open the search overlay, then select a result.
    const canvas = container.querySelector('.treemap-canvas');
    canvas.focus();
    fireEvent.keyDown(canvas, { key: '/' });
    const searchInput = await waitFor(() => {
      const el = container.querySelector('.canvas-search-input');
      expect(el).toBeTruthy();
      return el;
    });
    fireEvent.input(searchInput, { target: { value: 'create' } });
    const result = await waitFor(() => {
      const el = container.querySelector('.search-result-item');
      expect(el).toBeTruthy();
      return el;
    });
    fireEvent.click(result);

    await waitFor(() => {
      expect(container.querySelector('.explorer-detail-area')).toBeTruthy();
    });

    fireEvent.keyDown(window, { key: 'Escape' });

    await waitFor(() => {
      expect(container.querySelector('.explorer-detail-area')).toBeFalsy();
    });
  });

  it('Escape does not clear the active view query while a text input is focused', async () => {
    const { container } = await renderLoadedExplorerView();

    const riskBtn = container.querySelector('button[title^="Toggle risk heat map"]');
    fireEvent.click(riskBtn);
    await waitFor(() => {
      expect(container.querySelector('.query-annotation')).toBeTruthy();
    });

    // Escape typed inside the concept search input must NOT clear the query.
    const conceptInput = container.querySelector('.concept-input');
    conceptInput.focus();
    fireEvent.keyDown(conceptInput, { key: 'Escape' });

    // Give any (incorrect) clearing a chance to flush before asserting.
    await new Promise(r => setTimeout(r, 30));
    expect(container.querySelector('.query-annotation')).toBeTruthy();
  });
});
