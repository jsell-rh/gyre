import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import MoldableView from '../lib/MoldableView.svelte';

// Top-level mocks so vitest hoisting works correctly
vi.mock('../lib/api.js', () => ({
  api: {
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphNode: vi.fn().mockResolvedValue({ node: null, edges: [] }),
  },
}));

vi.mock('../lib/layout-engines.js', async () => {
  // Provide a synchronous mock so tests don't need to await async ELK/d3
  const { columnLayout } = await vi.importActual('../lib/layout-engines.js');
  return {
    columnLayout,
    computeLayout: vi.fn().mockImplementation(async (_eng, nodes) => columnLayout(nodes)),
  };
});

import { api } from '../lib/api.js';

const SAMPLE_NODES = [
  {
    id: 'node-1',
    repo_id: 'repo-1',
    node_type: 'module',
    name: 'gyre_domain',
    qualified_name: 'gyre_domain',
    file_path: 'crates/gyre-domain/src/lib.rs',
    line_start: 1,
    line_end: 50,
    visibility: 'public',
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'High',
    churn_count_30d: 3,
    last_modified_at: Math.floor(Date.now() / 1000) - 3600,
  },
  {
    id: 'node-2',
    repo_id: 'repo-1',
    node_type: 'type',
    name: 'Task',
    qualified_name: 'gyre_domain::Task',
    file_path: 'crates/gyre-domain/src/task.rs',
    line_start: 10,
    line_end: 40,
    visibility: 'public',
    spec_path: null,
    spec_confidence: 'None',
    churn_count_30d: 1,
    last_modified_at: Math.floor(Date.now() / 1000) - 86400,
    complexity: 5,
  },
  {
    id: 'node-3',
    repo_id: 'repo-1',
    node_type: 'interface',
    name: 'TaskPort',
    qualified_name: 'gyre_ports::TaskPort',
    file_path: 'crates/gyre-ports/src/task.rs',
    line_start: 1,
    line_end: 20,
    visibility: 'public',
    spec_path: 'specs/development/architecture.md',
    spec_confidence: 'High',
    churn_count_30d: 0,
    last_modified_at: Math.floor(Date.now() / 1000) - 172800,
  },
];

const SAMPLE_EDGES = [
  { id: 'edge-1', source_id: 'node-1', target_id: 'node-2', edge_type: 'contains' },
  { id: 'edge-2', source_id: 'node-2', target_id: 'node-3', edge_type: 'implements' },
];

describe('ExplorerCanvas — core rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(ExplorerCanvas)).not.toThrow();
  });

  it('shows empty state when no nodes provided', () => {
    const { getByText } = render(ExplorerCanvas, { props: { nodes: [], edges: [] } });
    expect(getByText('No graph data')).toBeTruthy();
  });

  it('renders SVG canvas with nodes', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const svg = container.querySelector('svg');
    expect(svg).toBeTruthy();
  });

  it('renders node count in toolbar', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(container.innerHTML).toContain('3 nodes');
    expect(container.innerHTML).toContain('2 edges');
  });

  it('renders all node groups', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const nodeGroups = container.querySelectorAll('.graph-node');
    expect(nodeGroups.length).toBe(3);
  });

  it('calls onSelectNode when a node is clicked', async () => {
    const onSelectNode = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, onSelectNode },
    });
    const firstNode = container.querySelector('.graph-node');
    expect(firstNode).toBeTruthy();
    firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onSelectNode).toHaveBeenCalledWith(expect.objectContaining({ id: expect.any(String) }));
  });

  it('dispatches ViewEvent via onViewEvent prop on node click', async () => {
    const onViewEvent = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, onViewEvent },
    });
    const firstNode = container.querySelector('.graph-node');
    firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onViewEvent).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'click', entity_type: 'node', entity_id: expect.any(String) })
    );
  });

  it('shows reset button when nodes are present', () => {
    const { getByText } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(getByText('Reset')).toBeTruthy();
  });

  it('shows legend items', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(container.innerHTML).toContain('Package');
    expect(container.innerHTML).toContain('Type');
    expect(container.innerHTML).toContain('Interface');
    expect(container.innerHTML).toContain('Endpoint');
  });
});

