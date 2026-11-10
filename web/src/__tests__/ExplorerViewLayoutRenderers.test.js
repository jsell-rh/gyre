/**
 * Explorer shell layout-dispatch tests (ui-layout.md §10 Layout Engines):
 * the generated ViewSpec's `layout` field selects the renderer through the
 * layout registry — hierarchical/layered render on the SVG canvas
 * (positioned by ELK), side-by-side on the CSS-Grid composite, diff on the
 * structural-diff component, list on the shell's virtualized node list —
 * and the spec's ViewQuery scope filters the node set the renderer
 * receives. The default (graph/flow/timeline) stays on the unified
 * ExplorerCanvas (Canvas 2D).
 *
 * GeneratedViewChat is stubbed with a trigger button that applies a fixed
 * spec through the production onApplyView → applyGeneratedViewSpec path.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const { apiMocks, smallGraph } = vi.hoisted(() => {
  const smallGraph = {
    nodes: [
      { id: 'n1', name: 'AuthService', node_type: 'function', visibility: 'public', file_path: 'src/a.rs', repo_id: 'r1' },
      { id: 'n2', name: 'UserModel', node_type: 'type', visibility: 'public', file_path: 'src/b.rs', repo_id: 'r1' },
      { id: 'n3', name: 'RouteHandler', node_type: 'endpoint', visibility: 'public', file_path: 'src/c.rs', repo_id: 'r1' },
    ],
    edges: [
      { id: 'e1', source_id: 'n3', target_id: 'n1', edge_type: 'calls' },
    ],
  };
  const apiMocks = {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue(smallGraph),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    repoDependencies: vi.fn().mockResolvedValue([]),
    repoDependents: vi.fn().mockResolvedValue([]),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphTypes: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphModules: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
  };
  return { apiMocks, smallGraph };
});

vi.mock('../lib/api.js', () => ({ api: apiMocks }));
vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));
vi.mock('../lib/ExplorerChat.svelte', () => ({ default: function ExplorerChatStub() {} }));
// Stub the generated-view chat: a button applies the spec under test via
// the production onApplyView prop (the same callback the real component's
// SSE complete handler invokes). Svelte 5 instantiates child components as
// Component(anchor, props), so the stub receives the anchor placeholder
// node and the props object; the spec under test is read from a global at
// click time (mock factories cannot close over test locals).
vi.mock('../lib/GeneratedViewChat.svelte', () => ({
  default: function GeneratedViewChatStub(anchor, props) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'gen-chat-stub');
    const btn = document.createElement('button');
    btn.setAttribute('data-testid', 'apply-generated-view');
    btn.type = 'button';
    btn.textContent = 'Apply generated view';
    btn.onclick = () => props?.onApplyView?.(globalThis.__genViewSpecFixture);
    el.appendChild(btn);
    (anchor?.parentElement ?? anchor)?.appendChild?.(el);
  },
}));

import ExplorerView from '../components/ExplorerView.svelte';

const REPO_SCOPE = { type: 'repo', workspaceId: 'ws-1', repoId: 'r1' };

beforeEach(() => {
  vi.clearAllMocks();
  // @ts-expect-error test plumbing
  globalThis.__genViewSpecFixture = null;
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
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
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
  }));
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

/** Render the Explorer at repo scope with the generated-view chat open and
 *  its trigger button wired to `spec`. */
async function renderExplorerWithSpec(spec) {
  // @ts-expect-error test plumbing
  globalThis.__genViewSpecFixture = spec;
  const rendered = render(ExplorerView, { props: { scope: REPO_SCOPE } });
  await waitFor(() => {
    expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
  });
  // Switch the chat column to 'generate' (GeneratedViewChat renders there)
  const generateTab = rendered.getAllByRole('tab').find(
    (t) => t.textContent === 'Generated Views'
  );
  await fireEvent.click(generateTab);
  const trigger = await waitFor(() => {
    const el = document.querySelector('[data-testid="apply-generated-view"]');
    expect(el).toBeTruthy();
    return el;
  });
  return { ...rendered, trigger };
}

