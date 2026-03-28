import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import TaskBoard from '../components/TaskBoard.svelte';

// ─── Helpers ─────────────────────────────────────────────────────────────────

const TASKS = [
  {
    id: 'task-1',
    title: 'Fix the login bug',
    status: 'backlog',
    priority: 'High',
    assigned_to: null,
    labels: [],
    created_at: 1000,
    updated_at: 1000,
  },
  {
    id: 'task-2',
    title: 'Implement OAuth',
    status: 'in_progress',
    priority: 'Medium',
    assigned_to: 'agent-1',
    labels: [],
    created_at: 2000,
    updated_at: 2000,
  },
  {
    id: 'task-3',
    title: 'Write unit tests',
    status: 'review',
    priority: 'Low',
    assigned_to: 'agent-2',
    labels: ['testing'],
    created_at: 3000,
    updated_at: 3000,
  },
  {
    id: 'task-4',
    title: 'Deploy to staging',
    status: 'done',
    priority: 'Critical',
    assigned_to: 'agent-1',
    labels: ['ops', 'deploy'],
    pr_link: 'https://github.com/org/repo/pull/42',
    created_at: 4000,
    updated_at: 4000,
  },
  {
    id: 'task-5',
    title: 'Waiting on infra',
    status: 'blocked',
    priority: 'Medium',
    assigned_to: null,
    labels: [],
    spec_path: 'specs/infra/deploy.md',
    created_at: 5000,
    updated_at: 5000,
  },
];

function mockFetch(data) {
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      statusText: 'OK',
      json: () => Promise.resolve(data),
    })
  );
}

function mockFetchError(message = 'Server error') {
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: false,
      status: 500,
      statusText: message,
      json: () => Promise.resolve({ error: message }),
    })
  );
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe('TaskBoard — rendering', () => {
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

  it('shows all 5 column headers', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const headers = container.querySelectorAll('.col-header');
    // During loading we still see 5 columns
    expect(headers.length).toBe(5);
  });

  it('displays sr-only live region for accessibility', () => {
    const { container } = render(TaskBoard);
    const srOnly = container.querySelector('.sr-only[aria-live="polite"]');
    expect(srOnly).toBeTruthy();
  });
});

describe('TaskBoard — loading & skeleton', () => {
  it('shows skeleton cards during loading', () => {
    const { container } = render(TaskBoard);
    // While loading=true, board shows skeleton elements
    const board = container.querySelector('.board[aria-busy]');
    expect(board).toBeTruthy();
  });

  it('aria-busy is set on the board during loading', () => {
    const { container } = render(TaskBoard);
    const board = container.querySelector('[aria-busy]');
    expect(board).toBeTruthy();
  });
});

describe('TaskBoard — data rendering', () => {
  beforeEach(() => {
    mockFetch(TASKS);
  });

  it('renders task cards when API returns tasks', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const text = container.textContent;
    expect(text).toContain('Fix the login bug');
    expect(text).toContain('Implement OAuth');
    expect(text).toContain('Write unit tests');
    expect(text).toContain('Deploy to staging');
    expect(text).toContain('Waiting on infra');
  });

  it('places tasks in correct kanban columns by status', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const columns = container.querySelectorAll('.column');
    expect(columns.length).toBe(5);
    // Backlog = 0, InProgress = 1, Review = 2, Done = 3, Blocked = 4
    expect(columns[0].textContent).toContain('Fix the login bug');
    expect(columns[1].textContent).toContain('Implement OAuth');
    expect(columns[2].textContent).toContain('Write unit tests');
    expect(columns[3].textContent).toContain('Deploy to staging');
    expect(columns[4].textContent).toContain('Waiting on infra');
  });

  it('shows correct task count per column', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const counts = container.querySelectorAll('.col-count');
    const nonSkel = Array.from(counts).filter(el => !el.classList.contains('col-count-skel'));
    // 5 columns, each with 1 task
    expect(nonSkel.length).toBe(5);
    nonSkel.forEach(el => {
      expect(el.textContent.trim()).toBe('1');
    });
  });

  it('shows total task count in header', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.textContent).toContain('5 tasks total');
  });

  it('shows assignee when present', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const assignees = container.querySelectorAll('.assignee');
    const texts = Array.from(assignees).map(el => el.textContent);
    expect(texts).toContain('agent-1');
    expect(texts).toContain('agent-2');
  });

  it('shows label pills when task has labels', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const pills = container.querySelectorAll('.label-pill');
    const texts = Array.from(pills).map(el => el.textContent);
    expect(texts).toContain('testing');
    expect(texts).toContain('ops');
    expect(texts).toContain('deploy');
  });

  it('shows PR link when task has pr_link', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const prLinks = container.querySelectorAll('.pr-link');
    expect(prLinks.length).toBe(1);
    expect(prLinks[0].getAttribute('href')).toBe('https://github.com/org/repo/pull/42');
    expect(prLinks[0].getAttribute('target')).toBe('_blank');
    expect(prLinks[0].getAttribute('rel')).toBe('noreferrer');
  });

  it('shows spec chip when task has spec_path', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const specChips = container.querySelectorAll('.spec-chip');
    expect(specChips.length).toBe(1);
    expect(specChips[0].getAttribute('title')).toBe('specs/infra/deploy.md');
  });
});

