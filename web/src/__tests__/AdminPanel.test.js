import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import AdminPanel from '../components/AdminPanel.svelte';

// NOTE: vi.mock is hoisted — no top-level variables allowed inside the factory.
// All mock data is inlined here.
vi.mock('../lib/api.js', () => ({
  api: {
    // Workspace
    workspace: vi.fn().mockResolvedValue({
      id: 'ws-1', name: 'Test Workspace', description: 'A test workspace', trust_level: 'Autonomous',
    }),
    workspaceBudget: vi.fn().mockResolvedValue({ used: 50, limit: 100, currency: 'USD' }),
    workspaceMembers: vi.fn().mockResolvedValue([
      { id: 'u1', name: 'Alice', email: 'alice@example.com', role: 'admin', last_active: null },
    ]),
    workspaceAbacPolicies: vi.fn().mockResolvedValue([
      { id: 'p1', name: 'builtin:default-deny', effect: 'Deny', actions: ['merge'], resource_types: ['mr'] },
      { id: 'p2', name: 'trust:require-human-mr-review', effect: 'Deny', actions: ['merge'], resource_types: ['mr'] },
      { id: 'p3', name: 'my-custom-policy', effect: 'Allow', actions: ['read'], resource_types: ['spec'] },
    ]),
    updateWorkspace: vi.fn().mockResolvedValue({
      id: 'ws-1', name: 'Test Workspace', description: 'A test workspace', trust_level: 'Guided',
    }),
    setWorkspaceBudget: vi.fn().mockResolvedValue(null),
    addWorkspaceMember: vi.fn().mockResolvedValue(null),
    removeWorkspaceMember: vi.fn().mockResolvedValue(null),
    createWorkspaceAbacPolicy: vi.fn().mockResolvedValue({ id: 'p4', name: 'new-policy', effect: 'Allow', actions: ['read'], resource_types: ['spec'] }),
    deleteWorkspaceAbacPolicy: vi.fn().mockResolvedValue(null),
    simulateAbacPolicy: vi.fn().mockResolvedValue({ outcome: 'Allow', matched_policies: ['my-custom-policy'] }),
    // Tenant
    computeList: vi.fn().mockResolvedValue([]),
    budgetSummary: vi.fn().mockResolvedValue(null),
    auditEvents: vi.fn().mockResolvedValue({ events: [] }),
    workspaces: vi.fn().mockResolvedValue([
      { id: 'ws-1', name: 'Test Workspace', trust_level: 'Autonomous' },
    ]),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-2', name: 'New Workspace' }),
    computeCreate: vi.fn().mockResolvedValue(null),
    computeDelete: vi.fn().mockResolvedValue(null),
    // Repo
    repoGates: vi.fn().mockResolvedValue([]),
    repoAbacPolicy: vi.fn().mockResolvedValue([]),
    createRepoGate: vi.fn().mockResolvedValue(null),
    deleteRepoGate: vi.fn().mockResolvedValue(null),
  },
}));

// Mock toast module
vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

describe('AdminPanel (tenant scope — no props)', () => {
  it('renders without throwing', () => {
    expect(() => render(AdminPanel)).not.toThrow();
  });

  it('shows Admin heading for tenant scope', () => {
    const { getByText } = render(AdminPanel);
    expect(getByText('Admin')).toBeTruthy();
  });

  it('shows tenant tab bar with Workspaces tab', () => {
    const { getByText } = render(AdminPanel);
    expect(getByText('Workspaces')).toBeTruthy();
    expect(getByText('Compute')).toBeTruthy();
    expect(getByText('Budget')).toBeTruthy();
    expect(getByText('Audit')).toBeTruthy();
  });
});

