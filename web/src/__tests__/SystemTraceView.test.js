/**
 * SystemTraceView.test.js — HSI §3 System Trace View
 * ("what actually happened?" — SDLC timeline for an MR).
 *
 * Data from GET /api/v1/merge-requests/:id/timeline. Event rows render
 * timestamp + type + detail; gate results expand to the individual gate
 * pass/fail; [View Conversation] [View Diff] [View Attestation] open inline
 * sections backed by the corresponding APIs.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import SystemTraceView from '../components/explorer/SystemTraceView.svelte';

const mergeRequests = vi.fn();
const mrTimeline = vi.fn();
const mrDiff = vi.fn();
const mrAttestation = vi.fn();
const conversationProvenance = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    mergeRequests: (...a) => mergeRequests(...a),
    mrTimeline: (...a) => mrTimeline(...a),
    mrDiff: (...a) => mrDiff(...a),
    mrAttestation: (...a) => mrAttestation(...a),
    conversationProvenance: (...a) => conversationProvenance(...a),
  },
}));

const MRS = [
  { id: 'mr-52', title: 'Payment retry endpoint', status: 'merged', author_agent_id: 'worker-12', spec_ref: 'specs/system/payment-retry.md@abc123', source_branch: 'feat/retry', target_branch: 'main' },
  { id: 'mr-51', title: 'Ledger fix', status: 'open', author_agent_id: 'worker-8' },
];

const EVENTS = [
  { timestamp: 1711324800, type: 'SpecLifecycleTrigger', detail: { spec_path: 'specs/system/payment-retry.md', task_id: 'task-42' } },
  { timestamp: 1711324815, type: 'AgentSpawned', detail: { agent_id: 'worker-12', persona: 'backend-dev' } },
  { timestamp: 1711324860, type: 'GitPush', detail: { commit_sha: 'abc1234567890', files_changed: 3 } },
  { timestamp: 1711324861, type: 'GateResult', detail: { gate: 'cargo-test', status: 'pass' } },
  { timestamp: 1711324862, type: 'GateResult', detail: { gate: 'cargo-clippy', status: 'fail' } },
  { timestamp: 1711324863, type: 'GraphExtraction', detail: { commit_sha: 'abc1234567890', nodes_added: 2, nodes_modified: 1 } },
  { timestamp: 1711324870, type: 'MergeQueueEnqueued', detail: { position: 3 } },
  { timestamp: 1711324880, type: 'Merged', detail: {} },
];

beforeEach(() => {
  vi.clearAllMocks();
  mergeRequests.mockResolvedValue(MRS);
  mrTimeline.mockResolvedValue({ mr_id: 'mr-52', events: EVENTS });
  mrDiff.mockResolvedValue({ diff: '--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n+new line' });
  mrAttestation.mockResolvedValue({
    attestation: {
      merge_commit_sha: 'deadbeef',
      merged_at: 1711324880,
      conversation_sha: 'conv-sha-1',
      gate_results: [{ gate_id: 'g1', gate_name: 'cargo-test', status: 'pass' }],
    },
  });
  conversationProvenance.mockResolvedValue({ turns: [{ role: 'assistant', content: 'Created RetryPolicy type' }] });
});

describe('SystemTraceView', () => {
  it('loads the MR list for the repository and auto-selects the first', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(mergeRequests).toHaveBeenCalledWith({ repository_id: 'repo-1' }));
    expect(await screen.findByText('System Trace')).toBeTruthy();
  });

  it('renders the vertical SDLC timeline with typed event rows', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => {
      expect(screen.getByText('Spec lifecycle')).toBeTruthy();
    });
    expect(screen.getByText('Agent spawned')).toBeTruthy();
    expect(screen.getByText('Git push')).toBeTruthy();
    expect(screen.getByText('Graph extraction')).toBeTruthy();
    expect(screen.getByText('Merge queue')).toBeTruthy();
    expect(screen.getByText('Merged')).toBeTruthy();

    const timeline = document.querySelector('.st-timeline');
    expect(Number(timeline.dataset.count)).toBe(EVENTS.length);
    const types = [...timeline.querySelectorAll('.st-event')].map((e) => e.dataset.eventType);
    expect(types.filter((t) => t === 'GateResult')).toHaveLength(2);
  });

  it('shows the MR header with author and spec reference', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByText('Payment retry endpoint')).toBeTruthy());
    const meta = document.querySelector('.st-mr-meta');
    expect(meta.textContent).toContain('worker-12');
    expect(meta.textContent).toContain('specs/system/payment-retry.md@abc123');
    expect(meta.textContent).toContain('feat/retry');
  });

  it('expands gate results to show individual gate pass/fail', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByText('cargo-test: pass')).toBeTruthy());

    // Gate rows are buttons; the collapsed detail is not shown yet.
    const gateRow = screen.getByRole('button', { name: /cargo-test/ });
    expect(document.querySelector('[data-testid="gate-detail"]')).toBeNull();

    await fireEvent.click(gateRow);
    const detail = document.querySelector('[data-testid="gate-detail"]');
    expect(detail).toBeTruthy();
    expect(detail.textContent).toContain('cargo-test');
    expect(detail.textContent).toContain('pass');

    await fireEvent.click(gateRow);
    expect(document.querySelector('[data-testid="gate-detail"]')).toBeNull();
  });

  it('opens the diff inline from the View Diff link', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByText('Merged')).toBeTruthy());
    await fireEvent.click(screen.getByText('View Diff'));
    expect(mrDiff).toHaveBeenCalledWith('mr-52');
    const section = await screen.findByTestId('st-section-diff');
    expect(section.textContent).toContain('+new line');
  });

  it('opens the attestation inline from the View Attestation link', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByText('Merged')).toBeTruthy());
    await fireEvent.click(screen.getByText('View Attestation'));
    expect(mrAttestation).toHaveBeenCalledWith('mr-52');
    const section = await screen.findByTestId('st-section-attestation');
    expect(section.textContent).toContain('deadbeef');
    expect(section.textContent).toContain('cargo-test');
  });

  it('opens the conversation inline, resolving the SHA from the attestation', async () => {
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByText('Merged')).toBeTruthy());
    await fireEvent.click(screen.getByText('View Conversation'));
    expect(mrAttestation).toHaveBeenCalledWith('mr-52');
    await waitFor(() => expect(conversationProvenance).toHaveBeenCalledWith('conv-sha-1'));
    const section = await screen.findByTestId('st-section-conversation');
    expect(section.textContent).toContain('Created RetryPolicy type');
  });

  it('switches MRs from the selector and reloads the timeline', async () => {
    mrTimeline.mockResolvedValue({ mr_id: 'mr-51', events: [{ timestamp: 1, type: 'GitPush', detail: { commit_sha: 'fff' } }] });
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    const select = await screen.findByLabelText(/Select merge request/);
    await fireEvent.change(select, { target: { value: 'mr-51' } });
    await waitFor(() => expect(mrTimeline).toHaveBeenCalledWith('mr-51'));
  });

  it('renders an empty state when the repository has no merge requests', async () => {
    mergeRequests.mockResolvedValue([]);
    render(SystemTraceView, { props: { repoId: 'repo-1' } });
    expect(await screen.findByText('No merge requests for this repository')).toBeTruthy();
  });
});
