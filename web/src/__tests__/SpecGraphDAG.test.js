import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import SpecGraphDAG from '../components/SpecGraphDAG.svelte';

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
  { path: 'system/vision.md', title: 'Vision', approval_status: 'approved' },
  { path: 'system/auth.md', title: 'Auth', approval_status: 'pending' },
  { path: 'system/deprecated.md', title: 'Deprecated', approval_status: 'deprecated' },
  { path: 'system/rejected.md', title: 'Rejected', approval_status: 'rejected' },
];

const MOCK_EDGES = [
  { source: 'system/auth.md', target: 'system/vision.md', link_type: 'implements', status: 'active' },
  { source: 'system/deprecated.md', target: 'system/vision.md', link_type: 'supersedes', status: 'stale' },
  { source: 'system/rejected.md', target: 'system/auth.md', link_type: 'conflicts_with', status: 'active' },
  { source: 'system/auth.md', target: 'system/deprecated.md', link_type: 'depends_on', status: 'active' },
  { source: 'system/rejected.md', target: 'system/vision.md', link_type: 'extends', status: 'active' },
];

describe('SpecGraphDAG', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Render ────────────────────────────────────────────────────────────────

  it('renders SVG with nodes after layout', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="dag-svg"]');
    expect(svg).toBeTruthy();
    expect(svg.tagName).toBe('svg');
  });

  it('renders one g.dag-node per node', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const nodeGroups = container.querySelectorAll('.dag-node');
    expect(nodeGroups.length).toBe(MOCK_NODES.length);
  });

  it('renders edges between nodes', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const edgeGroups = container.querySelectorAll('.dag-edge');
    expect(edgeGroups.length).toBe(MOCK_EDGES.length);
  });

  // ── Node coloring by approval status ──────────────────────────────────────

  it('colors approved nodes green', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const approvedNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    expect(approvedNode).toBeTruthy();
    const rect = approvedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#22c55e');
  });

  it('colors pending nodes amber', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const pendingNode = container.querySelector('[data-testid="dag-node-system/auth.md"]');
    expect(pendingNode).toBeTruthy();
    const rect = pendingNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#eab308');
  });

  it('colors rejected nodes red', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const rejectedNode = container.querySelector('[data-testid="dag-node-system/rejected.md"]');
    expect(rejectedNode).toBeTruthy();
    const rect = rejectedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#ef4444');
  });

  it('colors deprecated nodes gray', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const deprecatedNode = container.querySelector('[data-testid="dag-node-system/deprecated.md"]');
    expect(deprecatedNode).toBeTruthy();
    const rect = deprecatedNode.querySelector('rect');
    expect(rect.getAttribute('stroke')).toBe('#6b7280');
  });

  // ── Edge styling by link type ─────────────────────────────────────────────

  it('styles depends_on edges with solid blue', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const dependsOnEdge = container.querySelector('[data-link-type="depends_on"]');
    expect(dependsOnEdge).toBeTruthy();
    const path = dependsOnEdge.querySelectorAll('path')[1]; // [0] is hit area
    expect(path.getAttribute('stroke')).toBe('#60a5fa');
    expect(path.getAttribute('stroke-dasharray')).toBe('');
  });

  it('styles implements edges with dashed green', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const implementsEdge = container.querySelector('[data-link-type="implements"]');
    expect(implementsEdge).toBeTruthy();
    const path = implementsEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#34d399');
    expect(path.getAttribute('stroke-dasharray')).toBe('6 3');
  });

  it('styles conflicts_with edges in red', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const conflictEdge = container.querySelector('[data-link-type="conflicts_with"]');
    expect(conflictEdge).toBeTruthy();
    const path = conflictEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#ef4444');
  });

  it('styles extends edges with distinct dash pattern', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const extendsEdge = container.querySelector('[data-link-type="extends"]');
    expect(extendsEdge).toBeTruthy();
    const path = extendsEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#fb923c');
    expect(path.getAttribute('stroke-dasharray')).toBe('8 2 2 2');
  });

  // ── Staleness highlighting ────────────────────────────────────────────────

  it('highlights stale edges in yellow', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const staleEdge = container.querySelector('[data-status="stale"]');
    expect(staleEdge).toBeTruthy();
    const path = staleEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#eab308');
  });

  it('shows stale label on stale edges', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: MOCK_EDGES },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const staleEdge = container.querySelector('[data-status="stale"]');
    expect(staleEdge).toBeTruthy();
    const label = staleEdge.querySelector('.dag-edge-label');
    expect(label.textContent).toContain('(stale)');
  });

  // ── Click to navigate ─────────────────────────────────────────────────────

  it('calls onNodeClick when a node is clicked', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [], onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    expect(nodeGroup).toBeTruthy();
    await fireEvent.click(nodeGroup);
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[0]);
  });

  it('calls onNodeClick on Enter key', async () => {
    const onNodeClick = vi.fn();
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [], onNodeClick },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dag-node-system/auth.md"]');
    await fireEvent.keyDown(nodeGroup, { key: 'Enter' });
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(MOCK_NODES[1]);
  });

  // ── Empty state ───────────────────────────────────────────────────────────

  it('shows empty state when no nodes', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: [], edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-empty"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="dag-svg"]')).toBeNull();
  });

  // ── Node labels ───────────────────────────────────────────────────────────

  it('displays short filename labels on nodes (strips directory and .md)', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    const label = nodeGroup.querySelector('.dag-node-label');
    expect(label.textContent).toBe('vision');
  });

  // ── Accessibility ─────────────────────────────────────────────────────────

  it('nodes have accessible role and aria-label', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const nodeGroup = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    expect(nodeGroup.getAttribute('role')).toBe('button');
    expect(nodeGroup.getAttribute('aria-label')).toContain('vision');
    expect(nodeGroup.getAttribute('aria-label')).toContain('approved');
  });

  it('SVG has role=img and aria-label', async () => {
    const { container } = render(SpecGraphDAG, {
      props: { nodes: MOCK_NODES, edges: [] },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const svg = container.querySelector('[data-testid="dag-svg"]');
    expect(svg.getAttribute('role')).toBe('img');
    expect(svg.getAttribute('aria-label')).toBe('Spec relationship graph');
  });

  // ── Supersedes edge styling ───────────────────────────────────────────────

  it('styles supersedes edges with dotted pattern', async () => {
    const { container } = render(SpecGraphDAG, {
      props: {
        nodes: MOCK_NODES,
        edges: [{ source: 'system/deprecated.md', target: 'system/vision.md', link_type: 'supersedes', status: 'active' }],
      },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const supersedesEdge = container.querySelector('[data-link-type="supersedes"]');
    expect(supersedesEdge).toBeTruthy();
    const path = supersedesEdge.querySelectorAll('path')[1];
    expect(path.getAttribute('stroke')).toBe('#a78bfa');
    expect(path.getAttribute('stroke-dasharray')).toBe('3 3');
  });

  // ── Supersedes target strikethrough ──────────────────────────────────────

  it('applies strikethrough to supersedes target node', async () => {
    const { container } = render(SpecGraphDAG, {
      props: {
        nodes: MOCK_NODES,
        edges: [{ source: 'system/deprecated.md', target: 'system/vision.md', link_type: 'supersedes', status: 'active' }],
      },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    // Target node (vision.md) should have strikethrough decoration
    const targetNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    expect(targetNode).toBeTruthy();
    expect(targetNode.getAttribute('data-superseded')).toBe('true');
    const strikethrough = targetNode.querySelector('.dag-strikethrough');
    expect(strikethrough).toBeTruthy();
    expect(strikethrough.tagName.toLowerCase()).toBe('line');

    // Source node (deprecated.md) should NOT have strikethrough
    const sourceNode = container.querySelector('[data-testid="dag-node-system/deprecated.md"]');
    expect(sourceNode).toBeTruthy();
    expect(sourceNode.getAttribute('data-superseded')).toBeNull();
    expect(sourceNode.querySelector('.dag-strikethrough')).toBeNull();
  });

  it('does not apply strikethrough to nodes that are not supersedes targets', async () => {
    const { container } = render(SpecGraphDAG, {
      props: {
        nodes: MOCK_NODES,
        edges: [{ source: 'system/auth.md', target: 'system/deprecated.md', link_type: 'depends_on', status: 'active' }],
      },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    // No supersedes edges, so no node should have strikethrough
    const allNodes = container.querySelectorAll('.dag-node');
    allNodes.forEach(node => {
      expect(node.getAttribute('data-superseded')).toBeNull();
      expect(node.querySelector('.dag-strikethrough')).toBeNull();
    });
  });

  it('includes superseded status in aria-label for target node', async () => {
    const { container } = render(SpecGraphDAG, {
      props: {
        nodes: MOCK_NODES,
        edges: [{ source: 'system/deprecated.md', target: 'system/vision.md', link_type: 'supersedes', status: 'active' }],
      },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
    });

    const targetNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
    expect(targetNode.getAttribute('aria-label')).toContain('(superseded)');

    // Non-target node should not mention superseded
    const sourceNode = container.querySelector('[data-testid="dag-node-system/deprecated.md"]');
    expect(sourceNode.getAttribute('aria-label')).not.toContain('(superseded)');
  });

  // ── Impact analysis ───────────────────────────────────────────────────

  describe('impact analysis', () => {
    const IMPACT_NODES = [
      { path: 'system/core.md', approval_status: 'approved' },
      { path: 'system/auth.md', approval_status: 'pending' },
      { path: 'system/billing.md', approval_status: 'approved' },
      { path: 'system/unrelated.md', approval_status: 'approved' },
      { path: 'system/deep.md', approval_status: 'pending' },
    ];

    const IMPACT_EDGES = [
      // auth depends_on core, billing depends_on core, deep depends_on auth
      { source: 'system/auth.md', target: 'system/core.md', link_type: 'depends_on', status: 'active' },
      { source: 'system/billing.md', target: 'system/core.md', link_type: 'implements', status: 'active' },
      { source: 'system/deep.md', target: 'system/auth.md', link_type: 'extends', status: 'active' },
    ];

    it('dims non-dependent nodes when impactPath is set', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: 'system/core.md' },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // Unrelated node should be dimmed (opacity 0.2)
      const unrelatedNode = container.querySelector('[data-testid="dag-node-system/unrelated.md"]');
      expect(unrelatedNode).toBeTruthy();
      expect(unrelatedNode.getAttribute('data-impact')).toBe('dimmed');
      expect(unrelatedNode.getAttribute('opacity')).toBe('0.2');
    });

    it('highlights transitive dependent nodes', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: 'system/core.md' },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // auth, billing, deep are all transitive dependents of core
      const authNode = container.querySelector('[data-testid="dag-node-system/auth.md"]');
      expect(authNode.getAttribute('data-impact')).toBe('dependent');
      expect(authNode.getAttribute('opacity')).toBe('1');

      const billingNode = container.querySelector('[data-testid="dag-node-system/billing.md"]');
      expect(billingNode.getAttribute('data-impact')).toBe('dependent');

      // deep depends on auth which depends on core — transitive
      const deepNode = container.querySelector('[data-testid="dag-node-system/deep.md"]');
      expect(deepNode.getAttribute('data-impact')).toBe('dependent');
    });

    it('marks the impact root node', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: 'system/core.md' },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      const coreNode = container.querySelector('[data-testid="dag-node-system/core.md"]');
      expect(coreNode.getAttribute('data-impact')).toBe('root');
      expect(coreNode.getAttribute('opacity')).toBe('1');
      // Should have impact ring
      expect(coreNode.querySelector('[data-testid="impact-ring"]')).toBeTruthy();
    });

    it('includes impact status in aria-label', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: 'system/core.md' },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      const coreNode = container.querySelector('[data-testid="dag-node-system/core.md"]');
      expect(coreNode.getAttribute('aria-label')).toContain('(impact analysis root)');

      const authNode = container.querySelector('[data-testid="dag-node-system/auth.md"]');
      expect(authNode.getAttribute('aria-label')).toContain('(dependent)');
    });

    it('dims non-highlighted edges', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: 'system/core.md' },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // All edges in IMPACT_EDGES connect to dependents of core, so all should be highlighted
      const edgeGroups = container.querySelectorAll('.dag-edge');
      edgeGroups.forEach(eg => {
        expect(eg.getAttribute('opacity')).toBe('1');
      });
    });

    it('calls onImpactSelect in impact mode', async () => {
      const onImpactSelect = vi.fn();
      const onNodeClick = vi.fn();
      const { container } = render(SpecGraphDAG, {
        props: {
          nodes: IMPACT_NODES,
          edges: [],
          impactMode: true,
          onImpactSelect,
          onNodeClick,
        },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      const nodeGroup = container.querySelector('[data-testid="dag-node-system/core.md"]');
      await fireEvent.click(nodeGroup);
      expect(onImpactSelect).toHaveBeenCalledTimes(1);
      expect(onImpactSelect).toHaveBeenCalledWith(IMPACT_NODES[0]);
      // Should NOT call onNodeClick in impact mode
      expect(onNodeClick).not.toHaveBeenCalled();
    });

    it('shows no dimming when impactPath is null', async () => {
      const { container } = render(SpecGraphDAG, {
        props: { nodes: IMPACT_NODES, edges: IMPACT_EDGES, impactPath: null },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // No nodes should have data-impact attribute
      const allNodes = container.querySelectorAll('.dag-node');
      allNodes.forEach(node => {
        expect(node.getAttribute('data-impact')).toBeNull();
        expect(node.getAttribute('opacity')).toBe('1');
      });
    });

    it('renders empty state when no dependents exist', async () => {
      const { container } = render(SpecGraphDAG, {
        props: {
          nodes: [{ path: 'system/solo.md', approval_status: 'approved' }],
          edges: [],
          impactPath: 'system/solo.md',
        },
      });

      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // Root node should be highlighted as root
      const soloNode = container.querySelector('[data-testid="dag-node-system/solo.md"]');
      expect(soloNode.getAttribute('data-impact')).toBe('root');
    });
  });
});
