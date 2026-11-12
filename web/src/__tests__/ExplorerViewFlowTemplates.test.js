/**
 * task-184 Predefined flow templates in the Explorer shell
 * (system-explorer.md §5 Flow Traces): the View selector offers a
 * "Trace flows" section with the agent spawn / MR merge / spec approval
 * flows; selecting one applies the flow layout scoped to the template's
 * expected call path and animates the captured gate trace of a matching
 * MR. With no trace data, the path renders structurally (no fabricated
 * particles).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const { apiMocks, graph } = vi.hoisted(() => {
  // A graph whose node names cover the agent-spawn template's expected
  // path, plus an unrelated node that must be scoped OUT of the flow view.
  const N = (id, name) => ({
    id, name, node_type: 'function', visibility: 'public',
    qualified_name: `api::${name}`, file_path: 'src/x.rs', repo_id: 'r1',
  });
  const graph = {
    nodes: [
      N('spawn', 'spawn_agent'),
      N('assign', 'assign_task'),
      N('worktree', 'create_worktree'),
      N('container', 'create_container'),
      N('budget', 'allocate_budget'),
      N('runtime', 'start_runtime'),
      N('unrelated', 'format_string'),
    ],
    edges: [
      { id: 'e1', source_id: 'spawn', target_id: 'assign', edge_type: 'calls' },
      { id: 'e2', source_id: 'assign', target_id: 'worktree', edge_type: 'calls' },
      { id: 'e3', source_id: 'worktree', target_id: 'container', edge_type: 'calls' },
      { id: 'e4', source_id: 'container', target_id: 'runtime', edge_type: 'calls' },
      { id: 'e5', source_id: 'runtime', target_id: 'budget', edge_type: 'calls' },
      { id: 'e6', source_id: 'spawn', target_id: 'unrelated', edge_type: 'calls' },
    ],
  };
  const apiMocks = {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue(graph),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    repoDependencies: vi.fn().mockResolvedValue([]),
    repoDependents: vi.fn().mockResolvedValue([]),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphTypes: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphModules: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    workspaceMergeRequests: vi.fn().mockResolvedValue([]),
    mrTrace: vi.fn().mockResolvedValue(null),
    explorerViews: vi.fn().mockResolvedValue([]),
    savedViews: vi.fn().mockResolvedValue([]),
  };
  return { apiMocks, graph };
});

vi.mock('../lib/api.js', () => ({ api: apiMocks }));
vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));
vi.mock('../lib/ExplorerChat.svelte', () => ({ default: function ExplorerChatStub() {} }));
vi.mock('../lib/GeneratedViewChat.svelte', () => ({ default: function Stub() {} }));

import ExplorerView from '../components/ExplorerView.svelte';

const REPO_SCOPE = { type: 'repo', workspaceId: 'ws-1', repoId: 'r1' };

beforeEach(() => {
  vi.clearAllMocks();
  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  global.requestAnimationFrame = vi.fn((cb) => { cb(16); return 1; });
  global.cancelAnimationFrame = vi.fn();
  // Canvas contexts for the particle overlay (2D) and the WebGL2 probe.
  const ctx2d = {
    clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
    beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
    stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
    fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    translate: vi.fn(), rotate: vi.fn(),
    fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
    font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []), lineCap: '',
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ctx2d);
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
    }
  };
});

afterEach(() => {
  vi.clearAllMocks();
});

/** Open the View selector dropdown and click a Trace flows template. */
async function applyTemplate(rendered, testId) {
  const selectorBtn = rendered.container.querySelector(
    '[data-testid="explorer-view-selector"] button.view-select-btn');
  expect(selectorBtn).toBeTruthy();
  await fireEvent.click(selectorBtn);
  const option = await waitFor(() => {
    const el = rendered.container.querySelector(`[data-testid="${testId}"]`);
    expect(el).toBeTruthy();
    return el;
  });
  await fireEvent.click(option);
}

