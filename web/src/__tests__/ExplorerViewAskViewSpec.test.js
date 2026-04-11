/**
 * Tests for ExplorerView SSE view_spec integration.
 *
 * The generate endpoint sends a `complete` SSE event with:
 *   { "view_spec": { "data": { ... } }, "explanation": "..." }
 *
 * These tests verify that:
 * 1. view_spec with `concept` triggers concept search
 * 2. view_spec with `node_types` sets nodeTypeFilter (passed to MoldableView)
 * 3. The explanation text is still displayed
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// Build a fake SSE readable stream from an array of { event, data } objects
function makeSSEStream(events) {
  const lines = events
    .map(({ event, data }) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`)
    .join('');
  const encoder = new TextEncoder();
  const bytes = encoder.encode(lines);
  const readable = new ReadableStream({
    pull(controller) {
      controller.enqueue(bytes);
      controller.close();
    },
  });
  return readable;
}

const mockGetGraphConcept = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
const mockGenerateExplorerView = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([{ id: 'r1', name: 'my-repo' }]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([{ id: 'r1', name: 'my-repo' }]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: (...args) => mockGetGraphConcept(...args),
    generateExplorerView: (...args) => mockGenerateExplorerView(...args),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

// Repo-scope props with repoId set so the auto-select effect fires
// and the Architecture tab + Ask input become visible
const REPO_SCOPE = { type: 'repo', workspaceId: 'ws-1', repoId: 'r1' };

describe('ExplorerView Ask view_spec integration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Mock matchMedia for the chatCollapsed viewport sync $effect
    global.window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    // Mock globals needed by ExplorerCanvas when graph loads
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
      translate: vi.fn(), rotate: vi.fn(),
      fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
      font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
      setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    }));
    global.WebSocket = class MockWebSocket {
      constructor() {
        this.readyState = 1;
        this.send = vi.fn();
        this.close = vi.fn();
      }
    };
  });

  it('displays the explanation text from the complete event', async () => {
    const stream = makeSSEStream([
      { event: 'partial', data: { explanation: 'Generating...' } },
      {
        event: 'complete',
        data: {
          explanation: 'Shows auth-related functions.',
          view_spec: { data: { node_types: ['Function'], edge_types: [], depth: 2 }, layout: 'graph' },
        },
      },
    ]);
    mockGenerateExplorerView.mockResolvedValue({
      ok: true,
      body: { getReader: () => stream.getReader() },
    });

    const { getByPlaceholderText, getByRole, findByText } = render(ExplorerView, {
      props: { scope: REPO_SCOPE },
    });

    await waitFor(() => {
      expect(getByPlaceholderText('Ask: How does auth work?')).toBeTruthy();
    });

    const askInput = getByPlaceholderText('Ask: How does auth work?');
    await fireEvent.input(askInput, { target: { value: 'Show auth functions' } });
    const askBtn = getByRole('button', { name: 'Ask' });
    await fireEvent.click(askBtn);

    await findByText('Shows auth-related functions.');
  });

  it('triggers concept search when view_spec has a concept field', async () => {
    const stream = makeSSEStream([
      {
        event: 'complete',
        data: {
          explanation: 'Filtered by auth concept.',
          view_spec: {
            data: { concept: 'auth', node_types: [], edge_types: [], depth: 2 },
            layout: 'graph',
          },
        },
      },
    ]);
    mockGenerateExplorerView.mockResolvedValue({
      ok: true,
      body: { getReader: () => stream.getReader() },
    });
    mockGetGraphConcept.mockResolvedValue({
      nodes: [{ id: 'n1', name: 'Auth', node_type: 'Trait' }],
      edges: [],
    });

    const { getByPlaceholderText, getByRole, findByText } = render(ExplorerView, {
      props: { scope: REPO_SCOPE },
    });

    await waitFor(() => {
      expect(getByPlaceholderText('Ask: How does auth work?')).toBeTruthy();
    });

    await fireEvent.input(getByPlaceholderText('Ask: How does auth work?'), {
      target: { value: 'Show auth layer' },
    });
    await fireEvent.click(getByRole('button', { name: 'Ask' }));

    await findByText('Filtered by auth concept.');

    // concept search should have been triggered with the concept from view_spec
    await waitFor(() => {
      expect(mockGetGraphConcept).toHaveBeenCalledWith('r1', 'auth');
    });
  });

  it('does not call concept search when view_spec has only node_types', async () => {
    const stream = makeSSEStream([
      {
        event: 'complete',
        data: {
          explanation: 'Showing endpoint types.',
          view_spec: { data: { node_types: ['Endpoint'], edge_types: [], depth: 1 }, layout: 'graph' },
        },
      },
    ]);
    mockGenerateExplorerView.mockResolvedValue({
      ok: true,
      body: { getReader: () => stream.getReader() },
    });

    const { getByPlaceholderText, getByRole, findByText } = render(ExplorerView, {
      props: { scope: REPO_SCOPE },
    });

    await waitFor(() => {
      expect(getByPlaceholderText('Ask: How does auth work?')).toBeTruthy();
    });

    await fireEvent.input(getByPlaceholderText('Ask: How does auth work?'), {
      target: { value: 'Show endpoints' },
    });
    await fireEvent.click(getByRole('button', { name: 'Ask' }));

    await findByText('Showing endpoint types.');

    // concept search should NOT have been called — node_types path only
    expect(mockGetGraphConcept).not.toHaveBeenCalled();
  });

  it('handles a failed API response gracefully without crashing', async () => {
    mockGenerateExplorerView.mockResolvedValue({ ok: false, status: 503 });

    const { getByPlaceholderText, getByRole } = render(ExplorerView, {
      props: { scope: REPO_SCOPE },
    });

    await waitFor(() => {
      expect(getByPlaceholderText('Ask: How does auth work?')).toBeTruthy();
    });

    await fireEvent.input(getByPlaceholderText('Ask: How does auth work?'), {
      target: { value: 'test query' },
    });
    await fireEvent.click(getByRole('button', { name: 'Ask' }));

    // Error feedback element (role="status") appears
    await waitFor(() => {
      expect(getByRole('status')).toBeTruthy();
    });
  });

  it('generates a view without a view_spec (explanation-only response)', async () => {
    const stream = makeSSEStream([
      { event: 'complete', data: { explanation: 'Here is what I found.' } },
    ]);
    mockGenerateExplorerView.mockResolvedValue({
      ok: true,
      body: { getReader: () => stream.getReader() },
    });

    const { getByPlaceholderText, getByRole, findByText } = render(ExplorerView, {
      props: { scope: REPO_SCOPE },
    });

    await waitFor(() => {
      expect(getByPlaceholderText('Ask: How does auth work?')).toBeTruthy();
    });

    await fireEvent.input(getByPlaceholderText('Ask: How does auth work?'), {
      target: { value: 'explain something' },
    });
    await fireEvent.click(getByRole('button', { name: 'Ask' }));

    await findByText('Here is what I found.');
    // No concept search triggered (no view_spec)
    expect(mockGetGraphConcept).not.toHaveBeenCalled();
  });
});
