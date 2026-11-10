/**
 * GeneratedViewChat.test.js — HSI §3 "Generated Views (LLM-Powered,
 * On-Demand)" (task-089).
 *
 * Mounted-component tests for GeneratedViewChat.svelte:
 *   - a natural-language question calls the workspace generate endpoint
 *     and applies the returned view spec (graph query + layout) to the
 *     canvas via onApplyView, with the LLM explanation rendered
 *   - follow-up questions carry the previous view definition
 *     (previous_spec) so the LLM refines the current view
 *   - "Save this view" persists the generated spec (query + layout +
 *     explanation) via POST /workspaces/:id/explorer-views
 *   - an invalid LLM output (view_spec: null + fallback) still renders
 *     the fallback view instead of crashing
 *   - endpoint errors surface as messages, not silent failures
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const generateExplorerView = vi.fn();
const saveExplorerView = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    generateExplorerView: (...args) => generateExplorerView(...args),
    saveExplorerView: (...args) => saveExplorerView(...args),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import GeneratedViewChat from '../lib/GeneratedViewChat.svelte';

function sseResponse(events) {
  const text = events
    .map(({ event, data }) => `event: ${event}\ndata: ${JSON.stringify(data)}\n`)
    .join('\n');
  return {
    ok: true,
    status: 200,
    text: () => Promise.resolve(text),
  };
}

function errorResponse(status) {
  return {
    ok: false,
    status,
    text: () => Promise.resolve(''),
  };
}

const AUTH_SPEC = {
  name: 'How auth works',
  description: 'Authentication view',
  data: { concept: 'auth', node_types: ['Module', 'Function'], edge_types: [], depth: 2 },
  layout: 'hierarchical',
  explanation: 'Authentication flows through require_auth_middleware.',
};

const FLOW_SPEC = {
  name: 'Payment retry flow',
  data: {
    concept: 'retry',
    node_types: ['Function'],
    edge_types: [],
    depth: 3,
    trace_source: { mr_id: 'mr-47' },
  },
  layout: 'flow',
  explanation: 'Retry requests flow from the endpoint through the gateway.',
};

beforeEach(() => {
  generateExplorerView.mockReset();
  saveExplorerView.mockReset();
});

describe('GeneratedViewChat', () => {
  it('applies the generated view spec and renders the LLM explanation', async () => {
    generateExplorerView.mockResolvedValue(
      sseResponse([
        { event: 'partial', data: { explanation: 'Generating view...' } },
        { event: 'complete', data: { view_spec: AUTH_SPEC, explanation: AUTH_SPEC.explanation } },
      ])
    );
    const onApplyView = vi.fn();
    const { container, getByRole } = render(GeneratedViewChat, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1', onApplyView },
    });

    const input = getByRole('textbox');
    fireEvent.input(input, { target: { value: 'How does authentication work?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(onApplyView).toHaveBeenCalledWith(AUTH_SPEC);
    });
    // The question went to the workspace generate endpoint with the repo scope.
    expect(generateExplorerView).toHaveBeenCalledWith('ws-1', {
      question: 'How does authentication work?',
      repo_id: 'repo-1',
    });
    // The LLM's explanation is rendered (sidebar annotation content).
    expect(container.textContent).toContain('Authentication flows through require_auth_middleware.');
    // The view card shows the generated name + layout.
    expect(container.querySelector('[data-testid="generated-view-card"]').textContent).toContain('How auth works');
    expect(container.querySelector('[data-testid="generated-view-layout"]').textContent).toBe('hierarchical');
  });

  it('sends previous_spec on follow-up questions (refinement)', async () => {
    generateExplorerView.mockResolvedValue(
      sseResponse([{ event: 'complete', data: { view_spec: AUTH_SPEC, explanation: 'e1' } }])
    );
    const { getByRole, findByRole } = render(GeneratedViewChat, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1', onApplyView: () => {} },
    });

    const input = getByRole('textbox');
    fireEvent.input(input, { target: { value: 'How does authentication work?' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    // Wait for the turn to fully complete (view card rendered — this is
    // after status returns to 'ready', so the follow-up is not dropped by
    // the thinking guard).
    await findByRole('button', { name: 'Save this view' });
    // First call: no previous spec.
    expect(generateExplorerView.mock.calls[0][1].previous_spec).toBeUndefined();

    // Follow-up refines the current view.
    generateExplorerView.mockResolvedValue(
      sseResponse([{ event: 'complete', data: { view_spec: FLOW_SPEC, explanation: 'e2' } }])
    );
    fireEvent.input(input, { target: { value: 'Show me just the middleware chain' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => {
      expect(generateExplorerView).toHaveBeenCalledTimes(2);
    });
    expect(generateExplorerView.mock.calls[1][1]).toEqual({
      question: 'Show me just the middleware chain',
      repo_id: 'repo-1',
      previous_spec: AUTH_SPEC,
    });
  });

  it('saves the generated view (query + layout + explanation) to workspace views', async () => {
    generateExplorerView.mockResolvedValue(
      sseResponse([{ event: 'complete', data: { view_spec: AUTH_SPEC, explanation: AUTH_SPEC.explanation } }])
    );
    saveExplorerView.mockResolvedValue({ id: 'view-1' });
    const { getByRole, findByRole } = render(GeneratedViewChat, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1', onApplyView: () => {} },
    });

    const input = getByRole('textbox');
    fireEvent.input(input, { target: { value: 'How does authentication work?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    const saveBtn = await findByRole('button', { name: 'Save this view' });
    fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(saveExplorerView).toHaveBeenCalledWith('ws-1', {
        name: 'How auth works',
        description: 'Authentication flows through require_auth_middleware.',
        spec: AUTH_SPEC,
      });
    });
  });

  it('renders the fallback view when the LLM output is invalid', async () => {
    generateExplorerView.mockResolvedValue(
      sseResponse([
        {
          event: 'complete',
          data: {
            view_spec: null,
            explanation: 'Generated view was invalid — try rephrasing',
            fallback: { name: 'Fallback list view', data: { node_types: [], edge_types: [], depth: 1 }, layout: 'list' },
          },
        },
      ])
    );
    const onApplyView = vi.fn();
    const { getByRole, container } = render(GeneratedViewChat, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1', onApplyView },
    });

    const input = getByRole('textbox');
    fireEvent.input(input, { target: { value: 'What are the main components?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(onApplyView).toHaveBeenCalled();
    });
    // The fallback view spec is what gets applied (client renders it instead
    // of crashing), and the explanation tells the user what happened.
    expect(onApplyView.mock.calls[0][0].layout).toBe('list');
    expect(container.textContent).toContain('Generated view was invalid');
  });

  it('surfaces endpoint errors as messages', async () => {
    generateExplorerView.mockResolvedValue(errorResponse(503));
    const onApplyView = vi.fn();
    const { getByRole, container } = render(GeneratedViewChat, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1', onApplyView },
    });

    const input = getByRole('textbox');
    fireEvent.input(input, { target: { value: 'How does auth work?' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(container.textContent).toContain('Something went wrong generating the view');
    });
    expect(onApplyView).not.toHaveBeenCalled();
    // Status returns to ready so the user can retry.
    expect(container.textContent).toContain('Ready');
  });
});
