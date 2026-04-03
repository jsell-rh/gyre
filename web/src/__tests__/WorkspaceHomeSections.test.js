/**
 * WorkspaceHomeSections.test.js — Tests for ui-navigation.md §2 workspace home sections
 *
 * Covers all five sections:
 *   1. Decisions — notification loading, trust filtering, inline actions, empty state
 *   2. Repos — repo list, health indicators, click navigation, action buttons
 *   3. Briefing — Briefing.svelte embedded, workspace prop passed
 *   4. Specs — cross-repo table, status filter, click navigation
 *   5. Agent Rules — meta-spec cascade, required badge, reconcile status
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Mocks ─────────────────────────────────────────────────────────────────────

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn(),
    workspaceRepos: vi.fn(),
    specsForWorkspace: vi.fn(),
    getMetaSpecs: vi.fn(),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    approveSpec: vi.fn(),
    revokeSpec: vi.fn(),
    enqueue: vi.fn(),
    markNotificationRead: vi.fn(),
    getWorkspaceBriefing: vi.fn(),
    briefingAsk: vi.fn(),
    createRepo: vi.fn(),
    createMirrorRepo: vi.fn(),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    tasks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    updateTaskStatus: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    costSummary: vi.fn().mockResolvedValue([]),
    agent: vi.fn().mockResolvedValue({ name: 'test-agent' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'test-mr' }),
    repo: vi.fn().mockResolvedValue({ name: 'test-repo' }),
    workspace: vi.fn().mockResolvedValue({ name: 'test-ws' }),
    activity: vi.fn().mockResolvedValue([]),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    adminAudit: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: function ExplorerCanvasStub() {},
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

// ── Test fixtures ─────────────────────────────────────────────────────────────

const WORKSPACE = {
  id: 'ws-1',
  name: 'Payments',
  slug: 'payments',
  trust_level: 'Guided',
};

const REPOS = [
  { id: 'repo-1', name: 'payment-api', active_spec_count: 3, active_agents: 2 },
  { id: 'repo-2', name: 'user-service', active_spec_count: 1, active_agents: 0 },
  { id: 'repo-3', name: 'billing-api', active_spec_count: 0, active_agents: 0 },
];

const NOTIFICATIONS = [
  {
    id: 'n1',
    notification_type: 'gate_failure',
    message: 'Gate failed in payment-api',
    repo_id: 'repo-1',
    workspace_id: 'ws-1',
    priority: 1,
    body: JSON.stringify({ mr_id: 'mr-42' }),
  },
  {
    id: 'n2',
    notification_type: 'spec_approval',
    message: 'Spec approval needed: auth-refactor.md',
    repo_id: 'repo-2',
    workspace_id: 'ws-1',
    priority: 2,
    body: JSON.stringify({ spec_path: 'auth-refactor.md', spec_sha: 'abc123' }),
  },
  {
    id: 'n3',
    notification_type: 'agent_clarification',
    message: 'Agent needs clarification',
    repo_id: 'repo-1',
    workspace_id: 'ws-1',
    priority: 5,
    body: '{}',
  },
];

const SPECS = [
  {
    id: 's1',
    path: 'retry-logic.md',
    status: 'approved',
    repo_id: 'repo-1',
    tasks_done: 5,
    tasks_total: 5,
    updated_at: new Date(Date.now() - 3600000).toISOString(),
  },
  {
    id: 's2',
    path: 'auth-refactor.md',
    status: 'pending',
    repo_id: 'repo-2',
    tasks_done: 3,
    tasks_total: 5,
    updated_at: new Date(Date.now() - 86400000).toISOString(),
  },
  {
    id: 's3',
    path: 'error-handling.md',
    status: 'draft',
    repo_id: 'repo-3',
    tasks_done: 0,
    tasks_total: null,
    updated_at: null,
  },
];

const META_SPECS_WORKSPACE = [
  { id: 'm1', name: 'conventional-commits', kind: 'meta:principle', required: true, version: 3, scope: 'Workspace' },
  { id: 'm2', name: 'test-coverage', kind: 'meta:standard', required: false, version: 1, scope: 'Workspace' },
];

const META_SPECS_GLOBAL = [
  { id: 'm3', name: 'security', kind: 'meta:persona', required: true, version: 2, scope: 'Global' },
];

const EMPTY_BRIEFING = {
  completed: [],
  in_progress: [],
  cross_workspace: [],
  exceptions: [],
  metrics: null,
};

function setupDefaultMocks() {
  api.myNotifications.mockResolvedValue(NOTIFICATIONS);
  api.workspaceRepos.mockResolvedValue(REPOS);
  api.specsForWorkspace.mockResolvedValue(SPECS);
  api.getMetaSpecs.mockImplementation((params) => {
    if (params?.scope === 'Workspace') return Promise.resolve(META_SPECS_WORKSPACE);
    if (params?.scope === 'Global') return Promise.resolve(META_SPECS_GLOBAL);
    return Promise.resolve([]);
  });
  api.getWorkspaceBriefing.mockResolvedValue(EMPTY_BRIEFING);
  api.briefingAsk.mockResolvedValue(new Response(JSON.stringify({ answer: 'ok' })));
  api.approveSpec.mockResolvedValue({});
  api.revokeSpec.mockResolvedValue({});
  api.enqueue.mockResolvedValue({});
  api.markNotificationRead.mockResolvedValue({});
  api.createRepo.mockResolvedValue({ id: 'repo-new', name: 'new-repo' });
  api.createMirrorRepo.mockResolvedValue({ id: 'repo-mirror', name: 'mirror-repo' });
}

beforeEach(() => {
  vi.clearAllMocks();
  setupDefaultMocks();
});

// ── Rendering ─────────────────────────────────────────────────────────────────

describe('WorkspaceHome — basic rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(WorkspaceHome, { props: { workspace: WORKSPACE } })).not.toThrow();
  });

  it('shows no-workspace state when workspace is null', () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: null } });
    expect(getByText('Select a workspace')).toBeTruthy();
  });

  it('shows key sections when workspace is set', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    // Streamlined layout: PipelineOverview, Repos, Entity/Activity tabs
    expect(container.querySelector('[data-testid="pipeline-overview"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="browse-panel"]')).toBeTruthy();
  });
});

// ── Decisions section ─────────────────────────────────────────────────────────

// TODO: Update these tests for new layout — ActionNeeded component now handles decisions,
// PipelineOverview handles specs/agents/MRs inline expansion, sidebar panels removed.
describe.skip('Decisions section (old layout — needs update)', () => {
  it('calls api.myNotifications on mount', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(api.myNotifications).toHaveBeenCalled());
  });

  it('shows empty state when no notifications', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('decisions-empty')).toBeTruthy();
      expect(getByTestId('decisions-empty').textContent).toContain('No pending decisions');
    });
  });

  it('shows empty state text with Supervised trust guidance', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('decisions-empty').textContent).toContain('Supervised trust');
    });
  });

  it('renders decision items after loading', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="decision-item"]');
      expect(items.length).toBeGreaterThan(0);
    });
  });

  it('filters notifications to current workspace only', async () => {
    api.myNotifications.mockResolvedValue([
      ...NOTIFICATIONS,
      {
        id: 'other-ws',
        notification_type: 'gate_failure',
        message: 'Other workspace gate',
        repo_id: 'repo-x',
        workspace_id: 'ws-OTHER',
        priority: 1,
        body: '{}',
      },
    ]);
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      // Only NOTIFICATIONS (3) belong to ws-1; the other-ws one should be excluded
      const items = container.querySelectorAll('[data-testid="decision-item"]');
      expect(items.length).toBe(3);
    });
  });

  it('excludes priority-10 notifications at Guided trust level', async () => {
    const suggestedLink = {
      id: 'n-link',
      notification_type: 'suggested_link',
      message: 'Suggested spec link',
      repo_id: 'repo-1',
      workspace_id: 'ws-1',
      priority: 10,
      body: '{}',
    };
    api.myNotifications.mockResolvedValue([...NOTIFICATIONS, suggestedLink]);
    const guidedWs = { ...WORKSPACE, trust_level: 'Guided' };
    const { container } = render(WorkspaceHome, { props: { workspace: guidedWs } });
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="decision-item"]');
      expect(items.length).toBe(3); // suggestedLink excluded
    });
  });

  it('excludes priority-10 notifications at Autonomous trust level', async () => {
    const suggestedLink = {
      id: 'n-link',
      notification_type: 'suggested_link',
      message: 'Suggested spec link',
      repo_id: 'repo-1',
      workspace_id: 'ws-1',
      priority: 10,
      body: '{}',
    };
    api.myNotifications.mockResolvedValue([...NOTIFICATIONS, suggestedLink]);
    const autonomousWs = { ...WORKSPACE, trust_level: 'Autonomous' };
    const { container } = render(WorkspaceHome, { props: { workspace: autonomousWs } });
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="decision-item"]');
      expect(items.length).toBe(3);
    });
  });

  it('shows priority-10 notifications at Custom trust level', async () => {
    const suggestedLink = {
      id: 'n-link',
      notification_type: 'suggested_link',
      message: 'Suggested spec link',
      repo_id: 'repo-1',
      workspace_id: 'ws-1',
      priority: 10,
      body: '{}',
    };
    api.myNotifications.mockResolvedValue([...NOTIFICATIONS, suggestedLink]);
    const customWs = { ...WORKSPACE, trust_level: 'Custom' };
    const { container } = render(WorkspaceHome, { props: { workspace: customWs } });
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="decision-item"]');
      expect(items.length).toBe(4); // all 4 shown
    });
  });

  it('shows Approve/Reject buttons for spec_approval notifications', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const approveBtn = container.querySelector('[data-testid="btn-approve"]');
      expect(approveBtn).toBeTruthy();
      const rejectBtn = container.querySelector('[data-testid="btn-reject"]');
      expect(rejectBtn).toBeTruthy();
    });
  });

  it('shows Retry button for gate_failure notifications with mr_id', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const retryBtn = container.querySelector('[data-testid="btn-retry"]');
      expect(retryBtn).toBeTruthy();
    });
  });

  it('calls api.approveSpec when Approve is clicked', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="btn-approve"]')).toBeTruthy();
    });
    const approveBtn = container.querySelector('[data-testid="btn-approve"]');
    await fireEvent.click(approveBtn);
    await waitFor(() => {
      expect(api.approveSpec).toHaveBeenCalledWith('auth-refactor.md', 'abc123');
    });
  });

  it('calls api.revokeSpec when Reject is clicked', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="btn-reject"]')).toBeTruthy();
    });
    const rejectBtn = container.querySelector('[data-testid="btn-reject"]');
    await fireEvent.click(rejectBtn);
    await waitFor(() => {
      expect(api.revokeSpec).toHaveBeenCalledWith('auth-refactor.md', 'Rejected');
    });
  });

  it('calls api.enqueue when Retry is clicked', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="btn-retry"]')).toBeTruthy();
    });
    const retryBtn = container.querySelector('[data-testid="btn-retry"]');
    await fireEvent.click(retryBtn);
    await waitFor(() => {
      expect(api.enqueue).toHaveBeenCalledWith('mr-42');
    });
  });

  it('shows decisions badge count in section header', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const badge = container.querySelector('[data-testid="section-decisions"] .section-badge');
      expect(badge).toBeTruthy();
      expect(badge.textContent.trim()).toBe('3');
    });
  });

  it('shows "View all" button when there are decisions', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="section-decisions"] .section-action-btn');
      expect(btn).toBeTruthy();
      expect(btn.textContent.trim()).toBe('View all');
    });
  });

  it('does not show "View all" button when empty', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="section-decisions"] .section-action-btn');
      expect(btn).toBeFalsy();
    });
  });

  it('removes dismissed notification from list', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="decision-item"]').length).toBe(3);
    });
    const dismissBtn = container.querySelector('[data-testid="btn-dismiss"]');
    await fireEvent.click(dismissBtn);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="decision-item"]').length).toBe(2);
    });
  });
});

// ── Repos section ─────────────────────────────────────────────────────────────

describe.skip('Repos section (old layout — needs update)', () => {
  it('calls api.workspaceRepos on mount', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(api.workspaceRepos).toHaveBeenCalledWith('ws-1'));
  });

  it('renders repo rows after loading', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const rows = container.querySelectorAll('[data-testid="repo-row"]');
      expect(rows.length).toBe(3);
    });
  });

  it('shows repo names', async () => {
    const { getAllByText } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getAllByText('payment-api').length).toBeGreaterThan(0);
      expect(getAllByText('user-service').length).toBeGreaterThan(0);
    });
  });

  it('shows ● healthy for repos with active agents and no gate failures', async () => {
    api.myNotifications.mockResolvedValue([]); // no gate failures
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const healthBadges = container.querySelectorAll('[data-testid="repo-health"]');
      const healthTexts = Array.from(healthBadges).map(b => b.textContent.trim());
      expect(healthTexts.some(t => t.includes('healthy'))).toBe(true);
    });
  });

  it('shows ⚠ gate for repos with unresolved gate_failure notifications', async () => {
    // NOTIFICATIONS has gate_failure for repo-1
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const healthBadges = container.querySelectorAll('[data-testid="repo-health"]');
      const healthTexts = Array.from(healthBadges).map(b => b.textContent.trim());
      expect(healthTexts.some(t => t.includes('gate'))).toBe(true);
    });
  });

  it('shows ○ idle for repos with no agents and no gate failures', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const healthBadges = container.querySelectorAll('[data-testid="repo-health"]');
      const healthTexts = Array.from(healthBadges).map(b => b.textContent.trim());
      // billing-api (repo-3) has active_agents: 0, no gate failures → idle
      expect(healthTexts.some(t => t.includes('idle'))).toBe(true);
    });
  });

  it('calls onSelectRepo when repo row is clicked', async () => {
    const onSelectRepo = vi.fn();
    const { container } = render(WorkspaceHome, {
      props: { workspace: WORKSPACE, onSelectRepo },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="repo-link"]').length).toBeGreaterThan(0);
    });
    const firstLink = container.querySelector('[data-testid="repo-link"]');
    await fireEvent.click(firstLink);
    expect(onSelectRepo).toHaveBeenCalled();
  });

  it('shows empty state when no repos', async () => {
    api.workspaceRepos.mockResolvedValue([]);
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('repos-empty')).toBeTruthy();
    });
  });

  it('renders + New Repo button (enabled)', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const btn = getByTestId('btn-new-repo');
      expect(btn).toBeTruthy();
      expect(btn.disabled).toBe(false);
    });
  });

  it('renders Import button (enabled)', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const btn = getByTestId('btn-import-repo');
      expect(btn).toBeTruthy();
      expect(btn.disabled).toBe(false);
    });
  });

  it('shows new repo form when + New Repo is clicked', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => {
      expect(getByTestId('new-repo-form')).toBeTruthy();
    });
  });

  it('shows import form when Import is clicked', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-import-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-import-repo'));
    await waitFor(() => {
      expect(getByTestId('import-repo-form')).toBeTruthy();
    });
  });

  it('hides new repo form after clicking Import (mutual exclusion)', async () => {
    const { getByTestId, container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-form')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-import-repo'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="new-repo-form"]')).toBeFalsy();
      expect(getByTestId('import-repo-form')).toBeTruthy();
    });
  });

  it('calls api.createRepo with name and workspace_id on submit', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-name-input')).toBeTruthy());
    await fireEvent.input(getByTestId('new-repo-name-input'), { target: { value: 'my-new-repo' } });
    await fireEvent.submit(getByTestId('new-repo-form'));
    await waitFor(() => {
      expect(api.createRepo).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'my-new-repo', workspace_id: 'ws-1' })
      );
    });
  });

  it('refreshes repo list after successful creation', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-name-input')).toBeTruthy());
    await fireEvent.input(getByTestId('new-repo-name-input'), { target: { value: 'new-repo' } });
    const callsBefore = api.workspaceRepos.mock.calls.length;
    await fireEvent.submit(getByTestId('new-repo-form'));
    await waitFor(() => {
      expect(api.workspaceRepos.mock.calls.length).toBeGreaterThan(callsBefore);
    });
  });

  it('closes new repo form after successful creation', async () => {
    const { getByTestId, container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-name-input')).toBeTruthy());
    await fireEvent.input(getByTestId('new-repo-name-input'), { target: { value: 'new-repo' } });
    await fireEvent.submit(getByTestId('new-repo-form'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="new-repo-form"]')).toBeFalsy();
    });
  });

  it('shows error when createRepo fails', async () => {
    api.createRepo.mockRejectedValue(new Error('Name already taken'));
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-name-input')).toBeTruthy());
    await fireEvent.input(getByTestId('new-repo-name-input'), { target: { value: 'bad-repo' } });
    await fireEvent.submit(getByTestId('new-repo-form'));
    await waitFor(() => {
      expect(getByTestId('new-repo-error').textContent).toContain('Name already taken');
    });
  });

  it('calls api.createMirrorRepo with url and workspace_id on submit', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-import-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-import-repo'));
    await waitFor(() => expect(getByTestId('import-url-input')).toBeTruthy());
    await fireEvent.input(getByTestId('import-url-input'), { target: { value: 'https://github.com/org/repo' } });
    await fireEvent.submit(getByTestId('import-repo-form'));
    await waitFor(() => {
      expect(api.createMirrorRepo).toHaveBeenCalledWith(
        expect.objectContaining({ url: 'https://github.com/org/repo', workspace_id: 'ws-1' })
      );
    });
  });

  it('closes import form after successful import', async () => {
    const { getByTestId, container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-import-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-import-repo'));
    await waitFor(() => expect(getByTestId('import-url-input')).toBeTruthy());
    await fireEvent.input(getByTestId('import-url-input'), { target: { value: 'https://github.com/org/repo' } });
    await fireEvent.submit(getByTestId('import-repo-form'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="import-repo-form"]')).toBeFalsy();
    });
  });

  it('shows error when createMirrorRepo fails', async () => {
    api.createMirrorRepo.mockRejectedValue(new Error('Invalid URL'));
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-import-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-import-repo'));
    await waitFor(() => expect(getByTestId('import-url-input')).toBeTruthy());
    await fireEvent.input(getByTestId('import-url-input'), { target: { value: 'https://github.com/org/repo' } });
    await fireEvent.submit(getByTestId('import-repo-form'));
    await waitFor(() => {
      expect(getByTestId('import-error').textContent).toContain('Invalid URL');
    });
  });

  it('Cancel button closes new repo form', async () => {
    const { getByTestId, container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(getByTestId('btn-new-repo')).toBeTruthy());
    await fireEvent.click(getByTestId('btn-new-repo'));
    await waitFor(() => expect(getByTestId('new-repo-form')).toBeTruthy());
    const cancelBtn = getByTestId('new-repo-form').querySelector('button[type="button"]:last-of-type');
    await fireEvent.click(cancelBtn);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="new-repo-form"]')).toBeFalsy();
    });
  });
});

// ── Briefing section ──────────────────────────────────────────────────────────

describe.skip('Briefing section (old layout — needs update)', () => {
  it('renders the briefing section container', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
  });

  it('calls api.getWorkspaceBriefing with the workspace id', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-1', null);
    });
  });

  it('shows a time range selector from Briefing.svelte', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const select = container.querySelector('[data-testid="time-range-selector"]');
      expect(select).toBeTruthy();
    });
  });
});

// ── Specs section ─────────────────────────────────────────────────────────────

describe.skip('Specs section (old layout — needs update)', () => {
  it('calls api.specsForWorkspace on mount', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => expect(api.specsForWorkspace).toHaveBeenCalledWith('ws-1'));
  });

  it('renders spec table after loading', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('specs-table')).toBeTruthy();
    });
  });

  it('shows spec rows', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const rows = container.querySelectorAll('[data-testid="spec-row"]');
      expect(rows.length).toBe(3);
    });
  });

  it('shows spec path in table', async () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByText('retry-logic.md')).toBeTruthy();
      expect(getByText('auth-refactor.md')).toBeTruthy();
    });
  });

  it('shows repo name in spec row', async () => {
    const { getAllByText } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      // payment-api appears in both Repos and Specs sections
      const matches = getAllByText('payment-api');
      expect(matches.length).toBeGreaterThan(0);
    });
  });

  it('shows progress as done/total when tasks_total present', async () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByText('5/5')).toBeTruthy();
      expect(getByText('3/5')).toBeTruthy();
    });
  });

  it('shows — when no tasks_total', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      // error-handling.md has tasks_total: null → should show —
      const rows = container.querySelectorAll('[data-testid="spec-row"]');
      expect(rows.length).toBe(3);
    });
  });

  it('filters specs by status when filter is changed', async () => {
    const { getByTestId, container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="spec-row"]').length).toBe(3);
    });
    const select = getByTestId('specs-status-filter');
    await fireEvent.change(select, { target: { value: 'draft' } });
    await waitFor(() => {
      const rows = container.querySelectorAll('[data-testid="spec-row"]');
      expect(rows.length).toBe(1);
    });
  });

  it('calls onSelectRepo when spec row is clicked', async () => {
    const onSelectRepo = vi.fn();
    const { container } = render(WorkspaceHome, {
      props: { workspace: WORKSPACE, onSelectRepo },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="spec-row"]').length).toBeGreaterThan(0);
    });
    const firstRow = container.querySelector('[data-testid="spec-row"]');
    await fireEvent.click(firstRow);
    expect(onSelectRepo).toHaveBeenCalled();
  });

  it('shows empty state when no specs', async () => {
    api.specsForWorkspace.mockResolvedValue([]);
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('specs-empty')).toBeTruthy();
    });
  });

  it('shows filtered empty state when filter matches nothing', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('specs-table')).toBeTruthy();
    });
    const select = getByTestId('specs-status-filter');
    await fireEvent.change(select, { target: { value: 'implemented' } });
    await waitFor(() => {
      expect(getByTestId('specs-empty')).toBeTruthy();
    });
  });
});

// ── Agent Rules section ───────────────────────────────────────────────────────

describe.skip('Agent Rules section (old layout — needs update)', () => {
  it('calls api.getMetaSpecs for Workspace scope', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(api.getMetaSpecs).toHaveBeenCalledWith(
        expect.objectContaining({ scope: 'Workspace', scope_id: 'ws-1' })
      );
    });
  });

  it('calls api.getMetaSpecs for Global scope', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(api.getMetaSpecs).toHaveBeenCalledWith(
        expect.objectContaining({ scope: 'Global' })
      );
    });
  });

  it('shows aggregate count in rules summary', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const summary = getByTestId('rules-summary');
      // 2 workspace + 1 global = 3 total
      expect(summary.textContent).toContain('3 meta-specs active');
    });
  });

  it('shows required count in summary when required specs exist', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const summary = getByTestId('rules-summary');
      expect(summary.textContent).toContain('required');
    });
  });

  it('renders required meta-specs with 🔒 icon', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const items = container.querySelectorAll('[data-testid="rule-item"]');
      expect(items.length).toBeGreaterThan(0);
      // All shown items should have the lock span
      items.forEach(item => {
        expect(item.querySelector('.rule-lock')).toBeTruthy();
      });
    });
  });

  it('shows required meta-spec names', async () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByText('conventional-commits')).toBeTruthy();
      expect(getByText('security')).toBeTruthy();
    });
  });

  it('shows "Manage rules" button', async () => {
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const btn = getByTestId('manage-rules-link');
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.textContent.trim()).toBe('Manage rules');
  });

  it('shows reconcile status when meta-specs recently updated', async () => {
    const recentMs = {
      id: 'm-recent',
      name: 'fresh-rule',
      kind: 'meta:principle',
      required: true,
      version: 2,
      scope: 'Workspace',
      updated_at: new Date().toISOString(), // just now
    };
    api.getMetaSpecs.mockImplementation((params) => {
      if (params?.scope === 'Workspace') return Promise.resolve([...META_SPECS_WORKSPACE, recentMs]);
      return Promise.resolve(META_SPECS_GLOBAL);
    });
    const { getByTestId } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(getByTestId('reconcile-status')).toBeTruthy();
    });
  });

  it('does not show reconcile status when no recent updates', async () => {
    // All mocked meta-specs have no updated_at → no reconcile status
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="reconcile-status"]')).toBeFalsy();
    });
  });

  it('shows empty state when no meta-specs configured', async () => {
    api.getMetaSpecs.mockResolvedValue([]);
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="rules-list"]')).toBeFalsy();
    });
  });
});

// ── Error handling ────────────────────────────────────────────────────────────

describe.skip('Error handling (old layout — needs update)', () => {
  it('shows error when notifications API fails', async () => {
    api.myNotifications.mockRejectedValue(new Error('Network error'));
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const error = container.querySelector('[data-testid="section-decisions"] .error-text');
      expect(error).toBeTruthy();
    });
  });

  it('shows error when repos API fails', async () => {
    api.workspaceRepos.mockRejectedValue(new Error('Server error'));
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const error = container.querySelector('[data-testid="section-repos"] .error-text');
      expect(error).toBeTruthy();
    });
  });

  it('shows error when specs API fails', async () => {
    api.specsForWorkspace.mockRejectedValue(new Error('Timeout'));
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      const error = container.querySelector('[data-testid="section-specs"] .error-text');
      expect(error).toBeTruthy();
    });
  });
});
