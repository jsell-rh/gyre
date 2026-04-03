// FlowCanvas and FlowRenderer are superseded by the unified ExplorerTreemap
// canvas per explorer-canvas.md. The evaluative lens (OTLP particle overlay)
// will be integrated directly into ExplorerTreemap in a future phase.
// These tests are skipped until the evaluative overlay is built.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import FlowCanvas from '../lib/FlowCanvas.svelte';
import FlowRenderer from '../lib/FlowRenderer.svelte';
import NodeBadge from '../lib/NodeBadge.svelte';

// Mock api.js used by ExplorerCanvas
vi.mock('../lib/api.js', () => ({
  api: {
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphNode: vi.fn().mockResolvedValue({ node: null, edges: [] }),
  },
}));

// ----- Test data -----

const NODES = [
  { id: 'n1', node_type: 'module', name: 'domain', x: 80, y: 60, width: 64, height: 28 },
  { id: 'n2', node_type: 'type',   name: 'Task',   x: 240, y: 60, width: 64, height: 28 },
  { id: 'n3', node_type: 'function', name: 'save', x: 400, y: 60, width: 64, height: 28 },
];

const EDGES = [
  { id: 'e1', source: 'n1', target: 'n2' },
  { id: 'e2', source: 'n2', target: 'n3' },
];

const SPANS = [
  { id: 'root-1', parent_id: null,     node_id: 'n1', start_time: 0,    duration_us: 10000, status: 'ok'    },
  { id: 'child-1', parent_id: 'root-1', node_id: 'n2', start_time: 2000, duration_us: 5000,  status: 'ok'    },
  { id: 'root-2', parent_id: null,     node_id: 'n1', start_time: 0,    duration_us: 8000,  status: 'error' },
];

// ----- Canvas mock -----

let mockGetContext;
let mockCtx;

beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
  };
  mockGetContext = vi.fn((type) => {
    if (type === '2d') return mockCtx;
    if (type === 'webgl2') return null; // no WebGL in jsdom
    return null;
  });
  // Patch HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = mockGetContext;
  // Patch ResizeObserver (not available in jsdom)
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ============================================================
// FlowCanvas
// ============================================================

describe.skip('FlowCanvas — superseded by ExplorerTreemap (explorer-canvas.md)', () => {
  it('renders a canvas element', () => {
    const { container } = render(FlowCanvas, { props: { nodes: NODES, edges: EDGES, spans: SPANS } });
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('canvas has data-testid=flow-canvas', () => {
    const { getByTestId } = render(FlowCanvas, { props: { nodes: NODES, edges: EDGES, spans: SPANS } });
    expect(getByTestId('flow-canvas')).toBeTruthy();
  });

  it('canvas has correct width and height attributes', () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, spans: SPANS, width: 1024, height: 768 },
    });
    const canvas = container.querySelector('canvas');
    expect(Number(canvas.getAttribute('width'))).toBe(1024);
    expect(Number(canvas.getAttribute('height'))).toBe(768);
  });

  it('has correct aria-label mentioning active traces', () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, spans: SPANS },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas.getAttribute('aria-label')).toContain('traces');
  });

  it('renders without throwing when spans is empty', () => {
    expect(() => render(FlowCanvas, { props: { nodes: NODES, edges: EDGES, spans: [] } })).not.toThrow();
  });

  it('renders without throwing when nodes is empty', () => {
    expect(() => render(FlowCanvas, { props: { nodes: [], edges: [], spans: SPANS } })).not.toThrow();
  });

  it('renders canvas with correct default dimensions', () => {
    const { container } = render(FlowCanvas, { props: {} });
    const canvas = container.querySelector('canvas');
    expect(Number(canvas.getAttribute('width'))).toBe(800);
    expect(Number(canvas.getAttribute('height'))).toBe(600);
  });
});

// ============================================================
// Particle system (via DOM + internal state verification)
// ============================================================

