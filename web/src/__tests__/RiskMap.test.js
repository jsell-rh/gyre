import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import {
  RISK_MAP_METRICS,
  RISK_MAP_METRIC_KEYS,
  riskHeatColor,
  riskMetricDef,
  buildRiskHeatMap,
  detectRiskAnomalies,
} from '../lib/risk-map.js';

// system-explorer.md §7 Risk Map — heat map overlay on the architecture
// canvas. Unit tests pin the pure coloring/anomaly rules in risk-map.js;
// component tests pin the overlay wiring in ExplorerCanvas (activation via
// the Evaluative lens, metric selector, legend, clean deactivation).

// ── Unit: heat scale ──────────────────────────────────────────────────────

describe('risk-map — heat color scale', () => {
  it('runs cool blue → yellow → hot red across t ∈ [0,1]', () => {
    expect(riskHeatColor(0)).toBe('hsl(210, 70%, 42%)');   // cool blue
    expect(riskHeatColor(0.5)).toBe('hsl(55, 80%, 50%)');  // yellow
    expect(riskHeatColor(1)).toBe('hsl(0, 90%, 42%)');     // hot red
  });

  it('clamps out-of-range and non-finite t', () => {
    expect(riskHeatColor(2)).toBe(riskHeatColor(1));
    expect(riskHeatColor(-1)).toBe(riskHeatColor(0));
    expect(riskHeatColor(NaN)).toBe(riskHeatColor(0));
    expect(riskHeatColor(undefined)).toBe(riskHeatColor(0));
  });

  it('exposes the five spec metrics with labels', () => {
    expect(RISK_MAP_METRIC_KEYS).toEqual(['churn', 'coupling', 'complexity', 'spec_coverage', 'test_coverage']);
    for (const key of RISK_MAP_METRIC_KEYS) {
      const def = riskMetricDef(key);
      expect(def).toBeTruthy();
      expect(def.label).toBeTruthy();
      expect(def.lowLabel).toBeTruthy();
      expect(def.highLabel).toBeTruthy();
    }
    expect(riskMetricDef('nope')).toBeNull();
  });
});

// ── Unit: per-metric heat maps ────────────────────────────────────────────

const HEAT_NODES = [
  { id: 'hot', node_type: 'function', name: 'hot_fn', test_coverage: 0.2, complexity: 30 },
  { id: 'cold', node_type: 'function', name: 'cold_fn', test_coverage: 1.0, complexity: 0 },
  { id: 'nodata', node_type: 'function', name: 'nodata_fn', test_coverage: null },
];

const HEAT_RISKS = [
  { node_id: 'hot', churn_rate: 10, fan_in: 4, fan_out: 6, complexity: 5, spec_covered: false },
  { node_id: 'cold', churn_rate: 0, fan_in: 0, fan_out: 0, complexity: 99, spec_covered: true },
  { node_id: 'nodata', churn_rate: 1, fan_in: 0, fan_out: 0, complexity: null, spec_covered: false },
];