describe('AdminPanel (workspace scope)', () => {
  it('renders without throwing', () => {
    expect(() => render(AdminPanel, { props: { workspaceId: 'ws-1' } })).not.toThrow();
  });

  it('shows "Workspace Admin" heading', () => {
    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });
    expect(getByText('Workspace Admin')).toBeTruthy();
  });

  it('shows workspace scope tab bar', () => {
    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });
    expect(getByText('Settings')).toBeTruthy();
    expect(getByText('Budget')).toBeTruthy();
    expect(getByText('Trust Level')).toBeTruthy();
    expect(getByText('Teams')).toBeTruthy();
    expect(getByText('Policies')).toBeTruthy();
  });

  it('renders trust level tab with all four options', async () => {
    const { getByText, container } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Trust Level'));

    await waitFor(() => {
      // Check trust option labels are present (query by class to avoid duplicates with trust-current)
      const labels = Array.from(container.querySelectorAll('.trust-option-label')).map(el => el.textContent.trim());
      expect(labels).toContain('Supervised');
      expect(labels).toContain('Guided');
      expect(labels).toContain('Autonomous');
      expect(labels).toContain('Custom');
    });
  });

  it('trust level radio reflects current trust_level from workspace', async () => {
    const { getByText, container } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Trust Level'));

    await waitFor(() => {
      // "Autonomous" option should be selected (trust_level: 'Autonomous' from mockWorkspace)
      const autonomousOption = container.querySelector('.trust-option.selected');
      expect(autonomousOption).toBeTruthy();
      expect(autonomousOption.textContent).toContain('Autonomous');
    });
  });

  it('clicking a different trust level opens confirm modal', async () => {
    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Trust Level'));

    await waitFor(() => expect(getByText('Guided')).toBeTruthy());

    await fireEvent.click(getByText('Guided'));

    await waitFor(() => {
      expect(getByText('Change Trust Level')).toBeTruthy();
    });
  });

  it('confirming trust change calls updateWorkspace with correct trust_level', async () => {
    const { api } = await import('../lib/api.js');
    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Trust Level'));
    await waitFor(() => expect(getByText('Guided')).toBeTruthy());

    await fireEvent.click(getByText('Guided'));
    await waitFor(() => expect(getByText('Change Trust Level')).toBeTruthy());

    await fireEvent.click(getByText('Switch to Guided'));

    await waitFor(() => {
      expect(api.updateWorkspace).toHaveBeenCalledWith('ws-1', { trust_level: 'Guided' });
    });
  });

  it('409 error shows correct error toast', async () => {
    const { api } = await import('../lib/api.js');
    const { toastError } = await import('../lib/toast.svelte.js');

    api.updateWorkspace.mockRejectedValueOnce(new Error('API /workspaces/ws-1: 409 Conflict'));

    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Trust Level'));
    await waitFor(() => expect(getByText('Supervised')).toBeTruthy());

    await fireEvent.click(getByText('Supervised'));
    await waitFor(() => expect(getByText('Change Trust Level')).toBeTruthy());

    await fireEvent.click(getByText('Switch to Supervised'));

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith('Trust level transition failed — policies could not be created');
    });
  });

  it('policies tab groups policies by prefix', async () => {
    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Policies'));

    await waitFor(() => {
      // Should show prefix badge labels
      expect(getByText('builtin:')).toBeTruthy();
      expect(getByText('trust:')).toBeTruthy();
    });
  });

  it('policies tab shows locked note for non-Custom trust', async () => {
    const { container } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    // Click Policies tab — need to find it by text without being confused by other tabs
    const policiesTab = container.querySelector('[data-tab="policies"]') ??
      Array.from(container.querySelectorAll('button')).find(b => b.textContent.trim() === 'Policies');
    if (policiesTab) await fireEvent.click(policiesTab);

    await waitFor(() => {
      // mockWorkspace has trust_level: 'Autonomous' — editor should be locked
      expect(container.textContent).toContain('Switch to');
      expect(container.textContent).toContain('Custom');
      expect(container.textContent).toContain('trust level');
    });
  });

  it('Custom trust unlocks policy editor with New Policy button', async () => {
    const { api } = await import('../lib/api.js');

    // Override workspace mock to return Custom trust
    api.workspace.mockResolvedValueOnce({ id: 'ws-1', name: 'Test Workspace', description: '', trust_level: 'Custom' });

    const { getByText } = render(AdminPanel, { props: { workspaceId: 'ws-1' } });

    await fireEvent.click(getByText('Policies'));

    await waitFor(() => {
      expect(getByText('+ New Policy')).toBeTruthy();
    });
  });
});

describe('AdminPanel (repo scope)', () => {
  it('renders without throwing', () => {
    expect(() => render(AdminPanel, { props: { repoId: 'repo-1' } })).not.toThrow();
  });

  it('shows Repo Admin heading', () => {
    const { getByText } = render(AdminPanel, { props: { repoId: 'repo-1' } });
    expect(getByText('Repo Admin')).toBeTruthy();
  });

  it('shows repo scope tabs', () => {
    const { getByText } = render(AdminPanel, { props: { repoId: 'repo-1' } });
    expect(getByText('Settings')).toBeTruthy();
    expect(getByText('Gates')).toBeTruthy();
    expect(getByText('Policies')).toBeTruthy();
  });

  it('shows Add Gate button in Gates tab', async () => {
    const { getByText } = render(AdminPanel, { props: { repoId: 'repo-1' } });

    await fireEvent.click(getByText('Gates'));

    await waitFor(() => {
      expect(getByText('+ Add Gate')).toBeTruthy();
    });
  });
});
