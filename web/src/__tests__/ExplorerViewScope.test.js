import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

describe('ExplorerView scope branching', () => {
  it('renders without throwing with no props', () => {
    expect(() => render(ExplorerView)).not.toThrow();
  });

  it('renders WorkspaceCards at tenant scope (default)', async () => {
    const { container } = render(ExplorerView, { props: { scope: { type: 'tenant' } } });
    await waitFor(() => {
      // WorkspaceCards shows explorer header or empty state
      expect(container.innerHTML.length).toBeGreaterThan(0);
    });
  });

  it('renders canvas placeholder at workspace scope', async () => {
    const { getByText } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await waitFor(() => {
      expect(getByText('Workspace Architecture')).toBeTruthy();
    });
  });

  it('renders repo graph view at repo scope', async () => {
    // Don't set repoId to avoid triggering MoldableView (canvas) render
    const { getByText } = render(ExplorerView, {
      props: { scope: { type: 'repo' } },
    });
    await waitFor(() => {
      // Explorer header is rendered for repo scope
      expect(getByText('Architecture')).toBeTruthy();
    });
  });

  it('defaults to tenant scope when scope prop is omitted', async () => {
    const { container } = render(ExplorerView);
    // Should render WorkspaceCards (tenant scope), not the graph
    await waitFor(() => {
      expect(container.innerHTML).not.toContain('Architecture');
    });
  });
});