describe('risk-map — buildRiskHeatMap per metric', () => {
  it('churn: high churn → hot red, zero churn → cool blue (zero is a valid value)', () => {
    const heat = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'churn');
    expect(heat.max).toBe(10);
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(1));
    expect(heat.byNodeId.get('cold').color).toBe(riskHeatColor(0));
  });

  it('coupling: fan-in + fan-out drives the scale', () => {
    const heat = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'coupling');
    expect(heat.byNodeId.get('hot').value).toBe(10); // 4 + 6
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(1));
    expect(heat.byNodeId.get('cold').value).toBe(0);
  });

  it('complexity: comes from node properties, falling back to risk entries', () => {
    const heat = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'complexity');
    // Node props win: hot=30 (entry says 5), cold=0 (entry says 99).
    expect(heat.byNodeId.get('hot').value).toBe(30);
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(1));
    expect(heat.byNodeId.get('cold').value).toBe(0);
    expect(heat.byNodeId.get('cold').color).toBe(riskHeatColor(0));
    // nodata has neither node complexity nor entry complexity → no heat.
    expect(heat.byNodeId.has('nodata')).toBe(false);
  });

  it('spec coverage: uncovered → hot red, governed → cool blue', () => {
    const heat = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'spec_coverage');
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(1)); // spec_covered: false
    expect(heat.byNodeId.get('cold').color).toBe(riskHeatColor(0)); // spec_covered: true
  });

  it('test coverage: low coverage → hot red, well tested → cool blue, null → no data', () => {
    const heat = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'test_coverage');
    expect(heat.byNodeId.get('hot').t).toBe(0.8); // 1 - 0.2
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(0.8));
    expect(heat.byNodeId.get('cold').t).toBe(0); // 1 - 1.0
    expect(heat.byNodeId.get('cold').color).toBe(riskHeatColor(0));
    expect(heat.byNodeId.has('nodata')).toBe(false);
  });

  it('switching the metric changes the coloring (same data, different scale)', () => {
    const churn = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'churn');
    const spec = buildRiskHeatMap(HEAT_RISKS, HEAT_NODES, 'spec_coverage');
    // 'cold' is cool under churn (stable) but cool under spec too; 'hot' is
    // hot under both. Use a node whose position flips: 'nodata' has churn 1
    // (non-zero, but 1/10 of max → cool-ish) and no spec (red).
    const churnT = churn.byNodeId.get('nodata').t;
    const specT = spec.byNodeId.get('nodata').t;
    expect(churnT).toBeLessThan(0.5);
    expect(specT).toBe(1);
    expect(churn.byNodeId.get('nodata').color).not.toBe(spec.byNodeId.get('nodata').color);
  });

  it('nodes without a risk entry get no churn/coupling/spec heat (no fabricated data)', () => {
    const nodes = [{ id: 'orphan', node_type: 'function', name: 'orphan_fn' }];
    for (const metric of ['churn', 'coupling', 'spec_coverage']) {
      const heat = buildRiskHeatMap(HEAT_RISKS, nodes, metric);
      expect(heat.byNodeId.has('orphan')).toBe(false);
    }
  });

  it('uniform zero metric is a valid all-cool state, not no-data', () => {
    const risks = HEAT_RISKS.map((r) => ({ ...r, churn_rate: 0 }));
    const heat = buildRiskHeatMap(risks, HEAT_NODES, 'churn');
    expect(heat.max).toBe(0);
    expect(heat.byNodeId.get('hot').color).toBe(riskHeatColor(0));
    expect(heat.byNodeId.get('cold').color).toBe(riskHeatColor(0));
  });
});

// ── Unit: anomaly detection ───────────────────────────────────────────────

/** 10 filler functions + a 0.4-coverage helper so the p90/p10
 *  distribution thresholds land where the tests need them
 *  (p90 complexity = 10, p10 coverage = 0.4). */
function anomalyFixture() {
  const nodes = [];
  for (let i = 1; i <= 10; i++) {
    nodes.push({
      id: `filler${i}`,
      node_type: 'function',
      name: `filler${i}`,
      file_path: `src/filler${i}.rs`,
      complexity: i,            // 1..10 → p90 = 10
      test_coverage: 0.9,       // high → p10 comes from target + 0.4 helper
    });
  }
  // The 0.4-coverage helper pins p10(coverages) = 0.4.
  nodes.push({ id: 'covhelper', node_type: 'function', name: 'covhelper', file_path: 'src/covhelper.rs', complexity: 3, test_coverage: 0.4 });
  return nodes;
}

