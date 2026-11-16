/**
 * DiffView tests (ui-layout.md §10 Layout Engines: `diff` — custom component
 * with added/modified/removed node cards, evaluated against real node state).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { onViewEvent } from '../lib/viewEvents.js';

vi.mock('../lib/api.js', () => ({
  api: {
    repoGraphTimeline: vi.fn().mockResolvedValue([
      { id: 'd2', commit_sha: 'bbbbbbb2', timestamp: 2000 },
      { id: 'd1', commit_sha: 'aaaaaaa1', timestamp: 1000 },
    ]),
  },
}));

import DiffView from '../lib/DiffView.svelte';

const NOW = 100_000;

function node(id, fields) {
  return {
    id,
    name: `node-${id}`,
    node_type: 'function',
    file_path: `src/${id}.rs`,
    first_seen_at: 500,
    last_modified_at: 600,
    deleted_at: null,
    ...fields,
  };
}

beforeEach(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

describe('DiffView', () => {
  it('classifies added / modified / removed nodes against the baseline', async () => {
    // Baseline = delta d1 (t=1000). Compare = d2 (t=2000).
    const nodes = [
      node('added', { first_seen_at: 1500 }), // appeared after baseline
      node('modified', { first_seen_at: 500, last_modified_at: 1500 }), // touched in window
      node('removed', { first_seen_at: 400, deleted_at: 1800 }), // soft-deleted after baseline
      node('stable', { first_seen_at: 500, last_modified_at: 700 }), // untouched
    ];
    const { container } = render(DiffView, {
      props: { nodes, edges: [], repoId: 'r1' },
    });

    await waitFor(() => {
      expect(container.querySelector('.diff-columns')).toBeTruthy();
    });

    const added = container.querySelectorAll("[aria-label='Added nodes'] .diff-card");
    const modified = container.querySelectorAll("[aria-label='Modified nodes'] .diff-card");
    const removed = container.querySelectorAll("[aria-label='Removed nodes'] .diff-card");
    expect(added).toHaveLength(1);
    expect(added[0].textContent).toContain('node-added');
    expect(modified).toHaveLength(1);
    expect(modified[0].textContent).toContain('node-modified');
    expect(removed).toHaveLength(1);
    expect(removed[0].textContent).toContain('node-removed');
  });

  it('shows an empty state when nothing changed', async () => {
    const nodes = [node('stable', { first_seen_at: 500, last_modified_at: 700 })];
    const { container } = render(DiffView, {
      props: { nodes, edges: [], repoId: 'r1' },
    });
    await waitFor(() => {
      expect(container.querySelector('.diff-empty')).toBeTruthy();
    });
  });

  it('emits ViewEvents and calls onSelectNode from cards', async () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    const onSelectNode = vi.fn();
    const nodes = [node('added', { first_seen_at: 1500 })];
    const { container } = render(DiffView, {
      props: { nodes, edges: [], repoId: 'r1', onSelectNode },
    });
    await waitFor(() => {
      expect(container.querySelector('.diff-card')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('.diff-card'));
    expect(onSelectNode).toHaveBeenCalledTimes(1);
    expect(handler.mock.calls.at(-1)?.[0]).toMatchObject({
      type: 'click',
      entity_type: 'node',
      entity_id: 'added',
    });
    await fireEvent.contextMenu(container.querySelector('.diff-card'));
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('context-menu');
    unsub();
  });
});
