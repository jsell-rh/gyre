/**
 * lens-rendering.test.js — HSI §3 Three Lenses rendering pipeline
 *
 * The lens decoration rules are pure functions so the join semantics
 * (node → governing specs → gate outcome / approval status) and the
 * coverage thresholds can be tested without the canvas render loop.
 */

import { describe, it, expect } from 'vitest';
import {
  normalizeLens,
  coverageLevel,
  coverageDecoration,
  nodeSpecPaths,
  gateStatusForPaths,
  gateStatusDecoration,
  gateEdgeDecoration,
  approvalStatusForRecord,
  approvalStatusForRecords,
  approvalDecoration,
  evaluativeNodeDecorations,
  lensFromUrl,
  lensToUrl,
} from '../lib/lens-rendering.js';

// ── Fixtures ───────────────────────────────────────────────────────────────

const NODE = (over = {}) => ({
  id: 'n1',
  node_type: 'function',
  name: 'create_user',
  qualified_name: 'api.handlers.create_user',
  ...over,
});

const SPEC_NODE = { id: 'spec1', node_type: 'module', name: 'specs/system/auth.md', file_path: 'specs/system/auth.md' };

const EDGES = [
  { id: 'e1', source_id: 'n1', target_id: 'spec1', edge_type: 'governed_by' },
  { id: 'e2', source_id: 'n1', target_id: 'n2', edge_type: 'calls' },
];

const NODES_BY_ID = new Map([
  ['n1', NODE()],
  ['n2', NODE({ id: 'n2', name: 'get_user' })],
  ['spec1', SPEC_NODE],
]);

// ── Lens normalization / URL persistence ───────────────────────────────────

describe('normalizeLens', () => {
  it('structural is the default for unknown/observable values', () => {
    expect(normalizeLens('structural')).toBe('structural');
    expect(normalizeLens('evaluative')).toBe('evaluative');
    expect(normalizeLens('observable')).toBe('structural'); // future work — cannot activate
    expect(normalizeLens('nonsense')).toBe('structural');
    expect(normalizeLens(null)).toBe('structural');
  });
});

describe('lensFromUrl / lensToUrl', () => {
  it('reads ?lens=evaluative from a URL', () => {
    expect(lensFromUrl('http://x/workspaces/ws/explorer?repo=api&lens=evaluative')).toBe('evaluative');
  });

  it('defaults to structural when the param is absent or invalid', () => {
    expect(lensFromUrl('http://x/explorer')).toBe('structural');
    expect(lensFromUrl('http://x/explorer?lens=observable')).toBe('structural');
    expect(lensFromUrl('http://x/explorer?lens=bogus')).toBe('structural');
  });

  it('encodes the evaluative lens and preserves other params + hash', () => {
    const out = lensToUrl('evaluative', 'http://x/explorer?repo=api#drill=pkg/mod');
    expect(out).toBe('http://x/explorer?repo=api&lens=evaluative#drill=pkg/mod');
  });

  it('drops the param for the structural default (clean URLs)', () => {
    const out = lensToUrl('structural', 'http://x/explorer?repo=api&lens=evaluative');
    expect(out).toBe('http://x/explorer?repo=api');
  });

  it('is idempotent for the default lens', () => {
    const url = 'http://x/explorer?repo=api';
    expect(lensToUrl('structural', url)).toBe(url);
  });
});

// ── Test coverage badge (green/yellow/red heatmap) ─────────────────────────

