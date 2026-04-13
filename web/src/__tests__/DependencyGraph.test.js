import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import DependencyGraph from '../components/DependencyGraph.svelte';

// Mock elkLayout to return predictable positions (ELK uses WASM, unavailable in jsdom)
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 200, y: 80 + i * 100 };
    });
    return positions;
  }),
}));

const MOCK_NODES = [
  { repo_id: 'repo-1', name: 'frontend' },
  { repo_id: 'repo-2', name: 'backend-api' },
  { repo_id: 'repo-3', name: 'shared-lib' },
];

const MOCK_EDGES = [
  { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
  { id: 'e2', source: 'repo-2', target: 'repo-3', type: 'spec', status: 'stale' },
  { id: 'e3', source: 'repo-1', target: 'repo-3', type: 'api', status: 'breaking' },
];

describe('DependencyGraph', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Render ────────────────────────────────────────────────────────────────

  it('renders SVG with nodes after layout', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="dep-svg"]');
    expect(svg).toBeTruthy();
    expect(svg.tagName).toBe('svg');
  });

  it('renders one g.dep-node per node', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const nodeGroups = container.querySelectorAll('.dep-node');
    expect(nodeGroups.length).toBe(MOCK_NODES.length);
  });

  it('renders edges between nodes', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const edgeGroups = container.querySelectorAll('.dep-edge');
    expect(edgeGroups.length).toBe(MOCK_EDGES.length);
  });

  // ── Edge styling by dependency type ───────────────────────────────────────

  it('styles code edges with solid blue', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const codeEdge = container.querySelector('[data-type="code"]');
    expect(codeEdge).toBeTruthy();
    const path = codeEdge.querySelectorAll('path')[1]; // [0] is hit area
    expect(path.getAttribute('stroke')).toBe('#60a5fa');
    expect(path.getAttribute('stroke-dasharray')).toBe('');
  });

  it('styles spec edges with dashed purple', async () => {
    const specEdges = [
      { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'spec', status: 'active' },
    ];
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: specEdges },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const specEdge = container.querySelector('[data-type="spec"]');
    expect(specEdge).toBeTruthy();
    const path = specEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#a78bfa');
    expect(path.getAttribute('stroke-dasharray')).toBe('6 3');
  });

  it('styles api edges with dotted green', async () => {
    const apiEdges = [
      { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'api', status: 'active' },
    ];
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: apiEdges },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const apiEdge = container.querySelector('[data-type="api"]');
    expect(apiEdge).toBeTruthy();
    const path = apiEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#34d399');
    expect(path.getAttribute('stroke-dasharray')).toBe('3 3');
  });

  // ── Status-based edge styling ─────────────────────────────────────────────

  it('styles stale edges yellow', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const staleEdge = container.querySelector('[data-status="stale"]');
    expect(staleEdge).toBeTruthy();
    const path = staleEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#eab308');
  });

  it('styles breaking edges red', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const breakingEdge = container.querySelector('[data-status="breaking"]');
    expect(breakingEdge).toBeTruthy();
    const path = breakingEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#ef4444');
  });

  // ── Node health indicators ────────────────────────────────────────────────

  it('marks nodes with breaking health when connected to breaking edges', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    // repo-1 has a breaking edge (e3: repo-1 → repo-3, status=breaking)
    const repo1Node = container.querySelector('[data-testid="dep-node-repo-1"]');
    expect(repo1Node).toBeTruthy();
    expect(repo1Node.getAttribute('data-health')).toBe('breaking');
    const rect = repo1Node.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#ef4444');
  });

  it('marks nodes as healthy when all edges are active', async () => {
    const healthyEdges = [
      { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
    ];
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES.slice(0, 2), edges: healthyEdges },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const repo1Node = container.querySelector('[data-testid="dep-node-repo-1"]');
    expect(repo1Node.getAttribute('data-health')).toBe('healthy');
  });

  // ── Click to navigate ─────────────────────────────────────────────────────

  it('calls onNodeClick when a node is clicked', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [], onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dep-node-repo-1"]');
    expect(nodeGroup).toBeTruthy();
    await fireEvent.click(nodeGroup);
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[0]);
  });

  it('calls onNodeClick on Enter key', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [], onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dep-node-repo-2"]');
    await fireEvent.keyDown(nodeGroup, { key: 'Enter' });
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[1]);
  });

  // ── Hover highlights ──────────────────────────────────────────────────────

  it('dims non-adjacent nodes on hover', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [
        { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
      ]},
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const repo1 = container.querySelector('[data-testid="dep-node-repo-1"]');
    await fireEvent.pointerEnter(repo1);

    // repo-3 is not connected to repo-1 via any edge, so it should be dimmed
    const repo3 = container.querySelector('[data-testid="dep-node-repo-3"]');
    expect(repo3.getAttribute('opacity')).toBe('0.25');

    // repo-2 is a direct dependency, should not be dimmed
    const repo2 = container.querySelector('[data-testid="dep-node-repo-2"]');
    expect(repo2.getAttribute('opacity')).toBe('1');
  });

  it('restores opacity on pointer leave', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [
        { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
      ]},
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const repo1 = container.querySelector('[data-testid="dep-node-repo-1"]');
    await fireEvent.pointerEnter(repo1);
    await fireEvent.pointerLeave(repo1);

    const repo3 = container.querySelector('[data-testid="dep-node-repo-3"]');
    expect(repo3.getAttribute('opacity')).toBe('1');
  });

  // ── Scope toggle ──────────────────────────────────────────────────────────

  it('renders scope toggle with workspace active by default', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: [], edges: [] },
    });

    const wsBtn = container.querySelector('[data-testid="dep-scope-workspace"]');
    const tenBtn = container.querySelector('[data-testid="dep-scope-tenant"]');
    expect(wsBtn).toBeTruthy();
    expect(tenBtn).toBeTruthy();
    expect(wsBtn.classList.contains('active')).toBe(true);
    expect(tenBtn.classList.contains('active')).toBe(false);
  });

  it('calls onScopeChange when tenant button is clicked', async () => {
    const onScopeChange = vi.fn();
    const { container } = render(DependencyGraph, {
      props: { nodes: [], edges: [], onScopeChange },
    });

    const tenBtn = container.querySelector('[data-testid="dep-scope-tenant"]');
    await fireEvent.click(tenBtn);
    expect(onScopeChange).toHaveBeenCalledWith('tenant');
  });

  // ── Empty state ───────────────────────────────────────────────────────────

  it('shows empty state when no nodes', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: [], edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-empty"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="dep-svg"]')).toBeNull();
  });

  // ── Accessibility ─────────────────────────────────────────────────────────

  it('nodes have accessible role and aria-label', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dep-node-repo-1"]');
    expect(nodeGroup.getAttribute('role')).toBe('button');
    expect(nodeGroup.getAttribute('aria-label')).toContain('frontend');
  });

  it('SVG has role=img and aria-label', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="dep-svg"]');
    expect(svg.getAttribute('role')).toBe('img');
    expect(svg.getAttribute('aria-label')).toBe('Cross-repo dependency graph');
  });

  // ── Legend ────────────────────────────────────────────────────────────────

  it('renders legend with edge type labels', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: [], edges: [] },
    });

    const legend = container.querySelector('[data-testid="dep-legend"]');
    expect(legend).toBeTruthy();
    expect(legend.textContent).toContain('Code');
    expect(legend.textContent).toContain('Spec');
    expect(legend.textContent).toContain('API');
    expect(legend.textContent).toContain('Stale');
    expect(legend.textContent).toContain('Breaking');
  });

  // ── Node labels ───────────────────────────────────────────────────────────

  it('displays repo names as node labels', async () => {
    const { container } = render(DependencyGraph, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dep-node-repo-1"]');
    const label = nodeGroup.querySelector('.dep-node-label');
    expect(label.textContent).toBe('frontend');
  });
});
