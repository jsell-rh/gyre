import { describe, it, expect } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import GhostOverlay from '../lib/GhostOverlay.svelte';
import { normalizeGhosts, positionGhosts, ghostEdges } from '../lib/ghostOverlay.js';

// Render helper: normalize → position → render the overlay exactly as
// ArchPreviewCanvas does, so the component tests exercise the production
// data path (not hand-built props).
function renderOverlay(predictions, { nodes = [], nodePositions = {}, mode = 'predicted' } = {}) {
  const ghosts = normalizeGhosts(predictions);
  const positions = positionGhosts(ghosts, nodePositions, nodes);
  const edges = ghostEdges(ghosts, positions, nodePositions, nodes);
  const { container } = render(GhostOverlay, {
    props: { ghosts, positions, edges, mode },
  });
  return { container, ghosts, positions, edges };
}

const NODES = [
  { id: 'n1', name: 'FullTextPort', node_type: 'interface' },
  { id: 'n2', name: 'SearchQuery', node_type: 'type' },
];
const NODE_POSITIONS = { n1: { x: 100, y: 100 }, n2: { x: 300, y: 100 } };

// ── §3 Ghost Overlay Visual Language ──────────────────────────────────────────

describe('GhostOverlay — §3 visual language', () => {
  it('renders a ghost node per prediction', () => {
    const { container } = renderOverlay([
      { action: 'add', name: 'VectorIndex', confidence: 0.8 },
      { action: 'modify', node_id: 'n2', name: 'SearchQuery', confidence: 0.9 },
      { action: 'remove', node_id: 'n1', name: 'FullTextPort', confidence: 0.7 },
    ]);
    const ghostNodes = container.querySelectorAll('[data-testid="ghost-node"]');
    expect(ghostNodes.length).toBe(3);
  });

  it('add ghosts: dotted outline with green stroke and green fill', () => {
    const { container } = renderOverlay([
      { action: 'add', name: 'VectorIndex', confidence: 1 },
    ]);
    const body = container.querySelector('[data-action="add"] .ghost-border');
    expect(body).toBeTruthy();
    expect(body.getAttribute('stroke')).toBe('#22c55e');
    const dash = body.getAttribute('stroke-dasharray') ?? '';
    expect(dash).not.toBe('none');
    expect(dash.length).toBeGreaterThan(0);
    // Green fill (semi-transparent by design, but the green channel dominates).
    const fill = body.getAttribute('fill') ?? '';
    expect(fill).toContain('34,197,94');
  });

  it('modify ghosts: yellow highlight (yellow dotted outline)', () => {
    const { container } = renderOverlay([
      { action: 'modify', node_id: 'n2', name: 'SearchQuery', confidence: 1 },
    ]);
    const body = container.querySelector('[data-action="modify"] .ghost-border');
    expect(body.getAttribute('stroke')).toBe('#eab308');
    expect((body.getAttribute('stroke-dasharray') ?? '')).not.toBe('none');
  });

  it('remove ghosts: red dotted outline plus a red strikethrough line', () => {
    const { container } = renderOverlay([
      { action: 'remove', node_id: 'n1', name: 'FullTextPort', confidence: 1 },
    ]);
    const group = container.querySelector('[data-action="remove"]');
    expect(group.querySelector('.ghost-border').getAttribute('stroke')).toBe('#ef4444');
    const strike = group.querySelector('[data-testid="ghost-strikethrough"]');
    expect(strike).toBeTruthy();
    expect(strike.getAttribute('stroke')).toBe('#ef4444');
  });

  it('renders dotted green edges for predicted new relationships', () => {
    const { container } = renderOverlay(
      [{ action: 'add', name: 'search_similar', parent_trait: 'FullTextPort' }],
      { nodes: NODES, nodePositions: NODE_POSITIONS },
    );
    const edge = container.querySelector('[data-testid="ghost-edge"]');
    expect(edge).toBeTruthy();
    expect(edge.getAttribute('stroke')).toBe('#22c55e');
    expect((edge.getAttribute('stroke-dasharray') ?? '')).not.toBe('none');
  });

  it('renders no ghost-edge when no relationship is predicted', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'Lonely' }]);
    expect(container.querySelector('[data-testid="ghost-edge"]')).toBeNull();
  });

  it('renders nothing for an empty ghost set', () => {
    const { container } = renderOverlay([]);
    expect(container.querySelector('[data-testid="ghost-node"]')).toBeNull();
    expect(container.querySelector('[data-testid="ghost-edge"]')).toBeNull();
  });
});

// ── Confidence → opacity (§3: higher confidence = more opaque ghost) ─────────