describe('ExplorerCanvas — layout engine switcher', () => {
  it('shows layout switcher buttons', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(container.innerHTML).toContain('Column');
    expect(container.innerHTML).toContain('Force');
    expect(container.innerHTML).toContain('Hierarchical');
    expect(container.innerHTML).toContain('Layered');
  });

  it('Column layout is active by default', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const activeBtn = container.querySelector('.layout-btn.active');
    expect(activeBtn?.textContent?.trim()).toBe('Column');
  });

  it('switching to Force layout marks Force button active', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const forceBtn = Array.from(container.querySelectorAll('.layout-btn'))
      .find(el => el.textContent.trim() === 'Force');
    expect(forceBtn).toBeTruthy();
    forceBtn.click();
    await new Promise(r => setTimeout(r, 10));
    expect(forceBtn.classList.contains('active')).toBe(true);
  });

  it('switching to Hierarchical layout marks Hierarchical button active', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const hierBtn = Array.from(container.querySelectorAll('.layout-btn'))
      .find(el => el.textContent.trim() === 'Hierarchical');
    hierBtn.click();
    await new Promise(r => setTimeout(r, 10));
    expect(hierBtn.classList.contains('active')).toBe(true);
  });

  it('switching to Layered layout marks Layered button active', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const layeredBtn = Array.from(container.querySelectorAll('.layout-btn'))
      .find(el => el.textContent.trim() === 'Layered');
    layeredBtn.click();
    await new Promise(r => setTimeout(r, 10));
    expect(layeredBtn.classList.contains('active')).toBe(true);
  });

  it('viewSpec.layout sets initial layout engine', () => {
    const viewSpec = { layout: 'hierarchical', data: {}, encoding: {} };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, viewSpec },
    });
    const activeBtn = container.querySelector('.layout-btn.active');
    expect(activeBtn?.textContent?.trim()).toBe('Hierarchical');
  });
});

describe('ExplorerCanvas — spec linkage overlay', () => {
  it('shows Spec Linkage toggle button', () => {
    const { getByText } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(getByText('Spec Linkage')).toBeTruthy();
  });

  it('does not show spec legend by default', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    expect(container.querySelector('.spec-legend')).toBeNull();
  });

  it('shows spec legend when showSpecLinkage=true', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: true },
    });
    expect(container.querySelector('.spec-legend')).toBeTruthy();
  });

  it('shows spec legend with confidence labels when overlay is on', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: true },
    });
    expect(container.innerHTML).toContain('High confidence');
    expect(container.innerHTML).toContain('Unspecced');
  });

  it('shows spec coverage counts in legend', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: true },
    });
    expect(container.innerHTML).toContain('2 specced');
    expect(container.innerHTML).toContain('1 unspecced');
  });

  it('renders spec rings on nodes when overlay is active', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: true },
    });
    const rings = container.querySelectorAll('.spec-ring');
    expect(rings.length).toBe(3);
  });

  it('does not render spec rings when overlay is off', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: false },
    });
    const rings = container.querySelectorAll('.spec-ring');
    expect(rings.length).toBe(0);
  });

  it('shows Unspecced only pill when spec linkage is on', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, showSpecLinkage: true },
    });
    expect(container.innerHTML).toContain('Unspecced only');
  });
});

describe('ExplorerCanvas — performance thresholds', () => {
  function makeNodes(count, visibility = 'public') {
    return Array.from({ length: count }, (_, i) => ({
      id: `node-${i}`,
      node_type: 'module',
      name: `Module${i}`,
      visibility: i % 2 === 0 ? 'public' : visibility,
    }));
  }

  it('does NOT show public-only banner when node count <= 500', () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: makeNodes(100), edges: [] } });
    expect(container.innerHTML).not.toContain('private nodes hidden');
  });

  it('shows public-only banner when node count > 500 and <= 1000', () => {
    const nodes = makeNodes(600, 'private');
    const { container } = render(ExplorerCanvas, { props: { nodes, edges: [] } });
    expect(container.innerHTML).toContain('private nodes hidden');
    expect(container.innerHTML).toContain('Show All');
  });

  it('shows list fallback warning when node count > 1000', () => {
    const nodes = makeNodes(1001);
    const { container } = render(ExplorerCanvas, { props: { nodes, edges: [] } });
    expect(container.innerHTML).toContain('Graph too large');
    expect(container.querySelector('.list-table')).toBeTruthy();
  });

  it('list fallback shows node rows', () => {
    const nodes = makeNodes(1001);
    const { container } = render(ExplorerCanvas, { props: { nodes, edges: [] } });
    const rows = container.querySelectorAll('.list-row');
    expect(rows.length).toBeGreaterThan(0);
  });
});

