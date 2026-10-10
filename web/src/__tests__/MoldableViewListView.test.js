/**
 * Regression test: the list layout must render one table row per node.
 *
 * A prior registry-dispatch edit (task-170) clobbered the list view's
 * <tbody>, leaving a header-only table. This test fails on that state.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrTrace: vi.fn().mockResolvedValue(null),
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import MoldableView from '../lib/MoldableView.svelte';

const NODES = [
  { id: 'n1', name: 'AuthService', node_type: 'Function', file_path: 'src/auth.rs' },
  { id: 'n2', name: 'UserEndpoint', node_type: 'Endpoint', file_path: 'src/users.rs' },
  { id: 'n3', name: 'UserModel', node_type: 'Struct', file_path: 'src/models.rs' },
];

beforeEach(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

describe('MoldableView list layout', () => {
  it('renders one table row per node in the list layout', async () => {
    const { container } = render(MoldableView, {
      props: { nodes: NODES, edges: [], repoId: 'r1' },
    });
    // Switch to the list layout via its registry-driven tab.
    await fireEvent.click(container.querySelector('#tab-list'));

    const rows = container.querySelectorAll('.list-table tbody tr');
    expect(rows.length).toBe(NODES.length);
    // Row content sanity: every node name appears in a row cell.
    const rowText = [...rows].map((r) => r.textContent);
    for (const node of NODES) {
      expect(rowText.some((t) => t.includes(node.name))).toBe(true);
    }
  });
});
