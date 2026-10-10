// REVIEW PROBE (temporary — not part of the candidate): verify the
// cross-workspace Decisions section renders items from the server's real
// envelope shape { notifications: [...] } (users.rs get_my_notifications).
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => {
  const methods = {
    myNotifications: vi.fn(),
    workspaces: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    budgetSummary: vi.fn().mockResolvedValue(null),
    adminHealth: vi.fn().mockResolvedValue(null),
    adminJobs: vi.fn().mockResolvedValue([]),
    version: vi.fn().mockResolvedValue(null),
    activity: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    getSpecs: vi.fn().mockResolvedValue([]),
    agents: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    markNotificationRead: vi.fn().mockResolvedValue({}),
  };
  return { api: new Proxy(methods, {
    get(target, name) {
      if (!(name in target)) target[name] = vi.fn().mockResolvedValue(null);
      return target[name];
    },
  }) };
});

import { api } from '../lib/api.js';
import CrossWorkspaceHome from '../components/CrossWorkspaceHome.svelte';

beforeEach(() => vi.clearAllMocks());

describe('review probe: CWH decisions envelope', () => {
  it('renders notification rows from the {notifications:[...]} envelope', async () => {
    api.myNotifications.mockResolvedValue({
      notifications: [
        { id: 'n-1', notification_type: 'spec_approval', title: 'Approve auth.md', message: 'Approve auth.md', workspace_id: 'ws-1', priority: 5 },
      ],
      limit: 50, offset: 0,
    });
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments' }]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="cwh-decision-item"]');
      expect(items.length).toBe(1);
    }, { timeout: 4000 });
    expect(container.textContent).toContain('Approve auth.md');
  });
});