describe.skip('FlowCanvas — particle system — superseded by ExplorerTreemap', () => {
  it('creates particles from root spans', () => {
    // Root spans are those with parent_id = null
    // We can verify indirectly via aria-label which shows particle count
    const rootCount = SPANS.filter(s => !s.parent_id).length;
    expect(rootCount).toBe(2); // root-1 and root-2
  });

  it('does not create particles from child spans (no root)', () => {
    const childOnly = SPANS.filter(s => s.parent_id !== null);
    expect(childOnly.length).toBe(1); // child-1 only
    // Render with only child spans — no root spans → no particles
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, spans: childOnly, currentTime: 3000 },
    });
    const canvas = container.querySelector('canvas');
    // aria-label should show 0 active traces
    expect(canvas.getAttribute('aria-label')).toContain('0 active');
  });

  it('filters particles by selectedTests', () => {
    // Only include root-1 in selectedTests → only 1 particle should be created
    const { container } = render(FlowCanvas, {
      props: {
        nodes: NODES, edges: EDGES, spans: SPANS,
        currentTime: 5000,
        selectedTests: ['root-1'],
      },
    });
    const canvas = container.querySelector('canvas');
    // With selectedTests=['root-1'], only 1 active particle
    expect(canvas.getAttribute('aria-label')).toContain('1 active');
  });
});

// ============================================================
// WebGL mode (> 100 particles)
// ============================================================

describe.skip('FlowCanvas — WebGL fallback — superseded by ExplorerTreemap', () => {
  it('uses 2D context for small particle counts', () => {
    render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, spans: SPANS, currentTime: 5000 },
    });
    // With 2 root spans → 2 particles → should use 2D
    expect(mockGetContext).toHaveBeenCalledWith('2d');
  });

  it('attempts webgl2 when > 100 particles (mocked spans)', () => {
    // Create 101 root spans
    const manySpans = Array.from({ length: 101 }, (_, i) => ({
      id: `root-${i}`,
      parent_id: null,
      node_id: 'n1',
      start_time: 0,
      duration_us: 10000,
      status: 'ok',
    }));
    render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, spans: manySpans, currentTime: 5000 },
    });
    // Should try webgl2 context (even if it returns null and falls back to 2D)
    const webglCalls = mockGetContext.mock.calls.filter(c => c[0] === 'webgl2');
    expect(webglCalls.length).toBeGreaterThan(0);
  });
});

// ============================================================
// NodeBadge
// ============================================================

describe('NodeBadge', () => {
  it('renders nothing when metrics is null', () => {
    const { container } = render(NodeBadge, {
      props: { node: NODES[0], metrics: null },
    });
    const badge = container.querySelector('.node-badge');
    expect(badge).toBeNull();
  });

  it('renders nothing when span_count is 0', () => {
    const { container } = render(NodeBadge, {
      props: { node: NODES[0], metrics: { span_count: 0, error_rate: 0, mean_duration_us: 0 } },
    });
    const badge = container.querySelector('.node-badge');
    expect(badge).toBeNull();
  });

  it('renders badge when node and metrics with span_count > 0 provided', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 5, error_rate: 0.2, mean_duration_us: 3000 },
      },
    });
    const badge = container.querySelector('.node-badge');
    expect(badge).toBeTruthy();
  });

  it('renders error_rate ring circle', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 3, error_rate: 0.5, mean_duration_us: 1000 },
      },
    });
    const circles = container.querySelectorAll('circle');
    // Should have background circle + error ring
    expect(circles.length).toBeGreaterThanOrEqual(2);
  });

  it('uses danger color for error_rate > 0.1', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 5, error_rate: 0.5, mean_duration_us: 0 },
      },
    });
    // The error ring circle should have danger color
    const circles = container.querySelectorAll('circle');
    const ringCircle = circles[1];
    expect(ringCircle?.getAttribute('stroke')).toContain('danger');
  });

  it('uses success color for error_rate <= 0.1', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 5, error_rate: 0.05, mean_duration_us: 0 },
      },
    });
    const circles = container.querySelectorAll('circle');
    const ringCircle = circles[1];
    expect(ringCircle?.getAttribute('stroke')).toContain('success');
  });

  it('renders span count text', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 7, error_rate: 0, mean_duration_us: 500 },
      },
    });
    expect(container.innerHTML).toContain('7');
  });

  it('truncates span count to "99+" when > 99', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 150, error_rate: 0, mean_duration_us: 0 },
      },
    });
    expect(container.innerHTML).toContain('99+');
  });

  it('has correct aria-label with metrics', () => {
    const { container } = render(NodeBadge, {
      props: {
        node: NODES[0],
        metrics: { span_count: 4, error_rate: 0.25, mean_duration_us: 2000 },
      },
    });
    const badge = container.querySelector('.node-badge');
    expect(badge?.getAttribute('aria-label')).toContain('4 spans');
    expect(badge?.getAttribute('aria-label')).toContain('25.0% errors');
  });
});