describe('ExplorerView generated-view layout dispatch (ui-layout.md §10)', () => {
  it('renders a hierarchical generated view on the SVG canvas, not the 2D canvas', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Service structure',
      layout: 'hierarchical',
      data: {},
      explanation: 'layered top-down',
    });
    await fireEvent.click(trigger);
    // The registry dispatch swaps the 2D canvas for the SVG renderer
    await waitFor(() => {
      expect(document.querySelector('[data-testid="generated-layout"][data-layout="hierarchical"]')).toBeTruthy();
    });
    expect(document.querySelector('svg.graph-svg')).toBeTruthy();
    expect(document.querySelector('canvas.treemap-canvas')).toBeNull();
    // All three nodes render as addressable SVG groups
    expect(document.querySelectorAll('g.svg-node')).toHaveLength(3);
    // The chip names the active layout via the registry labelKey
    expect(document.querySelector('.gen-layout-chip').textContent).toContain('Hierarchical');
  }, 20000);

  it('scopes the rendered node set by the spec ViewQuery (node_types filter)', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Types only',
      layout: 'hierarchical',
      data: { node_types: ['Type'] }, // viewSpecToViewQuery normalizes to lowercase
      explanation: 'just the types',
    });
    await fireEvent.click(trigger);
    await waitFor(() => {
      expect(document.querySelector('[data-testid="generated-layout"]')).toBeTruthy();
    });
    // Only the 'type' node renders — the filter scoped the SVG renderer's
    // node set, not just the canvas's.
    await waitFor(() => {
      expect(document.querySelectorAll('g.svg-node')).toHaveLength(1);
    });
    expect(document.querySelector('g.svg-node').getAttribute('aria-label')).toContain('UserModel');
    // Edges into filtered-out nodes are dropped too
    expect(document.querySelectorAll('line.svg-edge')).toHaveLength(0);
  }, 20000);

  it('renders a list generated view on the shell virtualized node list', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Everything as a list',
      layout: 'list',
      data: {},
      explanation: 'list layout',
    });
    await fireEvent.click(trigger);
    await waitFor(() => {
      expect(document.querySelector('[data-testid="generated-layout"][data-layout="list"]')).toBeTruthy();
    });
    expect(document.querySelector('.vlist')).toBeTruthy();
    expect(document.querySelector('canvas.treemap-canvas')).toBeNull();
    expect(document.querySelector('.gen-layout-chip').textContent).toContain('List');
  }, 20000);

  it('keeps the unified canvas for graph/flow/timeline layouts', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Regular graph',
      layout: 'graph',
      data: {},
      explanation: 'default canvas',
    });
    await fireEvent.click(trigger);
    await new Promise(r => setTimeout(r, 50));
    expect(document.querySelector('[data-testid="generated-layout"]')).toBeNull();
    expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
  }, 20000);

  it('falls back to the default canvas when the chip is dismissed', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Service structure',
      layout: 'hierarchical',
      data: {},
      explanation: 'layered top-down',
    });
    await fireEvent.click(trigger);
    await waitFor(() => {
      expect(document.querySelector('svg.graph-svg')).toBeTruthy();
    });
    const clear = document.querySelector('.gen-layout-chip-clear');
    expect(clear).toBeTruthy();
    await fireEvent.click(clear);
    await waitFor(() => {
      expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    });
    expect(document.querySelector('[data-testid="generated-layout"]')).toBeNull();
  }, 20000);

  it('treats an unregistered layout name as the default graph layout', async () => {
    const { trigger } = await renderExplorerWithSpec({
      name: 'Unknown layout',
      layout: 'sankey-not-registered',
      data: {},
      explanation: 'should fall back',
    });
    await fireEvent.click(trigger);
    await new Promise(r => setTimeout(r, 50));
    expect(document.querySelector('[data-testid="generated-layout"]')).toBeNull();
    expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
  }, 20000);
});
