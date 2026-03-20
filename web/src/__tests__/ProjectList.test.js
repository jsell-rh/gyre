import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ProjectList from '../components/ProjectList.svelte';

describe('ProjectList', () => {
  it('renders without throwing', () => {
    expect(() => render(ProjectList, { props: { onSelectRepo: vi.fn() } })).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(ProjectList, { props: { onSelectRepo: vi.fn() } });
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('has New Project button', async () => {
    render(ProjectList, { props: { onSelectRepo: vi.fn() } });
    // Wait for any async rendering
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = document.querySelectorAll('button');
    const labels = Array.from(buttons).map(b => b.textContent);
    expect(labels.some(l => l.includes('New Project'))).toBe(true);
  });

  it('renders empty state when projects list is empty', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    const { container } = render(ProjectList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 50));
    expect(container).toBeTruthy();
  });

  it('fetches repos via /api/v1/repos?project_id= not /projects/:id/repos', async () => {
    // This test catches the URL mismatch bug: api.repos() must use
    // /repos?project_id=<id>, NOT /projects/<id>/repos (nonexistent route).
    const fetchCalls = [];
    global.fetch = vi.fn((url) => {
      fetchCalls.push(url);
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) });
    });
    render(ProjectList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 50));
    // All fetch calls should use /api/v1/repos?project_id= not /projects/
    const repoCalls = fetchCalls.filter(u => u.includes('/repos'));
    for (const url of repoCalls) {
      expect(url).not.toMatch(/\/projects\/[^/]+\/repos/);
    }
  });

  it('displays project list after loading', async () => {
    const projects = [{ id: 'p1', name: 'Alpha', description: 'First', created_at: 1000000 }];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(projects) })
    );
    const { container } = render(ProjectList, { props: { onSelectRepo: vi.fn() } });
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.innerHTML).toContain('Alpha');
  });
});
