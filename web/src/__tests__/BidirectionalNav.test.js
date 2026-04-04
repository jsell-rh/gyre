/**
 * BidirectionalNav.test.js — TASK-360
 *
 * Tests for ArchPreviewCanvas ghost overlay rendering.
 * (ExplorerCanvas tests removed -- component superseded by ExplorerTreemap.)
 */
import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import ArchPreviewCanvas from '../lib/ArchPreviewCanvas.svelte';

// ── Test fixtures ────────────────────────────────────────────────────────────
const NODE_WITH_SPEC = {
  id: 'node-specced',
  repo_id: 'repo-1',
  node_type: 'module',
  name: 'MyModule',
  qualified_name: 'my_crate::MyModule',
  file_path: 'src/lib.rs',
  line_start: 1,
  visibility: 'public',
  spec_path: 'specs/system/my-module.md',
  spec_confidence: 'High',
};

const NODE_NO_SPEC = {
  id: 'node-unspecced',
  repo_id: 'repo-1',
  node_type: 'type',
  name: 'Orphan',
  qualified_name: 'my_crate::Orphan',
  file_path: 'src/orphan.rs',
  line_start: 5,
  visibility: 'public',
  spec_path: null,
  spec_confidence: 'None',
};

const NODE_OTHER_SPEC = {
  id: 'node-other',
  repo_id: 'repo-1',
  node_type: 'function',
  name: 'handler',
  qualified_name: 'my_crate::handler',
  file_path: 'src/handler.rs',
  line_start: 10,
  visibility: 'public',
  spec_path: 'specs/system/other.md',
  spec_confidence: 'Medium',
};

// ── ArchPreviewCanvas — ghost overlay rendering ───────────────────────────────
describe('ArchPreviewCanvas — ghost overlay rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(ArchPreviewCanvas)).not.toThrow();
  });

  it('shows empty state when no nodes', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    expect(container.innerHTML).toContain('No graph data');
  });

  it('renders SVG with nodes', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC], edges: [] },
    });
    const svg = container.querySelector('[data-testid="arch-preview-svg"]');
    expect(svg).toBeTruthy();
  });

  it('renders node groups for each node', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC, NODE_OTHER_SPEC], edges: [] },
    });
    const nodeGroups = container.querySelectorAll('.arch-node');
    expect(nodeGroups.length).toBe(3);
  });

  it('renders ghost border for node with ghost overlay', () => {
    const ghostOverlays = [{ nodeId: NODE_WITH_SPEC.id, type: 'modified' }];
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC], edges: [], ghostOverlays },
    });
    const ghostNode = container.querySelector('.ghost-node');
    expect(ghostNode).toBeTruthy();
  });

  it('renders ghost border with correct data-ghost-type', () => {
    const ghostOverlays = [{ nodeId: NODE_WITH_SPEC.id, type: 'new' }];
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], ghostOverlays },
    });
    const ghostNode = container.querySelector('[data-ghost-type="new"]');
    expect(ghostNode).toBeTruthy();
  });

  it('renders ghost overlay legend in full size when overlays present', () => {
    const ghostOverlays = [{ nodeId: NODE_WITH_SPEC.id, type: 'removed' }];
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], ghostOverlays, size: 'full' },
    });
    expect(container.innerHTML).toContain('new');
    expect(container.innerHTML).toContain('modified');
    expect(container.innerHTML).toContain('removed');
  });

  it('highlighted node receives highlighted class', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: {
        nodes: [NODE_WITH_SPEC, NODE_NO_SPEC],
        edges: [],
        highlightNodeIds: [NODE_WITH_SPEC.id],
      },
    });
    const highlighted = container.querySelector('.arch-node.highlighted');
    expect(highlighted).toBeTruthy();
    expect(highlighted.getAttribute('data-node-id')).toBe(NODE_WITH_SPEC.id);
  });

  it('calls onNodeClick when a node is clicked', () => {
    const onNodeClick = vi.fn();
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], onNodeClick },
    });
    const nodeEl = container.querySelector('.arch-node');
    nodeEl?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(onNodeClick).toHaveBeenCalledWith(NODE_WITH_SPEC.id);
  });

  it('shows toolbar with reset button in full size', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], size: 'full' },
    });
    expect(container.innerHTML).toContain('Reset');
  });

  it('does not show toolbar in mini size', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], size: 'mini' },
    });
    expect(container.querySelector('.arch-toolbar')).toBeNull();
  });

  it('shows node count in full size toolbar', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC], edges: [], size: 'full' },
    });
    expect(container.innerHTML).toContain('2 nodes');
  });

  it('applies mini CSS class in mini size', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], size: 'mini' },
    });
    expect(container.querySelector('.arch-canvas-wrap.mini')).toBeTruthy();
  });

  it('applies full CSS class in full size', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC], edges: [], size: 'full' },
    });
    expect(container.querySelector('.arch-canvas-wrap.full')).toBeTruthy();
  });

  it('renders edges between nodes', () => {
    const edges = [{ source_id: NODE_WITH_SPEC.id, target_id: NODE_NO_SPEC.id }];
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC], edges },
    });
    const edgeLines = container.querySelectorAll('.arch-edge');
    expect(edgeLines.length).toBe(1);
  });

  it('non-ghost nodes do not have ghost-node class', () => {
    const { container } = render(ArchPreviewCanvas, {
      props: { nodes: [NODE_WITH_SPEC, NODE_NO_SPEC], edges: [], ghostOverlays: [] },
    });
    const ghostNodes = container.querySelectorAll('.ghost-node');
    expect(ghostNodes.length).toBe(0);
  });
});
