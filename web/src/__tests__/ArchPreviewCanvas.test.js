import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import ArchPreviewCanvas from '../lib/ArchPreviewCanvas.svelte';

// Mock layout-engines so tests don't need ELK/d3
vi.mock('../lib/layout-engines.js', async () => {
  const { columnLayout } = await vi.importActual('../lib/layout-engines.js');
  return { columnLayout };
});

// ── Sample data ───────────────────────────────────────────────────────────────

const NODES = [
  { id: 'n1', node_type: 'module',   name: 'gyre_domain',  spec_path: 'specs/domain.md' },
  { id: 'n2', node_type: 'type',     name: 'Task',         spec_path: null },
  { id: 'n3', node_type: 'endpoint', name: 'POST /tasks',  spec_path: null },
];

const EDGES = [
  { source: 'n1', target: 'n2', label: 'contains' },
  { source: 'n2', target: 'n3', label: 'routes_to' },
];

// ── Core rendering ────────────────────────────────────────────────────────────

describe('ArchPreviewCanvas — core rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(ArchPreviewCanvas)).not.toThrow();
  });

  it('shows empty state when nodes is empty', () => {
    const { getByText } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    expect(getByText('No graph data')).toBeTruthy();
  });

  it('renders SVG canvas when nodes present', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    expect(container.querySelector('[data-testid="arch-preview-svg"]')).toBeTruthy();
  });

  it('renders a node group for each node', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    const nodeGroups = container.querySelectorAll('.arch-node');
    expect(nodeGroups.length).toBe(3);
  });

  it('renders edges as SVG lines', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    const lines = container.querySelectorAll('.arch-edge');
    expect(lines.length).toBe(2);
  });

  it('sets data-node-id on each node group', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    const ids = Array.from(container.querySelectorAll('.arch-node')).map(el => el.getAttribute('data-node-id'));
    expect(ids).toContain('n1');
    expect(ids).toContain('n2');
    expect(ids).toContain('n3');
  });

  it('uses node label for text content', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    expect(container.innerHTML).toContain('gyre_dom'); // truncated to 14 chars
  });

  it('accepts nodes with label prop instead of name', () => {
    const nodes = [{ id: 'x1', label: 'MyLabel', node_type: 'module' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes } });
    expect(container.innerHTML).toContain('MyLabel');
  });
});

// ── Ghost overlays ────────────────────────────────────────────────────────────

describe('ArchPreviewCanvas — ghost overlays', () => {
  it('renders ghost-border rect for "new" overlay', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghosts = container.querySelectorAll('.ghost-border');
    expect(ghosts.length).toBe(1);
  });

  it('"new" ghost border uses green stroke (#22c55e)', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghost = container.querySelector('.ghost-border');
    expect(ghost?.getAttribute('stroke')).toBe('#22c55e');
  });

  it('"modified" ghost border uses yellow stroke (#eab308)', () => {
    const ghostOverlays = [{ nodeId: 'n2', type: 'modified' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghost = container.querySelector('.ghost-border');
    expect(ghost?.getAttribute('stroke')).toBe('#eab308');
  });

  it('"removed" ghost border uses red stroke (#ef4444)', () => {
    const ghostOverlays = [{ nodeId: 'n3', type: 'removed' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghost = container.querySelector('.ghost-border');
    expect(ghost?.getAttribute('stroke')).toBe('#ef4444');
  });

  it('ghost borders use dashed stroke', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghost = container.querySelector('.ghost-border');
    const dasharray = ghost?.getAttribute('stroke-dasharray') ?? '';
    expect(dasharray).not.toBe('none');
    expect(dasharray.length).toBeGreaterThan(0);
  });

  it('nodes with ghost overlay get ghost-node class', () => {
    const ghostOverlays = [{ nodeId: 'n2', type: 'modified' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const ghostNode = container.querySelector('.arch-node.ghost-node');
    expect(ghostNode).toBeTruthy();
    expect(ghostNode.getAttribute('data-node-id')).toBe('n2');
  });

  it('sets data-ghost-type attribute on ghost nodes', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const n1 = container.querySelector('[data-node-id="n1"]');
    expect(n1?.getAttribute('data-ghost-type')).toBe('new');
  });

  it('non-ghost nodes have empty data-ghost-type', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    const n2 = container.querySelector('[data-node-id="n2"]');
    expect(n2?.getAttribute('data-ghost-type')).toBe('');
  });

  it('multiple ghost overlays render multiple ghost-border rects', () => {
    const ghostOverlays = [
      { nodeId: 'n1', type: 'new' },
      { nodeId: 'n2', type: 'modified' },
      { nodeId: 'n3', type: 'removed' },
    ];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays } });
    expect(container.querySelectorAll('.ghost-border').length).toBe(3);
  });

  it('no ghost-border rendered when ghostOverlays is empty', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays: [] } });
    expect(container.querySelectorAll('.ghost-border').length).toBe(0);
  });

  it('full-size mode shows ghost legend chips when ghostOverlays present', () => {
    const ghostOverlays = [{ nodeId: 'n1', type: 'new' }];
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES, ghostOverlays, size: 'full' } });
    expect(container.innerHTML).toContain('new');
    expect(container.innerHTML).toContain('modified');
    expect(container.innerHTML).toContain('removed');
  });
});

