/**
 * ExplorerViewLensUrl.test.js — HSI §3 Three Lenses: lens selection persists
 * in the URL query param (?lens=evaluative), making an overlaid view
 * deep-linkable. Structural is the default and drops the param (clean URLs).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    specsApprovals: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrTrace: vi.fn().mockResolvedValue({ spans: [] }),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
  },
}));

// The workspace scope renders the realized-architecture graph
// (ui-layout.md §5) via DependencyGraph — its ELK layout (WASM, unavailable
// in jsdom) is mocked with deterministic positions so repo nodes render.
vi.mock('../lib/layout-engines.js', async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    elkLayout: vi.fn().mockImplementation(async (nodes) => {
      const positions = {};
      nodes.forEach((n, i) => {
        positions[n.id] = { x: 100 + i * 200, y: 80 };
      });
      return positions;
    }),
  };
});

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import { api } from '../lib/api.js';
import ExplorerView from '../components/ExplorerView.svelte';

const REPO = { id: 'repo-1', name: 'payment-api' };

describe('ExplorerView — lens URL persistence (HSI §3 Three Lenses)', () => {
  let replaceStateSpy;

  beforeEach(() => {
    vi.clearAllMocks();
    global.window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    global.ResizeObserver = class ResizeObserver {
      observe() {}
      disconnect() {}
      unobserve() {}
    };
    global.requestAnimationFrame = vi.fn((cb) => { cb(); return 1; });
    global.cancelAnimationFrame = vi.fn();
    // Mock canvas context needed by ExplorerCanvas when a graph loads
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
      stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    }));
    const realReplaceState = window.history.replaceState.bind(window.history);
    const realPushState = window.history.pushState.bind(window.history);
    replaceStateSpy = vi.spyOn(window.history, 'replaceState').mockImplementation(realReplaceState);
    vi.spyOn(window.history, 'pushState').mockImplementation(realPushState);
  });

  afterEach(() => {
    replaceStateSpy.mockRestore();
    vi.restoreAllMocks();
  });

  /** Drill into a repo at workspace scope so the Explorer toolbar renders.
   *  API mocks must be configured BEFORE render: the workspace repo list
   *  loads during mount. */
  function primeApiMocks() {
    api.repos.mockResolvedValue([REPO]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    api.allRepos.mockResolvedValue([REPO]);
    api.repoGraph.mockResolvedValue({
      nodes: [
        { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
      ],
      edges: [],
    });
  }

  async function drillIntoRepo(container) {
    // Since task-174 the workspace scope renders the realized-architecture
    // graph (repos as nodes, ui-layout.md §5) — drill by double-clicking the
    // rendered repo node (double-click = scope change to repo).
    const repoNode = await waitFor(() => {
      const el = container.querySelector('[data-testid="dep-node-repo-1"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 5000 });
    await fireEvent.dblClick(repoNode);
    await waitFor(() => {
      expect(container.querySelector('.lens-group')).toBeTruthy();
    }, { timeout: 5000 });
  }

  it('structural is the default lens and the URL has no lens param', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    const structural = container.querySelector('[data-testid="lens-checkbox-structural"] input');
    expect(structural?.checked).toBe(true); // always-on base layer
    expect(structural?.disabled).toBe(true); // Structural cannot be toggled off
    const evaluative = container.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    expect(evaluative?.checked).toBe(false);
    expect(new URL(window.location.href).searchParams.get('lens')).toBeNull();
  });

  it('switching to the evaluative lens persists ?lens=evaluative in the URL', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    replaceStateSpy.mockClear();
    const evaluative = container.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    expect(evaluative).toBeTruthy();
    await fireEvent.click(evaluative);

    // Evaluative is now active and the URL carries the param
    expect(evaluative.checked).toBe(true);
    expect(new URL(window.location.href).searchParams.get('lens')).toBe('evaluative');
  });

  it('switching back to structural drops the lens param (clean URLs)', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    const evaluative = container.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    await fireEvent.click(evaluative);
    expect(new URL(window.location.href).searchParams.get('lens')).toBe('evaluative');

    // Unchecking the Evaluative overlay returns to structural (the base
    // layer's own checkbox is locked on — there is nothing to click back to).
    await fireEvent.click(evaluative);
    expect(evaluative.checked).toBe(false);
    expect(new URL(window.location.href).searchParams.get('lens')).toBeNull();
  });

  it('restores the evaluative lens from a ?lens=evaluative deep link', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer?lens=evaluative');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    const evaluative = container.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    expect(evaluative?.checked).toBe(true);
    expect(new URL(window.location.href).searchParams.get('lens')).toBe('evaluative');
  });

  it('loads evaluative gate data repo-scoped (repository_id, not the ignored repo_id param)', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    // loadEvaluativeData fires on graph load alongside the trace and
    // liveness-gate loaders. GET /api/v1/merge-requests only understands
    // repository_id / workspace_id / status / per_page (ListMrsQuery);
    // a repo_id param is silently ignored, and with no recognized scope
    // the handler returns ALL tenant MRs — attributing other repos' gate
    // outcomes to this repo's evaluative overlay. Every MR list call —
    // the evaluative gate-data join included — must therefore be
    // repository_id-scoped, never repo_id.
    await waitFor(() => {
      // trace + evaluative + liveness-gate loaders all fire on graph load
      expect(api.mergeRequests.mock.calls.length).toBeGreaterThanOrEqual(3);
    });
    for (const call of api.mergeRequests.mock.calls) {
      const params = call[0] ?? {};
      expect(params.repository_id).toBe('repo-1');
      expect(params).not.toHaveProperty('repo_id');
    }
  });

  it('the observable lens is disabled with a coming-soon tooltip', async () => {
    window.history.pushState({}, '', '/workspaces/ws-1/explorer');
    primeApiMocks();
    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await drillIntoRepo(container);

    const observable = container.querySelector('[data-testid="lens-checkbox-observable"]');
    expect(observable).toBeTruthy();
    const observableInput = observable.querySelector('input');
    expect(observableInput?.checked).toBe(false);
    expect(observableInput?.disabled).toBe(true);
    expect(observable.getAttribute('title')).toContain('requires production telemetry integration');
    expect(observable.textContent).toContain('requires production telemetry integration');
  });
});