describe('ExplorerView — predefined flow templates (system-explorer.md §5)', () => {
  it('lists the three spec-mandated flows in a "Trace flows" section of the View selector', async () => {
    const rendered = render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
      expect(document.querySelector('canvas.treemap-canvas, svg.flow-svg')).toBeTruthy();
    });
    const selectorBtn = rendered.container.querySelector(
      '[data-testid="explorer-view-selector"] button.view-select-btn');
    await fireEvent.click(selectorBtn);
    const menu = rendered.container.querySelector('.view-select-menu');
    expect(menu).toBeTruthy();
    expect(menu.textContent).toContain('Trace flows');
    for (const id of ['agent-spawn', 'mr-merge', 'spec-approval']) {
      expect(rendered.container.querySelector(`[data-testid="view-option-flow-${id}"]`)).toBeTruthy();
    }
  }, 20000);

  it('applies the flow layout scoped to the template path, animated by a matching MR trace', async () => {
    // The workspace's only traced MR matches the agent-spawn template.
    apiMocks.workspaceMergeRequests.mockResolvedValue([
      { id: 'mr-9', title: 'Update docs', source_branch: 'docs', spec_ref: null },
      { id: 'mr-1', title: 'Spawn agent for task 1', source_branch: 'task-1', spec_ref: null },
    ]);
    apiMocks.mrTrace.mockResolvedValue({
      spans: [
        { span_id: 's1', parent_span_id: null, operation_name: 'test_spawn', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'spawn' },
        { span_id: 's2', parent_span_id: 's1', operation_name: 'worktree', start_time: 1000, duration_us: 4000, status: 'ok', graph_node_id: 'worktree' },
      ],
      root_spans: ['s1'],
    });
    const rendered = render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
      expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    });

    await applyTemplate(rendered, 'view-option-flow-agent-spawn');

    // Flow layout: FlowCanvas's SVG layer replaces the 2D canvas.
    await waitFor(() => {
      expect(document.querySelector('svg.flow-svg')).toBeTruthy();
    });
    expect(document.querySelector('canvas.treemap-canvas')).toBeNull();

    // The active view query scoped the flow to the template's matched
    // path — the unrelated node is filtered out, path nodes render.
    await waitFor(() => {
      const nodeIds = [...document.querySelectorAll('g[data-flow-node]')].map(
        (g) => g.getAttribute('data-flow-node'));
      expect(nodeIds).not.toContain('unrelated');
      expect(nodeIds).toEqual(expect.arrayContaining(['spawn', 'worktree', 'container']));
    });

    // Trace resolution went through the workspace MR discovery (has_trace)
    // and picked the template-matching MR, not the newest one.
    expect(apiMocks.workspaceMergeRequests).toHaveBeenCalledWith('ws-1', { has_trace: true, limit: 10 });
    expect(apiMocks.mrTrace).toHaveBeenCalledWith('mr-1');

    // Evaluative lens + trace → the flow playback control bar renders.
    await waitFor(() => {
      expect(document.querySelector('.trace-playback-bar')).toBeTruthy();
    });
    // The query annotation names the flow and its governing spec.
    const annotation = document.querySelector('.query-annotation');
    expect(annotation).toBeTruthy();
    expect(annotation.textContent).toContain('Agent spawn flow trace');
    expect(annotation.textContent).toContain('specs/system/agent-runtime.md');
  }, 20000);

  it('renders the expected path structurally when no trace data exists — no fabricated particles', async () => {
    apiMocks.workspaceMergeRequests.mockResolvedValue([]);
    const rendered = render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
      expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    });

    await applyTemplate(rendered, 'view-option-flow-agent-spawn');

    // Flow layout renders the matched path…
    await waitFor(() => {
      expect(document.querySelector('svg.flow-svg')).toBeTruthy();
    });
    const nodeIds = [...document.querySelectorAll('g[data-flow-node]')].map(
      (g) => g.getAttribute('data-flow-node'));
    expect(nodeIds).not.toContain('unrelated');
    expect(nodeIds).toEqual(expect.arrayContaining(['spawn', 'worktree']));
    // …but with no captured trace there is no animation: no playback bar
    // and no particle canvas content (structural lens, honest empty state).
    expect(apiMocks.workspaceMergeRequests).toHaveBeenCalledWith('ws-1', { has_trace: true, limit: 10 });
    expect(document.querySelector('.trace-playback-bar')).toBeNull();
    // The annotation still names the expected path.
    const annotation = document.querySelector('.query-annotation');
    expect(annotation).toBeTruthy();
    expect(annotation.textContent).toContain('Agent spawn flow trace');
  }, 20000);

  it('warns and keeps the current view when the graph matches no path step', async () => {
    const rendered = render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
      expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    });
    await applyTemplate(rendered, 'view-option-flow-mr-merge');
    await new Promise((r) => setTimeout(r, 0));
    // No flow layout applied, no trace lookups — nothing matched.
    expect(document.querySelector('svg.flow-svg')).toBeNull();
    expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    expect(apiMocks.workspaceMergeRequests).not.toHaveBeenCalled();
    expect(apiMocks.mrTrace).not.toHaveBeenCalled();
  }, 20000);
});