describe('risk-map — detectRiskAnomalies', () => {
  it('flags high complexity + low test coverage with the real values (spec §7 example)', () => {
    const nodes = anomalyFixture();
    nodes.push({ id: 'target', node_type: 'function', name: 'PaymentRouter', file_path: 'src/router.rs', complexity: 42, test_coverage: 0.23 });
    const anomalies = detectRiskAnomalies({ nodes, edges: [] });
    const hit = anomalies.find((a) => a.nodeId === 'target');
    expect(hit).toBeTruthy();
    expect(hit.severity).toBe('high');
    expect(hit.message).toContain('High complexity (42');
    expect(hit.message).toContain('23%');
  });

  it('flags a trait with 5 implementations but only 2 tested', () => {
    const nodes = [
      { id: 'trait1', node_type: 'trait', name: 'Storage', file_path: 'src/storage.rs' },
      ...[1, 2, 3, 4, 5].map((i) => ({
        id: `impl${i}`,
        node_type: 'type',
        name: `Impl${i}`,
        file_path: `src/impl${i}.rs`,
        test_coverage: i <= 2 ? 0.8 : 0,
      })),
    ];
    const edges = [1, 2, 3, 4, 5].map((i) => ({
      id: `e${i}`, source_id: `impl${i}`, target_id: 'trait1', edge_type: 'implements',
    }));
    const anomalies = detectRiskAnomalies({ nodes, edges });
    const hit = anomalies.find((a) => a.nodeId === 'trait1');
    expect(hit).toBeTruthy();
    expect(hit.message).toBe('5 implementations but only 2 tested');
    expect(hit.severity).toBe('medium');
  });

  it('flags an agent-created type with no governing spec', () => {
    const nodes = [
      { id: 'type1', node_type: 'type', name: 'GeneratedConfig', file_path: 'src/gen.rs', last_modified_by: 'agent' },
    ];
    const anomalies = detectRiskAnomalies({ nodes, edges: [] });
    const hit = anomalies.find((a) => a.nodeId === 'type1');
    expect(hit).toBeTruthy();
    expect(hit.message).toBe('Agent-created type has no governing spec');
  });

  it('a governed_by edge suppresses the agent-created-spec anomaly', () => {
    const nodes = [
      { id: 'type1', node_type: 'type', name: 'GeneratedConfig', file_path: 'src/gen.rs', last_modified_by: 'agent' },
    ];
    const edges = [{ id: 'g1', source_id: 'type1', target_id: 'spec1', edge_type: 'governed_by' }];
    const anomalies = detectRiskAnomalies({ nodes, edges });
    expect(anomalies.find((a) => a.message.includes('governing spec'))).toBeUndefined();
  });

  it('flags 3 co-changing modules without a shared spec', () => {
    const nodes = ['a', 'b', 'c'].map((s) => ({
      id: `co_${s}`,
      node_type: 'function',
      name: `co_${s}`,
      file_path: `src/mod_${s}/file.rs`,
      last_modified_sha: 'deadbeefdeadbeef',
    }));
    const anomalies = detectRiskAnomalies({ nodes, edges: [] });
    const hit = anomalies.find((a) => a.nodeId === 'co_a');
    expect(hit).toBeTruthy();
    expect(hit.message).toBe('3 nodes across 3 modules change together but have no shared spec');
  });

  it('co-changing modules WITH a shared spec are not flagged', () => {
    const nodes = ['a', 'b', 'c'].map((s) => ({
      id: `co_${s}`,
      node_type: 'function',
      name: `co_${s}`,
      file_path: `src/mod_${s}/file.rs`,
      last_modified_sha: 'deadbeefdeadbeef',
      spec_path: 'specs/system/shared.md',
    }));
    const anomalies = detectRiskAnomalies({ nodes, edges: [] });
    expect(anomalies.find((a) => a.message.includes('change together'))).toBeUndefined();
  });

  it('server risk entries override stale node props (spec_covered: false wins over spec_path)', () => {
    // 10 callers → hub has 10 calls, far above the p90-based call
    // threshold; the node CLAIMS a spec_path — but the server says
    // spec_covered: false.
    const nodes = [
      { id: 'hub', node_type: 'function', name: 'hub', file_path: 'src/hub.rs', spec_path: 'specs/stale.md' },
      ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map((i) => ({
        id: `caller${i}`, node_type: 'function', name: `caller${i}`, file_path: `src/c${i}.rs`,
      })),
    ];
    const edges = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map((i) => ({
      id: `c${i}`, source_id: `caller${i}`, target_id: 'hub', edge_type: 'calls',
    }));
    const riskEntries = [{ node_id: 'hub', churn_rate: 0, fan_in: 10, fan_out: 0, complexity: 1, spec_covered: false }];
    const anomalies = detectRiskAnomalies({ nodes, edges, riskEntries });
    expect(anomalies.find((a) => a.nodeId === 'hub' && a.message.includes('with no spec'))).toBeTruthy();

    // With the server confirming coverage, the same hub is not flagged.
    const covered = [{ node_id: 'hub', churn_rate: 0, fan_in: 10, fan_out: 0, complexity: 1, spec_covered: true }];
    const anomalies2 = detectRiskAnomalies({ nodes, edges, riskEntries: covered });
    expect(anomalies2.find((a) => a.nodeId === 'hub' && a.message.includes('with no spec'))).toBeUndefined();
  });

  it('sorts callouts by severity (high first)', () => {
    const nodes = anomalyFixture();
    nodes.push({ id: 'target', node_type: 'function', name: 'PaymentRouter', file_path: 'src/router.rs', complexity: 42, test_coverage: 0.23 });
    const anomalies = detectRiskAnomalies({ nodes, edges: [] });
    expect(anomalies.length).toBeGreaterThan(1);
    const order = { high: 0, medium: 1, low: 2 };
    for (let i = 1; i < anomalies.length; i++) {
      expect(order[anomalies[i - 1].severity]).toBeLessThanOrEqual(order[anomalies[i].severity]);
    }
  });
});