// ── highlightNodeIds ──────────────────────────────────────────────────────────

describe('ArchPreviewCanvas — highlightNodeIds', () => {
  it('adds highlighted class to specified node', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, highlightNodeIds: ['n2'] },
    });
    const highlighted = container.querySelector('.arch-node.highlighted');
    expect(highlighted).toBeTruthy();
    expect(highlighted.getAttribute('data-node-id')).toBe('n2');
  });

  it('renders highlight-ring rect on highlighted nodes', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, highlightNodeIds: ['n1'] },
    });
    expect(container.querySelector('.highlight-ring')).toBeTruthy();
  });

  it('does not add highlighted class when highlightNodeIds is empty', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, highlightNodeIds: [] },
    });
    expect(container.querySelector('.arch-node.highlighted')).toBeNull();
  });

  it('highlights multiple nodes when multiple IDs provided', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, highlightNodeIds: ['n1', 'n3'] },
    });
    const highlighted = container.querySelectorAll('.arch-node.highlighted');
    expect(highlighted.length).toBe(2);
  });

  it('non-highlighted nodes do not have highlighted class', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, highlightNodeIds: ['n1'] },
    });
    const n2 = container.querySelector('[data-node-id="n2"]');
    expect(n2?.classList.contains('highlighted')).toBe(false);
  });
});

// ── onNodeClick callback ──────────────────────────────────────────────────────

describe('ArchPreviewCanvas — onNodeClick', () => {
  it('calls onNodeClick with nodeId when node is clicked', () => {
    const onNodeClick = vi.fn();
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeClick },
    });
    const firstNode = container.querySelector('.arch-node');
    expect(firstNode).toBeTruthy();
    firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onNodeClick).toHaveBeenCalledTimes(1);
    expect(onNodeClick).toHaveBeenCalledWith(expect.any(String));
  });

  it('calls onNodeClick with the correct nodeId', () => {
    const onNodeClick = vi.fn();
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeClick },
    });
    const n2El = container.querySelector('[data-node-id="n2"]');
    n2El.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onNodeClick).toHaveBeenCalledWith('n2');
  });

  it('does not throw when onNodeClick is not provided', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: NODES, edges: EDGES } });
    const firstNode = container.querySelector('.arch-node');
    expect(() => firstNode.dispatchEvent(new MouseEvent('click', { bubbles: true }))).not.toThrow();
  });
});

// ── Size modes ────────────────────────────────────────────────────────────────

describe('ArchPreviewCanvas — size modes', () => {
  it('mini mode renders without toolbar', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'mini' },
    });
    expect(container.querySelector('.arch-toolbar')).toBeNull();
  });

  it('full mode renders toolbar with Reset button', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'full' },
    });
    expect(container.querySelector('.arch-toolbar')).toBeTruthy();
    expect(container.innerHTML).toContain('Reset');
  });

  it('mini mode wrapper has class mini', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'mini' },
    });
    expect(container.querySelector('.arch-canvas-wrap.mini')).toBeTruthy();
  });

  it('full mode wrapper has class full', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'full' },
    });
    expect(container.querySelector('.arch-canvas-wrap.full')).toBeTruthy();
  });

  it('full mode shows node count in toolbar', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'full' },
    });
    expect(container.innerHTML).toContain('3 nodes');
    expect(container.innerHTML).toContain('2 edges');
  });

  it('mini mode uses smaller font size on labels', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'mini' },
    });
    const texts = container.querySelectorAll('text');
    const fontSizes = Array.from(texts).map(t => t.getAttribute('font-size'));
    expect(fontSizes.some(fs => fs === '8')).toBe(true);
  });

  it('full mode uses standard font size on labels', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: NODES, edges: EDGES, size: 'full' },
    });
    const texts = container.querySelectorAll('text');
    const fontSizes = Array.from(texts).map(t => t.getAttribute('font-size'));
    expect(fontSizes.some(fs => fs === '9')).toBe(true);
  });
});

// ── Empty state ───────────────────────────────────────────────────────────────

describe('ArchPreviewCanvas — empty state', () => {
  it('shows empty state title', () => {
    const { getByText } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    expect(getByText('No graph data')).toBeTruthy();
  });

  it('does not render the arch canvas SVG when nodes is empty', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    // EmptyState renders its own icon SVG; the arch canvas SVG should not be present
    expect(container.querySelector('[data-testid="arch-preview-svg"]')).toBeNull();
  });

  it('shows empty state description', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    expect(container.innerHTML).toContain('Select a repository to view its knowledge graph');
  });
});

// ── graphPredict API ──────────────────────────────────────────────────────────

describe('graphPredict API', () => {
  it('is exported from api.js', async () => {
    const mod = await import('../lib/api.js');
    expect(typeof mod.api.graphPredict).toBe('function');
  });
});
