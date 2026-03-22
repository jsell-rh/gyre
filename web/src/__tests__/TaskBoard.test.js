import { describe, it, expect, vi, beforeEach } from 'vitest';
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

  it('renders task cards when API returns tasks', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({
        ok: true,
        status: 200,
        statusText: 'OK',
        json: () => Promise.resolve([
          {
            id: 'task-1',
            title: 'Fix the login bug',
            status: 'backlog',
            priority: 'high',
            assigned_to: null,
            labels: [],
            created_at: 1000,
            updated_at: 1000,
          },
          {
            id: 'task-2',
            title: 'Implement OAuth',
            status: 'in_progress',
            priority: 'medium',
            assigned_to: 'agent-1',
            labels: [],
            created_at: 2000,
            updated_at: 2000,
          },
          {
            id: 'task-3',
            title: 'Write unit tests',
            status: 'review',
            priority: 'low',
            assigned_to: null,
            labels: ['testing'],
            created_at: 3000,
            updated_at: 3000,
          },
        ]),
      })
    );

    const { container } = render(TaskBoard);
    // Wait for the async loadTasks to complete
    await new Promise(resolve => setTimeout(resolve, 100));

    const text = container.textContent;
    expect(text).toContain('Fix the login bug');
    expect(text).toContain('Implement OAuth');
    expect(text).toContain('Write unit tests');
  });

  it('places tasks in correct kanban columns by status', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({
        ok: true,
        status: 200,
        statusText: 'OK',
        json: () => Promise.resolve([
          {
            id: 'task-a',
            title: 'Backlog Task',
            status: 'backlog',
            priority: 'medium',
            assigned_to: null,
            labels: [],
            created_at: 1000,
            updated_at: 1000,
          },
          {
            id: 'task-b',
            title: 'In Progress Task',
            status: 'in_progress',
            priority: 'high',
            assigned_to: null,
            labels: [],
            created_at: 2000,
            updated_at: 2000,
          },
          {
            id: 'task-c',
            title: 'Done Task',
            status: 'done',
            priority: 'low',
            assigned_to: null,
            labels: [],
            created_at: 3000,
            updated_at: 3000,
          },
        ]),
      })
    );

    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    const columns = container.querySelectorAll('.column');
    // columns: Backlog, InProgress, Review, Done, Blocked (5 total)
    expect(columns.length).toBe(5);

    // Backlog column (index 0) should contain "Backlog Task"
    expect(columns[0].textContent).toContain('Backlog Task');
    // InProgress column (index 1) should contain "In Progress Task"
    expect(columns[1].textContent).toContain('In Progress Task');
    // Done column (index 3) should contain "Done Task"
    expect(columns[3].textContent).toContain('Done Task');
  });

  it('shows zero task count per column when no tasks', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    const counts = container.querySelectorAll('.col-count');
    const nonSkeletonCounts = Array.from(counts).filter(
      el => !el.classList.contains('col-count-skel')
    );
    nonSkeletonCounts.forEach(el => {
      expect(el.textContent.trim()).toBe('0');
    });
  });
});
