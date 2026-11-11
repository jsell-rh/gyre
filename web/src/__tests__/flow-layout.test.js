/**
 * flow-layout.js — unit tests for the flow-layout node positioning
 * (ui-layout.md §4 Flow Layout): depth columns along directed edges,
 * contains-edges excluded from depth, deterministic ordering, and the
 * edge path shapes.
 */
import { describe, it, expect } from 'vitest';
import { layeredFlowLayout, flowEdgePath, FLOW_NODE_W, FLOW_NODE_H, FLOW_ROW_GAP } from '../lib/flow-layout.js';

const N = (id, name = id) => ({ id, name, node_type: 'function' });
const E = (id, source_id, target_id, edge_type = 'calls') => ({ id, source_id, target_id, edge_type });

describe('layeredFlowLayout', () => {
  it('places nodes in depth columns left→right along directed edges', () => {
    // a → b → c, plus a → c
    const { items } = layeredFlowLayout(
      [N('a'), N('b'), N('c')],
      [E('e1', 'a', 'b'), E('e2', 'b', 'c'), E('e3', 'a', 'c')]
    );
    expect(items.get('a').depth).toBe(0);
    // c is reachable via the longer path a→b→c, so it lands in column 2 —
    // longest-path depth keeps left→right edges acyclic in drawing.
    expect(items.get('b').depth).toBe(1);
    expect(items.get('c').depth).toBe(2);
    expect(items.get('c').x).toBeGreaterThan(items.get('b').x);
    expect(items.get('b').x).toBeGreaterThan(items.get('a').x);
  });

  it('does not let contains edges drive depth (containment is not data flow)', () => {
    const { items } = layeredFlowLayout(
      [N('pkg'), N('fn')],
      [E('c1', 'pkg', 'fn', 'contains')]
    );
    expect(items.get('pkg').depth).toBe(0);
    expect(items.get('fn').depth).toBe(0);
  });

  it('stacks same-column nodes and centers them vertically', () => {
    const { items } = layeredFlowLayout(
      [N('root'), N('b', 'bravo'), N('a', 'alpha'), N('c', 'charlie')],
      [E('e1', 'root', 'b'), E('e2', 'root', 'a'), E('e3', 'root', 'c')]
    );
    const col = ['a', 'b', 'c'].map((id) => items.get(id));
    // Same column, distinct rows, symmetric around 0.
    expect(new Set(col.map((i) => i.y)).size).toBe(3);
    expect(col.reduce((s, i) => s + i.y, 0)).toBe(0);
    // Deterministic order: name-sorted within the column.
    expect(col[0].y).toBeLessThan(col[1].y);
    expect(col[1].y).toBeLessThan(col[2].y);
  });

  it('is cycle-safe (a cycle terminates with bounded depths instead of inflating)', () => {
    const { items } = layeredFlowLayout(
      [N('a'), N('b')],
      [E('e1', 'a', 'b'), E('e2', 'b', 'a')]
    );
    // A pure 2-cycle: the back-edge contributes depth 0, so b settles at 1
    // and a at 2 — bounded and deterministic, where a naive relaxation loop
    // would inflate depths toward the node count on every iteration.
    expect(items.get('a').depth).toBe(2);
    expect(items.get('b').depth).toBe(1);
  });

  it('gives every node a fixed cell size and ignores edges to missing nodes', () => {
    const { items, list } = layeredFlowLayout(
      [N('a'), N('b')],
      [E('e1', 'a', 'ghost'), E('e2', 'a', 'b')]
    );
    expect(items.size).toBe(2);
    expect(list.length).toBe(2);
    for (const it of list) {
      expect(it.w).toBe(FLOW_NODE_W);
      expect(it.h).toBe(FLOW_NODE_H);
    }
  });

  it('returns an empty layout for an empty node set', () => {
    const { items, list } = layeredFlowLayout([], [E('e1', 'a', 'b')]);
    expect(items.size).toBe(0);
    expect(list).toEqual([]);
  });
});

describe('flowEdgePath', () => {
  const s = { x: 0, y: 0, w: 100, h: 30 };
  const t = { x: 300, y: 60, w: 100, h: 30 };

  it('connects source right edge to target left edge for forward edges', () => {
    const d = flowEdgePath(s, t);
    expect(d.startsWith('M 50 0')).toBe(true); // source right-center
    expect(d.endsWith('250 60')).toBe(true); // target left-center
    expect(d).toContain('C'); // cubic bezier
  });

  it('routes backward edges as an under-loop instead of through nodes', () => {
    const d = flowEdgePath(t, s); // target left of source
    expect(d.startsWith('M 350 60')).toBe(true);
    // The loop dips below both node bodies.
    const loopY = Math.max(t.y + t.h / 2, s.y + s.h / 2) + FLOW_ROW_GAP;
    expect(d).toContain(String(loopY));
    expect((d.match(/C/g) || []).length).toBe(2); // two cubic segments
  });
});
