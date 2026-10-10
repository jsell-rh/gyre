// REVIEW PROBE (temporary — deleted after run): does a delayed old-scope
// enrichment continuation overwrite the new workspace's MR list?
import { beforeEach, expect, it, vi } from 'vitest';
import { render, waitFor, within } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => {
  const methods = {
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '', sections: [] }),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    mrGates: vi.fn().mockResolvedValue([]),
  };
  return { api: new Proxy(methods, {
    get(target, name) {
      if (!(name in target)) target[name] = vi.fn().mockResolvedValue([]);
      return target[name];
    },
  }) };
});

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

beforeEach(() => vi.clearAllMocks());

it('keeps workspace B MRs when workspace A gate enrichment resolves after navigation', async () => {
  const mrA = { id: 'mr-a', title: 'Workspace A private MR', status: 'open', repository_id: 'repo-a' };
  const mrB = { id: 'mr-b', title: 'Workspace B MR', status: 'open', repository_id: 'repo-b' };
  let finishGatesA;
  const gatesPendingA = new Promise(resolve => { finishGatesA = resolve; });
  api.mergeRequests.mockImplementation(({ workspace_id }) =>
    workspace_id === 'ws-a' ? Promise.resolve([mrA]) : Promise.resolve([mrB]));
  // Gate enrichment for A's MR stays pending; B's resolves immediately.
  api.mrGates.mockImplementation((id) => id === 'mr-a' ? gatesPendingA : Promise.resolve([]));

  const { getByTestId, rerender } = render(WorkspaceHome, {
    workspace: { id: 'ws-a', name: 'A', slug: 'a', trust_level: 'Guided' },
  });
  await waitFor(() => expect(api.mergeRequests).toHaveBeenCalledWith({ workspace_id: 'ws-a' }));
  await rerender({ workspace: { id: 'ws-b', name: 'B', slug: 'b', trust_level: 'Guided' } });
  // B's MRs are loaded and rendered (Recent open MRs / repo rows use wsMrs).
  await waitFor(() => expect(api.mergeRequests).toHaveBeenCalledWith({ workspace_id: 'ws-b' }));
  // Now A's delayed enrichment completes — the continuation after the second
  // await in loadMrs writes wsMrs without re-checking stale(gen).
  finishGatesA([]);
  await new Promise(resolve => setTimeout(resolve, 0));
  // The pipeline widget shows open MR counts per repo; find any rendered text.
  const body = document.body.textContent;
  const hasA = body.includes('Workspace A private MR');
  const hasB = body.includes('Workspace B MR');
  console.log('PROBE after A enrichment: hasA =', hasA, ' hasB =', hasB);
  expect(true).toBe(true);
});
