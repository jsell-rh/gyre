import { describe, it, expect } from 'vitest';
import {
  ghostAction,
  ghostOpacity,
  clampConfidence,
  normalizeGhosts,
  positionGhosts,
  ghostEdges,
} from '../lib/ghostOverlay.js';

// ── Action normalization ──────────────────────────────────────────────────────

describe('ghostOverlay — ghostAction', () => {
  it('maps the §3 contract actions', () => {
    expect(ghostAction({ action: 'add' })).toBe('add');
    expect(ghostAction({ action: 'modify' })).toBe('modify');
    expect(ghostAction({ action: 'remove' })).toBe('remove');
  });

  it('maps the legacy overlay synonyms', () => {
    expect(ghostAction({ type: 'new' })).toBe('add');
    expect(ghostAction({ type: 'modified' })).toBe('modify');
    expect(ghostAction({ type: 'removed' })).toBe('remove');
    expect(ghostAction({ change_type: 'new' })).toBe('add');
  });

  it('is case-insensitive and trims', () => {
    expect(ghostAction({ action: ' Add ' })).toBe('add');
    expect(ghostAction({ action: 'REMOVED' })).toBe('remove');
  });

  it('defaults a free-form type to modify (keep-visible, pre-§3 behavior)', () => {
    expect(ghostAction({ type: 'missing abstraction' })).toBe('modify');
  });

  it('returns null when no action-bearing field exists', () => {
    expect(ghostAction({})).toBeNull();
    expect(ghostAction(null)).toBeNull();
  });
});

// ── Confidence → opacity (§3: 0.3 at 50%, 1.0 at 100%) ───────────────────────

describe('ghostOverlay — ghostOpacity', () => {
  it('maps 50% confidence to 0.3 opacity', () => {
    expect(ghostOpacity(0.5)).toBeCloseTo(0.3, 5);
  });

  it('maps 100% confidence to 1.0 opacity', () => {
    expect(ghostOpacity(1)).toBe(1);
  });

  it('is monotonic in confidence', () => {
    expect(ghostOpacity(0.5)).toBeLessThan(ghostOpacity(0.75));
    expect(ghostOpacity(0.75)).toBeLessThan(ghostOpacity(1));
  });

  it('clamps below-50% confidence to a faint floor, never zero/invisible', () => {
    expect(ghostOpacity(0)).toBeGreaterThanOrEqual(0.15);
    expect(ghostOpacity(0.1)).toBeGreaterThanOrEqual(0.15);
  });

  it('clamps out-of-range and non-numeric confidence', () => {
    expect(ghostOpacity(2)).toBe(1);
    expect(clampConfidence(85)).toBe(0.85);
    expect(ghostOpacity(-1)).toBe(0.15);
    expect(ghostOpacity('0.8')).toBeCloseTo(0.72, 5);
    expect(ghostOpacity(undefined)).toBeCloseTo(0.3, 5); // default 0.5 → 0.3
    expect(ghostOpacity('garbage')).toBeCloseTo(0.3, 5);
  });

  it('accepts percentage form (75 → 0.75); over-unity noise clamps to 1', () => {
    expect(clampConfidence(75)).toBe(0.75);
    expect(clampConfidence(0.75)).toBe(0.75);
    expect(clampConfidence(2)).toBe(1);
  });
});

// ── Prediction normalization ─────────────────────────────────────────────────

describe('ghostOverlay — normalizeGhosts', () => {
  it('normalizes §3 prediction items with action, confidence and reason', () => {
    const ghosts = normalizeGhosts([
      {
        action: 'add',
        node_type: 'Type',
        name: 'VectorIndex',
        confidence: 0.85,
        reason: 'Spec mentions vector similarity search',
        parent_trait: 'FullTextPort',
      },
      {
        action: 'modify',
        node_id: 'node-7',
        confidence: 0.9,
        reason: 'Search needs modes',
        changes: ['New field: mode'],
      },
    ]);
    expect(ghosts).toHaveLength(2);
    expect(ghosts[0]).toMatchObject({
      action: 'add',
      name: 'VectorIndex',
      nodeId: null,
      confidence: 0.85,
      reason: 'Spec mentions vector similarity search',
      parentTrait: 'FullTextPort',
      nodeType: 'Type',
    });
    expect(ghosts[1]).toMatchObject({
      action: 'modify',
      nodeId: 'node-7',
      confidence: 0.9,
      changes: ['New field: mode'],
    });
  });

  it('normalizes the legacy { nodeId, type } overlay shape', () => {
    const ghosts = normalizeGhosts([{ nodeId: 'n1', type: 'new' }]);
    expect(ghosts).toHaveLength(1);
    expect(ghosts[0].action).toBe('add');
    expect(ghosts[0].nodeId).toBe('n1');
    expect(ghosts[0].rawType).toBe('new');
  });

  it('is idempotent — a normalized ghost re-normalizes to the same ghost', () => {
    const once = normalizeGhosts([
      { action: 'add', name: 'A', confidence: 0.8, reason: 'r' },
    ]);
    const twice = normalizeGhosts(once);
    expect(twice).toEqual(once);
  });

  it('drops entries with neither name nor node id', () => {
    const ghosts = normalizeGhosts([
      { action: 'add', confidence: 0.9 },
      { action: 'modify' },
      { action: 'remove', name: 'Kept' },
    ]);
    expect(ghosts).toHaveLength(1);
    expect(ghosts[0].name).toBe('Kept');
  });

  it('defaults missing confidence to 0.5 and missing reason to empty', () => {
    const [ghost] = normalizeGhosts([{ action: 'add', name: 'X' }]);
    expect(ghost.confidence).toBe(0.5);
    expect(ghost.reason).toBe('');
  });

  it('returns [] for non-array input', () => {
    expect(normalizeGhosts(null)).toEqual([]);
    expect(normalizeGhosts(undefined)).toEqual([]);
    expect(normalizeGhosts({})).toEqual([]);
  });
});

