import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import MoldableView from '../lib/MoldableView.svelte';

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

  it('switches to timeline view and shows stub', async () => {
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
});
