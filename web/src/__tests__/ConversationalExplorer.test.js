/**
 * ConversationalExplorer.test.js — system-explorer.md §8 "Conversational
 * Exploration" (task-187).
 *
 * Mounted-component tests for ConversationalExplorer.svelte:
 *   - a question goes to POST /workspaces/:id/explorer-views/ask and the
 *     grounded answer renders with its entity references as clickable
 *     links (click → the right navigation callback per reference type)
 *   - node references in a response highlight the referenced nodes on the
 *     canvas (onHighlightNodes)
 *   - "show me X" responses apply the returned view spec (onApplyView)
 *   - "what would happen if…" responses trigger the structural prediction
 *     (onPrediction) and render a prediction chip
 *   - follow-up questions maintain conversation context (history is sent)
 *   - request failures surface as messages, not silent failures
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const explorerAsk = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    explorerAsk: (...args) => explorerAsk(...args),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import ConversationalExplorer from '../lib/ConversationalExplorer.svelte';

const GROUNDED_RESPONSE = {
  answer:
    'SearchService [entity:node:node-svc] was created to implement search.md in ' +
    '[entity:commit:abc123abc123] by [entity:agent:agent-worker-7], governed by ' +
    '[entity:spec:specs/system/search.md].',
  entity_refs: [
    { type: 'node', id: 'node-svc', label: 'SearchService' },
    { type: 'commit', id: 'abc123abc123', label: 'abc123a' },
    { type: 'agent', id: 'agent-worker-7', label: 'agent-worker-7' },
    { type: 'spec', id: 'specs/system/search.md', label: 'specs/system/search.md' },
  ],
  view_spec: null,
  prediction: null,
};

beforeEach(() => {
  explorerAsk.mockReset();
});

describe('ConversationalExplorer', () => {
  it('renders the question and answer with clickable entity references', async () => {
    explorerAsk.mockResolvedValue(GROUNDED_RESPONSE);
    const { container } = render(ConversationalExplorer, {
      props: { workspaceId: 'ws-1', repoId: 'repo-1' },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'Why does SearchService implement both ports?' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      expect(explorerAsk).toHaveBeenCalledTimes(1);
    });
    // The question went to the ask endpoint with the repo scope.
    expect(explorerAsk).toHaveBeenCalledWith('ws-1', {
      question: 'Why does SearchService implement both ports?',
      repo_id: 'repo-1',
      history: [],
    });

    await waitFor(() => {
      expect(container.textContent).toContain('Why does SearchService implement both ports?');
    });
    // All four entity references render as clickable links with labels.
    const refs = container.querySelectorAll('[data-testid="entity-ref"]');
    expect(refs.length).toBe(4);
    expect(refs[0].textContent).toBe('SearchService');
    expect(refs[0].dataset.entityType).toBe('node');
    expect(refs[1].dataset.entityType).toBe('commit');
    expect(refs[2].dataset.entityType).toBe('agent');
    expect(refs[3].dataset.entityType).toBe('spec');
  });

  it('clicking an entity reference dispatches navigation by type', async () => {
    explorerAsk.mockResolvedValue(GROUNDED_RESPONSE);
    const onOpenNode = vi.fn();
    const onOpenSpec = vi.fn();
    const onOpenCommit = vi.fn();
    const onShowAgent = vi.fn();
    const { container } = render(ConversationalExplorer, {
      props: {
        workspaceId: 'ws-1',
        onOpenNode,
        onOpenSpec,
        onOpenCommit,
        onShowAgent,
      },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'Why?' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="entity-ref"]').length).toBe(4);
    });
    const refs = container.querySelectorAll('[data-testid="entity-ref"]');
    fireEvent.click(refs[0]); // node
    expect(onOpenNode).toHaveBeenCalledWith('node-svc');
    fireEvent.click(refs[1]); // commit
    expect(onOpenCommit).toHaveBeenCalledWith('abc123abc123');
    fireEvent.click(refs[2]); // agent
    expect(onShowAgent).toHaveBeenCalledWith('agent-worker-7');
    fireEvent.click(refs[3]); // spec
    expect(onOpenSpec).toHaveBeenCalledWith('specs/system/search.md');
  });

  it('highlights referenced nodes on the canvas and applies view specs', async () => {
    explorerAsk.mockResolvedValue({
      ...GROUNDED_RESPONSE,
      view_spec: {
        name: 'Cache implementors',
        data: { node_types: ['Type'], edge_types: [], depth: 1 },
        layout: 'list',
      },
    });
    const onApplyView = vi.fn();
    const onHighlightNodes = vi.fn();
    const { container } = render(ConversationalExplorer, {
      props: { workspaceId: 'ws-1', onApplyView, onHighlightNodes },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'Show me other services that implement CachePort' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      expect(onApplyView).toHaveBeenCalledTimes(1);
    });
    expect(onApplyView.mock.calls[0][0].name).toBe('Cache implementors');
    // The node references in the answer highlight on the canvas.
    expect(onHighlightNodes).toHaveBeenCalledWith(['node-svc']);
    // The view card renders with an Apply button.
    expect(container.querySelector('[data-testid="conv-view-card"]').textContent).toContain('Cache implementors');
  });

  it('applies structural predictions from what-if questions', async () => {
    explorerAsk.mockResolvedValue({
      answer: 'Removing CachePort would break SearchService.',
      entity_refs: [],
      view_spec: null,
      prediction: {
        repo_id: 'repo-1',
        predictions: [
          { name: 'SearchService', action: 'change', confidence: 'high', reason: 'implements CachePort' },
        ],
      },
    });
    const onPrediction = vi.fn();
    const { container } = render(ConversationalExplorer, {
      props: { workspaceId: 'ws-1', onPrediction },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'What would happen if I removed CachePort?' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      expect(onPrediction).toHaveBeenCalledTimes(1);
    });
    expect(onPrediction.mock.calls[0][0].repo_id).toBe('repo-1');
    expect(onPrediction.mock.calls[0][0].predictions.length).toBe(1);
    expect(container.querySelector('[data-testid="conv-prediction-chip"]')).toBeTruthy();
    expect(container.textContent).toContain('Removing CachePort would break SearchService.');
  });

  it('sends conversation history on follow-up questions', async () => {
    explorerAsk.mockResolvedValue(GROUNDED_RESPONSE);
    const { container } = render(ConversationalExplorer, {
      props: { workspaceId: 'ws-1' },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'Why does SearchService implement both ports?' } });
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => {
      expect(explorerAsk).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="entity-ref"]').length).toBe(4);
    });

    // Follow-up: the first question + answer travel as history so the
    // LLM can build on them ("show me the others" knows the subject).
    fireEvent.input(box, { target: { value: 'Show me the others' } });
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => {
      expect(explorerAsk).toHaveBeenCalledTimes(2);
    });
    const secondCall = explorerAsk.mock.calls[1];
    expect(secondCall[0]).toBe('ws-1');
    expect(secondCall[1].question).toBe('Show me the others');
    expect(secondCall[1].history).toEqual([
      { role: 'user', content: 'Why does SearchService implement both ports?' },
      { role: 'assistant', content: GROUNDED_RESPONSE.answer },
    ]);
  });

  it('renders endpoint failures as error messages', async () => {
    explorerAsk.mockRejectedValue(new Error('500 internal'));
    const { container } = render(ConversationalExplorer, {
      props: { workspaceId: 'ws-1' },
    });

    const box = container.querySelector('textarea');
    fireEvent.input(box, { target: { value: 'Why?' } });
    fireEvent.keyDown(box, { key: 'Enter' });

    await waitFor(() => {
      expect(container.textContent).toContain('500 internal');
    });
  });
});
