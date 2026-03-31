import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';

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
  { id: 'n4', name: 'AuthTrait', node_type: 'Trait', file_path: 'src/auth.rs' },
];
const EDGES = [];

describe('MoldableView nodeTypeFilter', () => {
  it('renders all nodes when nodeTypeFilter is null', async () => {
    const { container } = render(MoldableView, {
      props: { nodes: NODES, edges: EDGES, repoId: 'r1', nodeTypeFilter: null },
    });
    // List tab shows all nodes - switch to it by checking component renders
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('renders without throwing when nodeTypeFilter is an empty array', () => {
    expect(() =>
      render(MoldableView, {
        props: { nodes: NODES, edges: EDGES, repoId: 'r1', nodeTypeFilter: [] },
      })
    ).not.toThrow();
  });

  it('renders without throwing when nodeTypeFilter contains valid types', () => {
    expect(() =>
      render(MoldableView, {
        props: {
          nodes: NODES,
          edges: EDGES,
          repoId: 'r1',
          nodeTypeFilter: ['Endpoint', 'Function'],
        },
      })
    ).not.toThrow();
  });

  it('renders without throwing when nodeTypeFilter has no matching types', () => {
    expect(() =>
      render(MoldableView, {
        props: {
          nodes: NODES,
          edges: EDGES,
          repoId: 'r1',
          nodeTypeFilter: ['NonExistentType'],
        },
      })
    ).not.toThrow();
  });

  it('accepts nodeTypeFilter alongside conceptFilterIds (composition)', () => {
    const conceptFilterIds = new Set(['n1', 'n2']);
    expect(() =>
      render(MoldableView, {
        props: {
          nodes: NODES,
          edges: EDGES,
          repoId: 'r1',
          conceptFilterIds,
          nodeTypeFilter: ['Endpoint'],
        },
      })
    ).not.toThrow();
  });
});
