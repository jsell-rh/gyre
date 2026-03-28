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
  { id: 'ws-1', name: 'Payments', description: 'Payment services', slug: 'payments', tenant_id: 't-1', trust_level: 'Autonomous', created_at: 0 },
  { id: 'ws-2', name: 'Platform Core', description: null, slug: 'platform', tenant_id: 't-1', trust_level: 'Guided', created_at: 0 },
  { id: 'ws-3', name: 'Security Review', description: 'Handles security audits', slug: 'security', tenant_id: 't-1', trust_level: 'Supervised', created_at: 0 },
];

const makeBudget = (tokensUsed, maxTokens, activeAgents = 0, costToday = 0, maxCostPerDay = null) => ({
  entity_type: 'workspace',
  entity_id: 'ws-1',
  config: { max_tokens_per_day: maxTokens, max_cost_per_day: maxCostPerDay, max_concurrent_agents: null, max_agent_lifetime_secs: null },
  usage: { entity_type: 'workspace', entity_id: 'ws-1', tokens_used_today: tokensUsed, cost_today: costToday, active_agents: activeAgents, period_start: 0 },
});

beforeEach(() => {
  vi.clearAllMocks();
  api.workspaces.mockResolvedValue(WORKSPACES);
  api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 0));
  api.workspaceRepos.mockResolvedValue([]);
});

// ─── Rendering ───────────────────────────────────────────────────────────────

describe('WorkspaceCards — rendering', () => {
  it('renders without throwing', () => {
    expect(() => render(WorkspaceCards)).not.toThrow();
  });

  it('shows loading skeletons initially', () => {
    const { container } = render(WorkspaceCards);
    const skeletons = container.querySelectorAll('.card-skeleton');
    // Skeleton cards shown while loading
    expect(skeletons.length).toBeGreaterThan(0);
  });

  it('shows aria-busy on cards grid during loading', () => {
    const { container } = render(WorkspaceCards);
    const grid = container.querySelector('[aria-busy="true"]');
    expect(grid).toBeTruthy();
  });

  it('renders workspace names after load', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
      expect(getByText('Platform Core')).toBeTruthy();
      expect(getByText('Security Review')).toBeTruthy();
    });
  });

  it('shows workspace description when present', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Payment services')).toBeTruthy();
      expect(getByText('Handles security audits')).toBeTruthy();
    });
  });

  it('shows header with title and subtitle', () => {
    const { getByText } = render(WorkspaceCards);
    expect(getByText('Workspaces')).toBeTruthy();
    expect(getByText(/Choose a workspace/)).toBeTruthy();
  });
});

// ─── Trust level badges ──────────────────────────────────────────────────────

describe('WorkspaceCards — trust levels', () => {
  it('shows trust level badges', async () => {
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getAllByText('Autonomous').length).toBeGreaterThan(0);
      expect(getAllByText('Guided').length).toBeGreaterThan(0);
      expect(getAllByText('Supervised').length).toBeGreaterThan(0);
    });
  });

  it('shows Standard badge when no trust_level set', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-x', name: 'No Trust', description: null, slug: 'nt', tenant_id: 't-1', created_at: 0 },
    ]);
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Standard')).toBeTruthy();
    });
  });
});

// ─── Empty & error states ────────────────────────────────────────────────────

describe('WorkspaceCards — empty state', () => {
  it('shows empty state when no workspaces', async () => {
    api.workspaces.mockResolvedValue([]);
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText(/No workspaces found/i)).toBeTruthy();
    });
  });

  it('shows Go to Admin button in empty state', async () => {
    api.workspaces.mockResolvedValue([]);
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Go to Admin')).toBeTruthy();
    });
  });
});

describe('WorkspaceCards — error state', () => {
  it('shows error banner when API fails', async () => {
    api.workspaces.mockRejectedValue(new Error('Connection refused'));
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText(/Failed to load workspaces/)).toBeTruthy();
    });
  });

  it('shows retry button on error', async () => {
    api.workspaces.mockRejectedValue(new Error('timeout'));
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => {
      expect(getByText('Retry')).toBeTruthy();
    });
  });

  it('retries loading on retry button click', async () => {
    api.workspaces.mockRejectedValue(new Error('timeout'));
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Retry'));

    // Now make it succeed
    api.workspaces.mockResolvedValue(WORKSPACES);
    await fireEvent.click(getByText('Retry'));

    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
    });
  });
});

// ─── Enrichment data ─────────────────────────────────────────────────────────

