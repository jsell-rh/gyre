import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import NotificationDrawer from '../components/NotificationDrawer.svelte';

// Fixtures across priority bands and read states. `priority_level` is the
// four-level band the drawer groups by; `priority` is the HSI rank.
const makeNotification = (overrides = {}) => ({
  id: 'notif-1',
  notification_type: 'AgentFailed',
  priority: 3,
  priority_level: 'High',
  title: 'Agent failed',
  body: JSON.stringify({ message: 'worker-3 exited nonzero' }),
  workspace_id: 'ws-1',
  repo_id: null,
  action_url: '/workspaces/acme/r/payment-api/agents/agent-9',
  read: false,
  read_at: null,
  created_at: Math.floor(Date.now() / 1000) - 120,
  ...overrides,
});

const fixtures = [
  makeNotification({ id: 'n-low', priority_level: 'Low', priority: 9, title: 'Suggested link', read: true }),
  makeNotification({ id: 'n-urgent', priority_level: 'Urgent', priority: 1, title: 'Queue escalation', notification_type: 'MergeQueueEscalation' }),
  makeNotification({ id: 'n-high', priority_level: 'High', priority: 3, title: 'Agent failed' }),
  makeNotification({ id: 'n-medium', priority_level: 'Medium', priority: 6, title: 'Meta-spec drift', notification_type: 'MetaSpecDrift' }),
];

const apiMocks = {
  notifications: vi.fn(),
  markNotificationRead: vi.fn(),
  markNotificationUnread: vi.fn(),
  markAllNotificationsRead: vi.fn(),
  workspaceRepos: vi.fn(),
};

vi.mock('../lib/api.js', () => ({
  api: {
    notifications: (...args) => apiMocks.notifications(...args),
    markNotificationRead: (...args) => apiMocks.markNotificationRead(...args),
    markNotificationUnread: (...args) => apiMocks.markNotificationUnread(...args),
    markAllNotificationsRead: (...args) => apiMocks.markAllNotificationsRead(...args),
    workspaceRepos: (...args) => apiMocks.workspaceRepos(...args),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastError: vi.fn(),
  toast: vi.fn(),
}));

// Skeleton/EmptyState pull no API. The unread-count refresh inside the
// drawer uses raw fetch (mocked globally in setup.js).

describe('NotificationDrawer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.notifications.mockResolvedValue({ notifications: fixtures.map(f => ({ ...f })) });
    apiMocks.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'payment-api' }]);
  });

  it('groups notifications by priority, Urgent first', async () => {
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-list"]').textContent).toContain('Queue escalation');
    });
    const groups = [...container.querySelectorAll('.priority-group')].map(g => g.dataset.priority);
    expect(groups).toEqual(['Urgent', 'High', 'Medium', 'Low']);

    // Urgent group contains only the urgent item.
    const urgent = container.querySelector('.priority-group[data-priority="Urgent"]');
    expect(urgent.textContent).toContain('Queue escalation');
    expect(urgent.textContent).not.toContain('Suggested link');
  });

  it('marks a notification read via the read endpoint and updates the item', async () => {
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => expect(container.querySelectorAll('[data-testid="notification-item"]').length).toBe(4));

    const toggle = container.querySelector('[data-testid="notification-toggle-n-high"]');
    await fireEvent.click(toggle);
    await waitFor(() => expect(apiMocks.markNotificationRead).toHaveBeenCalledWith('n-high'));

    // The read state now comes from the local update: the item loses the
    // unread styling and the toggle flips.
    await waitFor(() => {
      const item = container.querySelector('[data-testid="notification-item"][data-id="n-high"]');
      expect(item.classList.contains('unread')).toBe(false);
    });
  });

  it('marks unread via the unread endpoint for an already-read item', async () => {
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => expect(container.querySelectorAll('[data-testid="notification-item"]').length).toBe(4));

    await fireEvent.click(container.querySelector('[data-testid="notification-toggle-n-low"]'));
    await waitFor(() => expect(apiMocks.markNotificationUnread).toHaveBeenCalledWith('n-low'));
  });

  it('mark all read calls the bulk endpoint', async () => {
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => expect(container.querySelector('[data-testid="mark-all-read"]')).toBeTruthy());

    await fireEvent.click(container.querySelector('[data-testid="mark-all-read"]'));
    await waitFor(() => expect(apiMocks.markAllNotificationsRead).toHaveBeenCalledWith(undefined));

    await waitFor(() => {
      const unreadItems = container.querySelectorAll('.notif-item.unread');
      expect(unreadItems.length).toBe(0);
    });
  });

  it('deep-links to the entity on click: pushes the action_url and replays popstate routing', async () => {
    const pushSpy = vi.spyOn(window.history, 'pushState').mockImplementation(() => {});
    let popstateCount = 0;
    const onPop = () => { popstateCount += 1; };
    window.addEventListener('popstate', onPop);

    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => expect(container.querySelectorAll('[data-testid="notification-item"]').length).toBe(4));

    // Unread item: clicking opens the deep link (and marks read).
    await fireEvent.click(container.querySelector('[data-testid="notification-open-n-urgent"]'));
    await waitFor(() => expect(pushSpy).toHaveBeenCalledWith({}, '', '/workspaces/acme/r/payment-api/agents/agent-9'));
    expect(popstateCount).toBeGreaterThan(0);

    window.removeEventListener('popstate', onPop);
    pushSpy.mockRestore();
  });

  it('applies the priority filter to the API query', async () => {
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => expect(apiMocks.notifications).toHaveBeenCalled());

    await fireEvent.change(container.querySelector('[data-testid="filter-priority"]'), { target: { value: 'Urgent' } });
    await waitFor(() => {
      expect(apiMocks.notifications).toHaveBeenLastCalledWith(
        expect.objectContaining({ priority: 'Urgent' })
      );
    });
  });

  it('loads repos for the workspace filter and passes repo_id through', async () => {
    const { container } = render(NotificationDrawer, {
      open: true,
      workspaces: [{ id: 'ws-1', name: 'Acme' }],
    });
    await waitFor(() => expect(apiMocks.notifications).toHaveBeenCalled());

    await fireEvent.change(container.querySelector('[data-testid="filter-workspace"]'), { target: { value: 'ws-1' } });
    await waitFor(() => expect(apiMocks.workspaceRepos).toHaveBeenCalledWith('ws-1'));

    const repoSelect = container.querySelector('[data-testid="filter-repo"]');
    // Wait for the repo option to actually render (the select enables on
    // workspace selection, before the repo list resolves) — changing a
    // select whose options haven't rendered can't set the value.
    await waitFor(() =>
      expect(repoSelect.querySelector('option[value="repo-1"]')).toBeTruthy()
    );
    await fireEvent.change(repoSelect, { target: { value: 'repo-1' } });
    await waitFor(() => {
      expect(apiMocks.notifications).toHaveBeenLastCalledWith(
        expect.objectContaining({ workspace_id: 'ws-1', repo_id: 'repo-1' })
      );
    });
  });

  it('shows the empty state when there are no notifications', async () => {
    apiMocks.notifications.mockResolvedValue({ notifications: [] });
    const { container } = render(NotificationDrawer, { open: true, workspaces: [] });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-list"]').textContent).not.toContain('Agent failed');
    });
    expect(container.querySelector('[data-testid="mark-all-read"]')).toBeFalsy();
  });
});
