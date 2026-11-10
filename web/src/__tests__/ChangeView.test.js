/**
 * ChangeView.test.js — HSI §3 Change View ("what changed since last visit?").
 *
 * Drives the real component against a mocked architectural-delta timeline:
 *   - the query uses the client-cached last-seen value as `since`
 *   - Added (+) / Modified (~) / Removed (-) entries render with name, type,
 *     module, spec reference and authoring agent
 *   - entries are clickable and navigate to the node in the Boundary View
 *   - modified entries expand to show field changes
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import ChangeView from '../components/explorer/ChangeView.svelte';
import { __resetForTest } from '../lib/last-seen.js';

const repoGraphTimeline = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: { repoGraphTimeline: (...args) => repoGraphTimeline(...args) },
}));

// A previous visit persisted last-seen = 1000 → the Change View must query
// the timeline with since=1000, NOT the value this visit's responses produce.
const LAST_SEEN = 1000;

function delta(overrides = {}) {
  return {
    id: 'd-1',
    repo_id: 'repo-1',
    commit_sha: 'abc1234',
    timestamp: 5000,
    spec_ref: 'specs/system/payment-retry.md',
    agent_id: 'worker-12',
    delta_json: '{}',
    ...overrides,
  };
}

const GRAPH = {
  nodes: [
    {
      id: 'n-retry',
      node_type: 'type',
      name: 'RetryPolicy',
      qualified_name: 'payment-domain::retry::RetryPolicy',
      repo_id: 'repo-1',
    },
    {
      id: 'mod-retry',
      node_type: 'module',
      name: 'retry',
      qualified_name: 'payment-domain::retry',
      repo_id: 'repo-1',
    },
  ],
  edges: [
    { id: 'c1', source_id: 'mod-retry', target_id: 'n-retry', edge_type: 'contains' },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  __resetForTest();
  localStorage.clear();
  localStorage.setItem('gyre_last_seen', JSON.stringify({ value: LAST_SEEN, updated_at: 1 }));
});

describe('ChangeView', () => {
  it('queries the graph timeline with since = cached last-seen value', async () => {
    repoGraphTimeline.mockResolvedValue([]);
    render(ChangeView, { props: { repoId: 'repo-1', graph: GRAPH } });
    await waitFor(() => expect(repoGraphTimeline).toHaveBeenCalledTimes(1));
    expect(repoGraphTimeline).toHaveBeenCalledWith('repo-1', LAST_SEEN);
  });

  it('renders added, modified and removed entries with full metadata', async () => {
    repoGraphTimeline.mockResolvedValue([
      delta({
        id: 'd-add',
        delta_json: JSON.stringify({
          nodes_added: [
            { name: 'RetryPolicy', node_type: 'type', qualified_name: 'payment-domain::retry::RetryPolicy' },
          ],
        }),
      }),
      delta({
        id: 'd-mod',
        delta_json: JSON.stringify({
          nodes_modified: [
            {
              qualified_name: 'payment-domain::charge::ChargeService',
              field_changes: [{ field: 'complexity', old_value: '3', new_value: '6' }],
            },
          ],
        }),
      }),
      delta({
        id: 'd-del',
        delta_json: JSON.stringify({ nodes_removed: ['payment-domain::legacy::LegacyPaymentHandler'] }),
      }),
    ]);
    render(ChangeView, { props: { repoId: 'repo-1', graph: GRAPH } });

    await waitFor(() => {
      expect(screen.getByText('RetryPolicy')).toBeTruthy();
    });
    // Added entry: type + containing module + spec + agent.
    expect(screen.getByText('type')).toBeTruthy();
    expect(screen.getByText('in payment-domain::retry')).toBeTruthy();
    expect(screen.getAllByText('Spec: specs/system/payment-retry.md').length).toBe(3);
    expect(screen.getAllByText('Agent: worker-12').length).toBe(3);

    // Modified + removed entries.
    expect(screen.getByText('ChargeService')).toBeTruthy();
    expect(screen.getByText('(1 field changed)')).toBeTruthy();
    expect(screen.getByText('LegacyPaymentHandler')).toBeTruthy();

    const list = document.querySelector('.change-list');
    expect(list.dataset.count).toBe('3');
    const kinds = [...list.querySelectorAll('.change-entry')].map((e) => e.dataset.kind).sort();
    expect(kinds).toEqual(['added', 'modified', 'removed']);
  });

  it('navigates to the node in the Boundary View when a resolvable entry is clicked', async () => {
    repoGraphTimeline.mockResolvedValue([
      delta({
        id: 'd-add',
        delta_json: JSON.stringify({
          nodes_added: [
            { name: 'RetryPolicy', node_type: 'type', qualified_name: 'payment-domain::retry::RetryPolicy' },
          ],
        }),
      }),
    ]);
    const onNavigateToNode = vi.fn();
    render(ChangeView, {
      props: { repoId: 'repo-1', graph: GRAPH, onNavigateToNode },
    });
    await waitFor(() => expect(screen.getByText('RetryPolicy')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Added RetryPolicy/ }));
    expect(onNavigateToNode).toHaveBeenCalledTimes(1);
    expect(onNavigateToNode).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'n-retry', qualified_name: 'payment-domain::retry::RetryPolicy' })
    );
  });

  it('expands field changes for modified entries instead of navigating', async () => {
    repoGraphTimeline.mockResolvedValue([
      delta({
        id: 'd-mod',
        delta_json: JSON.stringify({
          nodes_modified: [
            {
              qualified_name: 'payment-domain::charge::ChargeService',
              field_changes: [{ field: 'complexity', old_value: '3', new_value: '6' }],
            },
          ],
        }),
      }),
    ]);
    const onNavigateToNode = vi.fn();
    render(ChangeView, {
      props: { repoId: 'repo-1', graph: GRAPH, onNavigateToNode },
    });
    await waitFor(() => expect(screen.getByText('ChargeService')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Modified ChargeService/ }));
    // Field-change detail appears (field name + old → new values)…
    expect(screen.getByText('complexity')).toBeTruthy();
    expect(screen.getByText('6')).toBeTruthy();
    // …and no Boundary navigation happened.
    expect(onNavigateToNode).not.toHaveBeenCalled();
  });

  it('shows an empty state when no deltas are recorded since the last visit', async () => {
    repoGraphTimeline.mockResolvedValue([]);
    render(ChangeView, { props: { repoId: 'repo-1', graph: GRAPH } });
    expect(await screen.findByText('No structural changes')).toBeTruthy();
  });

  it('surfaces a load error with a retry control', async () => {
    repoGraphTimeline.mockRejectedValue(new Error('boom'));
    render(ChangeView, { props: { repoId: 'repo-1', graph: GRAPH } });
    expect(await screen.findByText(/Failed to load changes: boom/)).toBeTruthy();
    expect(screen.getByText('Retry')).toBeTruthy();
  });
});
