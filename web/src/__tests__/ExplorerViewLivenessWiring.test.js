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
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

describe('ExplorerView — liveness data loading is repo-scoped (HSI §3)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
    global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
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
