/**
 * TenantSettings.test.js — Tests for ui-navigation.md §10 tenant administration page
 *
 * Covers:
 *   - Component renders without throwing
 *   - Has data-testid="tenant-settings"
 *   - Page title shows "Tenant Administration"
 *   - Back button calls onBack callback
 *   - All 6 tabs render: Users, Compute Targets, Budget, Audit, Health, Jobs
 *   - Tab keyboard navigation (ArrowRight/ArrowLeft/Home/End)
 *   - Users tab: loads api.me(), shows user info
 *   - Compute Targets tab: loads api.computeList(), shows table
 *   - Budget tab: loads api.budgetSummary(), shows budget cards
 *   - Audit tab: loads api.adminAudit(), shows events table, filter bar, refresh button
 *   - Health tab: loads api.adminHealth(), shows health grid
 *   - Jobs tab: loads api.adminJobs(), shows jobs table with run button
 *   - Error states handled gracefully per tab
 *   - Empty states per tab
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    me: vi.fn().mockResolvedValue({ username: 'admin', email: 'admin@example.com', role: 'Admin' }),
    computeList: vi.fn().mockResolvedValue([]),
    budgetSummary: vi.fn().mockResolvedValue(null),
    adminAudit: vi.fn().mockResolvedValue([]),
    adminHealth: vi.fn().mockResolvedValue(null),
    adminJobs: vi.fn().mockResolvedValue([]),
    adminRunJob: vi.fn().mockResolvedValue({}),
    version: vi.fn().mockResolvedValue({ version: '0.1.0', commit: 'abc1234' }),
    auditStreamUrl: vi.fn().mockReturnValue('http://localhost/api/v1/audit/stream'),
    bcpTargets: vi.fn().mockResolvedValue(null),
    bcpDrill: vi.fn().mockResolvedValue({}),
    adminCreateSnapshot: vi.fn().mockResolvedValue({}),
    adminListSnapshots: vi.fn().mockResolvedValue([]),
    adminDeleteSnapshot: vi.fn().mockResolvedValue({}),
    adminRetention: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

import { api } from '../lib/api.js';
import TenantSettings from '../components/TenantSettings.svelte';

describe('TenantSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.me.mockResolvedValue({ username: 'admin', email: 'admin@example.com', role: 'Admin' });
    api.computeList.mockResolvedValue([]);
    api.budgetSummary.mockResolvedValue(null);
    api.adminAudit.mockResolvedValue([]);
    api.adminHealth.mockResolvedValue(null);
    api.adminJobs.mockResolvedValue([]);
    api.adminRunJob.mockResolvedValue({});
  });

  it('renders without throwing', () => {
    expect(() => render(TenantSettings)).not.toThrow();
  });

  it('has data-testid="tenant-settings"', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('shows "Tenant Administration" title', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.textContent).toContain('Tenant Administration');
    }, { timeout: 3000 });
  });

  it('calls onBack when back button is clicked', async () => {
    const onBack = vi.fn();
    const { container } = render(TenantSettings, { props: { onBack } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-back"]')).toBeTruthy();
    }, { timeout: 3000 });
    container.querySelector('[data-testid="tenant-settings-back"]').click();
    expect(onBack).toHaveBeenCalled();
  });

  it('renders all 6 tab buttons', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const tabs = container.querySelector('[data-testid="tenant-settings-tabs"]');
      expect(tabs).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-users"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-compute"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-budget"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-audit"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-health"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-jobs"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('tab labels show correct text', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const tabsEl = container.querySelector('[data-testid="tenant-settings-tabs"]');
      const text = tabsEl.textContent;
      expect(text).toContain('Users');
      expect(text).toContain('Compute Targets');
      expect(text).toContain('Budget');
      expect(text).toContain('Audit');
      expect(text).toContain('Health');
      expect(text).toContain('Jobs');
    }, { timeout: 3000 });
  });

  it('Users tab is active by default', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const usersTab = container.querySelector('[data-testid="tenant-settings-tab-users"]');
      expect(usersTab?.getAttribute('aria-selected')).toBe('true');
    }, { timeout: 3000 });
  });

  // ── Users tab ──────────────────────────────────────────────────────────────
  it('Users tab calls api.me on mount', async () => {
    render(TenantSettings);
    await waitFor(() => {
      expect(api.me).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Users tab shows current user info', async () => {
    api.me.mockResolvedValue({ username: 'admin', email: 'admin@example.com', role: 'Admin' });
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-users"]');
      expect(panel?.textContent).toContain('admin');
    }, { timeout: 3000 });
  });

  it('Users tab shows error state gracefully', async () => {
    api.me.mockRejectedValue(new Error('Auth error'));
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-users"]');
      expect(panel?.textContent).toContain('Auth error');
    }, { timeout: 3000 });
  });

  // ── Compute Targets tab ────────────────────────────────────────────────────
  it('Compute Targets tab loads compute list on tab switch', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-tab-compute"]')).toBeTruthy();
    }, { timeout: 3000 });
    container.querySelector('[data-testid="tenant-settings-tab-compute"]').click();
    await waitFor(() => {
      expect(api.computeList).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Compute Targets tab shows empty state', async () => {
    api.computeList.mockResolvedValue([]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-compute"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-compute"]');
      expect(panel?.textContent).toContain('No compute targets');
    }, { timeout: 3000 });
  });

  it('Compute Targets tab shows compute table when data present', async () => {
    api.computeList.mockResolvedValue([
      { id: 'ct-1', name: 'k8s-prod', kind: 'kubernetes', status: 'healthy', capacity: 10 },
      { id: 'ct-2', name: 'k8s-dev', kind: 'kubernetes', status: 'degraded', capacity: 5 },
    ]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-compute"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-compute"]');
      const table = panel?.querySelector('[data-testid="compute-targets-table"]');
      expect(table).toBeTruthy();
      expect(table.textContent).toContain('k8s-prod');
      expect(table.textContent).toContain('k8s-dev');
    }, { timeout: 3000 });
  });

  it('Compute Targets tab shows error gracefully', async () => {
    api.computeList.mockRejectedValue(new Error('Compute error'));
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-compute"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-compute"]');
      expect(panel?.textContent).toContain('Compute error');
    }, { timeout: 3000 });
  });

  // ── Budget tab ─────────────────────────────────────────────────────────────
  it('Budget tab loads budgetSummary on tab switch', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-budget"]')?.click();
    await waitFor(() => {
      expect(api.budgetSummary).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Budget tab shows empty state when no data', async () => {
    api.budgetSummary.mockResolvedValue(null);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-budget"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-budget"]');
      expect(panel?.textContent).toContain('No budget data');
    }, { timeout: 3000 });
  });

  it('Budget tab shows budget cards when data present', async () => {
    api.budgetSummary.mockResolvedValue({
      total_credits: 10000,
      used_credits: 4500,
      remaining_credits: 5500,
    });
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-budget"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-budget"]');
      expect(panel?.textContent).toContain('10,000');
      expect(panel?.textContent).toContain('4,500');
    }, { timeout: 3000 });
  });

  it('Budget tab shows error gracefully', async () => {
    api.budgetSummary.mockRejectedValue(new Error('Budget error'));
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-budget"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-budget"]');
      expect(panel?.textContent).toContain('Budget error');
    }, { timeout: 3000 });
  });

  // ── Audit tab ──────────────────────────────────────────────────────────────
  it('Audit tab loads adminAudit on tab switch', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-audit"]')?.click();
    await waitFor(() => {
      expect(api.adminAudit).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Audit tab has filter bar and refresh button', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-audit"]')?.click();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="audit-filter-bar"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="audit-refresh"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('Audit tab shows empty state when no events', async () => {
    api.adminAudit.mockResolvedValue([]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-audit"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-audit"]');
      expect(panel?.textContent).toContain('No audit events');
    }, { timeout: 3000 });
  });

  it('Audit tab shows events in table when data present', async () => {
    api.adminAudit.mockResolvedValue([
      { id: 'ev-1', event_type: 'tenant_created', actor: 'admin', timestamp: '2026-03-30T10:00:00Z', detail: 'Tenant created' },
    ]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-audit"]')?.click();
    await waitFor(() => {
      const table = container.querySelector('[data-testid="audit-events-table"]');
      expect(table).toBeTruthy();
      expect(table.textContent).toContain('tenant created');
      expect(table.textContent).toContain('admin');
    }, { timeout: 3000 });
  });

  it('Audit tab shows error gracefully', async () => {
    api.adminAudit.mockRejectedValue(new Error('Audit error'));
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-audit"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-audit"]');
      expect(panel?.textContent).toContain('Audit error');
    }, { timeout: 3000 });
  });

  // ── Health tab ─────────────────────────────────────────────────────────────
  it('Health tab loads adminHealth on tab switch', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-health"]')?.click();
    await waitFor(() => {
      expect(api.adminHealth).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Health tab shows empty state when no data', async () => {
    api.adminHealth.mockResolvedValue(null);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-health"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-health"]');
      expect(panel?.textContent).toContain('No health data');
    }, { timeout: 3000 });
  });

  it('Health tab shows health grid when data present', async () => {
    api.adminHealth.mockResolvedValue({ database: 'ok', redis: 'ok', storage: 'degraded' });
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-health"]')?.click();
    await waitFor(() => {
      const grid = container.querySelector('[data-testid="health-grid"]');
      expect(grid).toBeTruthy();
      expect(grid.textContent).toContain('database');
      expect(grid.textContent).toContain('redis');
      expect(grid.textContent).toContain('storage');
    }, { timeout: 3000 });
  });

  it('Health tab shows error gracefully', async () => {
    api.adminHealth.mockRejectedValue(new Error('Health error'));
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-health"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-health"]');
      expect(panel?.textContent).toContain('Health error');
    }, { timeout: 3000 });
  });

  // ── Jobs tab ───────────────────────────────────────────────────────────────
  it('Jobs tab loads adminJobs on tab switch', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-jobs"]')?.click();
    await waitFor(() => {
      expect(api.adminJobs).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('Jobs tab shows empty state when no jobs', async () => {
    api.adminJobs.mockResolvedValue([]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-jobs"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-jobs"]');
      expect(panel?.textContent).toContain('No jobs registered');
    }, { timeout: 3000 });
  });

  it('Jobs tab shows jobs table with run button', async () => {
    api.adminJobs.mockResolvedValue([
      { name: 'cleanup', schedule: '0 * * * *', status: 'ok', last_run: '2026-03-30T09:00:00Z' },
      { name: 'reindex', schedule: '0 0 * * *', status: 'success', last_run: '2026-03-29T00:00:00Z' },
    ]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-jobs"]')?.click();
    await waitFor(() => {
      const table = container.querySelector('[data-testid="jobs-table"]');
      expect(table).toBeTruthy();
      expect(table.textContent).toContain('cleanup');
      expect(table.textContent).toContain('reindex');
      expect(container.querySelector('[data-testid="run-job-cleanup"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('Jobs tab run button calls adminRunJob', async () => {
    api.adminJobs.mockResolvedValue([
      { name: 'cleanup', schedule: '0 * * * *', status: 'ok' },
    ]);
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-jobs"]')?.click();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="run-job-cleanup"]')).toBeTruthy();
    }, { timeout: 3000 });
    container.querySelector('[data-testid="run-job-cleanup"]').click();
    await waitFor(() => {
      expect(api.adminRunJob).toHaveBeenCalledWith('cleanup');
    }, { timeout: 3000 });
  });

  it('Jobs tab shows error gracefully', async () => {
    api.adminJobs.mockRejectedValue(new Error('Jobs error'));
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-jobs"]')?.click();
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-jobs"]');
      expect(panel?.textContent).toContain('Jobs error');
    }, { timeout: 3000 });
  });

  // ── Tab switching ──────────────────────────────────────────────────────────
  it('clicking a tab shows its panel', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-health"]')?.click();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-tab-health"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('active tab has aria-selected=true', async () => {
    const { container } = render(TenantSettings);
    container.querySelector('[data-testid="tenant-settings-tab-compute"]')?.click();
    await waitFor(() => {
      const computeTab = container.querySelector('[data-testid="tenant-settings-tab-compute"]');
      expect(computeTab?.getAttribute('aria-selected')).toBe('true');
      const usersTab = container.querySelector('[data-testid="tenant-settings-tab-users"]');
      expect(usersTab?.getAttribute('aria-selected')).toBe('false');
    }, { timeout: 3000 });
  });

  // ── Tab keyboard navigation ────────────────────────────────────────────────
  it('ArrowRight moves focus to next tab', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-tab-users"]')).toBeTruthy();
    }, { timeout: 3000 });
    const tabBar = container.querySelector('[data-testid="tenant-settings-tabs"]');
    const usersTab = container.querySelector('[data-testid="tenant-settings-tab-users"]');
    usersTab.focus();
    fireEvent.keyDown(tabBar, { key: 'ArrowRight' });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe('tenant-settings-tab-compute');
    }, { timeout: 3000 });
  });

  it('ArrowLeft moves focus to previous tab', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-tab-compute"]')).toBeTruthy();
    }, { timeout: 3000 });
    const tabBar = container.querySelector('[data-testid="tenant-settings-tabs"]');
    const computeTab = container.querySelector('[data-testid="tenant-settings-tab-compute"]');
    computeTab.focus();
    fireEvent.keyDown(tabBar, { key: 'ArrowLeft' });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe('tenant-settings-tab-users');
    }, { timeout: 3000 });
  });

  it('Home moves focus to first tab', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-tab-jobs"]')).toBeTruthy();
    }, { timeout: 3000 });
    const tabBar = container.querySelector('[data-testid="tenant-settings-tabs"]');
    const jobsTab = container.querySelector('[data-testid="tenant-settings-tab-jobs"]');
    jobsTab.focus();
    fireEvent.keyDown(tabBar, { key: 'Home' });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe('tenant-settings-tab-users');
    }, { timeout: 3000 });
  });

  it('End moves focus to last tab', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-settings-tab-users"]')).toBeTruthy();
    }, { timeout: 3000 });
    const tabBar = container.querySelector('[data-testid="tenant-settings-tabs"]');
    const usersTab = container.querySelector('[data-testid="tenant-settings-tab-users"]');
    usersTab.focus();
    fireEvent.keyDown(tabBar, { key: 'End' });
    await waitFor(() => {
      expect(document.activeElement?.getAttribute('data-testid')).toBe('tenant-settings-tab-bcp');
    }, { timeout: 3000 });
  });
});