describe('GhostOverlay — confidence opacity', () => {
  it('50% confidence renders at 0.3 opacity', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'A', confidence: 0.5 }]);
    const body = container.querySelector('.ghost-border');
    expect(Number(body.getAttribute('opacity'))).toBeCloseTo(0.3, 5);
  });

  it('100% confidence renders fully opaque', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'A', confidence: 1 }]);
    const body = container.querySelector('.ghost-border');
    expect(Number(body.getAttribute('opacity'))).toBe(1);
  });

  it('higher confidence renders more opaque than lower confidence', () => {
    const low = renderOverlay([{ action: 'add', name: 'A', confidence: 0.5 }]);
    const high = renderOverlay([{ action: 'add', name: 'A', confidence: 0.95 }]);
    const lowOpacity = Number(low.container.querySelector('.ghost-border').getAttribute('opacity'));
    const highOpacity = Number(high.container.querySelector('.ghost-border').getAttribute('opacity'));
    expect(highOpacity).toBeGreaterThan(lowOpacity);
  });
});

// ── Tooltips (§3: each ghost shows the prediction reason) ─────────────────────

describe('GhostOverlay — tooltips', () => {
  it('shows the prediction reason on hover', async () => {
    const { container } = renderOverlay([
      {
        action: 'add',
        name: 'VectorIndex',
        confidence: 0.85,
        reason: 'Spec mentions vector similarity search, likely needs an index type',
      },
    ]);
    expect(container.querySelector('[data-testid="ghost-tooltip"]')).toBeNull();
    await fireEvent.mouseEnter(container.querySelector('[data-testid="ghost-node"]'));
    const tooltip = container.querySelector('[data-testid="ghost-tooltip"]');
    expect(tooltip).toBeTruthy();
    expect(tooltip.textContent).toContain('VectorIndex');
    expect(tooltip.textContent).toContain('85%');
    // The reason text (word-wrapped across tooltip lines).
    expect(tooltip.textContent).toContain('similarity');
    expect(tooltip.textContent).toContain('index type');
  });

  it('hides the tooltip on mouse leave', async () => {
    const { container } = renderOverlay([
      { action: 'modify', node_id: 'n2', name: 'SearchQuery', reason: 'needs modes' },
    ]);
    const node = container.querySelector('[data-testid="ghost-node"]');
    await fireEvent.mouseEnter(node);
    expect(container.querySelector('[data-testid="ghost-tooltip"]')).toBeTruthy();
    await fireEvent.mouseLeave(node);
    expect(container.querySelector('[data-testid="ghost-tooltip"]')).toBeNull();
  });

  it('shows the tooltip on keyboard focus', async () => {
    const { container } = renderOverlay([
      { action: 'remove', node_id: 'n1', name: 'FullTextPort', reason: 'superseded' },
    ]);
    await fireEvent.focus(container.querySelector('[data-testid="ghost-node"]'));
    expect(container.querySelector('[data-testid="ghost-tooltip"]').textContent).toContain('superseded');
  });

  it('lists the predicted changes in the tooltip', async () => {
    const { container } = renderOverlay([
      {
        action: 'modify',
        node_id: 'n2',
        name: 'SearchQuery',
        reason: 'r',
        changes: ['New field: mode (FullText | Semantic)'],
      },
    ]);
    await fireEvent.mouseEnter(container.querySelector('[data-testid="ghost-node"]'));
    expect(container.querySelector('[data-testid="ghost-tooltip"]').textContent)
      .toContain('New field: mode');
  });
});

// ── Preview modes (§3: pulsing while previewing, solid when confirmed) ────────

describe('GhostOverlay — preview modes', () => {
  it('predicted mode: no pulse, no confirm class', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'A' }], { mode: 'predicted' });
    const node = container.querySelector('[data-testid="ghost-node"]');
    expect(node.classList.contains('ghost-previewing')).toBe(false);
    expect(node.classList.contains('ghost-confirmed')).toBe(false);
  });

  it('previewing mode: ghosts pulse (agent running)', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'A' }], { mode: 'previewing' });
    const node = container.querySelector('[data-testid="ghost-node"]');
    expect(node.classList.contains('ghost-previewing')).toBe(true);
    // The dotted prediction outline is still dotted while pulsing.
    const dash = node.querySelector('.ghost-border').getAttribute('stroke-dasharray') ?? '';
    expect(dash).not.toBe('none');
  });

  it('confirmed mode: ghosts marked confirmed after real implementation', () => {
    const { container } = renderOverlay([{ action: 'add', name: 'A' }], { mode: 'confirmed' });
    const node = container.querySelector('[data-testid="ghost-node"]');
    expect(node.classList.contains('ghost-confirmed')).toBe(true);
    expect(node.classList.contains('ghost-previewing')).toBe(false);
  });
});

// ── Non-navigation (§3: ghosts are hoverable but never navigate) ──────────────

describe('GhostOverlay — interaction containment', () => {
  it('ghost nodes carry no click handler (no navigation from a ghost)', () => {
    const { container } = renderOverlay([
      { action: 'add', name: 'A', node_id: 'n1' },
    ]);
    const node = container.querySelector('[data-testid="ghost-node"]');
    expect(() => node.dispatchEvent(new MouseEvent('click', { bubbles: true }))).not.toThrow();
    // No onclick attribute is ever wired on the ghost layer.
    expect(node.getAttribute('onclick')).toBeNull();
  });
});
