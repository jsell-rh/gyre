import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import MergeQueueGraph from '../components/MergeQueueGraph.svelte';

// Mock elkLayout to return predictable positions (ELK uses WASM, unavailable in jsdom)
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 220, y: 80 + i * 80 };
    });
    return positions;
  }),
}));

const MOCK_NODES = [
  {
    mr_id: 'mr-1',
    title: 'Add auth middleware',
    status: 'approved',
    priority: 10,
    depends_on: [],
    atomic_group: null,
  },
  {
    mr_id: 'mr-2',
    title: 'Add user endpoints',
    status: 'open',
    priority: 20,
    depends_on: [{ mr_id: 'mr-1', source: 'explicit' }],
    atomic_group: null,
  },
  {
    mr_id: 'mr-3',
    title: 'Fix payment bug',
    status: 'merged',
    priority: 5,
    depends_on: [],
    atomic_group: null,
  },
];

const MOCK_NODES_WITH_GROUPS = [
  {
    mr_id: 'mr-a',
    title: 'Schema migration',
    status: 'approved',
    priority: 10,
    depends_on: [],
    atomic_group: 'db-update',
  },
  {
    mr_id: 'mr-b',
    title: 'Update ORM models',
    status: 'approved',
    priority: 10,
    depends_on: [{ mr_id: 'mr-a', source: 'explicit' }],
    atomic_group: 'db-update',
  },
  {
    mr_id: 'mr-c',
    title: 'Unrelated fix',
    status: 'open',
    priority: 50,
    depends_on: [],
    atomic_group: null,
  },
];

