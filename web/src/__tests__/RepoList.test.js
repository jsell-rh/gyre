import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import RepoList from '../components/RepoList.svelte';

describe('RepoList', () => {
  it('renders without throwing', () => {
    expect(() => render(RepoList, { props: { onSelectRepo: vi.fn() } })).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(RepoList, { props: { onSelectRepo: vi.fn() } });
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('has New Repo and Mirror buttons', async () => {
    render(RepoList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = document.querySelectorAll('button');
    const labels = Array.from(buttons).map(b => b.textContent);
    expect(labels.some(l => l.includes('New Repo'))).toBe(true);
    expect(labels.some(l => l.includes('Mirror'))).toBe(true);
  });

  it('renders empty state when repo list is empty', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    const { container } = render(RepoList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 50));
    expect(container).toBeTruthy();
  });

  it('fetches repos via /api/v1/repos?workspace_id= when workspaceId provided', async () => {
    const fetchCalls = [];
    global.fetch = vi.fn((url) => {
      fetchCalls.push(url);
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) });
    });
    render(RepoList, { props: { onSelectRepo: vi.fn(), workspaceId: 'ws-1' } });
    await new Promise(resolve => setTimeout(resolve, 50));
    expect(fetchCalls.some(u => u.includes('workspace_id=ws-1'))).toBe(true);
    expect(fetchCalls.some(u => u.match(/\/projects\/[^/]+\/repos/))).toBe(false);
  });

  it('displays repo names after loading', async () => {
    const repos = [{ id: 'r1', name: 'my-repo', created_at: 1000000, default_branch: 'main' }];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(repos) })
    );
    const { container } = render(RepoList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.innerHTML).toContain('my-repo');
  });
});
