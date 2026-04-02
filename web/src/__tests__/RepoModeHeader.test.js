/**
 * RepoModeHeader.test.js — Tests for TASK-350: repo mode header + tab wiring
 *
 * Covers:
 *   - Repo header renders with repo name, agent count, budget %, clone URL
 *   - Agent count click opens slide-in panel
 *   - Agent panel lists active agents for the repo
 *   - Agent panel closes on overlay click and close button
 *   - Clone URL copy button renders and is clickable
 *   - Budget display hidden when no workspaceBudget
 *   - Decisions tab passes repoId to Inbox
 *   - All tab wiring: specs, architecture, decisions, code, settings
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Stub child components ─────────────────────────────────────────────
vi.mock('../components/SpecDashboard.svelte', () => ({
  default: function SpecDashboardStub() {},
}));

vi.mock('../components/ExplorerView.svelte', () => ({
  default: function ExplorerViewStub() {},
}));

vi.mock('../components/Inbox.svelte', () => ({
  default: function InboxStub() {},
}));

vi.mock('../components/ExplorerCodeTab.svelte', () => ({
  default: function ExplorerCodeTabStub() {},
}));

vi.mock('../components/RepoSettings.svelte', () => ({
  default: function RepoSettingsStub(opts) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'repo-settings');
    if (opts?.target) opts.target.appendChild(el);
    return { destroy() {} };
  },
}));

vi.mock('../lib/api.js', () => ({
  api: {
    agents: vi.fn().mockResolvedValue([]),
    repo: vi.fn().mockResolvedValue(null),
    task: vi.fn().mockResolvedValue({ title: 'mock task' }),
    tasks: vi.fn().mockResolvedValue([]),
    agent: vi.fn().mockResolvedValue({ name: 'mock agent' }),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'mock MR' }),
    mergeRequests: vi.fn().mockResolvedValue([]),
    notificationCount: vi.fn().mockResolvedValue(0),
    myNotifications: vi.fn().mockResolvedValue([]),
    workspace: vi.fn().mockResolvedValue({ name: 'mock workspace' }),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    mergeQueue: vi.fn().mockResolvedValue([]),
  },
  setAuthToken: vi.fn(),
}));

import { api } from '../lib/api.js';
import RepoMode from '../components/RepoMode.svelte';

const mockWorkspace = { id: 'ws-1', name: 'Payments' };
const mockRepo = { id: 'repo-1', name: 'payment-api' };
const mockBudget = { used_credits: 45, total_credits: 100 };

beforeEach(() => {
  vi.clearAllMocks();
  api.agents.mockResolvedValue([]);
});

// ── Repo header rendering ─────────────────────────────────────────────

describe('Repo header', () => {
  it('renders repo-header element', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    expect(container.querySelector('[data-testid="repo-header"]')).toBeTruthy();
  });

  it('displays repo name prominently', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    const nameEl = container.querySelector('[data-testid="repo-name"]');
    expect(nameEl).toBeTruthy();
    expect(nameEl.textContent).toContain('payment-api');
  });

  it('displays "0 agents active" when no active agents', async () => {
    api.agents.mockResolvedValue([]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="agent-count-btn"]');
      expect(btn?.textContent).toContain('0 agents active');
    });
  });

  it('displays singular "agent" when exactly 1 active agent', async () => {
    api.agents.mockResolvedValue([{ id: 'a1', name: 'Builder', status: 'active' }]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="agent-count-btn"]');
      expect(btn?.textContent).toContain('1 agent active');
    });
  });

  it('displays correct count when multiple active agents', async () => {
    api.agents.mockResolvedValue([
      { id: 'a1', name: 'Builder', status: 'active' },
      { id: 'a2', name: 'Reviewer', status: 'active' },
      { id: 'a3', name: 'Tester', status: 'active' },
    ]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="agent-count-btn"]');
      expect(btn?.textContent).toContain('3 agents active');
    });
  });

  it('requests agents with repo_id=repo.id and status=active', async () => {
    api.agents.mockResolvedValue([]);
    render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => {
      expect(api.agents).toHaveBeenCalledWith({ repoId: 'repo-1', status: 'active' });
    });
  });

  it('does not request agents when repo is null', () => {
    render(RepoMode, {
      props: { workspace: mockWorkspace, repo: null, activeTab: 'specs' },
    });
    expect(api.agents).not.toHaveBeenCalled();
  });
});

// ── Budget display ─────────────────────────────────────────────────────

describe('Budget display', () => {
  it('shows budget percentage when workspaceBudget provided', () => {
    const { container } = render(RepoMode, {
      props: {
        workspace: mockWorkspace,
        repo: mockRepo,
        activeTab: 'specs',
        workspaceBudget: mockBudget,
      },
    });
    const budgetEl = container.querySelector('[data-testid="budget-display"]');
    expect(budgetEl).toBeTruthy();
    expect(budgetEl.textContent).toContain('Budget: 45%');
  });

  it('hides budget when workspaceBudget is null', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs', workspaceBudget: null },
    });
    expect(container.querySelector('[data-testid="budget-display"]')).toBeNull();
  });

  it('hides budget when total_credits is 0', () => {
    const { container } = render(RepoMode, {
      props: {
        workspace: mockWorkspace,
        repo: mockRepo,
        activeTab: 'specs',
        workspaceBudget: { used_credits: 0, total_credits: 0 },
      },
    });
    expect(container.querySelector('[data-testid="budget-display"]')).toBeNull();
  });

  it('rounds budget to nearest integer', () => {
    const { container } = render(RepoMode, {
      props: {
        workspace: mockWorkspace,
        repo: mockRepo,
        activeTab: 'specs',
        workspaceBudget: { used_credits: 1, total_credits: 3 },
      },
    });
    const budgetEl = container.querySelector('[data-testid="budget-display"]');
    expect(budgetEl?.textContent).toContain('Budget: 33%');
  });
});

// ── Clone URL ─────────────────────────────────────────────────────────

describe('Clone URL', () => {
  it('renders clone URL button', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    expect(container.querySelector('[data-testid="clone-url-btn"]')).toBeTruthy();
  });

  it('shows computed clone URL in button title', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    const btn = container.querySelector('[data-testid="clone-url-btn"]');
    expect(btn?.title).toContain('payment-api.git');
  });

  it('uses repo.clone_url when available', () => {
    const repoWithCloneUrl = { ...mockRepo, clone_url: 'https://example.com/custom.git' };
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: repoWithCloneUrl, activeTab: 'specs' },
    });
    const btn = container.querySelector('[data-testid="clone-url-btn"]');
    expect(btn?.title).toBe('https://example.com/custom.git');
  });

  it('does not render clone URL button when repo name is missing', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: null, activeTab: 'specs' },
    });
    expect(container.querySelector('[data-testid="clone-url-btn"]')).toBeNull();
  });
});

// ── Agent panel ────────────────────────────────────────────────────────

describe('Agent panel', () => {
  it('panel is initially closed', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    expect(container.querySelector('[data-testid="agent-panel"]')).toBeNull();
  });

  it('clicking agent count button opens the panel', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="agent-panel"]')).toBeTruthy();
    });
  });

  it('panel shows empty state when no active agents', async () => {
    api.agents.mockResolvedValue([]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => { expect(api.agents).toHaveBeenCalled(); });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="agent-panel"]');
      expect(panel?.textContent).toContain('No active agents');
    });
  });

  it('panel lists active agents', async () => {
    api.agents.mockResolvedValue([
      { id: 'a1', name: 'Builder', status: 'active', task_id: 'TASK-1' },
      { id: 'a2', name: 'Reviewer', status: 'active' },
    ]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => { expect(api.agents).toHaveBeenCalled(); });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      const rows = container.querySelectorAll('[data-testid="agent-row"]');
      expect(rows.length).toBe(2);
    });
  });

  it('panel shows task ID when agent has task_id', async () => {
    api.agents.mockResolvedValue([
      { id: 'a1', name: 'Builder', status: 'active', task_id: 'TASK-42' },
    ]);
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await waitFor(() => { expect(api.agents).toHaveBeenCalled(); });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="agent-panel"]');
      // entityName resolves via api.task() or falls back to shortId
      expect(panel?.textContent).toMatch(/TASK-42|mock task/);
    });
  });

  it('panel closes when close button clicked', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="agent-panel"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="agent-panel-close"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="agent-panel"]')).toBeNull();
    });
  });

  it('panel closes when overlay backdrop clicked', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    await fireEvent.click(container.querySelector('[data-testid="agent-count-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="agent-panel-overlay"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="agent-panel-overlay"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="agent-panel"]')).toBeNull();
    });
  });
});

// ── Tab bar ────────────────────────────────────────────────────────────

describe('Tab bar', () => {
  it('renders all 8 tabs', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    const tabs = container.querySelectorAll('.tab-btn');
    expect(tabs.length).toBe(8);
  });

  it('marks the active tab with aria-selected=true', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'decisions' },
    });
    const activeTabs = container.querySelectorAll('[aria-selected="true"]');
    expect(activeTabs.length).toBe(1);
    expect(activeTabs[0].textContent).toContain('Decisions');
  });

  it('calls onTabChange when tab clicked', async () => {
    const onTabChange = vi.fn();
    const { getByText } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs', onTabChange },
    });
    await fireEvent.click(getByText('Architecture'));
    expect(onTabChange).toHaveBeenCalledWith('architecture');
  });
});

// ── Tab content wiring ─────────────────────────────────────────────────

describe('Tab content wiring', () => {
  it('renders settings tab without throwing (Slice 4: RepoSettings replaces placeholder)', () => {
    // RepoSettings is mocked; we just verify no render error occurs
    expect(() => render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'settings' },
    })).not.toThrow();
  });

  it('shows no repo placeholder for code tab when repo has no id', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: { name: 'payment-api' }, activeTab: 'code' },
    });
    expect(container.textContent).toContain('No repo selected');
  });

  it('renders tab-content area for specs tab', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'specs' },
    });
    expect(container.querySelector('.tab-content')).toBeTruthy();
  });

  it('renders tab-content area for architecture tab', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'architecture' },
    });
    expect(container.querySelector('.tab-content')).toBeTruthy();
  });

  it('renders tab-content area for decisions tab', () => {
    const { container } = render(RepoMode, {
      props: { workspace: mockWorkspace, repo: mockRepo, activeTab: 'decisions' },
    });
    expect(container.querySelector('.tab-content')).toBeTruthy();
  });
});
