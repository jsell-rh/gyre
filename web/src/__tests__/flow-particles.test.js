import { describe, it, expect } from 'vitest';
import {
  SUCCESS_PARTICLE_COLOR,
  ERROR_PARTICLE_COLOR,
  TRAIL_FADE_MS,
  TRAIL_LENGTH_PX,
  rootSpanCases,
  spanRootMap,
  testCaseColor,
  particlesAtTime,
  nextSpanStart,
  spansTraversingEdge,
  nearestParticleToNode,
  trailSegments,
} from '../lib/flow-particles.js';

// Pure logic for the flow-layout particle system (ui-layout.md §4 Flow
// Layout, human-system-interface.md §3a). These tests kill real defects:
// the span→particle mapping, the test-case filter, step-through targets,
// and the trail decay/length contract are all spec-pinned.

// Two test cases (root spans) over a three-node call graph:
//   root s1 'test_create' (node fn1) → s2 (fn1→fn2, [1000,5000]),
//                                          s3 (fn1→fn3, [2000,5000], error)
//   root s4 'test_read'   (node fn1) → s5 (fn1→fn2, [6000,9000])
const SPANS = [
  { span_id: 's1', parent_span_id: null, operation_name: 'test_create', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn1' },
  { span_id: 's2', parent_span_id: 's1', operation_name: 'create_user', start_time: 1000, duration_us: 4000, status: 'ok', graph_node_id: 'fn2' },
  { span_id: 's3', parent_span_id: 's1', operation_name: 'validate', start_time: 2000, duration_us: 3000, status: 'error', graph_node_id: 'fn3' },
  { span_id: 's4', parent_span_id: null, operation_name: 'test_read', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn1' },
  { span_id: 's5', parent_span_id: 's4', operation_name: 'get_user', start_time: 6000, duration_us: 3000, status: 'ok', graph_node_id: 'fn2' },
];

describe('flow-particles — rootSpanCases', () => {
  it('returns root spans (test cases) in start-time order with names', () => {
    const cases = rootSpanCases(SPANS);
    expect(cases.map((c) => c.operationName)).toEqual(['test_create', 'test_read']);
    expect(cases.map((c) => c.index)).toEqual([0, 1]);
    expect(cases.every((c) => typeof c.spanId === 'string')).toBe(true);
  });

  it('ignores child spans and handles empty input', () => {
    expect(rootSpanCases([])).toEqual([]);
    expect(rootSpanCases(null)).toEqual([]);
    const onlyChild = [{ span_id: 'c', parent_span_id: 'p', operation_name: 'x', start_time: 0 }];
    expect(rootSpanCases(onlyChild)).toEqual([]);
  });
});

describe('flow-particles — spanRootMap', () => {
  it('maps every span to its root span id', () => {
    const m = spanRootMap(SPANS);
    expect(m.get('s1')).toBe('s1');
    expect(m.get('s2')).toBe('s1');
    expect(m.get('s3')).toBe('s1');
    expect(m.get('s4')).toBe('s4');
    expect(m.get('s5')).toBe('s4');
  });

  it('treats a span with an unknown parent as its own root (no silent re-targeting)', () => {
    const m = spanRootMap([
      { span_id: 'a', parent_span_id: 'ghost', operation_name: 'a', start_time: 0 },
      { span_id: 'b', parent_span_id: 'a', operation_name: 'b', start_time: 1 },
    ]);
    expect(m.get('a')).toBe('a');
    expect(m.get('b')).toBe('a');
  });

  it('survives a parent cycle without hanging', () => {
    const m = spanRootMap([
      { span_id: 'x', parent_span_id: 'y', operation_name: 'x', start_time: 0 },
      { span_id: 'y', parent_span_id: 'x', operation_name: 'y', start_time: 0 },
    ]);
    // No infinite loop; each span resolves to some root (itself or the other).
    expect(['x', 'y']).toContain(m.get('x'));
    expect(['x', 'y']).toContain(m.get('y'));
  });
});

describe('flow-particles — testCaseColor', () => {
  it('case 0 renders the exact spec base color', () => {
    expect(testCaseColor(SUCCESS_PARTICLE_COLOR, 0)).toBe(SUCCESS_PARTICLE_COLOR);
    expect(testCaseColor(ERROR_PARTICLE_COLOR, 0)).toBe(ERROR_PARTICLE_COLOR);
  });

  it('later cases hue-shift within the palette, distinctly and validly', () => {
    const c1 = testCaseColor(SUCCESS_PARTICLE_COLOR, 1);
    const c2 = testCaseColor(SUCCESS_PARTICLE_COLOR, 2);
    expect(c1).not.toBe(SUCCESS_PARTICLE_COLOR);
    expect(c2).not.toBe(SUCCESS_PARTICLE_COLOR);
    expect(c1).not.toBe(c2);
    for (const c of [c1, c2]) {
      expect(c).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it('passes through invalid input unchanged', () => {
    expect(testCaseColor('not-a-color', 3)).toBe('not-a-color');
  });
});

describe('flow-particles — particlesAtTime', () => {
  it('emits particles only for spans active at the timestamp, with progress', () => {
    // t=3000: s2 [1000,5000] at progress 0.5, s3 [2000,5000] at 1/3.
    const ps = particlesAtTime(SPANS, 3000);
    expect(ps).toHaveLength(2);
    const s2 = ps.find((p) => p.span.span_id === 's2');
    const s3 = ps.find((p) => p.span.span_id === 's3');
    expect(s2.progress).toBeCloseTo(0.5, 5);
    expect(s3.progress).toBeCloseTo(1 / 3, 5);
    // Edge follows the span tree parent→child node mapping
    expect(s2.fromId).toBe('fn1');
    expect(s2.toId).toBe('fn2');
  });

  it('excludes spans before their start and after their end', () => {
    expect(particlesAtTime(SPANS, 500)).toEqual([]);
    expect(particlesAtTime(SPANS, 9500)).toEqual([]);
    // Exactly at a boundary is still active (inclusive window)
    expect(particlesAtTime(SPANS, 1000)).toHaveLength(1);
  });

  it('colors particles by span status (blue success, red error)', () => {
    const ps = particlesAtTime(SPANS, 3000);
    expect(ps.find((p) => p.span.span_id === 's2').color).toBe(SUCCESS_PARTICLE_COLOR);
    expect(ps.find((p) => p.span.span_id === 's3').color).toBe(ERROR_PARTICLE_COLOR);
  });

  it('hue-shifts particles from later test cases within the palette', () => {
    // t=7000: only s5 (root s4 = case index 1) is active.
    const ps = particlesAtTime(SPANS, 7000);
    expect(ps).toHaveLength(1);
    expect(ps[0].color).toBe(testCaseColor(SUCCESS_PARTICLE_COLOR, 1));
    expect(ps[0].rootSpanId).toBe('s4');
    expect(ps[0].caseIndex).toBe(1);
  });

  it('testFilter keeps only the selected test case', () => {
    const rootOf = spanRootMap(SPANS);
    // t=3000: case s1 has s2+s3 active; case s4 has nothing.
    expect(particlesAtTime(SPANS, 3000, { testFilter: 's1', rootOf })).toHaveLength(2);
    expect(particlesAtTime(SPANS, 3000, { testFilter: 's4', rootOf })).toEqual([]);
    // t=7000: case s4 has s5 active.
    expect(particlesAtTime(SPANS, 7000, { testFilter: 's4', rootOf })).toHaveLength(1);
  });

  it('visibleNodeIds scopes particles to laid-out nodes (drill level)', () => {
    // fn3 not laid out → s3 filtered even though active.
    const visible = new Set(['fn1', 'fn2']);
    const ps = particlesAtTime(SPANS, 3000, { visibleNodeIds: visible });
    expect(ps.map((p) => p.span.span_id)).toEqual(['s2']);
    // An endpoint's parent not laid out filters the particle too.
    const visible2 = new Set(['fn2', 'fn3']);
    expect(particlesAtTime(SPANS, 3000, { visibleNodeIds: visible2 })).toEqual([]);
  });

  it('clamps progress into [0,1] and handles zero-duration spans', () => {
    const spans = [
      { span_id: 'r', parent_span_id: null, operation_name: 'r', start_time: 0, duration_us: 100, status: 'ok', graph_node_id: 'a' },
      { span_id: 'z', parent_span_id: 'r', operation_name: 'z', start_time: 50, duration_us: 0, status: 'ok', graph_node_id: 'b' },
    ];
    const ps = particlesAtTime(spans, 50);
    expect(ps).toHaveLength(1);
    expect(ps[0].progress).toBeGreaterThanOrEqual(0);
    expect(ps[0].progress).toBeLessThanOrEqual(1);
  });
});

describe('flow-particles — nextSpanStart (step-through)', () => {
  it('returns the earliest span start strictly after the current time', () => {
    expect(nextSpanStart(SPANS, 0)).toBe(1000);
    expect(nextSpanStart(SPANS, 1000)).toBe(2000);
    expect(nextSpanStart(SPANS, 2500)).toBe(6000);
  });

  it('returns null at the end of the trace', () => {
    expect(nextSpanStart(SPANS, 9000)).toBeNull();
  });

  it('respects the test-case filter', () => {
    const rootOf = spanRootMap(SPANS);
    // Case s4 spans start at 0 (root) and 6000 (s5) only.
    expect(nextSpanStart(SPANS, 0, rootOf, 's4')).toBe(6000);
    expect(nextSpanStart(SPANS, 1000, rootOf, 's4')).toBe(6000);
    expect(nextSpanStart(SPANS, 7000, rootOf, 's4')).toBeNull();
  });
});

describe('flow-particles — spansTraversingEdge', () => {
  it('lists spans whose parent→child pair maps onto the edge, with timings', () => {
    const onFn1Fn2 = spansTraversingEdge(SPANS, 'fn1', 'fn2');
    expect(onFn1Fn2.map((s) => s.span_id)).toEqual(['s2', 's5']);
    expect(onFn1Fn2[0]).toMatchObject({
      span_id: 's2',
      operation_name: 'create_user',
      status: 'ok',
      start_time: 1000,
      duration_us: 4000,
    });
    expect(spansTraversingEdge(SPANS, 'fn1', 'fn3').map((s) => s.span_id)).toEqual(['s3']);
  });

  it('returns empty for unrelated or reversed node pairs', () => {
    expect(spansTraversingEdge(SPANS, 'fn2', 'fn1')).toEqual([]);
    expect(spansTraversingEdge(SPANS, 'fn2', 'fn3')).toEqual([]);
    expect(spansTraversingEdge(SPANS, 'fnX', 'fnY')).toEqual([]);
  });
});

describe('flow-particles — nearestParticleToNode', () => {
  it('prefers the particle closest to the node in progress units', () => {
    const particles = [
      { fromId: 'fn1', toId: 'fn2', progress: 0.1 },  // leaving fn1: dist 0.1
      { fromId: 'fn9', toId: 'fn1', progress: 0.2 },  // heading to fn1: dist 0.8
    ];
    expect(nearestParticleToNode(particles, 'fn1')).toBe(particles[0]);
    expect(nearestParticleToNode(particles, 'fn2')).toBe(particles[0]);
  });

  it('returns null when no particle touches the node', () => {
    expect(nearestParticleToNode([{ fromId: 'a', toId: 'b', progress: 0.5 }], 'zzz')).toBeNull();
    expect(nearestParticleToNode([], 'a')).toBeNull();
    expect(nearestParticleToNode(null, 'a')).toBeNull();
  });
});

describe('flow-particles — trailSegments', () => {
  it('emits no segments for a stationary particle (paused/scrubbed)', () => {
    const now = 1000;
    const history = [
      { x: 100, y: 100, t: now - 60 },
      { x: 100, y: 100, t: now - 40 },
      { x: 100, y: 100, t: now },
    ];
    expect(trailSegments(history, now)).toEqual([]);
  });

  it('fades: segment alpha ramps toward the head, decays with age', () => {
    const now = 1000;
    // Head (newest) is fresh → alpha 1; the older segment decays.
    const history = [
      { x: 0, y: 0, t: now - 150 },
      { x: 5, y: 0, t: now - 100 },
      { x: 10, y: 0, t: now },
    ];
    const segs = trailSegments(history, now);
    expect(segs.length).toBeGreaterThan(0);
    for (const s of segs) {
      expect(s.alpha).toBeGreaterThan(0);
      expect(s.alpha).toBeLessThanOrEqual(1);
    }
    // The newest segment is the most opaque
    const alphas = segs.map((s) => s.alpha);
    expect(alphas[alphas.length - 1]).toBeCloseTo(1, 5);
    expect(alphas[alphas.length - 1]).toBeGreaterThanOrEqual(alphas[0]);
  });

  it('caps the trail at TRAIL_LENGTH_PX even for fast movement', () => {
    const now = 1000;
    const history = [
      { x: 0, y: 0, t: now - 150 },
      { x: 50, y: 0, t: now - 100 },
      { x: 100, y: 0, t: now - 50 },
      { x: 150, y: 0, t: now },
    ];
    const segs = trailSegments(history, now);
    let total = 0;
    for (const s of segs) {
      total += Math.hypot(s.x2 - s.x1, s.y2 - s.y1);
    }
    expect(total).toBeLessThanOrEqual(TRAIL_LENGTH_PX + 1e-9);
  });

  it('prunes points older than TRAIL_FADE_MS in place', () => {
    const now = 10000;
    const history = [
      { x: 0, y: 0, t: now - TRAIL_FADE_MS - 100 },
      { x: 5, y: 0, t: now - 50 },
      { x: 10, y: 0, t: now },
    ];
    trailSegments(history, now);
    expect(history[0].t).toBe(now - 50);
  });

  it('empty history yields nothing', () => {
    expect(trailSegments([], 0)).toEqual([]);
    expect(trailSegments(null, 0)).toEqual([]);
  });
});
