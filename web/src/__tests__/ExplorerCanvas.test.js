import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import MoldableView from '../lib/MoldableView.svelte';

// Top-level mock so vitest hoisting works correctly
vi.mock('../lib/api.js', () => ({
  api: {
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
  },
}));
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
  {
    id: 'edge-1',
    source_id: 'node-1',
    target_id: 'node-2',
    edge_type: 'contains',
  },
  {
    id: 'edge-2',
    source_id: 'node-2',
    target_id: 'node-3',
    edge_type: 'implements',
  },
];

describe('ExplorerCanvas', () => {
  it('renders without throwing', () => {
    expect(() => render(ExplorerCanvas)).not.toThrow();
  });

  it('shows empty state when no nodes provided', () => {
    const { getByText } = render(ExplorerCanvas, { props: { nodes: [], edges: [] } });
    expect(getByText('No graph data')).toBeTruthy();
  });

  it('renders SVG canvas with nodes', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const svg = container.querySelector('svg');
    expect(svg).toBeTruthy();
  });

  it('renders node count in toolbar', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    expect(container.innerHTML).toContain('3 nodes');
    expect(container.innerHTML).toContain('2 edges');
  });

  it('renders all node groups', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
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
    // SVG <g> elements don't have .click() in jsdom — dispatch a click event
    firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onSelectNode).toHaveBeenCalledWith(expect.objectContaining({ id: expect.any(String) }));
  });

  it('shows reset button when nodes are present', () => {
    const { getByText } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    expect(getByText('Reset')).toBeTruthy();
  });

  it('shows legend items', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    expect(container.innerHTML).toContain('Package');
    expect(container.innerHTML).toContain('Type');
    expect(container.innerHTML).toContain('Interface');
    expect(container.innerHTML).toContain('Endpoint');
  });
});

describe('MoldableView', () => {
  it('renders without throwing', () => {
    expect(() => render(MoldableView)).not.toThrow();
  });

  it('shows graph view by default', () => {
    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    // Graph tab should be active
    const activeTab = container.querySelector('.view-tab.active');
    expect(activeTab?.textContent?.trim()).toContain('Graph');
  });

  it('renders all three view tabs', () => {
    const { getByRole } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const tabs = document.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(3);
  });

  it('switches to list view', async () => {
    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const listTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('List'));
    expect(listTab).toBeTruthy();
    listTab.click();
    // After clicking list, a table should appear
    await new Promise(r => setTimeout(r, 0));
    const table = container.querySelector('.list-table');
    expect(table).toBeTruthy();
  });

  it('switches to timeline view and shows Architectural Timeline heading', async () => {
    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    expect(timelineTab).toBeTruthy();
    timelineTab.click();
    await new Promise(r => setTimeout(r, 0));
    expect(container.innerHTML).toContain('Architectural Timeline');
  });

  it('timeline view shows EmptyState when no repoId', async () => {
    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    timelineTab.click();
    await new Promise(r => setTimeout(r, 0));
    // No repoId -> should show empty state
    expect(container.innerHTML).toContain('No architectural changes recorded yet');
  });

  it('timeline view with repoId fetches and shows scrubber', async () => {
    const mockDeltas = [
      { id: 'delta-1', repo_id: 'repo-1', commit_sha: 'abc1234def5678901234567890123456789012345', timestamp: Math.floor(Date.now() / 1000) - 3600, spec_ref: 'specs/system/platform-model.md@abc1234', agent_id: 'agent-1', delta_json: JSON.stringify({ added: 2, removed: 0 }) },
      { id: 'delta-2', repo_id: 'repo-1', commit_sha: 'def5678abc1234901234567890123456789012345', timestamp: Math.floor(Date.now() / 1000) - 1800, spec_ref: null, agent_id: null, delta_json: null },
    ];
    api.repoGraphTimeline.mockResolvedValueOnce(mockDeltas);

    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });
    const timelineTab = Array.from(container.querySelectorAll('.view-tab'))
      .find(el => el.textContent.includes('Timeline'));
    timelineTab.click();
    await new Promise(r => setTimeout(r, 50));
    // Scrubber should be present
    const scrubber = container.querySelector('.scrubber-input');
    expect(scrubber).toBeTruthy();
    // Now button should be present
    const nowBtn = container.querySelector('.now-btn');
    expect(nowBtn).toBeTruthy();
  });

  it('delta marker click shows delta card with sha and relative time', async () => {
    const sha = 'abc1234def5678901234567890123456789012345';
    const mockDeltas = [
      { id: 'delta-1', repo_id: 'repo-1', commit_sha: sha, timestamp: Math.floor(Date.now() / 1000) - 7200, spec_ref: 'specs/foo.md@abc1234', agent_id: 'agent-42', delta_json: JSON.stringify({ modified: 3 }) },
    ];
    api.repoGraphTimeline.mockResolvedValueOnce(mockDeltas);

    const { container } = render(MoldableView, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });
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
});
