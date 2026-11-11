import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// ── API mock ─────────────────────────────────────────────────────────────
// Must be before the ExplorerView import so vi.mock runs first.
vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
    repoGraph: vi.fn(),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    generateExplorerView: vi.fn().mockResolvedValue({ ok: false }),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrTrace: vi.fn().mockResolvedValue({ spans: [] }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';
import { api } from '../lib/api.js';

// ── Mocks ────────────────────────────────────────────────────────────────

let mockCtx;

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
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();

  // Mock WebSocket for ExplorerChat
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
    }
  };

  global.window.matchMedia = vi.fn(() => ({
    matches: true, // wide viewport: chat always visible
    media: '',
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
});

afterEach(() => {
  vi.restoreAllMocks();
});

const REPO_SCOPE = { type: 'repo', repoId: 'test-repo-1', workspaceId: 'ws1' };

/**
 * Minimal graph of `count` nodes: one module + leaf functions.
 * Node shapes match what ExplorerCanvas/ExplorerView derive from
 * (id, node_type, name, qualified_name, file_path).
 */
function makeGraph(count) {
  const nodes = [
    { id: 'mod0', node_type: 'module', name: 'mod_0', qualified_name: 'pkg_0.mod_0', file_path: 'pkg_0/mod_0.py' },
  ];
  for (let i = 0; nodes.length < count; i++) {
    nodes.push({
      id: `fn${i}`,
      node_type: 'function',
      name: `fn_${i}`,
      qualified_name: `pkg_0.mod_0.fn_${i}`,
      file_path: 'pkg_0/mod_0.py',
    });
  }
  return { nodes, edges: [] };
}

function renderExplorerView(props = {}) {
  return render(ExplorerView, {
    props: { scope: REPO_SCOPE, ...props },
    context: new Map([
      ['navigate', vi.fn()],
      ['goToWorkspaceSettings', vi.fn()],
    ]),
  });
}

// ── Large graph performance warning (explorer-implementation.md §28 Phase 4) ──

describe('ExplorerView — large graph performance warning', () => {
  it('shows a warning banner above 10k nodes, names the count, and dismisses on click', { timeout: 90000 }, async () => {
    api.repoGraph.mockResolvedValue(makeGraph(10001));
    const { container } = renderExplorerView();

    await waitFor(() => {
      const banner = container.querySelector('[data-testid="large-graph-warning"]');
      expect(banner).toBeTruthy();
    });
    // The banner names the actual node count so the user knows the scale.
    expect(container.querySelector('[data-testid="large-graph-warning"]').textContent)
      .toContain('10,001');

    // Dismiss button hides the banner for this graph
    const dismiss = container.querySelector('[data-testid="large-graph-warning"] .toolchain-warning-dismiss');
    expect(dismiss).toBeTruthy();
    await fireEvent.click(dismiss);

    await waitFor(() => {
      expect(container.querySelector('[data-testid="large-graph-warning"]')).toBeFalsy();
    });
  });

  it('does not show the banner at or below the 10k threshold', { timeout: 90000 }, async () => {
    api.repoGraph.mockResolvedValue(makeGraph(10000));
    const { container } = renderExplorerView();

    // Wait until the graph has actually loaded (stats element renders),
    // so the absence of the banner is meaningful, not a race.
    await waitFor(() => {
      expect(container.querySelector('.explorer-split')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="large-graph-warning"]')).toBeFalsy();
  });

});
