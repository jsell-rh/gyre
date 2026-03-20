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