// ── Component: ExplorerCanvas Risk Map overlay ────────────────────────────

let mockCtx;
let drawnFillStyles;
beforeEach(() => {
  drawnFillStyles = [];
  mockCtx = {
    clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
    beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(),
    fill: vi.fn(), stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(), fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    translate: vi.fn(), rotate: vi.fn(),
    strokeStyle: '', lineWidth: 0, globalAlpha: 1,
    font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    roundRect: vi.fn(), clip: vi.fn(), ellipse: vi.fn(),
    strokeText: vi.fn(), createLinearGradient: vi.fn(() => ({ addColorStop: vi.fn() })),
    filter: '',
  };
  // Capture every fillStyle assignment so heat colors are observable.
  let fillStyle = '';
  Object.defineProperty(mockCtx, 'fillStyle', {
    get: () => fillStyle,
    set: (v) => { fillStyle = v; drawnFillStyles.push(String(v)); },
    configurable: true,
  });
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} unobserve() {} };
  global.requestAnimationFrame = vi.fn(cb => { cb(16); return 1; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

const CANVAS_NODES = [
  { id: 'pkg', node_type: 'package', name: 'src', qualified_name: 'src', file_path: '', visibility: 'public', test_node: false },
  { id: 'hot', node_type: 'function', name: 'hot_fn', qualified_name: 'src.hot_fn', file_path: 'src/hot.rs', visibility: 'public', test_node: false },
  { id: 'cold', node_type: 'function', name: 'cold_fn', qualified_name: 'src.cold_fn', file_path: 'src/cold.rs', visibility: 'public', test_node: false },
];

const CANVAS_EDGES = [
  { id: 'ce1', source_id: 'pkg', target_id: 'hot', edge_type: 'contains' },
  { id: 'ce2', source_id: 'pkg', target_id: 'cold', edge_type: 'contains' },
];

const CANVAS_RISK_ENTRIES = [
  { node_id: 'hot', name: 'hot_fn', qualified_name: 'hot_fn', churn_rate: 10, fan_in: 0, fan_out: 0, complexity: 3, spec_covered: false },
  { node_id: 'cold', name: 'cold_fn', qualified_name: 'cold_fn', churn_rate: 0, fan_in: 0, fan_out: 0, complexity: 3, spec_covered: true },
];

function riskByNodeFrom(entries) {
  const m = {};
  for (const r of entries) m[r.node_id] = (r.churn_rate ?? 0) * (r.complexity ?? 0);
  return m;
}

describe('ExplorerCanvas — Risk Map overlay (system-explorer.md §7)', () => {
  it('activates with the Evaluative lens and colors nodes on the heat scale', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: CANVAS_NODES,
        edges: CANVAS_EDGES,
        lens: 'evaluative',
        riskEntries: CANVAS_RISK_ENTRIES,
        riskByNode: riskByNodeFrom(CANVAS_RISK_ENTRIES),
      },
    });
    // Auto-selected risk metric (no trace data + risk data present).
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-controls"]')).toBeTruthy();
    });
    // hot_fn (churn 10) → hot red fill; cold_fn (churn 0) → cool blue fill.
    await waitFor(() => {
      expect(drawnFillStyles).toContain(riskHeatColor(1));
      expect(drawnFillStyles).toContain(riskHeatColor(0));
    });
  });

  it('renders the metric selector with the five spec metrics and a color scale legend', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: CANVAS_NODES,
        edges: CANVAS_EDGES,
        lens: 'evaluative',
        riskEntries: CANVAS_RISK_ENTRIES,
        riskByNode: riskByNodeFrom(CANVAS_RISK_ENTRIES),
      },
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-metric-select"]')).toBeTruthy();
    });
    const options = [...container.querySelectorAll('[data-testid="risk-map-metric-select"] option')];
    expect(options.map((o) => o.textContent.trim())).toEqual(
      RISK_MAP_METRICS.map((m) => m.label)
    );
    // Legend: gradient bar with the low/high scale labels.
    const legend = container.querySelector('[data-testid="risk-map-legend"]');
    expect(legend).toBeTruthy();
    expect(legend.querySelector('.risk-map-legend-bar')).toBeTruthy();
    expect(legend.textContent).toContain('Stable');
    expect(legend.textContent).toContain('High churn');
  });

  it('switching the metric re-colors the nodes (churn → spec coverage)', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: CANVAS_NODES,
        edges: CANVAS_EDGES,
        lens: 'evaluative',
        riskEntries: CANVAS_RISK_ENTRIES,
        riskByNode: riskByNodeFrom(CANVAS_RISK_ENTRIES),
      },
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-metric-select"]')).toBeTruthy();
    });
    // Under churn: hot_fn is red (churn 10), cold_fn is blue (churn 0).
    await waitFor(() => {
      expect(drawnFillStyles).toContain(riskHeatColor(1));
    });

    // Switch to Spec Coverage: hot_fn has no spec → still red; cold_fn is
    // governed → blue. The distinguishing case: both nodes now sit at the
    // scale ends by spec governance rather than churn. Use test_coverage —
    // neither node carries test_coverage data, so no heat fills at all
    // after the switch, proving the selector drives the coloring.
    drawnFillStyles.length = 0;
    await fireEvent.change(container.querySelector('[data-testid="risk-map-metric-select"]'), { target: { value: 'test_coverage' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-legend"]').textContent).toContain('Test Coverage');
    });
    // No node has test_coverage data → no heat fills drawn.
    expect(drawnFillStyles).not.toContain(riskHeatColor(1));
    expect(drawnFillStyles).not.toContain(riskHeatColor(0));
  });

  it('spec coverage metric colors uncovered nodes red via the selector', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: CANVAS_NODES,
        edges: CANVAS_EDGES,
        lens: 'evaluative',
        riskEntries: CANVAS_RISK_ENTRIES,
        riskByNode: riskByNodeFrom(CANVAS_RISK_ENTRIES),
      },
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-metric-select"]')).toBeTruthy();
    });
    drawnFillStyles.length = 0;
    await fireEvent.change(container.querySelector('[data-testid="risk-map-metric-select"]'), { target: { value: 'spec_coverage' } });
    // hot_fn (spec_covered: false) → red; cold_fn (spec_covered: true) → blue.
    await waitFor(() => {
      expect(drawnFillStyles).toContain(riskHeatColor(1));
      expect(drawnFillStyles).toContain(riskHeatColor(0));
    });
  });

  it('deactivates cleanly when the Evaluative lens is toggled off', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: CANVAS_NODES,
        edges: CANVAS_EDGES,
        lens: 'evaluative',
        riskEntries: CANVAS_RISK_ENTRIES,
        riskByNode: riskByNodeFrom(CANVAS_RISK_ENTRIES),
      },
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="risk-map-controls"]')).toBeTruthy();
    });
    const evaluativeBox = container.querySelector('[data-testid="lens-checkbox-evaluative"] input');
    await fireEvent.click(evaluativeBox);
    expect(evaluativeBox.checked).toBe(false);
    // Controls AND legend are removed; nodes return to structural coloring.
    expect(container.querySelector('[data-testid="risk-map-controls"]')).toBeNull();
    expect(container.querySelector('[data-testid="risk-map-legend"]')).toBeNull();
    drawnFillStyles.length = 0;
    await new Promise((r) => setTimeout(r, 20));
    expect(drawnFillStyles).not.toContain(riskHeatColor(1));
  });

  it('anomaly callouts are generated and clickable — navigating to the node', async () => {
    const onNodeDetail = vi.fn();
    // A trait with 2 implementations, 1 tested → anomaly callout.
    const nodes = [
      { id: 'trait1', node_type: 'trait', name: 'Storage', qualified_name: 'Storage', file_path: 'src/storage.rs', visibility: 'public' },
      { id: 'impl1', node_type: 'type', name: 'Impl1', qualified_name: 'Impl1', file_path: 'src/impl1.rs', visibility: 'public', test_coverage: 0.9 },
      { id: 'impl2', node_type: 'type', name: 'Impl2', qualified_name: 'Impl2', file_path: 'src/impl2.rs', visibility: 'public', test_coverage: 0 },
    ];
    const edges = [
      { id: 'e1', source_id: 'impl1', target_id: 'trait1', edge_type: 'implements' },
      { id: 'e2', source_id: 'impl2', target_id: 'trait1', edge_type: 'implements' },
    ];
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes,
        edges,
        lens: 'evaluative',
        riskEntries: [],
        riskByNode: {},
        onNodeDetail,
      },
    });
    const item = await waitFor(() => {
      const el = container.querySelector('.anomaly-item');
      expect(el).toBeTruthy();
      return el;
    });
    expect(item.textContent).toContain('2 implementations but only 1 tested');
    await fireEvent.click(item);
    expect(onNodeDetail).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'trait1' })
    );
  });
});