describe('coverageDecoration', () => {
  it('green at >= 70%, yellow at >= 30%, red below', () => {
    expect(coverageLevel(0.85)).toBe('high');
    expect(coverageLevel(0.7)).toBe('high');
    expect(coverageLevel(0.5)).toBe('partial');
    expect(coverageLevel(0.3)).toBe('partial');
    expect(coverageLevel(0.29)).toBe('low');
    expect(coverageLevel(0)).toBe('low');
  });

  it('badge label is a rounded percentage with level-matched colors', () => {
    const d = coverageDecoration(NODE({ test_coverage: 0.85 }));
    expect(d.label).toBe('85%');
    expect(d.level).toBe('high');
    expect(d.color).toBe('#22c55e'); // green
    const p = coverageDecoration(NODE({ test_coverage: 0.5 }));
    expect(p.color).toBe('#eab308'); // yellow
    const l = coverageDecoration(NODE({ test_coverage: 0.1 }));
    expect(l.color).toBe('#ef4444'); // red
  });

  it('no badge when coverage data is unavailable', () => {
    expect(coverageDecoration(NODE({ test_coverage: null }))).toBeNull();
    expect(coverageDecoration(NODE())).toBeNull();
  });
});

// ── Node → spec path join (gate results and approvals arrive per spec) ─────

describe('nodeSpecPaths', () => {
  it('collects spec_path, spec_paths extras, and GovernedBy edge targets', () => {
    const node = NODE({ spec_path: 'specs/a.md', spec_paths: ['specs/b.md'] });
    const paths = nodeSpecPaths(node, EDGES, NODES_BY_ID);
    expect(paths).toContain('specs/a.md');
    expect(paths).toContain('specs/b.md');
    expect(paths).toContain('specs/system/auth.md'); // from the governed_by edge target
    expect(paths.length).toBe(3);
  });

  it('returns an empty list for ungoverned nodes', () => {
    expect(nodeSpecPaths(NODE({ id: 'other' }), EDGES, NODES_BY_ID)).toEqual([]);
  });

  it('ignores soft-deleted edges', () => {
    const edges = [{ source_id: 'n1', target_id: 'spec1', edge_type: 'governed_by', deleted_at: 123 }];
    expect(nodeSpecPaths(NODE(), edges, NODES_BY_ID)).toEqual([]);
  });
});

// ── Gate status (most recent gate results, green pass / red fail) ──────────

describe('gate status decorations', () => {
  it('a failing gate on any governing spec fails the node', () => {
    expect(gateStatusForPaths(['specs/a.md', 'specs/b.md'], { 'specs/a.md': 'pass', 'specs/b.md': 'fail' })).toBe('fail');
  });

  it('pass only when every governing spec with results passed', () => {
    expect(gateStatusForPaths(['specs/a.md', 'specs/b.md'], { 'specs/a.md': 'pass', 'specs/b.md': 'pass' })).toBe('pass');
  });

  it('null when no governing spec has a concluded gate run', () => {
    expect(gateStatusForPaths(['specs/a.md'], {})).toBeNull();
    expect(gateStatusForPaths([], { 'specs/a.md': 'pass' })).toBeNull();
  });

  it('node decoration colors green on pass, red on fail', () => {
    const pass = gateStatusDecoration(NODE(), { 'specs/system/auth.md': 'pass' }, EDGES, NODES_BY_ID);
    expect(pass.status).toBe('pass');
    expect(pass.color).toBe('#22c55e');
    const fail = gateStatusDecoration(NODE(), { 'specs/system/auth.md': 'fail' }, EDGES, NODES_BY_ID);
    expect(fail.status).toBe('fail');
    expect(fail.color).toBe('#ef4444');
  });

  it('edge decoration is keyed on the edge source node', () => {
    const d = gateEdgeDecoration(NODE(), { 'specs/system/auth.md': 'fail' }, EDGES, NODES_BY_ID);
    expect(d.status).toBe('fail');
    // Ungoverned source node → no decoration
    expect(gateEdgeDecoration(NODE({ id: 'n2' }), { 'specs/system/auth.md': 'fail' }, EDGES, NODES_BY_ID)).toBeNull();
  });
});

// ── Spec approval status (✓ approved / ⏳ pending / ✗ rejected) ─────────────