describe('MergeQueueGraph', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Render DAG ──────────────────────────────────────────────────────────────

  it('renders SVG with nodes after layout', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="mq-svg"]');
    expect(svg).toBeTruthy();
    expect(svg.tagName).toBe('svg');
  });

  it('renders one g.mq-node per node', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroups = container.querySelectorAll('.mq-node');
    expect(nodeGroups.length).toBe(MOCK_NODES.length);
  });

  it('renders edges from depends_on relationships', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const edgeGroups = container.querySelectorAll('.mq-edge');
    // mr-2 depends on mr-1, so there should be 1 edge
    expect(edgeGroups.length).toBe(1);
  });

  // ── Empty state ───────────────────────────────────────────────────────────

  it('shows empty state when no nodes', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-empty"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="mq-svg"]')).toBeNull();
    expect(container.querySelector('[data-testid="mq-empty"]').textContent).toContain('No MRs in the merge queue');
  });

  // ── Node coloring by status ──────────────────────────────────────────────

  it('colors merged nodes green', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const mergedNode = container.querySelector('[data-testid="mq-node-mr-3"]');
    expect(mergedNode).toBeTruthy();
    const rect = mergedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#22c55e');
  });

  it('colors approved nodes blue', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const approvedNode = container.querySelector('[data-testid="mq-node-mr-1"]');
    expect(approvedNode).toBeTruthy();
    const rect = approvedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#60a5fa');
  });

  it('marks blocked nodes with blocked status and grayed style', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    // mr-2 depends on mr-1 which is approved (not merged), so mr-2 is blocked
    const blockedNode = container.querySelector('[data-testid="mq-node-mr-2"]');
    expect(blockedNode).toBeTruthy();
    expect(blockedNode.getAttribute('data-status')).toBe('blocked');
    const rect = blockedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#78716c');
  });

  // ── Dependency edge coloring ──────────────────────────────────────────────

  it('colors edges green when dependency is merged', async () => {
    const nodesWithMergedDep = [
      { mr_id: 'mr-x', title: 'Base', status: 'merged', priority: 10, depends_on: [], atomic_group: null },
      { mr_id: 'mr-y', title: 'Dependent', status: 'open', priority: 20, depends_on: [{ mr_id: 'mr-x', source: 'explicit' }], atomic_group: null },
    ];

    const { container } = render(MergeQueueGraph, {
      props: { nodes: nodesWithMergedDep },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const edgePath = container.querySelector('.mq-edge path:nth-child(2)');
    expect(edgePath.getAttribute('stroke')).toBe('#22c55e');
  });

  it('colors edges amber when dependency is pending', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    // mr-2 depends on mr-1 (approved, not merged) = amber
    const edgePath = container.querySelector('.mq-edge path:nth-child(2)');
    expect(edgePath.getAttribute('stroke')).toBe('#eab308');
  });

  it('colors edges red when dependency is closed', async () => {
    const nodesWithClosedDep = [
      { mr_id: 'mr-x', title: 'Closed one', status: 'closed', priority: 10, depends_on: [], atomic_group: null },
      { mr_id: 'mr-y', title: 'Dependent', status: 'open', priority: 20, depends_on: [{ mr_id: 'mr-x', source: 'explicit' }], atomic_group: null },
    ];

    const { container } = render(MergeQueueGraph, {
      props: { nodes: nodesWithClosedDep },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const edgePath = container.querySelector('.mq-edge path:nth-child(2)');
    expect(edgePath.getAttribute('stroke')).toBe('#ef4444');
  });

  // ── Atomic group boundary ─────────────────────────────────────────────────

  it('renders atomic group boundary for grouped MRs', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES_WITH_GROUPS },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const groupBoundary = container.querySelector('[data-testid="mq-group-db-update"]');
    expect(groupBoundary).toBeTruthy();
    const rect = groupBoundary.querySelector('rect');
    expect(rect).toBeTruthy();
    expect(rect.getAttribute('stroke')).toBe('#a78bfa');
    expect(rect.getAttribute('stroke-dasharray')).toBe('6 4');

    // Group label
    const label = groupBoundary.querySelector('text');
    expect(label.textContent).toBe('db-update');
  });

  it('does not render group boundary for ungrouped MRs', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const groups = container.querySelectorAll('.mq-group');
    expect(groups.length).toBe(0);
  });

  // ── Click to navigate ─────────────────────────────────────────────────────

  it('calls onNodeClick when a node is clicked', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES, onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="mq-node-mr-1"]');
    expect(nodeGroup).toBeTruthy();
    await fireEvent.click(nodeGroup);
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[0]);
  });

  it('calls onNodeClick on Enter key', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES, onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="mq-node-mr-2"]');
    await fireEvent.keyDown(nodeGroup, { key: 'Enter' });
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[1]);
  });

  // ── Hover tooltip ─────────────────────────────────────────────────────────

  it('shows tooltip on hover with MR details', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="mq-node-mr-2"]');
    await fireEvent.pointerEnter(nodeGroup, { clientX: 200, clientY: 100 });

    const tooltip = container.querySelector('[data-testid="mq-tooltip"]');
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain('Add user endpoints');
    expect(tooltip.textContent).toContain('blocked');
    expect(tooltip.textContent).toContain('Blocked by');
    expect(tooltip.textContent).toContain('Add auth middleware');
  });

  it('hides tooltip on pointer leave', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="mq-node-mr-2"]');
    await fireEvent.pointerEnter(nodeGroup, { clientX: 200, clientY: 100 });
    expect(container.querySelector('[data-testid="mq-tooltip"]')).toBeTruthy();

    await fireEvent.pointerLeave(nodeGroup);
    expect(container.querySelector('[data-testid="mq-tooltip"]')).toBeNull();
  });

  // ── Node labels ───────────────────────────────────────────────────────────

  it('displays truncated title and status badge on nodes', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const mergedNode = container.querySelector('[data-testid="mq-node-mr-3"]');
    const label = mergedNode.querySelector('.mq-node-label');
    expect(label.textContent).toContain('Fix payment bug');

    const status = mergedNode.querySelector('.mq-node-status');
    expect(status.textContent).toContain('merged');
  });

  it('shows priority on node status line', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const node = container.querySelector('[data-testid="mq-node-mr-1"]');
    const status = node.querySelector('.mq-node-status');
    expect(status.textContent).toContain('p10');
  });

  // ── Accessibility ─────────────────────────────────────────────────────────

  it('nodes have accessible role and aria-label', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="mq-node-mr-1"]');
    expect(nodeGroup.getAttribute('role')).toBe('button');
    expect(nodeGroup.getAttribute('aria-label')).toContain('Add auth middleware');
    expect(nodeGroup.getAttribute('aria-label')).toContain('ready');
  });

  it('SVG has role=img and aria-label', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="mq-svg"]');
    expect(svg.getAttribute('role')).toBe('img');
    expect(svg.getAttribute('aria-label')).toBe('Merge queue dependency graph');
  });

  // ── Legend ────────────────────────────────────────────────────────────────

  it('renders legend with edge status labels', async () => {
    const { container } = render(MergeQueueGraph, {
      props: { nodes: MOCK_NODES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-svg"]')).toBeTruthy();
    });

    const legend = container.querySelector('[data-testid="mq-legend"]');
    expect(legend).toBeTruthy();
    expect(legend.textContent).toContain('Satisfied');
    expect(legend.textContent).toContain('Pending');
    expect(legend.textContent).toContain('Failed');
    expect(legend.textContent).toContain('Atomic group');
  });
});
