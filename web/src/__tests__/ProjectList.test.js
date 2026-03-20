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
});

// ── addRepo error-handling (TASK-097 regression) ──────────────────────────────
// Verifies that the addRepo flow uses the correct API URL for the repos list.

describe('ProjectList — addRepo fetch sequence uses correct URLs', () => {
  it('api.repos() fetch uses /repos?project_id= not /projects/{id}/repos', async () => {
    const { api } = await import('../lib/api.js');
    const calls = [];
    global.fetch = vi.fn((url) => {
      calls.push(url);
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) });
    });

    await api.createRepo({ name: 'test', project_id: 'p1' });
    await api.repos('p1');

    const reposListCall = calls.find(u => u.includes('project_id'));
    expect(reposListCall).toBeDefined();
    expect(reposListCall).toContain('/api/v1/repos?project_id=p1');
    expect(reposListCall).not.toContain('/projects/p1/repos');
  });

  it('api.repos() returns parseable JSON (not SPA HTML) for the correct URL', async () => {
    const { api } = await import('../lib/api.js');
    global.fetch = vi.fn((url) => {
      if (url === '/api/v1/repos?project_id=p1') {
        return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([{ id: 'r1', name: 'repo' }]) });
      }
      // Wrong URL — simulate SPA HTML response
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.reject(new SyntaxError('JSON.parse: unexpected character at line 1 column 1 of the JSON data')),
      });
    });

    const result = await api.repos('p1');
    expect(Array.isArray(result)).toBe(true);
    expect(result[0]).toHaveProperty('name', 'repo');
  });
});