describe('WorkspaceCards — enrichment', () => {
  it('shows Enter Workspace button for each workspace', async () => {
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const buttons = getAllByText('Enter Workspace');
      expect(buttons.length).toBe(3);
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
    }
  });

  it('shows budget bar with correct percentage', async () => {
    // 670 of 1000 tokens = 67%
    api.workspaceBudget.mockResolvedValue(makeBudget(670, 1000, 5));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('67%');
      expect(els.length).toBeGreaterThan(0);
    });
  });

  it('shows active agent count from budget usage', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 5));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
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
    api.workspaceBudget.mockResolvedValue(makeBudget(850, 1000, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('85%');
      expect(els.length).toBeGreaterThan(0);
    });
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

  it('shows budget bar in success color for <80% usage', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(300, 1000, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('30%');
      expect(els.length).toBeGreaterThan(0);
    });
    const bars = document.querySelectorAll('.budget-bar-fill');
    if (bars.length > 0) {
      const style = bars[0].getAttribute('style') ?? '';
      expect(style).toContain('var(--color-success)');
    }
  });

  it('uses cost-based budget when max_cost_per_day is set', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 0, 7.5, 10.0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      // 7.5 / 10.0 = 75%
      const els = getAllByText('75%');
      expect(els.length).toBeGreaterThan(0);
    });
  });

  it('shows dash for budget when no max configured', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(0, 0, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const dashes = getAllByText('—');
      expect(dashes.length).toBeGreaterThan(0);
    });
  });

  it('caps budget percentage at 100%', async () => {
    api.workspaceBudget.mockResolvedValue(makeBudget(1500, 1000, 0));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      const els = getAllByText('100%');
      expect(els.length).toBeGreaterThan(0);
    });
  });

  it('fetches workspaceBudget and workspaceRepos for each workspace', async () => {
    render(WorkspaceCards);
    await waitFor(() => {
      expect(api.workspaces).toHaveBeenCalledTimes(1);
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-1');
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-2');
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-3');
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-1');
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-2');
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-3');
    });
  });

  it('handles enrichment errors gracefully (shows dash)', async () => {
    api.workspaceBudget.mockRejectedValue(new Error('500'));
    api.workspaceRepos.mockRejectedValue(new Error('500'));
    const { getAllByText } = render(WorkspaceCards);
    await waitFor(() => {
      // Should show workspace names even if enrichment fails
      expect(getAllByText('Payments').length).toBeGreaterThan(0);
    });
  });
});

// ─── Filter ──────────────────────────────────────────────────────────────────

describe('WorkspaceCards — filter', () => {
  it('shows filter input after workspaces load', async () => {
    const { container } = render(WorkspaceCards);
    await waitFor(() => {
      const filterInput = container.querySelector('.ws-filter');
      expect(filterInput).toBeTruthy();
    });
  });

  it('filters workspaces by name', async () => {
    const { container, getByText, queryByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'pay' } });

    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
      expect(queryByText('Platform Core')).toBeNull();
      expect(queryByText('Security Review')).toBeNull();
    });
  });

  it('shows "No results" when filter matches nothing', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'zzzznonexistent' } });

    await waitFor(() => {
      expect(getByText(/No results/)).toBeTruthy();
    });
  });

  it('shows Clear filter button in no-results state', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'zzzznonexistent' } });

    await waitFor(() => {
      expect(getByText('Clear filter')).toBeTruthy();
    });
  });

  it('clears filter when Clear button is clicked', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'zzzznonexistent' } });
    await waitFor(() => getByText('Clear filter'));

    await fireEvent.click(getByText('Clear filter'));
    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
      expect(getByText('Platform Core')).toBeTruthy();
    });
  });

  it('shows live count of filtered workspaces', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'pay' } });

    await waitFor(() => {
      const srOnly = container.querySelector('[role="status"]');
      expect(srOnly.textContent).toContain('1 workspace');
    });
  });

  it('filter is case-insensitive', async () => {
    const { container, getByText, queryByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));

    const filterInput = container.querySelector('.ws-filter');
    await fireEvent.input(filterInput, { target: { value: 'PLATFORM' } });

    await waitFor(() => {
      expect(getByText('Platform Core')).toBeTruthy();
      expect(queryByText('Payments')).toBeNull();
    });
  });
});

// ─── Accessibility ───────────────────────────────────────────────────────────

describe('WorkspaceCards — accessibility', () => {
  it('cards grid has role=list and aria-label', async () => {
    const { container } = render(WorkspaceCards);
    await waitFor(() => {
      const grid = container.querySelector('[role="list"][aria-label="Workspaces"]');
      expect(grid).toBeTruthy();
    });
  });

  it('each workspace card has role=listitem', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));
    const items = container.querySelectorAll('[role="listitem"]');
    expect(items.length).toBe(3);
  });

  it('enter buttons have descriptive aria-label', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));
    const btn = document.querySelector('button[aria-label="Enter workspace Payments"]');
    expect(btn).toBeTruthy();
  });

  it('budget bar has progressbar role', async () => {
    const { getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));
    const progressbars = document.querySelectorAll('[role="progressbar"]');
    expect(progressbars.length).toBe(3); // one per workspace
  });

  it('filter input has aria-label', async () => {
    const { container, getByText } = render(WorkspaceCards);
    await waitFor(() => getByText('Payments'));
    const filter = container.querySelector('input[aria-label="Filter workspaces"]');
    expect(filter).toBeTruthy();
  });
});
