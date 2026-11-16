/**
 * SpecRealizationView.test.js — HSI §3 Spec Realization View
 * ("what implements this spec?").
 *
 * Two-panel layout: spec list (status badge + linked-node count + task
 * progress) on the left, linked graph nodes on the right. Links come from
 * GovernedBy edges (canonical) and node spec_path/spec_paths fields.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import SpecRealizationView from '../components/explorer/SpecRealizationView.svelte';

const getSpecs = vi.fn();
const getSpecProgress = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    getSpecs: (...args) => getSpecs(...args),
    getSpecProgress: (...args) => getSpecProgress(...args),
  },
}));

const GRAPH = {
  nodes: [
    // Linked via spec_path field.
    {
      id: 'n-1',
      node_type: 'type',
      name: 'RetryPolicy',
      qualified_name: 'payment-domain::retry::RetryPolicy',
      spec_path: 'specs/system/payment-retry.md',
      spec_confidence: 'high',
    },
    // Linked via spec_paths (N:N) array.
    {
      id: 'n-2',
      node_type: 'function',
      name: 'should_retry',
      qualified_name: 'payment-domain::retry::should_retry',
      spec_paths: ['specs/system/payment-retry.md'],
    },
    // Linked ONLY via a GovernedBy edge (no spec_path field).
    {
      id: 'n-3',
      node_type: 'endpoint',
      name: 'POST /payments/retry',
      qualified_name: 'payment-api::POST /payments/retry',
    },
    // The spec node itself (excluded from realization list).
    {
      id: 'spec-node-1',
      node_type: 'spec',
      name: 'payment-retry.md',
      qualified_name: 'payment-retry.md',
      spec_path: 'specs/system/payment-retry.md',
    },
    // Unrelated node.
    {
      id: 'n-4',
      node_type: 'type',
      name: 'Unrelated',
      qualified_name: 'other::Unrelated',
      spec_path: 'specs/system/other.md',
    },
  ],
  edges: [
    {
      id: 'g-1',
      source_id: 'n-3',
      target_id: 'spec-node-1',
      edge_type: 'governed_by',
    },
  ],
};

const SPECS = [
  {
    path: 'specs/system/payment-retry.md',
    title: 'Payment Retry',
    approval_status: 'approved',
    repo_id: 'repo-1',
  },
  {
    path: 'specs/system/charge-processing.md',
    title: 'Charge Processing',
    approval_status: 'pending',
    repo_id: 'repo-1',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  getSpecs.mockResolvedValue(SPECS);
  getSpecProgress.mockResolvedValue({ total_tasks: 5, completed_tasks: 4 });
});

describe('SpecRealizationView', () => {
  it('renders the two-panel layout with spec statuses and linked-node counts', async () => {
    render(SpecRealizationView, { props: { repoId: 'repo-1', graph: GRAPH } });

    // Left panel: both specs with status badges and linked counts.
    const retryItem = await screen.findByText('payment-retry.md', { selector: '.sr-spec-name' });
    expect(retryItem.closest('button').textContent).toContain('Approved');
    expect(retryItem.closest('button').textContent).toContain('3 nodes');
    const chargeItem = screen.getByText('charge-processing.md', { selector: '.sr-spec-name' });
    expect(chargeItem.closest('button').textContent).toContain('Pending');
    expect(chargeItem.closest('button').textContent).toContain('0 nodes');

    // Right panel exists (realization).
    expect(screen.getByText('Realization')).toBeTruthy();
  });

  it('shows the nodes linked to the selected spec via spec_path, spec_paths and GovernedBy edges', async () => {
    render(SpecRealizationView, { props: { repoId: 'repo-1', graph: GRAPH } });

    // Auto-selects the spec with linked nodes.
    await waitFor(() => {
      expect(screen.getByText('RetryPolicy')).toBeTruthy();
    });
    expect(screen.getByText('should_retry')).toBeTruthy();
    expect(screen.getByText('POST /payments/retry')).toBeTruthy();
    // The unrelated node and the spec node itself are NOT realization entries.
    expect(screen.queryByText('Unrelated')).toBeNull();
    expect(screen.queryByText('payment-retry.md', { selector: '.sr-node-name' })).toBeNull();  });

  it('shows task progress for the selected spec', async () => {
    render(SpecRealizationView, { props: { repoId: 'repo-1', graph: GRAPH } });
    await waitFor(() => {
      expect(getSpecProgress).toHaveBeenCalledWith('specs/system/payment-retry.md');
    });
    expect(await screen.findByText(/4\/5 tasks/)).toBeTruthy();
  });

  it('navigates to implementation details when a linked node is clicked', async () => {
    const onSelectNode = vi.fn();
    render(SpecRealizationView, {
      props: { repoId: 'repo-1', graph: GRAPH, onSelectNode },
    });
    await waitFor(() => expect(screen.getByText('RetryPolicy')).toBeTruthy());
    await fireEvent.click(screen.getByText('RetryPolicy'));
    expect(onSelectNode).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'n-1', name: 'RetryPolicy' })
    );
  });

  it('opens the spec editor from the Read / Edit Spec link', async () => {
    const onOpenSpec = vi.fn();
    render(SpecRealizationView, {
      props: { repoId: 'repo-1', graph: GRAPH, onOpenSpec },
    });
    await waitFor(() => expect(screen.getByText('RetryPolicy')).toBeTruthy());
    await fireEvent.click(screen.getByText('Read / Edit Spec'));
    expect(onOpenSpec).toHaveBeenCalledWith('specs/system/payment-retry.md');
  });

  it('switches the selected spec from the left panel', async () => {
    render(SpecRealizationView, { props: { repoId: 'repo-1', graph: GRAPH } });
    await waitFor(() => expect(screen.getByText('RetryPolicy')).toBeTruthy());
    await fireEvent.click(screen.getByText('charge-processing.md'));
    // Right panel now shows the empty-realization state for the other spec.
    expect(await screen.findByText('No linked nodes')).toBeTruthy();
    expect(screen.queryByText('RetryPolicy')).toBeNull();
  });

  it('renders an empty state when no specs exist', async () => {
    getSpecs.mockResolvedValue([]);
    render(SpecRealizationView, { props: { repoId: 'repo-1', graph: GRAPH } });
    expect(await screen.findByText('No specs found')).toBeTruthy();
  });
});
