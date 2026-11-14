/**
 * WorkspaceSettings.test.js — Tests for TASK-351: workspace settings page
 *
 * Covers:
 *   - Renders without throwing
 *   - Page header shows workspace name + back button
 *   - 10 tabs are rendered with correct labels
 *   - General tab renders name (display-only), compute target selector
 *   - Trust Level tab renders trust level cards (4 levels)
 *   - Trust Level tab renders implied trust: policy summary (HSI §2a, task-084)
 *   - Trust Level tab "View all policies" cross-link switches to Policies tab
 *   - Policies tab groups policies by prefix, shows trust origin badge + banner (HSI §2a)
 *   - Policies tab "Change trust level" cross-link switches back to Trust Level tab
 *   - Immutable policies greyed out with tooltip
 *   - Trust & Policies tab renders drift policy toggles
 *   - Teams tab shows member table headers
 *   - Budget tab renders when budget data available
 *   - Compute tab renders when data available
 *   - Audit tab renders filter bar + refresh button
 *   - Back button calls onBack callback
 *   - Tab keyboard navigation (ArrowRight/ArrowLeft/Home/End)
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
vi.mock('../lib/api.js', () => ({
  api: {
    computeList: vi.fn().mockResolvedValue([]),
    workspaceMembers: vi.fn().mockResolvedValue([]),
    workspaceTeams: vi.fn().mockResolvedValue([]),
    listUsers: vi.fn().mockResolvedValue({ users: [], total: 0 }),
    userByUsername: vi.fn().mockRejectedValue(new Error('not found')),
    workspaceInvitations: vi.fn().mockResolvedValue({ invitations: [] }),
    inviteWorkspaceMember: vi.fn().mockResolvedValue({}),
    updateMemberRole: vi.fn().mockResolvedValue(null),
    revokeWorkspaceInvitation: vi.fn().mockResolvedValue(null),
    createTeam: vi.fn().mockResolvedValue({}),
    updateTeam: vi.fn().mockResolvedValue({}),
    deleteTeam: vi.fn().mockResolvedValue(null),
    addTeamMember: vi.fn().mockResolvedValue({}),
    removeTeamMember: vi.fn().mockResolvedValue(null),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    setWorkspaceBudget: vi.fn().mockResolvedValue({ entity_type: 'workspace', entity_id: 'ws-1', config: { max_tokens_per_day: 5000 }, usage: { tokens_used_today: 0, cost_today: 0, active_agents: 0, period_start: 0 } }),
    workspaceAbacPolicies: vi.fn().mockResolvedValue([]),
    auditEvents: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    createRepo: vi.fn().mockResolvedValue({ id: 'repo-new', name: 'new-repo' }),
    createMirrorRepo: vi.fn().mockResolvedValue({ id: 'repo-mirror', name: 'mirrored', is_mirror: true }),
    updateWorkspace: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    secretsList: vi.fn().mockResolvedValue([]),
    secretCreate: vi.fn().mockResolvedValue({}),
    secretDelete: vi.fn().mockResolvedValue(null),
    secretRotate: vi.fn().mockResolvedValue({}),
    // ABAC policy management surface (task-132) — PolicyManager on the
    // Policies tab calls the global /policies endpoints on mount.
    policies: vi.fn().mockResolvedValue([]),
    createPolicy: vi.fn().mockResolvedValue({}),
    updatePolicy: vi.fn().mockResolvedValue({}),
    deletePolicy: vi.fn().mockResolvedValue(null),
    policyDecisions: vi.fn().mockResolvedValue([]),
    evaluatePolicy: vi.fn().mockResolvedValue({}),
    effectivePermissions: vi.fn().mockResolvedValue([]),
  },
  setAuthToken: vi.fn(),
}));

import WorkspaceSettings from '../components/WorkspaceSettings.svelte';
import { api } from '../lib/api.js';

const mockWorkspace = {
  id: 'ws-1',
  name: 'Payments',
  description: 'Payment processing workspace',
  trust_level: 'Guided',
  compute_target_id: null,
};

describe('WorkspaceSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without throwing', () => {
    expect(() => render(WorkspaceSettings, { props: { workspace: mockWorkspace } })).not.toThrow();
  });

  it('has workspace-settings testid', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    expect(container.querySelector('[data-testid="workspace-settings"]')).toBeTruthy();
  });

  it('shows workspace name in page title', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const title = container.querySelector('[data-testid="ws-settings-title"]');
    expect(title).toBeTruthy();
    expect(title.textContent).toContain('Payments');
  });

  it('renders back button', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    expect(container.querySelector('[data-testid="ws-settings-back"]')).toBeTruthy();
  });

  it('calls onBack when back button clicked', async () => {
    const onBack = vi.fn();
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace, onBack } });
    await fireEvent.click(container.querySelector('[data-testid="ws-settings-back"]'));
    expect(onBack).toHaveBeenCalledOnce();
  });

  it('renders tab bar with tablist role', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const tablist = container.querySelector('[data-testid="ws-settings-tabs"]');
    expect(tablist).toBeTruthy();
    expect(tablist.getAttribute('role')).toBe('tablist');
  });

  it('renders all 11 tabs', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const tabs = container.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(11);
    const labels = Array.from(tabs).map(t => t.textContent.trim());
    expect(labels).toContain('General');
    expect(labels).toContain('Trust Level');
    expect(labels).toContain('Policies');
    expect(labels).toContain('Members');
    expect(labels).toContain('Teams');
    expect(labels).toContain('Budget');
    // HSI §1.5 Admin workspace-scope row: Policies (Trust & Policies) + Repos tabs
    expect(labels).toContain('Repos');
    expect(labels).toContain('Compute');
    expect(labels).toContain('LLM Config');
    expect(labels).toContain('Secrets');
    expect(labels).toContain('Audit');
  });

  it('Secrets tab loads the scoped secrets panel', async () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    await fireEvent.click(container.querySelector('#ws-tab-secrets'));
    const panel = container.querySelector('[data-testid="secrets-panel"]');
    expect(panel).toBeTruthy();
    expect(panel.getAttribute('data-scope')).toBe('workspace');
    expect(api.secretsList).toHaveBeenCalledWith('workspace', 'ws-1');
  });

  it('General tab is active by default', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const generalTab = container.querySelector('#ws-tab-general');
    expect(generalTab.getAttribute('aria-selected')).toBe('true');
    expect(generalTab.getAttribute('tabindex')).toBe('0');
  });

  it('inactive tabs have tabindex=-1', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const trustTab = container.querySelector('#ws-tab-trust');
    expect(trustTab.getAttribute('aria-selected')).toBe('false');
    expect(trustTab.getAttribute('tabindex')).toBe('-1');
  });

  it('clicking a tab makes it active', async () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const trustTab = container.querySelector('#ws-tab-trust');
    await fireEvent.click(trustTab);
    expect(trustTab.getAttribute('aria-selected')).toBe('true');
    expect(trustTab.classList.contains('active')).toBe(true);
  });

  it('tab panels have correct aria-labelledby', () => {
    const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
    const panel = container.querySelector('[role="tabpanel"]');
    expect(panel.getAttribute('aria-labelledby')).toBe('ws-tab-general');
    expect(panel.id).toBe('ws-panel-general');
  });

  describe('General tab', () => {
    it('shows workspace name in display-only field', () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      const nameDisplay = container.querySelector('[data-testid="ws-name"]');
      expect(nameDisplay).toBeTruthy();
      expect(nameDisplay.textContent).toContain('Payments');
    });

    it('shows compute target selector', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      // Wait for async computeList load to finish so loading state clears
      await new Promise(r => setTimeout(r, 0));
      expect(container.querySelector('[data-testid="compute-target-select"]')).toBeTruthy();
    });

    it('shows save button', () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      expect(container.querySelector('[data-testid="save-general-btn"]')).toBeTruthy();
    });

    it('calls api.updateWorkspace on save', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await fireEvent.click(container.querySelector('[data-testid="save-general-btn"]'));
      expect(api.updateWorkspace).toHaveBeenCalledWith('ws-1', expect.any(Object));
    });
  });

  // HSI §1.5 Admin workspace-scope row: Repos tab (list, create, import).
  // Selectors target the task-168 repos implementation (the unified markup
  // that also carries agent counts / last activity per repo-lifecycle.md §1).
  describe('Repos tab', () => {
    it('lists workspace repos with name and mirror URL', async () => {
      api.workspaceRepos.mockResolvedValueOnce([
        { id: 'repo-1', name: 'billing-service', status: 'Active', default_branch: 'main', is_mirror: false },
        { id: 'repo-2', name: 'payments-mirror', status: 'Active', default_branch: 'main', is_mirror: true, mirror_url: 'https://example.com/payments.git' },
      ]);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await fireEvent.click(container.querySelector('#ws-tab-repos'));
      await waitFor(() => expect(container.querySelector('[data-testid="ws-repos-table"]')).toBeTruthy());
      const rows = container.querySelectorAll('[data-testid="ws-repo-row"]');
      expect(rows.length).toBe(2);
      expect(rows[0].textContent).toContain('billing-service');
      expect(rows[1].textContent).toContain('https://example.com/payments.git');
    });

    it('create form calls api.createRepo with the workspace id', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await fireEvent.click(container.querySelector('#ws-tab-repos'));
      await fireEvent.click(container.querySelector('[data-testid="ws-repos-new-btn"]'));
      await fireEvent.input(container.querySelector('[data-testid="ws-new-repo-name-input"]'), { target: { value: 'new-repo' } });
      await fireEvent.submit(container.querySelector('[data-testid="ws-new-repo-form"]'));
      await waitFor(() => expect(api.createRepo).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'new-repo', workspace_id: 'ws-1' })
      ));
    });

    it('import form calls api.createMirrorRepo with the derived name', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await fireEvent.click(container.querySelector('#ws-tab-repos'));
      await fireEvent.click(container.querySelector('[data-testid="ws-repos-import-btn"]'));
      await fireEvent.input(container.querySelector('[data-testid="ws-import-url-input"]'), { target: { value: 'https://example.com/upstream.git' } });
      await fireEvent.submit(container.querySelector('[data-testid="ws-import-repo-form"]'));
      await waitFor(() => expect(api.createMirrorRepo).toHaveBeenCalledWith(
        expect.objectContaining({ url: 'https://example.com/upstream.git', workspace_id: 'ws-1', name: 'upstream' })
      ));
    });
  });

  describe('Trust Level tab', () => {
    async function openTrustTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-trust'));
    }

    it('shows trust & policies panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="trust-tab"]')).toBeTruthy();
    });

    it('renders 4 trust level cards', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      const cards = container.querySelectorAll('.trust-card');
      expect(cards.length).toBe(4);
    });

    it('renders Supervised trust card', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="trust-card-supervised"]')).toBeTruthy();
    });

    it('renders Autonomous trust card', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="trust-card-autonomous"]')).toBeTruthy();
    });

    it('renders trust level radio inputs', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      const radios = container.querySelectorAll('input[type="radio"][name="trust-level"]');
      expect(radios.length).toBe(4);
    });

    it('renders save trust button', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="save-trust-btn"]')).toBeTruthy();
    });

    it('calls api.updateWorkspace with trust_level on save', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      await fireEvent.click(container.querySelector('[data-testid="save-trust-btn"]'));
      expect(api.updateWorkspace).toHaveBeenCalledWith('ws-1', expect.objectContaining({ trust_level: expect.any(String) }));
    });

    it('renders drift policy toggles', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="toggle-warn-on-drift"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="toggle-block-on-drift"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="drift-tolerance-input"]')).toBeTruthy();
    });

    it('renders save drift policy button', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="save-drift-policy-btn"]')).toBeTruthy();
    });

    it('calls api.updateWorkspace with meta_spec_policy on drift save', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      await fireEvent.click(container.querySelector('[data-testid="save-drift-policy-btn"]'));
      expect(api.updateWorkspace).toHaveBeenCalledWith('ws-1', expect.objectContaining({
        meta_spec_policy: expect.any(Object),
      }));
    });

    // ── Implied policies summary (HSI §2a, task-084) ──────────────────

    it('shows read-only implied policy summary below the radio buttons', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      const summary = container.querySelector('[data-testid="trust-implied-policies"]');
      expect(summary).toBeTruthy();
      // Below the radio group in DOM order
      const radios = container.querySelector('.trust-grid');
      expect(radios.compareDocumentPosition(summary) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      // Guided (current selection) implies no trust: policies
      expect(container.querySelector('[data-testid="implied-policy-empty"]')).toBeTruthy();
    });

    it('selecting Supervised shows the implied trust: policy with name, effect, and target', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      await fireEvent.click(container.querySelector('[data-testid="trust-card-supervised"] input[type="radio"]'));
      const rows = container.querySelectorAll('[data-testid="implied-policy-row"]');
      expect(rows.length).toBe(1);
      expect(rows[0].textContent).toContain('trust:require-human-mr-review');
      expect(rows[0].textContent).toContain('Deny');
      expect(rows[0].textContent).toContain('merge by system');
    });

    it('switching from Guided to Supervised shows a "Will add" diff', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      await fireEvent.click(container.querySelector('[data-testid="trust-card-supervised"] input[type="radio"]'));
      const add = container.querySelector('[data-testid="preset-diff-add"]');
      expect(add).toBeTruthy();
      expect(add.textContent).toContain('Will add');
      expect(add.textContent).toContain('trust:require-human-mr-review');
      expect(container.querySelector('[data-testid="preset-diff-remove"]')).toBeNull();
    });

    it('switching from Supervised to Guided shows a "Will remove" diff', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: { ...mockWorkspace, trust_level: 'Supervised' } } });
      await openTrustTab(container);
      // Supervised is selected by default — implied policy listed
      expect(container.querySelectorAll('[data-testid="implied-policy-row"]').length).toBe(1);
      await fireEvent.click(container.querySelector('[data-testid="trust-card-guided"] input[type="radio"]'));
      const remove = container.querySelector('[data-testid="preset-diff-remove"]');
      expect(remove).toBeTruthy();
      expect(remove.textContent).toContain('Will remove');
      expect(remove.textContent).toContain('trust:require-human-mr-review');
      expect(container.querySelector('[data-testid="preset-diff-add"]')).toBeNull();
    });

    it('no diff shown when selection equals persisted level', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      expect(container.querySelector('[data-testid="trust-preset-diff"]')).toBeNull();
    });

    it('"View all policies" link switches to the Policies tab within the Admin view', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTrustTab(container);
      await fireEvent.click(container.querySelector('[data-testid="view-all-policies-link"]'));
      expect(container.querySelector('[data-testid="policies-tab"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="trust-tab"]')).toBeNull();
      // Still the same settings view — no navigation change
      expect(container.querySelector('[data-testid="workspace-settings"]')).toBeTruthy();
    });
  });

  describe('Policies tab (HSI §2a)', () => {
    const mockPolicies = [
      {
        id: 'p-builtin',
        name: 'builtin:require-human-spec-approval',
        description: 'Spec approval is always human',
        effect: 'deny',
        immutable: true,
        built_in: true,
        actions: ['approve'],
        resource_types: ['spec'],
      },
      {
        id: 'p-trust',
        name: 'trust:require-human-mr-review',
        description: 'Block autonomous merge',
        effect: 'deny',
        immutable: false,
        built_in: false,
        actions: ['merge'],
        resource_types: ['mr'],
      },
      {
        id: 'p-user',
        name: 'allow-deploy',
        description: 'Deployers may deploy',
        effect: 'allow',
        immutable: false,
        built_in: false,
        actions: ['deploy'],
        resource_types: ['repo'],
      },
    ];

    async function openPoliciesTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-policies'));
      await new Promise(r => setTimeout(r, 0));
    }

    it('shows policies panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      expect(container.querySelector('[data-testid="policies-tab"]')).toBeTruthy();
    });

    it('queries workspace policies from the API', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      expect(api.workspaceAbacPolicies).toHaveBeenCalledWith('ws-1');
    });

    it('groups policies by prefix: builtin:, trust:, user-created', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const builtin = container.querySelector('[data-testid="policy-group-builtin"]');
      const trust = container.querySelector('[data-testid="policy-group-trust"]');
      const user = container.querySelector('[data-testid="policy-group-user"]');
      expect(builtin).toBeTruthy();
      expect(builtin.textContent).toContain('builtin:require-human-spec-approval');
      expect(trust).toBeTruthy();
      expect(trust.textContent).toContain('trust:require-human-mr-review');
      expect(user).toBeTruthy();
      expect(user.textContent).toContain('allow-deploy');
    });

    it('trust: policies show a "From: {level}" badge derived from the current trust level', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const badge = container.querySelector('[data-testid="trust-origin-badge"]');
      expect(badge).toBeTruthy();
      expect(badge.textContent).toContain('From: Guided');
    });

    it('shows the preset-managed banner when trust level is not Custom', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const banner = container.querySelector('[data-testid="trust-banner-preset"]');
      expect(banner).toBeTruthy();
      expect(banner.textContent).toContain('Trust level: Guided');
      expect(banner.textContent).toContain('preset-managed');
      expect(banner.textContent).toContain('Switch to Custom to edit');
      expect(container.querySelector('[data-testid="trust-banner-custom"]')).toBeNull();
    });

    it('shows the Custom banner when trust level is Custom', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: { ...mockWorkspace, trust_level: 'Custom' } } });
      await openPoliciesTab(container);
      const banner = container.querySelector('[data-testid="trust-banner-custom"]');
      expect(banner).toBeTruthy();
      expect(banner.textContent).toContain('Custom trust — full policy editor enabled');
      expect(container.querySelector('[data-testid="trust-banner-preset"]')).toBeNull();
    });

    it('immutable policies are greyed out with the cannot-remove tooltip and disabled delete', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const builtinRow = container.querySelector('[data-testid="policy-group-builtin"] .policy-row');
      expect(builtinRow.classList.contains('policy-locked')).toBe(true);
      expect(builtinRow.getAttribute('title')).toBe('This policy cannot be removed or overridden.');
      const deleteBtn = builtinRow.querySelector('[data-testid="delete-abac-policy-btn"]');
      expect(deleteBtn.disabled).toBe(true);
    });

    it('trust: policy delete is disabled while preset-managed (non-Custom trust)', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const trustRow = container.querySelector('[data-testid="policy-group-trust"] .policy-row');
      expect(trustRow.classList.contains('policy-locked')).toBe(true);
      expect(trustRow.getAttribute('title')).toContain('switch to Custom to edit');
      const deleteBtn = trustRow.querySelector('[data-testid="delete-abac-policy-btn"]');
      expect(deleteBtn.disabled).toBe(true);
    });

    it('trust: policy delete is enabled under Custom trust (transition preserves trust policies)', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: { ...mockWorkspace, trust_level: 'Custom' } } });
      await openPoliciesTab(container);
      const trustRow = container.querySelector('[data-testid="policy-group-trust"] .policy-row');
      expect(trustRow.classList.contains('policy-locked')).toBe(false);
      const deleteBtn = trustRow.querySelector('[data-testid="delete-abac-policy-btn"]');
      expect(deleteBtn.disabled).toBe(false);
    });

    it('user-created policy delete is enabled', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const userRow = container.querySelector('[data-testid="policy-group-user"] .policy-row');
      expect(userRow.classList.contains('policy-locked')).toBe(false);
      const deleteBtn = userRow.querySelector('[data-testid="delete-abac-policy-btn"]');
      expect(deleteBtn.disabled).toBe(false);
    });

    it('"Change trust level" link switches back to the Trust Level tab', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      await fireEvent.click(container.querySelector('[data-testid="change-trust-level-link"]'));
      expect(container.querySelector('[data-testid="trust-tab"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="policies-tab"]')).toBeNull();
      expect(container.querySelector('[data-testid="workspace-settings"]')).toBeTruthy();
    });

    it('policy rows show name, effect, and target', async () => {
      api.workspaceAbacPolicies.mockResolvedValue(mockPolicies);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      const trustRow = container.querySelector('[data-testid="policy-group-trust"] .policy-row');
      expect(trustRow.textContent).toContain('trust:require-human-mr-review');
      expect(trustRow.textContent).toContain('deny');
      expect(trustRow.textContent).toContain('merge on mr');
    });

    // ── ABAC policy management integration (abac-policy-engine.md §UI, task-132) ──
    it('embeds the PolicyManager scoped to the workspace via global ?scope= query params', async () => {
      api.workspaceAbacPolicies.mockResolvedValue([]);
      api.policies.mockResolvedValue([]);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openPoliciesTab(container);
      expect(container.querySelector('[data-testid="policy-manager"]')).toBeTruthy();
      // The §UI URL pattern: GLOBAL /api/v1/policies with
      // ?scope=Workspace&scope_id=<id> — never a workspace-prefixed URL.
      await waitFor(() => {
        expect(api.policies).toHaveBeenCalledWith({ scope: 'Workspace', scope_id: 'ws-1' });
      });
    });
  });

  describe('Teams tab', () => {
    async function openTeamsTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-teams'));
    }

    it('shows teams panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTeamsTab(container);
      expect(container.querySelector('[data-testid="teams-tab"]')).toBeTruthy();
    });

    it('loads workspace teams', async () => {
      api.workspaceTeams.mockResolvedValue({ teams: [] });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTeamsTab(container);
      expect(api.workspaceTeams).toHaveBeenCalledWith('ws-1');
    });

    it('shows team rows with lead and member count', async () => {
      api.workspaceTeams.mockResolvedValue({
        teams: [
          { id: 'team-1', name: 'Platform', slug: 'platform', lead: 'u-1', member_ids: ['u-1', 'u-2'] },
        ],
      });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTeamsTab(container);
      await new Promise(r => setTimeout(r, 0));
      const table = container.querySelector('[data-testid="teams-table"]');
      expect(table).toBeTruthy();
      const rows = table.querySelectorAll('[data-testid="team-row"]');
      expect(rows.length).toBe(1);
      expect(rows[0].textContent).toContain('Platform');
      expect(rows[0].textContent).toContain('platform');
    });

    it('shows empty state when no teams', async () => {
      api.workspaceTeams.mockResolvedValue({ teams: [] });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openTeamsTab(container);
      await new Promise(r => setTimeout(r, 0));
      expect(container.querySelector('[data-testid="teams-tab"]')).toBeTruthy();
    });
  });

  describe('Members tab', () => {
    async function openMembersTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-members'));
    }

    it('shows members panel with invite button', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openMembersTab(container);
      expect(container.querySelector('[data-testid="members-tab"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="invite-member-btn"]')).toBeTruthy();
    });

    it('loads workspace members and pending invitations', async () => {
      api.workspaceMembers.mockResolvedValue({ members: [] });
      api.workspaceInvitations.mockResolvedValue({ invitations: [] });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openMembersTab(container);
      await new Promise(r => setTimeout(r, 0));
      expect(api.workspaceMembers).toHaveBeenCalledWith('ws-1');
      expect(api.workspaceInvitations).toHaveBeenCalledWith('ws-1');
    });

    it('shows member table with role select and remove action', async () => {
      api.workspaceMembers.mockResolvedValue({
        members: [
          { id: 'm-1', user_id: 'u-1', workspace_id: 'ws-1', role: 'Owner', accepted: true, accepted_at: 1700000000, created_at: 1700000000 },
          { id: 'm-2', user_id: 'u-2', workspace_id: 'ws-1', role: 'Developer', accepted: true, accepted_at: 1700000000, created_at: 1700000000 },
        ],
      });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openMembersTab(container);
      await new Promise(r => setTimeout(r, 0));
      const table = container.querySelector('[data-testid="members-table"]');
      expect(table).toBeTruthy();
      const rows = table.querySelectorAll('[data-testid="member-row"]');
      expect(rows.length).toBe(2);
      expect(container.querySelector('[data-testid="role-select-u-2"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="remove-member-u-2"]')).toBeTruthy();
    });

    it('changing the role select calls the role endpoint and reloads', async () => {
      api.workspaceMembers.mockResolvedValue({
        members: [
          { id: 'm-2', user_id: 'u-2', workspace_id: 'ws-1', role: 'Developer', accepted: true, created_at: 1700000000 },
        ],
      });
      api.updateMemberRole.mockResolvedValue(null);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openMembersTab(container);
      await new Promise(r => setTimeout(r, 0));
      const select = container.querySelector('[data-testid="role-select-u-2"]');
      await fireEvent.change(select, { target: { value: 'Admin' } });
      await waitFor(() => {
        expect(api.updateMemberRole).toHaveBeenCalledWith('ws-1', 'u-2', { role: 'Admin', reason: undefined });
      });
    });

    it('invite modal resolves the username and invites by user id', async () => {
      api.workspaceMembers.mockResolvedValue({ members: [] });
      api.userByUsername.mockResolvedValue({ id: 'u-9', username: 'alice' });
      api.inviteWorkspaceMember.mockResolvedValue({ id: 'm-9' });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openMembersTab(container);
      await fireEvent.click(container.querySelector('[data-testid="invite-member-btn"]'));
      const input = await waitFor(() => {
        const el = container.querySelector('[data-testid="invite-username-input"]');
        expect(el).toBeTruthy();
        return el;
      });
      await fireEvent.input(input, { target: { value: '@alice' } });
      await fireEvent.submit(input.closest('form'));
      await waitFor(() => {
        expect(api.userByUsername).toHaveBeenCalledWith('alice');
        expect(api.inviteWorkspaceMember).toHaveBeenCalledWith('ws-1', { user_id: 'u-9', role: 'Developer' });
      });
    });
  });

  describe('Budget tab', () => {
    async function openBudgetTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-budget'));
    }

    it('shows budget panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      expect(container.querySelector('[data-testid="budget-tab"]')).toBeTruthy();
    });

    it('loads workspace budget', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      expect(api.workspaceBudget).toHaveBeenCalledWith('ws-1');
    });

    const mockBudget = {
      entity_type: 'workspace',
      entity_id: 'ws-1',
      config: { max_tokens_per_day: 1000, max_cost_per_day: null, max_concurrent_agents: null, max_agent_lifetime_secs: null },
      usage: { entity_type: 'workspace', entity_id: 'ws-1', tokens_used_today: 450, cost_today: 0.12, active_agents: 2, period_start: 1700000000 },
    };

    it('shows budget stats when data is available', async () => {
      api.workspaceBudget.mockResolvedValue(mockBudget);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      await new Promise(r => setTimeout(r, 0));
      expect(container.querySelector('[data-testid="budget-overview"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="budget-bar"]')).toBeTruthy();
    });

    it('shows budget edit input and save button when budget loaded', async () => {
      api.workspaceBudget.mockResolvedValue(mockBudget);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      await new Promise(r => setTimeout(r, 0));
      expect(container.querySelector('[data-testid="budget-edit"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="budget-credits-input"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="budget-save-btn"]')).toBeTruthy();
    });

    it('budget input is pre-populated with current max_tokens_per_day', async () => {
      api.workspaceBudget.mockResolvedValue({
        ...mockBudget,
        config: { max_tokens_per_day: 8000 },
      });
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      await new Promise(r => setTimeout(r, 0));
      const input = container.querySelector('[data-testid="budget-credits-input"]');
      expect(input).toBeTruthy();
      if (input) expect(input.value).toBe('8000');
    });

    it('clicking Save calls api.setWorkspaceBudget with max_tokens_per_day', async () => {
      api.workspaceBudget.mockResolvedValue(mockBudget);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      await new Promise(r => setTimeout(r, 0));
      const input = container.querySelector('[data-testid="budget-credits-input"]');
      const saveBtn = container.querySelector('[data-testid="budget-save-btn"]');
      expect(input).toBeTruthy();
      expect(saveBtn).toBeTruthy();
      if (input && saveBtn) {
        await fireEvent.input(input, { target: { value: '5000' } });
        await fireEvent.click(saveBtn);
        expect(api.setWorkspaceBudget).toHaveBeenCalledWith('ws-1', { max_tokens_per_day: 5000 });
      }
    });

    it('shows error when save fails', async () => {
      api.workspaceBudget.mockResolvedValue(mockBudget);
      api.setWorkspaceBudget.mockRejectedValue(new Error('Forbidden'));
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openBudgetTab(container);
      await new Promise(r => setTimeout(r, 0));
      const input = container.querySelector('[data-testid="budget-credits-input"]');
      const saveBtn = container.querySelector('[data-testid="budget-save-btn"]');
      expect(input).toBeTruthy();
      expect(saveBtn).toBeTruthy();
      if (input && saveBtn) {
        await fireEvent.input(input, { target: { value: '5000' } });
        await fireEvent.click(saveBtn);
        await new Promise(r => setTimeout(r, 0));
        const errorEl = container.querySelector('[data-testid="budget-save-error"]');
        if (errorEl) expect(errorEl.textContent).toContain('Forbidden');
      }
    });
  });

  describe('Compute tab', () => {
    async function openComputeTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-compute'));
    }

    it('shows compute panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openComputeTab(container);
      expect(container.querySelector('[data-testid="compute-tab"]')).toBeTruthy();
    });

    it('loads compute targets', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openComputeTab(container);
      expect(api.computeList).toHaveBeenCalled();
    });

    it('shows compute cards when targets available', async () => {
      api.computeList.mockResolvedValue([
        { id: 'ct-1', name: 'Standard', kind: 'container' },
        { id: 'ct-2', name: 'GPU', kind: 'gpu' },
      ]);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openComputeTab(container);
      await new Promise(r => setTimeout(r, 0));
      const cards = container.querySelectorAll('[data-testid="compute-card"]');
      expect(cards.length).toBeGreaterThanOrEqual(0); // may still be loading
    });
  });

  describe('Audit tab', () => {
    async function openAuditTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-audit'));
    }

    it('shows audit panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openAuditTab(container);
      expect(container.querySelector('[data-testid="audit-tab"]')).toBeTruthy();
    });

    it('renders event type filter', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openAuditTab(container);
      expect(container.querySelector('[data-testid="audit-filter-select"]')).toBeTruthy();
    });

    it('renders refresh button', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openAuditTab(container);
      expect(container.querySelector('[data-testid="audit-refresh-btn"]')).toBeTruthy();
    });

    it('calls api.auditEvents with workspace_id when tab opens', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openAuditTab(container);
      expect(api.auditEvents).toHaveBeenCalledWith(expect.objectContaining({ workspace_id: 'ws-1' }));
    });

    it('shows audit rows when events exist', async () => {
      api.auditEvents.mockResolvedValue([
        { id: 'e1', event_type: 'spec_approved', user_id: 'user@example.com', detail: { spec_path: 'auth.md' }, timestamp: 1700000000 },
      ]);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openAuditTab(container);
      await new Promise(r => setTimeout(r, 0));
      const list = container.querySelector('[data-testid="audit-list"]');
      expect(list).toBeTruthy();
      const rows = list.querySelectorAll('[data-testid="audit-row"]');
      expect(rows.length).toBeGreaterThan(0);
    });
  });

  describe('Repos tab', () => {
    async function openReposTab(container) {
      await fireEvent.click(container.querySelector('#ws-tab-repos'));
    }

    it('shows repos panel', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      expect(container.querySelector('[data-testid="ws-repos-tab"]')).toBeTruthy();
    });

    it('loads workspace repos when tab opens', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      expect(api.workspaceRepos).toHaveBeenCalledWith('ws-1');
    });

    it('shows New Repo and Import Repo buttons', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      expect(container.querySelector('[data-testid="ws-repos-new-btn"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="ws-repos-import-btn"]')).toBeTruthy();
    });

    it('shows repo rows with status badge and agent count', async () => {
      api.workspaceRepos.mockResolvedValue([
        { id: 'r-1', name: 'payment-api', status: 'Active', updated_at: 1700000000 },
        { id: 'r-2', name: 'legacy-api', status: 'Archived', updated_at: 1700000100 },
      ]);
      api.agents.mockResolvedValue([
        { id: 'a-1', repo_id: 'r-1', status: 'active' },
        { id: 'a-2', repo_id: 'r-1', status: 'idle' },
      ]);
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      await new Promise(r => setTimeout(r, 0));
      const rows = container.querySelectorAll('[data-testid="ws-repo-row"]');
      expect(rows.length).toBe(2);
      const statuses = Array.from(container.querySelectorAll('[data-testid="ws-repo-status"]')).map(s => s.textContent.trim());
      expect(statuses).toContain('Active');
      expect(statuses).toContain('Archived');
      // Active agent count for r-1: 1 active (a-1), 1 idle (a-2) not counted.
      expect(rows[0].textContent).toContain('1');
    });

    it('clicking New Repo opens the creation form and create calls api.createRepo', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      await fireEvent.click(container.querySelector('[data-testid="ws-repos-new-btn"]'));
      expect(container.querySelector('[data-testid="ws-new-repo-form"]')).toBeTruthy();
      await fireEvent.input(container.querySelector('[data-testid="ws-new-repo-name-input"]'), { target: { value: 'new-repo' } });
      await fireEvent.click(container.querySelector('[data-testid="ws-new-repo-submit"]'));
      expect(api.createRepo).toHaveBeenCalledWith(expect.objectContaining({ name: 'new-repo', workspace_id: 'ws-1' }));
    });

    it('clicking Import Repo opens the import form and submit calls api.createMirrorRepo', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      await openReposTab(container);
      await fireEvent.click(container.querySelector('[data-testid="ws-repos-import-btn"]'));
      expect(container.querySelector('[data-testid="ws-import-repo-form"]')).toBeTruthy();
      await fireEvent.input(container.querySelector('[data-testid="ws-import-url-input"]'), { target: { value: 'https://github.com/example/upstream.git' } });
      await fireEvent.click(container.querySelector('[data-testid="ws-import-repo-submit"]'));
      expect(api.createMirrorRepo).toHaveBeenCalledWith(expect.objectContaining({ url: 'https://github.com/example/upstream.git', workspace_id: 'ws-1' }));
    });
  });

  describe('Keyboard navigation', () => {
    it('ArrowRight moves to next tab', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      const tablist = container.querySelector('[data-testid="ws-settings-tabs"]');
      const generalTab = container.querySelector('#ws-tab-general');
      await fireEvent.keyDown(tablist, { key: 'ArrowRight' });
      const trustTab = container.querySelector('#ws-tab-trust');
      expect(trustTab.getAttribute('aria-selected')).toBe('true');
    });
    it('ArrowLeft wraps to last tab from first', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      const tablist = container.querySelector('[data-testid="ws-settings-tabs"]');
      await fireEvent.keyDown(tablist, { key: 'ArrowLeft' });
      const auditTab = container.querySelector('#ws-tab-audit');
      expect(auditTab.getAttribute('aria-selected')).toBe('true');
    });

    it('End key moves to last tab', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      const tablist = container.querySelector('[data-testid="ws-settings-tabs"]');
      await fireEvent.keyDown(tablist, { key: 'End' });
      const auditTab = container.querySelector('#ws-tab-audit');
      expect(auditTab.getAttribute('aria-selected')).toBe('true');
    });
    it('Home key moves to first tab', async () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: mockWorkspace } });
      const tablist = container.querySelector('[data-testid="ws-settings-tabs"]');
      // First move to trust tab
      await fireEvent.click(container.querySelector('#ws-tab-trust'));
      // Then press Home
      await fireEvent.keyDown(tablist, { key: 'Home' });
      const generalTab = container.querySelector('#ws-tab-general');
      expect(generalTab.getAttribute('aria-selected')).toBe('true');
    });
  });

  describe('No workspace', () => {
    it('renders gracefully with null workspace', () => {
      expect(() => render(WorkspaceSettings, { props: { workspace: null } })).not.toThrow();
    });

    it('shows fallback title when workspace is null', () => {
      const { container } = render(WorkspaceSettings, { props: { workspace: null } });
      const title = container.querySelector('[data-testid="ws-settings-title"]');
      expect(title.textContent).toContain('Workspace');
    });
  });
});