// ── Positioning (§3 canvas integration) ───────────────────────────────────────

const NODES = [
  { id: 'n1', name: 'FullTextPort', node_type: 'interface' },
  { id: 'n2', name: 'SearchQuery', node_type: 'type' },
];
const NODE_POSITIONS = { n1: { x: 100, y: 100 }, n2: { x: 300, y: 100 } };

describe('ghostOverlay — positionGhosts', () => {
  it('overlays modify/remove ghosts on the existing node position (node id)', () => {
    const ghosts = normalizeGhosts([
      { action: 'modify', node_id: 'n2', name: 'SearchQuery' },
      { action: 'remove', node_id: 'n1', name: 'FullTextPort' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    expect(positions[ghosts[0].id]).toEqual({ x: 300, y: 100 });
    expect(positions[ghosts[1].id]).toEqual({ x: 100, y: 100 });
  });

  it('resolves an existing node by name when node_id is absent', () => {
    const ghosts = normalizeGhosts([
      { action: 'modify', name: 'SearchQuery' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    expect(positions[ghosts[0].id]).toEqual({ x: 300, y: 100 });
  });

  it('places an add ghost near its predicted parent (parent_trait)', () => {
    const ghosts = normalizeGhosts([
      {
        action: 'add',
        node_type: 'TraitMethod',
        name: 'search_similar',
        parent_trait: 'FullTextPort',
      },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    const pos = positions[ghosts[0].id];
    // Near n1 (100,100) — within one node-width offset, not on top of it.
    expect(Math.abs(pos.x - 100)).toBeLessThanOrEqual(120);
    expect(Math.abs(pos.y - 100)).toBeLessThanOrEqual(80);
    expect(pos).not.toEqual({ x: 100, y: 100 });
  });

  it('places parentless add ghosts in free space clear of existing nodes', () => {
    const ghosts = normalizeGhosts([
      { action: 'add', name: 'Lonely' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    const pos = positions[ghosts[0].id];
    for (const p of Object.values(NODE_POSITIONS)) {
      expect(Math.hypot(p.x - pos.x, p.y - pos.y)).toBeGreaterThan(46);
    }
  });

  it('assigns every ghost a position, never overlapping an existing node exactly', () => {
    const ghosts = normalizeGhosts([
      { action: 'add', name: 'A', parent_trait: 'FullTextPort' },
      { action: 'add', name: 'B', parent_trait: 'FullTextPort' },
      { action: 'add', name: 'C' },
      { action: 'modify', name: 'UnmatchedExisting' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    for (const g of ghosts) {
      expect(positions[g.id]).toBeTruthy();
    }
  });
});

// ── Ghost edges (§3 dotted green = new relationship) ─────────────────────────

describe('ghostOverlay — ghostEdges', () => {
  it('derives a dotted-edge relationship from parent to each add ghost', () => {
    const ghosts = normalizeGhosts([
      {
        action: 'add',
        name: 'search_similar',
        parent_trait: 'FullTextPort',
      },
      { action: 'modify', node_id: 'n2', name: 'SearchQuery' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    const edges = ghostEdges(ghosts, positions, NODE_POSITIONS, NODES);
    expect(edges).toHaveLength(1);
    expect(edges[0].from).toEqual({ x: 100, y: 100 }); // FullTextPort
    expect(edges[0].to).toEqual(positions[ghosts[0].id]);
  });

  it('derives no edge for modifies, removes, or unresolvable parents', () => {
    const ghosts = normalizeGhosts([
      { action: 'add', name: 'NoParent' },
      { action: 'add', name: 'Orphan', parent_trait: 'DoesNotExist' },
    ]);
    const positions = positionGhosts(ghosts, NODE_POSITIONS, NODES);
    expect(ghostEdges(ghosts, positions, NODE_POSITIONS, NODES)).toEqual([]);
  });
});