describe('TaskBoard — empty state', () => {
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

  it('shows "0 tasks total" when no tasks', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.textContent).toContain('0 tasks total');
  });
});

describe('TaskBoard — error handling', () => {
  it('shows error message when API fails', async () => {
    mockFetchError('Internal Server Error');
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const errorMsg = container.querySelector('.error-msg');
    expect(errorMsg).toBeTruthy();
    expect(errorMsg.textContent).toContain('Error');
  });

  it('shows retry button on error', async () => {
    mockFetchError('Server error');
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const retryBtn = container.querySelector('.btn-retry');
    expect(retryBtn).toBeTruthy();
    expect(retryBtn.textContent).toBe('Retry');
  });

  it('retries loading when retry button is clicked', async () => {
    mockFetchError('Server error');
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    // Now make fetch succeed
    mockFetch(TASKS);
    const retryBtn = container.querySelector('.btn-retry');
    await fireEvent.click(retryBtn);
    await new Promise(resolve => setTimeout(resolve, 100));

    expect(container.textContent).toContain('Fix the login bug');
  });
});

describe('TaskBoard — filters', () => {
  beforeEach(() => {
    mockFetch(TASKS);
  });

  it('has priority filter dropdown', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const selects = container.querySelectorAll('select[aria-label="Filter by priority"]');
    expect(selects.length).toBe(1);
  });

  it('has agent filter dropdown', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const selects = container.querySelectorAll('select[aria-label="Filter by agent"]');
    expect(selects.length).toBe(1);
  });

  it('agent filter contains unique agents from tasks', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    const agentSelect = container.querySelector('select[aria-label="Filter by agent"]');
    const options = Array.from(agentSelect.querySelectorAll('option')).map(o => o.value);
    // First option is "" (all), then unique agents sorted
    expect(options).toContain('agent-1');
    expect(options).toContain('agent-2');
  });

  it('filters tasks by priority', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    const prioritySelect = container.querySelector('select[aria-label="Filter by priority"]');
    await fireEvent.change(prioritySelect, { target: { value: 'Critical' } });
    await new Promise(resolve => setTimeout(resolve, 50));

    // Only "Deploy to staging" has Critical priority
    const cards = container.querySelectorAll('.task-card');
    // Visible cards should only be the Critical one
    const cardTitles = Array.from(cards).map(c => c.querySelector('.card-title')?.textContent);
    expect(cardTitles.filter(Boolean)).toContain('Deploy to staging');
    expect(cardTitles.filter(Boolean)).not.toContain('Fix the login bug');
  });

  it('filters tasks by agent', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    const agentSelect = container.querySelector('select[aria-label="Filter by agent"]');
    await fireEvent.change(agentSelect, { target: { value: 'agent-2' } });
    await new Promise(resolve => setTimeout(resolve, 50));

    // Only task-3 is assigned to agent-2
    const cards = container.querySelectorAll('.task-card');
    const cardTitles = Array.from(cards).map(c => c.querySelector('.card-title')?.textContent).filter(Boolean);
    expect(cardTitles).toContain('Write unit tests');
    expect(cardTitles).not.toContain('Implement OAuth');
  });
});

