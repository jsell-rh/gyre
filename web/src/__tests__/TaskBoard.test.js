import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import TaskBoard from '../components/TaskBoard.svelte';

describe('TaskBoard', () => {
  it('renders without throwing', () => {
    expect(() => render(TaskBoard)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(TaskBoard);
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('has kanban column labels', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 50));
    const text = container.textContent;
    expect(text).toMatch(/Backlog|In Progress|Review|Done/);
  });

  it('has New Task button', async () => {
    render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = document.querySelectorAll('button');
    const labels = Array.from(buttons).map(b => b.textContent.trim());
    expect(labels.some(l => l.includes('New Task'))).toBe(true);
  });
});
