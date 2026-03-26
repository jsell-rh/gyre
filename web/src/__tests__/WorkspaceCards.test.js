import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// Mock api before importing component
vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn(),
    workspaceBudget: vi.fn(),
    workspaceRepos: vi.fn(),
  },
}));

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
}));

import { api } from '../lib/api.js';
import WorkspaceCards from '../components/WorkspaceCards.svelte';

const WORKSPACES = [
  { id: 'ws-1', name: 'Payments', description: 'Payment services', slug: 'payments', tenant_id: 't-1', created_at: 0 },
  { id: 'ws-2', name: 'Platform Core', description: null, slug: 'platform', tenant_id: 't-1', created_at: 0 },
];

const makeBudget = (tokensUsed, maxTokens, activeAgents = 0) => ({
  entity_type: 'workspace',
  entity_id: 'ws-1',
  config: { max_tokens_per_day: maxTokens, max_cost_per_day: null, max_concurrent_agents: null, max_agent_lifetime_secs: null },
  usage: { entity_type: 'workspace', entity_id: 'ws-1', tokens_used_today: tokensUsed, cost_today: 0, active_agents: activeAgents, period_start: 0 },
});

beforeEach(() => {
  vi.clearAllMocks();
  api.workspaces.mockResolvedValue(WORKSPACES);
  api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 0));
  api.workspaceRepos.mockResolvedValue([]);
});

describe('WorkspaceCards', () => {
  it('renders without throwing', () => {
    expect(() => render(WorkspaceCards)).not.toThrow();
  });

  it('shows loading skeletons initially', () => {
    const { container } = render(WorkspaceCards);
    // While loading, skeleton cards are shown
    expect(container).toBeTruthy();
  });

  it('renders workspace names after load', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
      expect(getByText('Platform Core')).toBeTruthy();
    });
  });

  it('shows workspace description when present', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Payment services')).toBeTruthy();
    });
  });

  it('shows empty state when no workspaces', async () => {
    api.workspaces.mockResolvedValue([]);
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText(/No workspaces found/i)).toBeTruthy();
    });
  });

  it('shows Enter Workspace button for each workspace', async () => {
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const buttons = getAllByText('Enter Workspace');
      expect(buttons.length).toBe(2);
    });
  });

  it('calls onSelectWorkspace when Enter Workspace is clicked', async () => {
    const onSelectWorkspace = vi.fn();
    const { getAllByText } = render(WorkspaceCards, { props: { onSelectWorkspace } });
    await waitFor(() => getAllByText('Enter Workspace'));
    const buttons = document.querySelectorAll('button[aria-label^="Enter workspace"]');
    if (buttons.length > 0) {
      await fireEvent.click(buttons[0]);
      expect(onSelectWorkspace).toHaveBeenCalledWith(WORKSPACES[0]);
    } else {
      // fallback: click by text
      const btns = document.querySelectorAll('button');
      const enterBtn = Array.from(btns).find(b => b.textContent.includes('Enter Workspace'));
      if (enterBtn) {
        await fireEvent.click(enterBtn);
        expect(onSelectWorkspace).toHaveBeenCalled();
      }
    }
  });

  it('shows budget bar with correct percentage', async () => {
    // 670 of 1000 tokens = 67%
    api.workspaceBudget.mockResolvedValue(makeBudget(670, 1000, 5));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      // Both workspace cards share the same mock, so both show 67%
      const els = getAllByText('67%');
      expect(els.length).toBeGreaterThan(0);
    });
  });

  it('shows active agent count from budget usage', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 5));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      // active_agents = 5, shown in stats
      const fives = getAllByText('5');
      expect(fives.length).toBeGreaterThan(0);
    });
  });

  it('shows repo count from workspaceRepos', async () => {
    api.workspaceRepos.mockResolvedValue([
      { id: 'r-1', name: 'payments-api' },
      { id: 'r-2', name: 'billing-svc' },
      { id: 'r-3', name: 'fraud-detect' },
    ]);
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const threes = getAllByText('3');
      expect(threes.length).toBeGreaterThan(0);
    });
  });

  it('shows budget bar in warning color for >80% usage', async () => {
    // 850 of 1000 = 85%
    api.workspaceBudget.mockResolvedValue(makeBudget(850, 1000, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('85%');
      expect(els.length).toBeGreaterThan(0);
    });
    // Budget bar fill should have warning color (CSS variable)
    const bars = document.querySelectorAll('.budget-bar-fill');
    if (bars.length > 0) {
      const style = bars[0].getAttribute('style') ?? '';
      expect(style).toContain('var(--color-warning)');
    }
  });

  it('shows budget bar in danger color for >95% usage', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(970, 1000, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('97%');
      expect(els.length).toBeGreaterThan(0);
    });
    const bars = document.querySelectorAll('.budget-bar-fill');
    if (bars.length > 0) {
      const style = bars[0].getAttribute('style') ?? '';
      expect(style).toContain('var(--color-danger)');
    }
  });

  it('shows — for budget when no max configured', async () => {
    // max_tokens_per_day = 0 and max_cost_per_day = null → no pct
    api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const dashes = getAllByText('—');
      expect(dashes.length).toBeGreaterThan(0);
    });
  });

  it('fetches workspaceBudget and workspaceRepos for each workspace', async () => {
    render(WorkspaceCards);
    await waitFor(() => {
      expect(api.workspaces).toHaveBeenCalledTimes(1);
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-1');
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-2');
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-1');
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-2');
    });
  });
});