describe('TaskBoard — task selection', () => {
  beforeEach(() => {
    mockFetch(TASKS);
  });

  it('calls onSelectTask when a task card is clicked', async () => {
    const onSelectTask = vi.fn();
    const { container } = render(TaskBoard, { props: { onSelectTask } });
    await new Promise(resolve => setTimeout(resolve, 100));

    const cards = container.querySelectorAll('.task-card.clickable');
    expect(cards.length).toBeGreaterThan(0);
    await fireEvent.click(cards[0]);
    expect(onSelectTask).toHaveBeenCalledTimes(1);
    expect(onSelectTask).toHaveBeenCalledWith(expect.objectContaining({ id: 'task-1' }));
  });

  it('cards have role=button when onSelectTask is provided', async () => {
    const onSelectTask = vi.fn();
    const { container } = render(TaskBoard, { props: { onSelectTask } });
    await new Promise(resolve => setTimeout(resolve, 100));

    const cards = container.querySelectorAll('.task-card[role="button"]');
    expect(cards.length).toBe(5);
  });

  it('cards do NOT have role=button when no onSelectTask', async () => {
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));

    const cards = container.querySelectorAll('.task-card[role="button"]');
    expect(cards.length).toBe(0);
  });

  it('supports keyboard activation on task cards', async () => {
    const onSelectTask = vi.fn();
    const { container } = render(TaskBoard, { props: { onSelectTask } });
    await new Promise(resolve => setTimeout(resolve, 100));

    const cards = container.querySelectorAll('.task-card.clickable');
    await fireEvent.keyDown(cards[0], { key: 'Enter' });
    expect(onSelectTask).toHaveBeenCalledTimes(1);
  });

  it('supports space key activation on task cards', async () => {
    const onSelectTask = vi.fn();
    const { container } = render(TaskBoard, { props: { onSelectTask } });
    await new Promise(resolve => setTimeout(resolve, 100));

    const cards = container.querySelectorAll('.task-card.clickable');
    await fireEvent.keyDown(cards[0], { key: ' ' });
    expect(onSelectTask).toHaveBeenCalledTimes(1);
  });
});

describe('TaskBoard — new task modal', () => {
  it('opens new task modal on button click', async () => {
    render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = Array.from(document.querySelectorAll('button'));
    const newTaskBtn = buttons.find(b => b.textContent.includes('New Task'));
    expect(newTaskBtn).toBeTruthy();
    await fireEvent.click(newTaskBtn);
    // Modal should now be open with form fields
    await new Promise(resolve => setTimeout(resolve, 50));
    const inputs = document.querySelectorAll('.form-input');
    expect(inputs.length).toBeGreaterThan(0);
  });

  it('modal has priority select with all options', async () => {
    render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = Array.from(document.querySelectorAll('button'));
    const newTaskBtn = buttons.find(b => b.textContent.includes('New Task'));
    await fireEvent.click(newTaskBtn);
    await new Promise(resolve => setTimeout(resolve, 50));

    const selects = document.querySelectorAll('.form-input');
    const options = Array.from(document.querySelectorAll('option'));
    const optValues = options.map(o => o.value);
    expect(optValues).toContain('Critical');
    expect(optValues).toContain('High');
    expect(optValues).toContain('Medium');
    expect(optValues).toContain('Low');
  });

  it('modal has status select with all statuses', async () => {
    render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = Array.from(document.querySelectorAll('button'));
    const newTaskBtn = buttons.find(b => b.textContent.includes('New Task'));
    await fireEvent.click(newTaskBtn);
    await new Promise(resolve => setTimeout(resolve, 50));

    const options = Array.from(document.querySelectorAll('option'));
    const optValues = options.map(o => o.value);
    expect(optValues).toContain('backlog');
    expect(optValues).toContain('in_progress');
    expect(optValues).toContain('review');
    expect(optValues).toContain('done');
    expect(optValues).toContain('blocked');
  });

  it('create button is disabled when title is empty', async () => {
    render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 0));
    const buttons = Array.from(document.querySelectorAll('button'));
    const newTaskBtn = buttons.find(b => b.textContent.includes('New Task'));
    await fireEvent.click(newTaskBtn);
    await new Promise(resolve => setTimeout(resolve, 50));

    const createBtn = Array.from(document.querySelectorAll('button')).find(
      b => b.textContent.trim().endsWith('Task') && !b.textContent.includes('New')
    );
    if (createBtn) {
      expect(createBtn.disabled).toBe(true);
    }
  });
});

describe('TaskBoard — API response formats', () => {
  it('handles { tasks: [...] } response shape', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({
        ok: true,
        status: 200,
        statusText: 'OK',
        json: () => Promise.resolve({ tasks: TASKS.slice(0, 2) }),
      })
    );
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.textContent).toContain('Fix the login bug');
    expect(container.textContent).toContain('Implement OAuth');
  });

  it('handles empty array response', async () => {
    mockFetch([]);
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.textContent).toContain('0 tasks total');
  });

  it('handles null response gracefully', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({
        ok: true,
        status: 200,
        statusText: 'OK',
        json: () => Promise.resolve(null),
      })
    );
    const { container } = render(TaskBoard);
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(container.textContent).toContain('0 tasks total');
  });
});

describe('TaskBoard — workspaceId prop', () => {
  it('passes workspaceId to API call', async () => {
    mockFetch([]);
    render(TaskBoard, { props: { workspaceId: 'ws-42' } });
    await new Promise(resolve => setTimeout(resolve, 100));
    expect(global.fetch).toHaveBeenCalled();
    const url = global.fetch.mock.calls[0][0];
    expect(url).toContain('workspace_id=ws-42');
  });
});