describe('approval status', () => {
  it('active record approves the path', () => {
    expect(approvalStatusForRecord({ active: true })).toBe('approved');
  });

  it('inactive without revocation = rejected by a reviewer', () => {
    expect(approvalStatusForRecord({ active: false, revoked_at: null, rejected_at: 123 })).toBe('rejected');
  });

  it('revoked approval returns to pending (withdrawn, not rejected)', () => {
    expect(approvalStatusForRecord({ active: false, revoked_at: 123 })).toBe('pending');
  });

  it('any active record in the history approves; otherwise the latest decides', () => {
    expect(approvalStatusForRecords([
      { active: false, revoked_at: 1 },
      { active: true, approved_at: 5 },
      { active: false, revoked_at: 2, approved_at: 2 },
    ])).toBe('approved');
    expect(approvalStatusForRecords([
      { active: false, revoked_at: 1, approved_at: 1 },
      { active: false, rejected_at: 9, approved_at: 5 },
    ])).toBe('rejected');
  });

  it('node with spec linkage but no ledger record is pending', () => {
    const d = approvalDecoration(NODE(), new Map(), EDGES, NODES_BY_ID);
    expect(d.status).toBe('pending');
    expect(d.icon).toBe('\u23F3'); // ⏳
  });

  it('node with no spec linkage gets no approval icon', () => {
    expect(approvalDecoration(NODE(), new Map(), EDGES, NODES_BY_ID)).not.toBeNull();
    expect(approvalDecoration(NODE({ spec_path: null }), new Map(), [EDGES[1]], NODES_BY_ID)).toBeNull();
  });

  it('approved and rejected icons map to ✓ / ✗', () => {
    const approved = approvalDecoration(
      NODE(),
      new Map([['specs/system/auth.md', [{ active: true, approved_at: 1 }]]]),
      EDGES, NODES_BY_ID,
    );
    expect(approved.icon).toBe('\u2713'); // ✓
    expect(approved.color).toBe('#22c55e');
    const rejected = approvalDecoration(
      NODE(),
      new Map([['specs/system/auth.md', [{ active: false, rejected_at: 2, approved_at: 1 }]]]),
      EDGES, NODES_BY_ID,
    );
    expect(rejected.icon).toBe('\u2717'); // ✗
    expect(rejected.color).toBe('#ef4444');
  });
});

// ── Lens composition (evaluative overlays on the structural base) ──────────

describe('evaluativeNodeDecorations', () => {
  it('composes coverage, gate, and approval decorations on one node', () => {
    const node = NODE({ test_coverage: 0.85, spec_path: 'specs/system/auth.md' });
    const d = evaluativeNodeDecorations(node, {
      gateStatusBySpec: { 'specs/system/auth.md': 'pass' },
      approvalRecordsByPath: new Map([['specs/system/auth.md', [{ active: true, approved_at: 1 }]]]),
      edges: EDGES,
      nodesById: NODES_BY_ID,
    });
    // A node shows its type (structural) AND its coverage (evaluative)
    // simultaneously — the decorations coexist, neither replaces the other.
    expect(d.coverage.level).toBe('high');
    expect(d.gate.status).toBe('pass');
    expect(d.approval.status).toBe('approved');
  });

  it('null when the node has no evaluative data at all', () => {
    expect(evaluativeNodeDecorations(NODE(), {})).toBeNull();
  });

  it('partial data still decorates (coverage alone, approval alone)', () => {
    const covOnly = evaluativeNodeDecorations(NODE({ test_coverage: 0.1 }), {});
    expect(covOnly.coverage.level).toBe('low');
    expect(covOnly.gate).toBeNull();
    expect(covOnly.approval).toBeNull();
    const approvalOnly = evaluativeNodeDecorations(NODE({ spec_path: 'x.md' }), {});
    expect(approvalOnly.coverage).toBeNull();
    expect(approvalOnly.approval.status).toBe('pending');
  });
});
