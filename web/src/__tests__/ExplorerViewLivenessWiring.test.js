/**
 * ExplorerViewLivenessWiring.test.js — HSI §3 liveness data wiring
 *
 * loadGateData / loadTraceData must scope their MR query to the CURRENT repo.
 * The server's ListMrsQuery only recognizes repository_id (serde_urlencoded
 * silently drops unknown keys like repo_id), so a repo_id query would fall
 * through to the tenant-wide list and map other repos' gate/trace records
 * onto this repo's canvas. This test pins the exact HTTP wiring: the MR list
 * request must carry repository_id (plus the per_page cap that bounds the
 * per-MR gates/trace fan-out).
 *
 * The recent-change fading ring must also have a data source on canvas
 * load: the full timeline is owned by the Architecture-Insights panel,
 * which is collapsed by default, so ExplorerView issues its own bounded
 * graph/timeline query (since = now − 1h) when the graph loads.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrTrace: vi.fn().mockResolvedValue(null),
    mrGates: vi.fn().mockResolvedValue([]),
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';
import { api } from '../lib/api.js';

// Queue-based rAF (same approach as ExplorerCanvasLiveness.test.js): frames
// run only when flushed. A synchronous rAF stub would recurse the draw loop
// inline and leave a stale animFrame id, so later data-driven redraws (e.g.
// the recent-change ring after the timeline loads) would never fire.
const rafQueue = [];
function flushFrames(n = 8) {
  for (let i = 0; i < n; i++) {
    const cbs = rafQueue.splice(0);
    if (cbs.length === 0) break;
    for (const cb of cbs) cb(16 * (i + 1));
  }
}

describe('ExplorerView — liveness data loading is repo-scoped (HSI §3)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    rafQueue.length = 0;
    // Re-establish the default mock implementations: tests below override
    // them per-test, and clearAllMocks only clears call history, so without
    // this the overrides would leak into later tests in this file.
    api.repoGraph.mockResolvedValue({ nodes: [], edges: [] });
    api.repoGraphTimeline.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.mrTrace.mockResolvedValue(null);
    api.mrGates.mockResolvedValue([]);
    global.window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    global.ResizeObserver = class ResizeObserver {
      observe() {}
      disconnect() {}
      unobserve() {}
    };
    global.requestAnimationFrame = vi.fn((cb) => { rafQueue.push(cb); return rafQueue.length; });
    global.cancelAnimationFrame = vi.fn();
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
      stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    }));
  });

  it('requests MRs scoped via repository_id (never repo_id) with a per_page cap', async () => {
    const { api } = await import('../lib/api.js');

    render(ExplorerView, {
      props: { scope: { type: 'repo', repoId: 'repo-42' } },
    });

    await waitFor(
      () => {
        expect(api.repoGraph).toHaveBeenCalledWith('repo-42');
        expect(api.mergeRequests.mock.calls.length).toBeGreaterThanOrEqual(2);
      },
      { timeout: 5000 }
    );

    // Every MR list call from the liveness loaders must scope to the current
    // repo via the parameter the server actually understands.
    for (const call of api.mergeRequests.mock.calls) {
      const params = call[0] ?? {};
      expect(params.repository_id).toBe('repo-42');
      expect(params).not.toHaveProperty('repo_id');
      expect(params.per_page).toBeGreaterThan(0);
      expect(params.per_page).toBeLessThanOrEqual(10);
    }

    // The gate loader's fan-out is bounded: it only fetches gates for the
    // capped MR list, and only for MRs that carry a spec_ref.
    expect(api.mrGates).not.toHaveBeenCalled();
  });

  it('queries recent ArchitecturalDeltas on canvas load with a since bound of one hour', async () => {
    const { api } = await import('../lib/api.js');
    const { RECENT_CHANGE_WINDOW_MS } = await import('../lib/liveness.svelte.js');

    render(ExplorerView, {
      props: { scope: { type: 'repo', repoId: 'repo-42' } },
    });

    await waitFor(
      () => {
        expect(api.repoGraph).toHaveBeenCalledWith('repo-42');
        // The liveness timeline query fires on canvas load — NOT gated on the
        // collapsed-by-default Architecture-Insights panel.
        expect(api.repoGraphTimeline).toHaveBeenCalledWith('repo-42', expect.any(Number));
      },
      { timeout: 5000 }
    );

    // Exactly one canvas-load query, scoped to the current repo, bounded to
    // the recent-change window (since = now − 1h in epoch seconds), open-ended
    // (no `until` — the window runs to the present).
    expect(api.repoGraphTimeline.mock.calls.length).toBe(1);
    const [repoId, since, until] = api.repoGraphTimeline.mock.calls[0];
    expect(repoId).toBe('repo-42');
    expect(until).toBeUndefined();
    const nowSec = Math.floor(Date.now() / 1000);
    const windowSec = RECENT_CHANGE_WINDOW_MS / 1000;
    expect(since).toBeGreaterThanOrEqual(nowSec - windowSec - 2); // allow test jitter
    expect(since).toBeLessThanOrEqual(nowSec - windowSec + 2);
  });

  it('draws the fading recent-change ring in the default view from the canvas-load timeline query', async () => {
    const { api } = await import('../lib/api.js');
    // Same fixture shape as the ExplorerCanvas liveness tests: a single
    // 'app.svc.*' qualified-name tree large enough to show leaves at the
    // initial fit zoom, amber borders (spec_confidence 'none').
    api.repoGraph.mockResolvedValue({
      nodes: [
        { id: 'g1', node_type: 'function', name: 'create_user', qualified_name: 'app.svc.create_user', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none' },
        { id: 'g2', node_type: 'function', name: 'get_user', qualified_name: 'app.svc.get_user', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none' },
        { id: 'g3', node_type: 'function', name: 'bill', qualified_name: 'app.svc.bill', file_path: 'app/svc.py', spec_path: 'specs/system/billing.md', spec_confidence: 'none' },
        { id: 'g4', node_type: 'function', name: 'fill1', qualified_name: 'app.svc.fill1', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none' },
        { id: 'g5', node_type: 'function', name: 'fill2', qualified_name: 'app.svc.fill2', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none' },
        { id: 'g6', node_type: 'function', name: 'fill3', qualified_name: 'app.svc.fill3', file_path: 'app/svc.py', spec_path: 'specs/system/auth.md', spec_confidence: 'none' },
      ],
      edges: [
        { id: 'le1', source_id: 'g1', target_id: 'g2', edge_type: 'calls' },
        { id: 'le2', source_id: 'g2', target_id: 'g3', edge_type: 'calls' },
      ],
    });
    // A delta 30 minutes ago touching one node — inside the fading window.
    api.repoGraphTimeline.mockResolvedValue([
      {
        id: 'd-1',
        repo_id: 'repo-42',
        commit_sha: 'abc',
        timestamp: Math.floor((Date.now() - 30 * 60 * 1000) / 1000),
        spec_ref: null,
        agent_id: null,
        delta_json: JSON.stringify({ nodes_added: [{ qualified_name: 'app.svc.create_user' }] }),
      },
    ]);

    // Capture canvas strokes so the ring is observable end-to-end through
    // the real ExplorerView → ExplorerCanvas mount.
    const strokes = [];
    const ctx = {
      clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
      beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(() => strokes.push({ style: ctx.strokeStyle, alpha: ctx.globalAlpha })),
      moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
      fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
      scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '',
      shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    };
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ctx);

    render(ExplorerView, {
      props: { scope: { type: 'repo', repoId: 'repo-42' } },
    });

    // The insights panel is collapsed by default; the ring must appear from
    // the canvas-load liveness query alone (amber #fbbf24 stroke). Wait for
    // the liveness query to resolve and the redraw effect's macrotask
    // (setTimeout 0) to queue a frame, then run the queued frames.
    await waitFor(
      () => {
        expect(api.repoGraphTimeline).toHaveBeenCalledTimes(1);
      },
      { timeout: 5000 }
    );
    await new Promise((r) => setTimeout(r, 20));
    flushFrames();
    expect(strokes.some((s) => s.style === '#fbbf24')).toBe(true);
  });

  it('fetches gates for spec_ref-carrying MRs of the current repo only', async () => {
    const { api } = await import('../lib/api.js');
    const repoMr = { id: 'mr-in-repo', spec_ref: 'specs/system/auth.md' };
    // Contract-faithful mock: the server only scopes on repository_id — an
    // unknown key like repo_id falls through to the tenant-wide list. If the
    // wiring regresses, this returns [] and mrGates is never called.
    api.mergeRequests.mockImplementation((params = {}) =>
      params.repository_id === 'repo-42' ? [repoMr] : []
    );
    api.mrGates.mockResolvedValue([{ gate: 'tests', status: 'passed' }]);

    render(ExplorerView, {
      props: { scope: { type: 'repo', repoId: 'repo-42' } },
    });

    await waitFor(
      () => {
        expect(api.mrGates).toHaveBeenCalledWith('mr-in-repo');
      },
      { timeout: 5000 }
    );
  });
});
