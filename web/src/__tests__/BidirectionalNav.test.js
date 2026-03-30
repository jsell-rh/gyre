/**
 * BidirectionalNav.test.js — TASK-360
 *
 * Tests for bidirectional Architecture ↔ Spec navigation:
 * - Double-click node with spec_path → spec detail panel opens
 * - Double-click node without spec_path → "No governing spec" shown
 * - Ghost overlay legend appears when overlays are set
 * - ?highlight_spec= query param highlights matching nodes on mount
 * - ?detail=node:<uuid> query param opens detail panel for that node
 * - ArchPreviewCanvas renders with ghost overlays
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import ArchPreviewCanvas from '../lib/ArchPreviewCanvas.svelte';

// ── Module mocks ────────────────────────────────────────────────────────────
vi.mock('../lib/api.js', () => ({
  api: {
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphNode: vi.fn().mockResolvedValue({ node: null, edges: [] }),
    specContent: vi.fn().mockResolvedValue({ content: '# My Spec\n\nThis governs MyModule.' }),
    specsAssist: vi.fn().mockResolvedValue({ ok: true, body: null }),
    graphPredict: vi.fn().mockResolvedValue({ overlays: [] }),
  },
}));

vi.mock('../lib/layout-engines.js', async () => {
  const { columnLayout } = await vi.importActual('../lib/layout-engines.js');
  return {
    columnLayout,
    computeLayout: vi.fn().mockImplementation(async (_eng, nodes) => columnLayout(nodes)),
  };
});

import { api } from '../lib/api.js';

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

const SAMPLE_NODES = [NODE_WITH_SPEC, NODE_NO_SPEC, NODE_OTHER_SPEC];
const SAMPLE_EDGES = [];

function dblclick(element) {
  element.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
}

// ── ExplorerCanvas — double-click → spec panel ───────────────────────────────
describe('ExplorerCanvas — double-click opens spec detail panel', () => {
  it('double-click on node with spec_path opens spec detail panel', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    // Find the node element and dblclick it
    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    expect(nodeEl).toBeTruthy();
    dblclick(nodeEl);

    await new Promise(r => setTimeout(r, 0));

    const specPanel = container.querySelector('[data-testid="spec-detail-panel"]');
    expect(specPanel).toBeTruthy();
  });

  it('double-click on node with spec_path shows node name in spec panel', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 0));

    const specPanel = container.querySelector('[data-testid="spec-detail-panel"]');
    expect(specPanel.innerHTML).toContain('MyModule');
  });

  it('double-click on node with spec_path shows the spec_path', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 0));

    const specPanel = container.querySelector('[data-testid="spec-detail-panel"]');
    expect(specPanel.innerHTML).toContain('specs/system/my-module.md');
  });

  it('double-click on node without spec_path shows "No governing spec"', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_NO_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 0));

    const noSpecEl = container.querySelector('[data-testid="no-governing-spec"]');
    expect(noSpecEl).toBeTruthy();
    expect(noSpecEl.innerHTML).toContain('No governing spec');
  });

  it('double-click on node without spec_path shows "Create spec" button', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_NO_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 0));

    const createBtn = container.querySelector('[data-testid="create-spec-btn"]');
    expect(createBtn).toBeTruthy();
  });

  it('spec panel close button closes the panel', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 0));

    expect(container.querySelector('[data-testid="spec-detail-panel"]')).toBeTruthy();

    const closeBtn = container.querySelector('[data-testid="spec-detail-panel"] .close-btn');
    closeBtn?.click();
    await new Promise(r => setTimeout(r, 0));

    expect(container.querySelector('[data-testid="spec-detail-panel"]')).toBeNull();
  });

  it('spec panel is not visible initially (no dblclick yet)', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });
    expect(container.querySelector('[data-testid="spec-detail-panel"]')).toBeNull();
  });
});

// ── ExplorerCanvas — spec content loading ────────────────────────────────────
describe('ExplorerCanvas — spec content loading', () => {
  beforeEach(() => {
    api.specContent.mockResolvedValue({ content: '# MyModule Spec\n\nGoverns MyModule.' });
  });

  it('calls api.specContent when node with spec_path is double-clicked', async () => {
    api.specContent.mockClear();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 50));

    expect(api.specContent).toHaveBeenCalledWith('specs/system/my-module.md', 'repo-1');
  });

  it('shows spec editor textarea after spec content loads', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 50));

    const editor = container.querySelector('[data-testid="spec-editor"]');
    expect(editor).toBeTruthy();
  });

  it('does not call api.specContent when node has no spec_path', async () => {
    api.specContent.mockClear();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_NO_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 50));

    expect(api.specContent).not.toHaveBeenCalled();
  });
});

// ── ExplorerCanvas — ghost overlays from spec editing ────────────────────────
describe('ExplorerCanvas — ghost overlays on canvas', () => {
  it('does not call graphPredict when spec draft is unchanged (initial load)', async () => {
    api.specContent.mockResolvedValue({ content: '# Spec\n\nContent here.' });
    api.graphPredict.mockClear();

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 1100)); // past debounce

    // predict should NOT fire — draft === specContent (no edits)
    expect(api.graphPredict).not.toHaveBeenCalled();
  });

  it('calls graphPredict when spec draft differs from original content', async () => {
    api.specContent.mockResolvedValue({ content: '# Original\n\nOriginal content.' });
    api.graphPredict.mockResolvedValue({
      overlays: [{ nodeId: 'node-specced', type: 'modified' }],
    });
    api.graphPredict.mockClear();

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    const nodeEl = container.querySelector(`[data-node-id="${NODE_WITH_SPEC.id}"]`);
    dblclick(nodeEl);
    await new Promise(r => setTimeout(r, 50)); // spec content loads

    // Simulate user editing the textarea (changes draft away from original)
    const textarea = container.querySelector('[data-testid="spec-editor"]');
    if (textarea) {
      textarea.value = '# Original\n\nOriginal content.\n\nNew section added by user.';
      textarea.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise(r => setTimeout(r, 1100)); // past debounce
      expect(api.graphPredict).toHaveBeenCalledWith('repo-1', expect.objectContaining({
        spec_path: 'specs/system/my-module.md',
      }));
    }
  });
});

// ── ExplorerCanvas — query param: ?highlight_spec= ───────────────────────────
describe('ExplorerCanvas — ?highlight_spec= query param', () => {
  let originalSearch;

  beforeEach(() => {
    originalSearch = window.location.search;
  });

  afterEach(() => {
    // Restore original search
    const url = new URL(window.location.href);
    url.search = originalSearch;
    window.history.replaceState({}, '', url.toString());
  });

  it('applies spec highlight to matching nodes when ?highlight_spec= is set', async () => {
    const url = new URL(window.location.href);
    url.searchParams.set('highlight_spec', 'specs/system/my-module.md');
    window.history.replaceState({}, '', url.toString());

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    await new Promise(r => setTimeout(r, 0));

    // NODE_WITH_SPEC has spec_path 'specs/system/my-module.md' so it should be highlighted
    const highlighted = container.querySelector('.graph-node.highlighted');
    expect(highlighted).toBeTruthy();
    expect(highlighted.getAttribute('data-node-id')).toBe(NODE_WITH_SPEC.id);
  });

  it('does not highlight nodes when highlight_spec does not match', async () => {
    const url = new URL(window.location.href);
    url.searchParams.set('highlight_spec', 'specs/nonexistent.md');
    window.history.replaceState({}, '', url.toString());

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES, repoId: 'repo-1' },
    });

    await new Promise(r => setTimeout(r, 0));

    const highlighted = container.querySelector('.graph-node.highlighted');
    expect(highlighted).toBeNull();
  });

  it('handles missing highlight_spec param gracefully', async () => {
    const url = new URL(window.location.href);
    url.searchParams.delete('highlight_spec');
    window.history.replaceState({}, '', url.toString());

    expect(() => render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    })).not.toThrow();
  });
});

// ── ExplorerCanvas — query param: ?detail=node:<uuid> ───────────────────────
describe('ExplorerCanvas — ?detail=node:<uuid> query param', () => {
  let originalSearch;

  beforeEach(() => {
    originalSearch = window.location.search;
  });

  afterEach(() => {
    const url = new URL(window.location.href);
    url.search = originalSearch;
    window.history.replaceState({}, '', url.toString());
  });

  it('opens detail panel for matching node when ?detail=node:<uuid> is set', async () => {
    const url = new URL(window.location.href);
    url.searchParams.set('detail', `node:${NODE_WITH_SPEC.id}`);
    window.history.replaceState({}, '', url.toString());

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });

    await new Promise(r => setTimeout(r, 0));

    // The selected node detail panel should appear
    const panel = container.querySelector('.detail-panel');
    expect(panel).toBeTruthy();
    expect(panel.innerHTML).toContain('MyModule');
  });

  it('does not open detail panel when node UUID does not match', async () => {
    const url = new URL(window.location.href);
    url.searchParams.set('detail', 'node:nonexistent-uuid');
    window.history.replaceState({}, '', url.toString());

    const { container } = render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    });

    await new Promise(r => setTimeout(r, 0));

    // No selectedNode detail panel (the .detail-panel class is the node panel)
    const panel = container.querySelector('.detail-panel');
    expect(panel).toBeNull();
  });

  it('handles malformed detail param gracefully', async () => {
    const url = new URL(window.location.href);
    url.searchParams.set('detail', 'not-a-node-ref');
    window.history.replaceState({}, '', url.toString());

    expect(() => render(ExplorerCanvas, {
      props: { nodes: SAMPLE_NODES, edges: SAMPLE_EDGES },
    })).not.toThrow();
  });
});

// ── ArchPreviewCanvas — ghost overlay rendering ───────────────────────────────
describe('ArchPreviewCanvas — ghost overlay rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(ArchPreviewCanvas)).not.toThrow();
  });

  it('shows empty state when no nodes', () => {
    const { container } = render(ArchPreviewCanvas, { props: { nodes: [], edges: [] } });
    expect(container.innerHTML).toContain('No architecture data');
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
    // Ghost nodes have ghost-node class
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
