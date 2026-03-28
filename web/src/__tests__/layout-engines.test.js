import { describe, it, expect } from 'vitest';
import { columnLayout, computeLayout } from '../lib/layout-engines.js';

describe('columnLayout', () => {
  it('returns empty object for empty node list', () => {
    expect(columnLayout([])).toEqual({});
  });

  it('assigns positions to all nodes', () => {
    const nodes = [
      { id: 'a', node_type: 'function' },
      { id: 'b', node_type: 'function' },
      { id: 'c', node_type: 'module' },
    ];
    const positions = columnLayout(nodes);
    expect(Object.keys(positions)).toHaveLength(3);
    expect(positions.a).toHaveProperty('x');
    expect(positions.a).toHaveProperty('y');
    expect(positions.b).toHaveProperty('x');
    expect(positions.c).toHaveProperty('x');
  });

  it('groups nodes of the same type in the same column (same x)', () => {
    const nodes = [
      { id: 'f1', node_type: 'function' },
      { id: 'f2', node_type: 'function' },
    ];
    const positions = columnLayout(nodes);
    expect(positions.f1.x).toBe(positions.f2.x);
  });

  it('places different node types in different columns (different x)', () => {
    const nodes = [
      { id: 'f1', node_type: 'function' },
      { id: 'm1', node_type: 'module' },
    ];
    const positions = columnLayout(nodes);
    expect(positions.f1.x).not.toBe(positions.m1.x);
  });

  it('spaces nodes of the same type vertically (increasing y)', () => {
    const nodes = [
      { id: 'a', node_type: 'type' },
      { id: 'b', node_type: 'type' },
      { id: 'c', node_type: 'type' },
    ];
    const positions = columnLayout(nodes);
    expect(positions.b.y).toBeGreaterThan(positions.a.y);
    expect(positions.c.y).toBeGreaterThan(positions.b.y);
  });

  it('orders columns according to TYPE_ORDER (module before function)', () => {
    const nodes = [
      { id: 'fn', node_type: 'function' },
      { id: 'mod', node_type: 'module' },
    ];
    const positions = columnLayout(nodes);
    // module is earlier in TYPE_ORDER, so it should have a smaller x
    expect(positions.mod.x).toBeLessThan(positions.fn.x);
  });

  it('handles nodes with missing node_type gracefully', () => {
    const nodes = [
      { id: 'x' },
      { id: 'y', node_type: 'function' },
    ];
    const positions = columnLayout(nodes);
    expect(Object.keys(positions)).toHaveLength(2);
    expect(positions.x).toHaveProperty('x');
    expect(positions.x).toHaveProperty('y');
  });
});

describe('computeLayout', () => {
  it('defaults to columnLayout for unknown engine', async () => {
    const nodes = [{ id: 'a', node_type: 'type' }];
    const result = await computeLayout('column', nodes, []);
    expect(result.a).toHaveProperty('x');
    expect(result.a).toHaveProperty('y');
  });

  it('returns empty object for empty nodes regardless of engine', async () => {
    const result = await computeLayout('column', [], []);
    expect(result).toEqual({});
  });

  it('falls back to column layout for unrecognized engine names', async () => {
    const nodes = [{ id: 'n1', node_type: 'module' }];
    const result = await computeLayout('nonexistent', nodes, []);
    expect(result.n1).toHaveProperty('x');
    expect(result.n1).toHaveProperty('y');
  });
});