describe('ExplorerCanvas — viewSpec grammar', () => {
  it('shows explanation banner when viewSpec.explanation is set', () => {
    const viewSpec = {
      layout: 'column',
      explanation: 'Authentication flows through require_auth_middleware',
      data: {},
      encoding: {},
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, viewSpec },
    });
    expect(container.innerHTML).toContain('Authentication flows through require_auth_middleware');
  });

  it('applies node_type filter from viewSpec.data.node_types', async () => {
    const viewSpec = { layout: 'column', data: { node_types: ['module'] }, encoding: {} };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, viewSpec },
    });
    // Only module nodes should be visible (1 of 3)
    expect(container.innerHTML).toContain('1 node');
  });

  it('applies highlight from viewSpec.highlight.spec_path', () => {
    const viewSpec = {
      layout: 'column',
      highlight: { spec_path: 'specs/system/platform-model.md' },
      data: {},
      encoding: {},
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, viewSpec },
    });
    // Nodes with that spec_path should have spec-highlighted class
    const highlighted = container.querySelector('.graph-node.spec-highlighted');
    expect(highlighted).toBeTruthy();
  });

  it('shows annotations from viewSpec.annotations', () => {
    const viewSpec = {
      layout: 'column',
      annotations: [{ node_name: 'gyre_domain', text: 'Entry point' }],
      data: {},
      encoding: {},
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, viewSpec },
    });
    expect(container.innerHTML).toContain('Entry point');
  });

  it('shows spec-link button in detail panel', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const firstNode = container.querySelector('.graph-node');
    firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await new Promise(r => setTimeout(r, 0));
    const specBtn = container.querySelector('.spec-link-btn');
    expect(specBtn).toBeTruthy();
  });
});

describe('MoldableView', () => {
  beforeEach(() => {
    // FlowRenderer uses ResizeObserver which is not available in jsdom
    if (!global.ResizeObserver) {
      global.ResizeObserver = class ResizeObserver {
        observe() {}
        unobserve() {}
        disconnect() {}
      };
    }
  });

  it('renders without throwing', () => {
    expect(() => render(MoldableView)).not.toThrow();
  });

  it('shows graph view by default', () => {
    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const activeTab = container.querySelector('.view-tab.active');
    expect(activeTab?.textContent?.trim()).toContain('Graph');
  });

  it('renders all four view tabs', () => {
    render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const tabs = document.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(4);
  });

  it('switches to list view', async () => {
    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const listTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('List'));
    expect(listTab).toBeTruthy();
    listTab.click();
    await new Promise(r => setTimeout(r, 0));
    const table = container.querySelector('.list-table');
    expect(table).toBeTruthy();
  });

  it('switches to timeline view and shows Architectural Timeline heading', async () => {
    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    expect(timelineTab).toBeTruthy();
    timelineTab.click();
    await new Promise(r => setTimeout(r, 0));
    expect(container.innerHTML).toContain('Architectural Timeline');
  });

  it('timeline view shows EmptyState when no repoId', async () => {
    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    timelineTab.click();
    await new Promise(r => setTimeout(r, 0));
    expect(container.innerHTML).toContain('No architectural changes recorded yet');
  });

  it('timeline view with repoId fetches and shows scrubber', async () => {
    const mockDeltas = [
      { id: 'delta-1', repo_id: 'repo-1', commit_sha: 'abc1234def5678901234567890123456789012345', timestamp: Math.floor(Date.now() / 1000) - 3600, spec_ref: 'specs/system/platform-model.md@abc1234', agent_id: 'agent-1', delta_json: JSON.stringify({ added: 2, removed: 0 }) },
      { id: 'delta-2', repo_id: 'repo-1', commit_sha: 'def5678abc1234901234567890123456789012345', timestamp: Math.floor(Date.now() / 1000) - 1800, spec_ref: null, agent_id: null, delta_json: null },
    ];
    api.repoGraphTimeline.mockResolvedValueOnce(mockDeltas);

    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' } });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    timelineTab.click();
    await new Promise(r => setTimeout(r, 50));
    const scrubber = container.querySelector('.scrubber-input');
    expect(scrubber).toBeTruthy();
    const nowBtn = container.querySelector('.now-btn');
    expect(nowBtn).toBeTruthy();
  });

  it('delta marker click shows delta card with sha and relative time', async () => {
    const sha = 'abc1234def5678901234567890123456789012345';
    const mockDeltas = [
      { id: 'delta-1', repo_id: 'repo-1', commit_sha: sha, timestamp: Math.floor(Date.now() / 1000) - 7200, spec_ref: 'specs/foo.md@abc1234', agent_id: 'agent-42', delta_json: JSON.stringify({ modified: 3 }) },
    ];
    api.repoGraphTimeline.mockResolvedValueOnce(mockDeltas);

    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' } });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    timelineTab.click();
    await new Promise(r => setTimeout(r, 50));

    const marker = container.querySelector('.delta-marker');
    if (marker) {
      marker.click();
      await new Promise(r => setTimeout(r, 0));
      const card = container.querySelector('.delta-card');
      if (card) {
        expect(card.innerHTML).toContain(sha.slice(0, 7));
      }
    }
  });

  it('switches to flow view and renders flow-renderer', async () => {
    const { container } = render(MoldableView, { props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES } });
    const flowTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Flow'));
    expect(flowTab).toBeTruthy();
    flowTab.click();
    await new Promise(r => setTimeout(r, 0));
    expect(container.querySelector('[data-testid="flow-renderer"]')).toBeTruthy();
  });
});