// ============================================================
// FlowRenderer
// ============================================================

describe.skip('FlowRenderer — superseded by ExplorerTreemap', () => {
  const GRAPH_NODES = [
    { id: 'n1', node_type: 'module',   name: 'domain', qualified_name: 'domain' },
    { id: 'n2', node_type: 'function', name: 'save',   qualified_name: 'save'   },
  ];
  const GRAPH_EDGES = [
    { id: 'e1', source_id: 'n1', target_id: 'n2', edge_type: 'calls' },
  ];

  it('renders without throwing', () => {
    expect(() => render(FlowRenderer)).not.toThrow();
  });

  it('renders with data-testid=flow-renderer', () => {
    const { getByTestId } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    expect(getByTestId('flow-renderer')).toBeTruthy();
  });

  it('renders playback controls toolbar', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const toolbar = container.querySelector('[role="toolbar"]');
    expect(toolbar).toBeTruthy();
  });

  it('renders Play button by default', () => {
    const { getByText } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    expect(getByText('Play')).toBeTruthy();
  });

  it('renders scrubber input', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const scrubber = container.querySelector('.scrubber-input');
    expect(scrubber).toBeTruthy();
  });

  it('renders speed buttons', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const speedBtns = container.querySelectorAll('.speed-btn');
    expect(speedBtns.length).toBe(5); // 0.25×, 0.5×, 1×, 2×, 5×
  });

  it('renders FlowCanvas (canvas element)', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('renders badge SVG overlay', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const overlay = container.querySelector('.badge-overlay');
    expect(overlay).toBeTruthy();
  });

  it('1× speed button is active by default', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const activeSpeed = container.querySelector('.speed-btn.active');
    expect(activeSpeed?.textContent?.trim()).toBe('1×');
  });

  it('scrubber max equals max span time', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const scrubber = container.querySelector('.scrubber-input');
    const expectedMax = Math.max(...SPANS.map(s => s.start_time + (s.duration_us ?? 0)));
    expect(Number(scrubber?.getAttribute('max'))).toBe(expectedMax);
  });

  it('renders time label', () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const timeLabel = container.querySelector('.time-label');
    expect(timeLabel).toBeTruthy();
    expect(timeLabel.textContent).toContain('s');
  });

  it('play button toggles aria-pressed when clicked', async () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const playBtn = container.querySelector('.play-btn');
    expect(playBtn?.getAttribute('aria-pressed')).toBe('false');
    playBtn.click();
    await new Promise(r => setTimeout(r, 0));
    expect(playBtn?.getAttribute('aria-pressed')).toBe('true');
  });

  it('clicking play shows Pause text', async () => {
    const { container } = render(FlowRenderer, {
      props: { nodes: GRAPH_NODES, edges: GRAPH_EDGES, spans: SPANS },
    });
    const playBtn = container.querySelector('.play-btn');
    playBtn.click();
    await new Promise(r => setTimeout(r, 0));
    expect(container.innerHTML).toContain('Pause');
  });
});
