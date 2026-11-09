/**
 * Rendering density policy tests (ui-layout.md §10 Canvas Rendering).
 *
 * These pin the spec'd thresholds:
 *   - ≤500 nodes → full render
 *   - >500 nodes → public-only with hidden count (banner + [Show All])
 *   - >1000 nodes *even after* the public filter → list fallback
 *   - [Show All] override disables both filters
 */
import { describe, it, expect } from 'vitest';
import {
  applyDensityPolicy,
  isPublicNode,
  PUBLIC_FILTER_THRESHOLD,
  LIST_FALLBACK_THRESHOLD,
} from '../lib/canvas-density.js';

function node(id, visibility) {
  return { id, name: `n${id}`, visibility };
}

describe('isPublicNode', () => {
  it('treats public and missing visibility as public', () => {
    expect(isPublicNode({ visibility: 'public' })).toBe(true);
    expect(isPublicNode({})).toBe(true);
    expect(isPublicNode(node('x', undefined))).toBe(true);
  });

  it('treats private, pub_crate and internal as not public', () => {
    expect(isPublicNode({ visibility: 'private' })).toBe(false);
    expect(isPublicNode({ visibility: 'pub_crate' })).toBe(false);
    expect(isPublicNode({ visibility: 'internal' })).toBe(false);
    expect(isPublicNode(null)).toBe(false);
  });
});

describe('applyDensityPolicy', () => {
  it('exports the spec thresholds (500 / 1000)', () => {
    expect(PUBLIC_FILTER_THRESHOLD).toBe(500);
    expect(LIST_FALLBACK_THRESHOLD).toBe(1000);
  });

  it('renders everything at exactly the threshold (boundary: >500 filters)', () => {
    const nodes = Array.from({ length: 500 }, (_, i) => node(String(i), 'private'));
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('full');
    expect(r.nodes).toHaveLength(500);
    expect(r.hiddenCount).toBe(500); // counted, but not applied at ≤500
  });

  it('filters to public-only at 501 nodes', () => {
    const nodes = [
      ...Array.from({ length: 300 }, (_, i) => node(`p${i}`, 'public')),
      ...Array.from({ length: 201 }, (_, i) => node(`x${i}`, 'private')),
    ];
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('public-only');
    expect(r.nodes).toHaveLength(300);
    expect(r.nodes.every(isPublicNode)).toBe(true);
    expect(r.hiddenCount).toBe(201);
    expect(r.total).toBe(501);
  });

  it('stays public-only when total >1000 but public count ≤1000 (filter was enough)', () => {
    const nodes = [
      ...Array.from({ length: 1000 }, (_, i) => node(`p${i}`, 'public')),
      ...Array.from({ length: 200 }, (_, i) => node(`x${i}`, 'private')),
    ];
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('public-only');
    expect(r.nodes).toHaveLength(1000);
    expect(r.hiddenCount).toBe(200);
  });

  it('switches to list when >1000 nodes remain even after the public filter', () => {
    const nodes = Array.from({ length: 1500 }, (_, i) => node(String(i), 'public'));
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('list');
    // The virtualized list renders every node — the threshold protects SVG,
    // not the list.
    expect(r.nodes).toHaveLength(1500);
  });

  it('switches to list for mixed visibility when public count alone exceeds 1000', () => {
    const nodes = [
      ...Array.from({ length: 1001 }, (_, i) => node(`p${i}`, 'public')),
      ...Array.from({ length: 50 }, (_, i) => node(`x${i}`, 'private')),
    ];
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('list');
  });

  it('showAll override renders everything regardless of count', () => {
    const nodes = [
      ...Array.from({ length: 2000 }, (_, i) => node(`p${i}`, 'public')),
      ...Array.from({ length: 500 }, (_, i) => node(`x${i}`, 'private')),
    ];
    const r = applyDensityPolicy(nodes, { showAll: true });
    expect(r.mode).toBe('full');
    expect(r.nodes).toHaveLength(2500);
  });

  it('over threshold with nothing to hide stays full', () => {
    const nodes = Array.from({ length: 600 }, (_, i) => node(String(i), 'public'));
    const r = applyDensityPolicy(nodes);
    expect(r.mode).toBe('full');
    expect(r.nodes).toHaveLength(600);
    expect(r.hiddenCount).toBe(0);
  });

  it('handles empty and non-array input', () => {
    expect(applyDensityPolicy([])).toMatchObject({ mode: 'full', total: 0 });
    expect(applyDensityPolicy(/** @type {any} */ (null))).toMatchObject({ mode: 'full', total: 0 });
  });
});
