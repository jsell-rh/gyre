/**
 * ExplorerDefaultViews.test.js — HSI §3 Default Views view selector (task-088).
 *
 * The Explorer toolbar exposes Boundary | Spec Realization | Change |
 * System Trace. Boundary is the default (the ExplorerCanvas C4 drill-down);
 * the other three swap the canvas area for their dedicated panels.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';

// DependencyGraph lays out via ELK (WASM, unavailable in jsdom) — mock the
// layout engine with deterministic positions (same pattern as
// DependencyGraph.test.js). Keep the module's other exports intact for the
// repo-scope ExplorerCanvas tests in this file.
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

beforeEach(() => {
  fns.clear();
  fn('repoGraph').mockResolvedValue(GRAPH);
  fn('allRepos').mockResolvedValue([{ id: 'repo-1', name: 'payment-api' }]);
  fn('repos').mockResolvedValue([{ id: 'repo-1', name: 'payment-api' }]);
  fn('mergeRequests').mockResolvedValue([]);
  fn('mrTrace').mockResolvedValue({ spans: [] });
  fn('getSpecs').mockResolvedValue([
    { path: 'specs/system/payment-retry.md', title: 'Payment Retry', approval_status: 'approved' },
  ]);
  fn('getSpecProgress').mockResolvedValue({ total_tasks: 2, completed_tasks: 1 });
  fn('repoGraphTimeline').mockResolvedValue([
    {
      id: 'd-1',
      repo_id: 'repo-1',
      commit_sha: 'abc',
      timestamp: 5000,
      spec_ref: 'specs/system/payment-retry.md',
      agent_id: 'worker-12',
      delta_json: JSON.stringify({
        nodes_added: [{ name: 'RetryPolicy', node_type: 'type', qualified_name: 'payment-domain::retry::RetryPolicy' }],
      }),
    },
  ]);
  fn('mrTimeline').mockResolvedValue({
    mr_id: 'mr-1',
    events: [{ timestamp: 1711324800, type: 'Merged', detail: {} }],
  });
  fn('mrAttestation').mockResolvedValue(null);
  fn('mrDiff').mockResolvedValue({ diff: '' });

  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  global.ResizeObserver = class ResizeObserver {
    observe() {} disconnect() {} unobserve() {}
  };
  global.requestAnimationFrame = vi.fn((cb) => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
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
    roundRect: vi.fn(),
  }));
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
    }
  };
});

function renderRepoScope() {
  return render(ExplorerView, {
    props: { scope: { type: 'repo', repoId: 'repo-1', workspaceId: 'ws-1' } },
  });
}

function renderWorkspaceScope() {
  return render(ExplorerView, {
    props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
  });
}

// The repo-scope header also carries the Graph|Briefing arch sub-tabs
// (ui-navigation.md §2, task-083) — a second tablist. The view selector is
// a dropdown (ui-navigation.md §3: built-in views → saved views →
// "Generate view..."), so tests open it first.
async function openViewMenu() {
  const selector = await screen.findByTestId('explorer-view-selector');
  await fireEvent.click(within(selector).getByRole('button', { name: /view selector/i }));
  const menu = selector.querySelector('.view-select-menu');
  expect(menu).toBeTruthy();
  return { selector, menu };
}

async function viewOptions() {
  const { menu } = await openViewMenu();
  return within(menu).getAllByRole('option');
}

describe('Explorer default views selector (HSI §3)', () => {
  it('shows the four built-in views in the dropdown with Boundary active by default', async () => {
    renderRepoScope();
    const options = await viewOptions();
    expect(options.map((t) => t.textContent.trim())).toEqual([
      'Boundary', 'Spec Realization', 'Change', 'System Trace', 'Generate view…',
    ]);
    const boundary = options.find((t) => t.textContent === 'Boundary');
    expect(boundary.getAttribute('aria-selected')).toBe('true');
    // Boundary = the canvas renders.
    await waitFor(() => {
      expect(document.querySelector('.explorer-canvas-area canvas')).toBeTruthy();
    });
    // The collapsed selector button shows the active view's label.
    const btn = document.querySelector('.view-select-btn');
    expect(btn.textContent).toContain('Boundary');
  });

  it('switches to the Change View and queries the timeline since last visit', async () => {
    localStorage.setItem('gyre_last_seen', JSON.stringify({ value: 1700000000, updated_at: 1 }));
    renderRepoScope();
    const { menu } = await openViewMenu();
    await fireEvent.click(within(menu).getByText('Change'));

    await waitFor(() => {
      expect(document.querySelector('[data-testid="change-view"]')).toBeTruthy();
    });
    // Canvas swapped out for the Change panel.
    expect(document.querySelector('.explorer-canvas-area canvas')).toBeNull();
    // The timeline query used the cached last-seen value.
    await waitFor(() => {
      expect(fn('repoGraphTimeline')).toHaveBeenCalledWith('repo-1', 1700000000);
    });
    // An added entry renders.
    expect(await screen.findByText('RetryPolicy')).toBeTruthy();
  });

  it('switches to the Spec Realization View with linked nodes', async () => {
    renderRepoScope();
    const { menu } = await openViewMenu();
    await fireEvent.click(within(menu).getByText('Spec Realization'));

    await waitFor(() => {
      expect(document.querySelector('[data-testid="spec-realization-view"]')).toBeTruthy();
    });
    expect(await screen.findByText('payment-retry.md')).toBeTruthy();
    // Linked node from the graph (spec_path match).
    await waitFor(() => {
      expect(screen.getByText('RetryPolicy')).toBeTruthy();
    });
  });

  it('switches to the System Trace View and renders the MR timeline', async () => {
    fn('mergeRequests').mockResolvedValue([
      { id: 'mr-1', title: 'Payment retry endpoint', status: 'merged' },
    ]);
    renderRepoScope();
    const { menu } = await openViewMenu();
    await fireEvent.click(within(menu).getByText('System Trace'));

    await waitFor(() => {
      expect(document.querySelector('[data-testid="system-trace-view"]')).toBeTruthy();
    });
    expect(await screen.findByText('Merged')).toBeTruthy();
    expect(fn('mrTimeline')).toHaveBeenCalledWith('mr-1');
  });

  it('returns to the Boundary canvas when Boundary is re-selected', async () => {
    renderRepoScope();
    let menu;
    ({ menu } = await openViewMenu());
    await fireEvent.click(within(menu).getByText('Change'));
    await waitFor(() => {
      expect(document.querySelector('[data-testid="change-view"]')).toBeTruthy();
    });
    ({ menu } = await openViewMenu());
    await fireEvent.click(within(menu).getByText('Boundary'));
    await waitFor(() => {
      expect(document.querySelector('.explorer-canvas-area canvas')).toBeTruthy();
    });
    expect(document.querySelector('[data-testid="change-view"]')).toBeNull();
  });
});

describe('Boundary View Level 1 — workspace graph nodes (HSI §3, ui-layout.md §5)', () => {
  it('renders repos as graph nodes with type and endpoint counts from the workspace graph', async () => {
    fn('repos').mockResolvedValue([
      { id: 'repo-1', name: 'payment-api', description: 'Payments' },
      { id: 'repo-2', name: 'ledger-svc' },
    ]);
    fn('workspaceGraph').mockResolvedValue({
      nodes: [
        // repo-1: 2 types + 1 endpoint (interface/trait count as types).
        { id: 'a', repo_id: 'repo-1', node_type: 'type' },
        { id: 'b', repo_id: 'repo-1', node_type: 'interface' },
        { id: 'c', repo_id: 'repo-1', node_type: 'endpoint' },
        // repo-2: 1 type, 2 endpoints.
        { id: 'd', repo_id: 'repo-2', node_type: 'type' },
        { id: 'e', repo_id: 'repo-2', node_type: 'endpoint' },
        { id: 'f', repo_id: 'repo-2', node_type: 'endpoint' },
        // Nodes without repo attribution are ignored.
        { id: 'g', node_type: 'type' },
      ],
      edges: [],
    });
    // Cross-repo dependency renders as a canvas edge (ui-layout.md §5:
    // repos as nodes, cross-repo dependencies as edges).
    fn('workspaceDependencyGraph').mockResolvedValue({
      nodes: [],
      edges: [
        { id: 'dep-1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
      ],
    });

    renderWorkspaceScope();

    const node1 = await waitFor(() => screen.getByTestId('dep-node-repo-1'));
    expect(node1.textContent).toContain('payment-api');
    expect(node1.textContent).toContain('2 types');
    expect(node1.textContent).toContain('1 endpoints');

    const node2 = screen.getByTestId('dep-node-repo-2');
    expect(node2.textContent).toContain('ledger-svc');
    expect(node2.textContent).toContain('1 types');
    expect(node2.textContent).toContain('2 endpoints');

    // The cross-repo dependency is an edge on the canvas, not a chip.
    expect(document.querySelectorAll('[data-testid^="dep-edge-"]').length).toBe(1);
  });
});
