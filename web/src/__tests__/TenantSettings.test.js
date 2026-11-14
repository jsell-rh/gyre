/**
 * TenantSettings.test.js — Tests for ui-navigation.md §10 tenant administration page
 *
 * Covers:
 *   - Component renders without throwing
 *   - Has data-testid="tenant-settings"
 *   - Page title shows "Tenant Administration"
 *   - Back button calls onBack callback
 *   - All 8 tabs render: Users, Compute Targets, Budget, Secrets, Workspaces, Audit, Health, Jobs
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
    listUsers: vi.fn().mockResolvedValue({ users: [], total: 0 }),
    deactivateUser: vi.fn().mockResolvedValue({}),
    provisionUser: vi.fn().mockResolvedValue({ user: {}, api_key: { key: 'gyre_x' } }),
    computeList: vi.fn().mockResolvedValue([]),
    budgetSummary: vi.fn().mockResolvedValue(null),
    adminAudit: vi.fn().mockResolvedValue([]),
    adminHealth: vi.fn().mockResolvedValue(null),
    adminJobs: vi.fn().mockResolvedValue([]),
    workspaces: vi.fn().mockResolvedValue([]),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    adminRunJob: vi.fn().mockResolvedValue({}),
    version: vi.fn().mockResolvedValue({ version: '0.1.0', commit: 'abc1234' }),
    auditStreamUrl: vi.fn().mockReturnValue('http://localhost/api/v1/audit/stream'),
    bcpTargets: vi.fn().mockResolvedValue(null),
    bcpDrill: vi.fn().mockResolvedValue({}),
    adminCreateSnapshot: vi.fn().mockResolvedValue({}),
    adminListSnapshots: vi.fn().mockResolvedValue([]),
    adminDeleteSnapshot: vi.fn().mockResolvedValue({}),
    adminRetention: vi.fn().mockResolvedValue(null),
    secretsList: vi.fn().mockResolvedValue([]),
    secretCreate: vi.fn().mockResolvedValue({}),
    secretDelete: vi.fn().mockResolvedValue(null),
    secretRotate: vi.fn().mockResolvedValue({}),
    // ABAC policy management surface (task-132) — the Policies tab's
    // PolicyManager calls the global /policies endpoints.
    policies: vi.fn().mockResolvedValue([]),
    createPolicy: vi.fn().mockResolvedValue({}),
    updatePolicy: vi.fn().mockResolvedValue({}),
    deletePolicy: vi.fn().mockResolvedValue(null),
    policyDecisions: vi.fn().mockResolvedValue([]),
    evaluatePolicy: vi.fn().mockResolvedValue({}),
    effectivePermissions: vi.fn().mockResolvedValue([]),
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
      expect(container.querySelector('[data-testid="tenant-settings-tab-users"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="tenant-settings-tab-compute"]')).toBeTruthy();
      // HSI §1.5 Admin tenant scope: workspace creation + Manage Workspaces
      expect(container.querySelector('[data-testid="tenant-settings-tab-workspaces"]')).toBeTruthy();
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
      expect(text).toContain('Secrets');
      expect(text).toContain('Audit');
      expect(text).toContain('Health');
      expect(text).toContain('Jobs');
    }, { timeout: 3000 });
  });

  it('Secrets tab loads the tenant-scoped secrets panel', async () => {
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const secretsTab = container.querySelector('[data-testid="tenant-settings-tab-secrets"]');
      expect(secretsTab).toBeTruthy();
    }, { timeout: 3000 });
    await fireEvent.click(container.querySelector('[data-testid="tenant-settings-tab-secrets"]'));
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="secrets-panel"]');
      expect(panel).toBeTruthy();
      expect(panel.getAttribute('data-scope')).toBe('tenant');
      expect(api.secretsList).toHaveBeenCalledWith('tenant', null);
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

  it('Users tab lists tenant users with search and deactivate action', async () => {
    api.me.mockResolvedValue({ id: 'u-admin', username: 'admin', tenant_id: 't-1' });
    api.listUsers.mockResolvedValue({
      users: [
        { id: 'u-1', username: 'alice', display_name: 'Alice', global_role: 'Member', created_at: 1690000000, deactivated_at: null },
        { id: 'u-admin', username: 'admin', global_role: 'TenantAdmin', created_at: 1690000000, deactivated_at: null },
      ],
      total: 2,
    });
    const { container } = render(TenantSettings);
    await waitFor(() => {
      expect(api.listUsers).toHaveBeenCalledWith({ limit: 200 });
    }, { timeout: 3000 });
    const table = await waitFor(() => {
      const el = container.querySelector('[data-testid="tenant-users-table"]');
      expect(el).toBeTruthy();
      return el;
    });
    const rows = table.querySelectorAll('[data-testid="tenant-user-row"]');
    expect(rows.length).toBe(2);
    // Self-deactivation is hidden (server rejects it too).
    expect(container.querySelector('[data-testid="deactivate-user-u-admin"]')).toBeNull();
    expect(container.querySelector('[data-testid="deactivate-user-u-1"]')).toBeTruthy();
    // Search filters the list client-side.
    const search = container.querySelector('[data-testid="tenant-users-search"]');
    await fireEvent.input(search, { target: { value: 'alice' } });
    expect(container.querySelectorAll('[data-testid="tenant-user-row"]').length).toBe(1);
  });

  it('Users tab deactivates a user through the API', async () => {
    window.prompt = vi.fn(() => 'offboarding');
    window.confirm = vi.fn(() => true);
    api.me.mockResolvedValue({ id: 'u-admin', username: 'admin', tenant_id: 't-1' });
    api.listUsers.mockResolvedValue({
      users: [{ id: 'u-1', username: 'alice', global_role: 'Member', created_at: 1690000000, deactivated_at: null }],
      total: 1,
    });
    const { container } = render(TenantSettings);
    const btn = await waitFor(() => {
      const el = container.querySelector('[data-testid="deactivate-user-u-1"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 3000 });
    await fireEvent.click(btn);
    await waitFor(() => {
      expect(api.deactivateUser).toHaveBeenCalledWith('u-1', 'offboarding');
    });
  });

  it('Users tab shows the not-admin note when the listing is forbidden', async () => {
    api.me.mockResolvedValue({ username: 'dev', tenant_id: 't-1' });
    api.listUsers.mockRejectedValue(Object.assign(new Error('forbidden'), { status: 403 }));
    const { container } = render(TenantSettings);
    await waitFor(() => {
      const panel = container.querySelector('[data-testid="tenant-tab-users"]');
      expect(panel?.textContent).toContain('TenantAdmin');
    }, { timeout: 3000 });
    expect(container.querySelector('[data-testid="tenant-users-table"]')).toBeNull();
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

  // HSI §1.5 Admin tenant scope: workspace creation (+ New Workspace button)
  // and Manage Workspaces list.
  describe('Workspaces tab', () => {
    it('lists all tenant workspaces', async () => {
      api.workspaces.mockResolvedValueOnce([
        { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided', created_at: 1700000000 },
        { id: 'ws-2', name: 'Platform', slug: 'platform', trust_level: 'Autonomous', created_at: 1700000001 },
      ]);
      const { container } = render(TenantSettings);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="tenant-settings-tab-workspaces"]')).toBeTruthy();
      }, { timeout: 3000 });
      container.querySelector('[data-testid="tenant-settings-tab-workspaces"]').click();
      await waitFor(() => {
        expect(container.querySelector('[data-testid="tenant-workspaces-table"]')).toBeTruthy();
      }, { timeout: 3000 });
      const rows = container.querySelectorAll('[data-testid="tenant-workspace-row"]');
      expect(rows.length).toBe(2);
      expect(rows[0].textContent).toContain('Payments');
      expect(rows[1].textContent).toContain('Platform');
    });

    it('New Workspace button opens the form; submit calls api.createWorkspace', async () => {
      const { container } = render(TenantSettings);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="tenant-settings-tab-workspaces"]')).toBeTruthy();
      }, { timeout: 3000 });
      container.querySelector('[data-testid="tenant-settings-tab-workspaces"]').click();
      await waitFor(() => {
        expect(container.querySelector('[data-testid="tenant-new-workspace-btn"]')).toBeTruthy();
      }, { timeout: 3000 });
      await fireEvent.click(container.querySelector('[data-testid="tenant-new-workspace-btn"]'));
      await waitFor(() => {
        const nameInput = container.querySelector('[data-testid="tenant-new-ws-name"]');
        expect(nameInput).toBeTruthy();
      }, { timeout: 3000 });
      await fireEvent.input(container.querySelector('[data-testid="tenant-new-ws-name"]'), { target: { value: 'Risk Engine' } });
      await fireEvent.submit(container.querySelector('[data-testid="tenant-new-workspace-form"]'));
      await waitFor(() => {
        expect(api.createWorkspace).toHaveBeenCalledWith(
          expect.objectContaining({ name: 'Risk Engine', slug: 'risk-engine' })
        );
      }, { timeout: 3000 });
    });
  });

  // ── ABAC policy management (abac-policy-engine.md §UI, task-132) ─────
  describe('Policies tab', () => {
    it('embeds the PolicyManager at tenant level on the global /policies endpoints', async () => {
      api.me.mockResolvedValue({ username: 'admin', email: 'admin@example.com', role: 'Admin', tenant_id: 'tenant-acme' });
      const { container } = render(TenantSettings);
      await fireEvent.click(container.querySelector('[data-testid="tenant-settings-tab-policies"]'));
      await waitFor(() => {
        expect(container.querySelector('[data-testid="policy-manager"]')).toBeTruthy();
      }, { timeout: 3000 });
      await waitFor(() => {
        expect(api.policies).toHaveBeenCalledWith({ scope: 'Tenant' });
      }, { timeout: 3000 });
      // The tab resolves the admin's own tenant to prefill the scope filter
      // once me() returns.
      await waitFor(() => {
        expect(api.policies).toHaveBeenCalledWith({ scope: 'Tenant', scope_id: 'tenant-acme' });
      }, { timeout: 3000 });
    });
  });
});
