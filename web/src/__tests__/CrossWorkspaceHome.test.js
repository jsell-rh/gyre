/**
 * CrossWorkspaceHome.test.js — Tests for ui-navigation.md §10 (Cross-Workspace View)
 *
 * Covers:
 *   - Component renders with all five sections
 *   - Decisions section: loads notifications, shows empty state
 *   - Workspaces section: loads workspace list, shows "All Workspaces" label
 *   - Specs section: loads cross-workspace specs
 *   - Agent Rules section: loads global meta-specs
 *   - onSelectWorkspace callback fires on workspace click
 *   - Section error states handled gracefully
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaces: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    metaSpecs: vi.fn().mockResolvedValue([]),
    getWorkspaceBriefing: vi.fn().mockResolvedValue(null),
  },
}));

import { api } from '../lib/api.js';
import CrossWorkspaceHome from '../components/CrossWorkspaceHome.svelte';

describe('CrossWorkspaceHome', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.myNotifications.mockResolvedValue([]);
    api.workspaces.mockResolvedValue([]);
    api.specsForWorkspace.mockResolvedValue([]);
    api.metaSpecs.mockResolvedValue([]);
    api.getWorkspaceBriefing.mockResolvedValue(null);
  });

  it('renders without throwing', () => {
    expect(() => render(CrossWorkspaceHome)).not.toThrow();
  });

  it('has data-testid="cross-workspace-home"', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="cross-workspace-home"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('renders all five section headings', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const text = container.textContent;
      expect(text).toContain('Decisions');
      expect(text).toContain('Workspaces');
      expect(text).toContain('Specs');
      expect(text).toContain('Briefing');
      expect(text).toContain('Agent Rules');
    }, { timeout: 3000 });
  });

  it('has testids for all five sections', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-decisions"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="section-workspaces"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="section-specs"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="section-agent-rules"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('calls api.myNotifications on mount', async () => {
    render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(api.myNotifications).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('calls api.workspaces on mount', async () => {
    render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(api.workspaces).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  it('calls api.specsForWorkspace(null) for all-workspace specs', async () => {
    render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(api.specsForWorkspace).toHaveBeenCalledWith(null);
    }, { timeout: 3000 });
  });

  it('calls api.metaSpecs with scope=Global for tenant rules', async () => {
    render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(api.metaSpecs).toHaveBeenCalledWith(expect.objectContaining({ scope: 'Global' }));
    }, { timeout: 3000 });
  });

  it('shows empty state when no decisions', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-decisions"]');
      expect(section?.textContent).toContain('No decisions needed');
    }, { timeout: 3000 });
  });

  it('shows empty state when no workspaces', async () => {
    api.workspaces.mockResolvedValue([]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-workspaces"]');
      expect(section?.textContent).toContain('No workspaces found');
    }, { timeout: 3000 });
  });

  it('shows empty state when no specs', async () => {
    api.specsForWorkspace.mockResolvedValue([]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-specs"]');
      expect(section?.textContent).toContain('No specs found');
    }, { timeout: 3000 });
  });

  it('shows empty state when no global meta-specs', async () => {
    api.metaSpecs.mockResolvedValue([]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-agent-rules"]');
      expect(section?.textContent).toContain('No tenant-level agent rules');
    }, { timeout: 3000 });
  });

  it('shows notification items when decisions present', async () => {
    api.myNotifications.mockResolvedValue([
      { id: 'n1', notification_type: 'gate_failure', message: 'Gate failed in payment-api', workspace_name: 'Payments' },
      { id: 'n2', notification_type: 'spec_approval', message: 'Spec approval needed', workspace_name: 'Billing' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-decisions"]');
      expect(section?.textContent).toContain('Gate failed in payment-api');
      expect(section?.textContent).toContain('Spec approval needed');
    }, { timeout: 3000 });
  });

  it('shows workspace badge on each decision', async () => {
    api.myNotifications.mockResolvedValue([
      { id: 'n1', notification_type: 'gate_failure', message: 'Gate failed', workspace_name: 'Payments' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-decisions"]');
      expect(section?.textContent).toContain('Payments');
    }, { timeout: 3000 });
  });

  it('shows workspace list items', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments', slug: 'payments' },
      { id: 'ws-2', name: 'Billing', slug: 'billing' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-workspaces"]');
      expect(section?.textContent).toContain('Payments');
      expect(section?.textContent).toContain('Billing');
    }, { timeout: 3000 });
  });

  it('workspace rows have testids', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments', slug: 'payments' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="workspace-row-ws-1"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('clicking workspace row calls onSelectWorkspace', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments', slug: 'payments' },
    ]);
    const onSelectWorkspace = vi.fn();
    const { container } = render(CrossWorkspaceHome, { props: { onSelectWorkspace } });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="workspace-row-ws-1"]')).toBeTruthy();
    }, { timeout: 3000 });

    const btn = container.querySelector('[data-testid="workspace-row-ws-1"]');
    btn.click();
    expect(onSelectWorkspace).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'ws-1', name: 'Payments' })
    );
  });

  it('shows specs in a table', async () => {
    api.specsForWorkspace.mockResolvedValue([
      { path: 'specs/auth.md', status: 'approved', workspace_name: 'Payments', repo_name: 'payment-api' },
      { path: 'specs/retry.md', status: 'draft', workspace_name: 'Billing', repo_name: 'billing-api' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const table = container.querySelector('[data-testid="specs-table"]');
      expect(table).toBeTruthy();
      expect(table.textContent).toContain('specs/auth.md');
      expect(table.textContent).toContain('approved');
      expect(table.textContent).toContain('specs/retry.md');
    }, { timeout: 3000 });
  });

  it('shows global meta-specs in agent rules section', async () => {
    api.metaSpecs.mockResolvedValue([
      { id: 'ms-1', name: 'security', kind: 'Persona', version: 2, required: true, status: 'Approved' },
      { id: 'ms-2', name: 'conventional-commits', kind: 'Principle', version: 3, required: false, status: 'Approved' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-agent-rules"]');
      expect(section?.textContent).toContain('security');
      expect(section?.textContent).toContain('conventional-commits');
    }, { timeout: 3000 });
  });

  it('decisions section shows badge with count', async () => {
    api.myNotifications.mockResolvedValue([
      { id: 'n1', notification_type: 'gate_failure', message: 'Gate failed' },
      { id: 'n2', notification_type: 'spec_approval', message: 'Approval' },
      { id: 'n3', notification_type: 'budget_warning', message: 'Budget' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const badge = container.querySelector('[data-testid="section-decisions"] .section-badge');
      expect(badge).toBeTruthy();
      expect(badge.textContent).toBe('3');
    }, { timeout: 3000 });
  });

  it('agent rules section shows Tenant-level scope tag', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-agent-rules"]');
      expect(section?.textContent).toContain('Tenant');
    }, { timeout: 3000 });
  });

  it('handles api error for decisions gracefully', async () => {
    api.myNotifications.mockRejectedValue(new Error('Network error'));
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-decisions"]');
      expect(section?.textContent).toContain('Network error');
    }, { timeout: 3000 });
  });

  it('handles api error for workspaces gracefully', async () => {
    api.workspaces.mockRejectedValue(new Error('Failed'));
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-workspaces"]');
      expect(section?.textContent).toContain('Failed');
    }, { timeout: 3000 });
  });

  it('handles api error for specs gracefully', async () => {
    api.specsForWorkspace.mockRejectedValue(new Error('Specs error'));
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-specs"]');
      expect(section?.textContent).toContain('Specs error');
    }, { timeout: 3000 });
  });

  it('handles api error for agent rules gracefully', async () => {
    api.metaSpecs.mockRejectedValue(new Error('Rules error'));
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-agent-rules"]');
      expect(section?.textContent).toContain('Rules error');
    }, { timeout: 3000 });
  });

  it('shows health indicator when workspace has health field', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments', health: 'healthy' },
      { id: 'ws-2', name: 'Billing', health: 'gate_failure' },
    ]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-workspaces"]');
      expect(section?.textContent).toContain('healthy');
    }, { timeout: 3000 });
  });

  it('shows "All Workspaces" title', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(container.querySelector('.cwh-title')?.textContent).toContain('All Workspaces');
    }, { timeout: 3000 });
  });

  // §10 Briefing section tests
  it('briefing section shows empty state when no workspaces have briefing data', async () => {
    api.workspaces.mockResolvedValue([]);
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-briefing"]');
      expect(section?.textContent).toContain('No briefing data available');
    }, { timeout: 3000 });
  });

  it('briefing section calls getWorkspaceBriefing per workspace', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments' },
      { id: 'ws-2', name: 'Billing' },
    ]);
    api.getWorkspaceBriefing.mockResolvedValue({ summary: 'All is well' });
    render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-1');
      expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-2');
    }, { timeout: 3000 });
  });

  it('briefing section shows workspace-attributed summaries', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments' },
    ]);
    api.getWorkspaceBriefing.mockResolvedValue({ summary: 'Payment retry implemented' });
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-briefing"]');
      expect(section?.textContent).toContain('Payments');
      expect(section?.textContent).toContain('Payment retry implemented');
    }, { timeout: 3000 });
  });

  it('briefing section handles partial workspace briefing failures gracefully', async () => {
    api.workspaces.mockResolvedValue([
      { id: 'ws-1', name: 'Payments' },
      { id: 'ws-2', name: 'Billing' },
    ]);
    // ws-1 succeeds, ws-2 fails
    api.getWorkspaceBriefing.mockImplementation((id) => {
      if (id === 'ws-1') return Promise.resolve({ summary: 'Payments: all good' });
      return Promise.reject(new Error('briefing unavailable'));
    });
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-briefing"]');
      // ws-1 summary should be shown; ws-2 failure should be silently skipped
      expect(section?.textContent).toContain('Payments: all good');
    }, { timeout: 3000 });
  });

  it('briefing section shows aggregated scope tag', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      const section = container.querySelector('[data-testid="section-briefing"]');
      expect(section?.textContent).toContain('Aggregated');
    }, { timeout: 3000 });
  });

  // ── Gear icon (tenant admin, §10) ─────────────────────────────────────
  it('does not show gear icon when onSettings prop is not provided', async () => {
    const { container } = render(CrossWorkspaceHome);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="cross-workspace-home"]')).toBeTruthy();
    }, { timeout: 3000 });
    expect(container.querySelector('[data-testid="tenant-gear-btn"]')).toBeNull();
  });

  it('shows gear icon when onSettings prop is provided', async () => {
    const onSettings = vi.fn();
    const { container } = render(CrossWorkspaceHome, { props: { onSettings } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-gear-btn"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('clicking gear icon calls onSettings', async () => {
    const onSettings = vi.fn();
    const { container } = render(CrossWorkspaceHome, { props: { onSettings } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="tenant-gear-btn"]')).toBeTruthy();
    }, { timeout: 3000 });
    container.querySelector('[data-testid="tenant-gear-btn"]').click();
    expect(onSettings).toHaveBeenCalled();
  });

  it('gear icon has correct aria-label', async () => {
    const onSettings = vi.fn();
    const { container } = render(CrossWorkspaceHome, { props: { onSettings } });
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="tenant-gear-btn"]');
      expect(btn?.getAttribute('aria-label')).toContain('Tenant administration');
    }, { timeout: 3000 });
  });
});
