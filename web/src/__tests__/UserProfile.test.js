import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// Mock api before importing component
vi.mock('../lib/api.js', () => ({
  api: {
    me: vi.fn(),
    myNotifications: vi.fn(),
    myJudgments: vi.fn(),
    workspaces: vi.fn(),
    updateMe: vi.fn(),
    markNotificationRead: vi.fn(),
    notificationCount: vi.fn(),
    getNotificationPreferences: vi.fn(),
    updateNotificationPreferences: vi.fn(),
    userByUsername: vi.fn(),
    myInvitations: vi.fn(),
    acceptWorkspaceInvitation: vi.fn(),
    declineWorkspaceInvitation: vi.fn(),
    listMySessions: vi.fn(),
    revokeMySession: vi.fn(),
    revokeAllMySessions: vi.fn(),
  },
}));

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
}));

import { api } from '../lib/api.js';
import { NOTIF_TYPE_IDS } from '../lib/notification-types.js';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import UserProfile from '../components/UserProfile.svelte';

const __dirname = dirname(fileURLToPath(import.meta.url));

// Canonical NotificationType variant names, extracted from the Rust enum
// itself (the single source of truth). Deriving the expected set here --
// instead of hardcoding a count that mirrors the implementation's own list
// -- makes this test fail when the enum gains a variant with no toggle
// (review finding: 'WorkspaceOwnerless' shipped with no toggle while a
// hardcoded toBe(40) still passed).
const RUST_ENUM_PATH = '../../../crates/gyre-common/src/notification.rs';
const rustNotificationTypeVariants = () => {
  const src = readFileSync(resolve(__dirname, RUST_ENUM_PATH), 'utf8');
  const body = src.match(/^pub enum NotificationType \{[\s\S]*?^\}/m)[0];
  return body
    .match(/^    [A-Z][A-Za-z0-9]+,?$/gm)
    .map((line) => line.trim().replace(/,$/, ''));
};

const ME = {
  username: 'jsell',
  display_name: 'James Sell',
  email: 'jsell@example.com',
  global_role: 'admin',
  timezone: 'America/New_York',
  locale: 'en-US',
  oidc_issuer: 'https://idp.example.com/realms/gyre',
  last_login_at: 1759000000,
};

const WORKSPACES = [
  { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'autonomous', role: 'admin' },
  { id: 'ws-2', name: 'Platform', slug: 'platform', trust_level: null, role: 'member' },
];

// Real GET /users/me/notifications wire shape: NotificationResponse items.
// Read state is resolved_at/dismissed_at (both null = unread), NOT a `read` bool.
const NOW = Math.floor(Date.now() / 1000);
const NOTIFICATIONS = [
  { id: 'n-1', notification_type: 'SpecApprovalRequested', title: 'Spec approved', resolved_at: null, dismissed_at: null, created_at: (NOW - 300) },
  { id: 'n-2', notification_type: 'GateFailure', title: 'Gate crashed', resolved_at: null, dismissed_at: NOW - 100, created_at: (NOW - 7200) },
];

// Real GET /users/me/judgments wire shape: JudgmentEntryResponse items.
const JUDGMENTS = [
  { judgment_type: 'approval', entity_ref: 'specs/auth.md', workspace_id: 'ws-1', timestamp: NOW - 60, detail: null },
  { judgment_type: 'gate', entity_ref: 'mr-9', workspace_id: 'ws-1', timestamp: NOW - 86400, detail: 'gate test_command overridden: failed -> overridden' },
];

const CTX = new Map([['navigate', vi.fn()], ['goToWorkspaceHome', vi.fn()], ['openDetailPanel', vi.fn()]]);
const r = (props = {}) => render(UserProfile, { props, context: CTX });

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  api.me.mockResolvedValue({ ...ME });
  api.myNotifications.mockResolvedValue({ notifications: [...NOTIFICATIONS] });
  api.myJudgments.mockResolvedValue({ judgments: [...JUDGMENTS] });
  api.workspaces.mockResolvedValue([...WORKSPACES]);
  api.updateMe.mockResolvedValue({ ...ME, display_name: 'Updated Name' });
  api.markNotificationRead.mockResolvedValue({});
  api.notificationCount.mockResolvedValue(1);
  api.getNotificationPreferences.mockResolvedValue({ preferences: [] });
  api.updateNotificationPreferences.mockResolvedValue({});
  api.userByUsername.mockResolvedValue({ id: 'u-1', username: 'jsell', memberships: [] });
  // Real GET /users/me/invitations wire shape (task-110): WorkspaceInvitation
  // rows with status/expires_at, not unaccepted membership rows.
  api.myInvitations.mockResolvedValue({ invitations: [] });
  api.acceptWorkspaceInvitation.mockResolvedValue(null); // 204
  api.declineWorkspaceInvitation.mockResolvedValue(null);
  api.listMySessions.mockResolvedValue({ sessions: [] });
  api.revokeMySession.mockResolvedValue(null);
  api.revokeAllMySessions.mockResolvedValue(null);
});

describe('UserProfile', () => {
  it('renders without throwing', () => {
    expect(() => r()).not.toThrow();
  });

  it('shows display name and username after loading', async () => {
    const { findAllByText, findByText } = r();
    const nameEls = await findAllByText('James Sell');
    expect(nameEls.length).toBeGreaterThan(0);
    expect(await findByText('@jsell')).toBeTruthy();
  });

  it('shows avatar with first letter of display name', async () => {
    const { container, findAllByText } = r();
    await findAllByText('James Sell');
    const avatar = container.querySelector('.avatar');
    expect(avatar.textContent.trim()).toBe('J');
  });

  it('shows global role badge', async () => {
    const { findAllByText } = r();
    await findAllByText('James Sell');
    const adminEls = await findAllByText('admin');
    expect(adminEls.length).toBeGreaterThan(0);
  });

  it('exposes exactly the eight permitted tabs — HSI §12 forbids My Agents/Tasks/MRs', async () => {
    const { container, findAllByText } = r();
    await findAllByText('James Sell');
    const ids = [...container.querySelectorAll('[role="tab"]')].map((el) => el.dataset.id);
    expect(ids).toEqual(['info', 'tokens', 'sessions', 'memberships', 'invitations', 'ledger', 'notif-prefs', 'notifications']);
    // HSI §12 "What the Profile Is NOT": tasks are agent work units, MRs are
    // judged not authored, agents are machinery — none is a per-user surface.
    // Sessions IS a per-user surface (user-management.md §UI Pages >
    // Session Management) and is served from User Settings per that spec.
    for (const forbidden of ['my-agents', 'my-tasks', 'my-mrs']) {
      expect(ids).not.toContain(forbidden);
    }
  });

  it('shows profile info fields in Profile tab', async () => {
    const { findByText } = r();
    expect(await findByText('Username')).toBeTruthy();
    expect(await findByText('jsell')).toBeTruthy();
    expect(await findByText('jsell@example.com')).toBeTruthy();
    expect(await findByText('America/New_York')).toBeTruthy();
    expect(await findByText('en-US')).toBeTruthy();
  });

  it('shows read-only OIDC issuer in Profile tab', async () => {
    const { findByText } = r();
    expect(await findByText('https://idp.example.com/realms/gyre')).toBeTruthy();
  });

  it('shows read-only last login in Profile tab', async () => {
    const { findByText } = r();
    // 1759000000 * 1000 formatted by toLocaleString -- assert the label and
    // that some rendered value carries the year of the recorded login.
    expect(await findByText('Last Login')).toBeTruthy();
    const d = new Date(1759000000 * 1000);
    expect(await findByText(d.toLocaleString())).toBeTruthy();
  });

  it('shows Edit button that opens edit form', async () => {
    const { findByText, findByDisplayValue } = r();
    const editBtn = await findByText('Edit');
    await fireEvent.click(editBtn);
    expect(await findByDisplayValue('James Sell')).toBeTruthy();
    expect(await findByText('Cancel')).toBeTruthy();
    expect(await findByText('Save')).toBeTruthy();
  });

  it('Cancel closes the edit form', async () => {
    const { findByText, queryByText } = r();
    await fireEvent.click(await findByText('Edit'));
    await fireEvent.click(await findByText('Cancel'));
    await waitFor(() => {
      expect(queryByText('Save')).toBeNull();
    });
  });

  it('badge count comes from the server-side count endpoint, not the list page', async () => {
    const { findByText } = r();
    // notificationCount resolves 7 while the list mock holds only 2 items --
    // a client-side `list.filter(unread)` badge would show a different number.
    api.notificationCount.mockResolvedValue(7);
    expect(await findByText('7')).toBeTruthy();
    expect(api.notificationCount).toHaveBeenCalledTimes(1);
  });

  it('shows workspace memberships in Workspaces tab', async () => {
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('Payments')).toBeTruthy();
    expect(await findByText('Platform')).toBeTruthy();
  });

  it('shows Switch button for each workspace', async () => {
    const { findByText, getAllByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    await waitFor(() => {
      const switches = getAllByText('Switch');
      expect(switches.length).toBe(2);
    });
  });

  it('shows trust level when present on workspace', async () => {
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('Trust: autonomous')).toBeTruthy();
  });

  it('shows judgment entries with real wire fields in Judgment Ledger tab', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    // judgment_type / entity_ref are the JudgmentEntryResponse field names.
    expect(await findByText('approval')).toBeTruthy();
    expect(await findByText('gate')).toBeTruthy();
    expect(await findByText('specs/auth.md')).toBeTruthy();
    expect(await findByText('mr-9')).toBeTruthy();
  });

  it('shows judgment detail line when present', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText('gate test_command overridden: failed -> overridden')).toBeTruthy();
  });

  it('shows empty state when no judgments', async () => {
    api.myJudgments.mockResolvedValue({ judgments: [] });
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText(/No activity recorded/)).toBeTruthy();
  });

  it('ledger type filter re-fetches with the server ?type= param', async () => {
    const { findByText, container } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    await findByText('specs/auth.md');
    const typeSelect = container.querySelector('.ledger-select[aria-label="Filter judgments by type"]');
    expect(typeSelect).toBeTruthy();
    await fireEvent.change(typeSelect, { target: { value: 'gate' } });
    await waitFor(() => {
      const last = api.myJudgments.mock.calls[api.myJudgments.mock.calls.length - 1][0];
      expect(last.type).toBe('gate');
      expect(last.offset).toBe(0);
    });
  });

  it('ledger workspace filter re-fetches with the server ?workspace_id= param', async () => {
    const { findByText, container } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    await findByText('specs/auth.md');
    const wsSelect = container.querySelector('.ledger-select[aria-label="Filter judgments by workspace"]');
    expect(wsSelect).toBeTruthy();
    await fireEvent.change(wsSelect, { target: { value: 'ws-1' } });
    await waitFor(() => {
      const last = api.myJudgments.mock.calls[api.myJudgments.mock.calls.length - 1][0];
      expect(last.workspace_id).toBe('ws-1');
      expect(last.offset).toBe(0);
    });
  });

  it('ledger paginates past the first page via Load More and offset', async () => {
    // First page: a full 50-item page (page size) so "Load more" appears;
    // second page: a single item, which ends pagination.
    const page1 = Array.from({ length: 50 }, (_, i) => ({
      judgment_type: 'approval',
      entity_ref: `specs/p1-${i}.md`,
      workspace_id: 'ws-1',
      timestamp: NOW - 60 - i,
      detail: null,
    }));
    const page2 = [
      { judgment_type: 'trust', entity_ref: 'ws-1', workspace_id: 'ws-1', timestamp: NOW - 5000, detail: null },
    ];
    api.myJudgments.mockResolvedValueOnce({ judgments: page1 }).mockResolvedValueOnce({ judgments: page2 });
    const { findByText, findAllByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    await findAllByText('specs/p1-0.md');
    const moreBtn = await findByText('Load more');
    await fireEvent.click(moreBtn);
    await waitFor(() => {
      expect(api.myJudgments).toHaveBeenCalledTimes(2);
    });
    expect(api.myJudgments.mock.calls[1][0].offset).toBe(50);
    expect(api.myJudgments.mock.calls[1][0].limit).toBe(50);
    // Appended, not replaced: both pages' entries render.
    expect(await findAllByText('specs/p1-0.md')).toBeTruthy();
    expect(await findByText('ws-1', { selector: '.ledger-target' })).toBeTruthy();
  });

  it('shows a toggle for every canonical NotificationType', async () => {
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    // Spot-check canonical NotificationType::as_str() names, including
    // types the curated subset previously omitted (review F3: the spec
    // requires a toggle for ANY NotificationType variant).
    expect(await findByText('Gate Failures')).toBeTruthy();
    expect(await findByText('Specs Pending Approval')).toBeTruthy();
    expect(await findByText('Merge Queue Escalations')).toBeTruthy();
    expect(await findByText('Agent Failures')).toBeTruthy();
    expect(await findByText('Security Findings')).toBeTruthy();
    expect(await findByText('Merge Queue Paused')).toBeTruthy();
    expect(await findByText('Persona Changes Pending Approval')).toBeTruthy();
    // Review F1: 'ComputeTargetUnavailable' (Priority-1 inbox type) had no
    // toggle -- a human could not disable it. It must render like the rest.
    expect(await findByText('Compute Target Unavailable')).toBeTruthy();
    // Review round 2: 'WorkspaceOwnerless' (Priority-2, task-121 lineage) had
    // no toggle either. It must render like the rest.
    expect(await findByText('Ownerless Workspaces')).toBeTruthy();
    const checkboxes = container.querySelectorAll('.pref-checkbox');
    // One toggle per canonical NotificationType variant. The expected set is
    // extracted from the gyre-common enum source -- NOT a count mirrored from
    // the implementation's own list, which silently forgives a missing toggle.
    const variants = rustNotificationTypeVariants();
    expect(NOTIF_TYPE_IDS).toEqual(variants);
    expect(checkboxes.length).toBe(variants.length);
  });

  it('loads server-side prefs rows into the toggles', async () => {
    api.getNotificationPreferences.mockResolvedValue({
      preferences: [
        { notification_type: 'GateFailure', enabled: false },
        { notification_type: 'AgentBudgetWarning', enabled: true },
      ],
    });
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    await findByText('Gate Failures');
    const boxes = container.querySelectorAll('.pref-checkbox');
    const gate = [...boxes].find(b => b.getAttribute('aria-label')?.includes('Gate Failures'));
    expect(gate?.checked).toBe(false);
  });

  it('ignores legacy alias-named prefs rows instead of toggling canonical switches', async () => {
    // Rows written by older clients may carry parse aliases
    // ('SpecPendingApproval'/'BudgetWarning'); the canonical toggles must
    // stay at their default (enabled) rather than keying on a name that
    // never matches a stored notification.
    api.getNotificationPreferences.mockResolvedValue({
      preferences: [
        { notification_type: 'SpecPendingApproval', enabled: false },
        { notification_type: 'BudgetWarning', enabled: false },
      ],
    });
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    await findByText('Specs Pending Approval');
    const boxes = container.querySelectorAll('.pref-checkbox');
    const specApproval = [...boxes].find(b => b.getAttribute('aria-label')?.includes('Specs Pending Approval'));
    expect(specApproval?.checked).toBe(true);
  });

  it('saves prefs with the server wire format and canonical names', async () => {
    const { findByText } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    const saveBtn = await findByText('Save Preferences');
    await fireEvent.click(saveBtn);
    expect(api.updateNotificationPreferences).toHaveBeenCalledTimes(1);
    const arg = api.updateNotificationPreferences.mock.calls[0][0];
    // Real PUT /users/me/notification-preferences contract (task-112/125):
    // per-type items with canonical NotificationType names + delivery
    // channels — not a flat type→bool map and not fabricated names.
    expect(Array.isArray(arg.preferences)).toBe(true);
    expect(arg.preferences.length).toBe(NOTIF_TYPE_IDS.length);
    const types = arg.preferences.map(p => p.notification_type);
    expect(types).toContain('GateFailure');
    expect(types).toContain('MergeQueueEscalation');
    expect(types).toContain('ComputeTargetUnavailable');
    // Previously-omitted canonical variants are now toggleable and saved.
    expect(types).toContain('AgentFailed');
    expect(types).toContain('SecurityFinding');
    expect(types).toContain('BreakingChangeDetected');
    expect(types).toContain('PersonaChangePendingApproval');
    // Fabricated names must be gone.
    expect(types).not.toContain('SpecApproval');
    expect(types).not.toContain('MergeRequestMerged');
    // Legacy parse aliases are not canonical as_str() names -- a toggle
    // keyed on an alias never matches a stored notification.
    expect(types).not.toContain('SpecPendingApproval');
    expect(types).not.toContain('BudgetWarning');
    for (const p of arg.preferences) {
      expect(typeof p.enabled).toBe('boolean');
    }
    // Delivery channels ride the same PUT (user-management.md
    // §Delivery Channels): in_app always on, email defaults off.
    expect(arg.channels.in_app).toBe(true);
    expect(arg.channels.email).toEqual({ enabled: false, digest: 'Off', min_priority: 'Medium' });
  });

  it('lists pending invitations and accepts one via the API', async () => {
    api.myInvitations.mockResolvedValue({
      invitations: [
        { id: 'inv-1', workspace_id: 'ws-9', user_id: 'u-1', invited_by: 'u-2', role: 'Developer', status: 'Pending', expires_at: NOW + 6 * 86400, created_at: 1700000000, accepted_at: null },
      ],
    });
    const { container, findByText } = r();
    const invitationsTab = await findByText('Invitations');
    await fireEvent.click(invitationsTab);
    const acceptBtn = await waitFor(() => container.querySelector('[data-testid="invitation-accept-inv-1"]'));
    expect(container.querySelector('[data-testid="invitation-decline-inv-1"]')).toBeTruthy();
    await fireEvent.click(acceptBtn);
    await waitFor(() => {
      expect(api.acceptWorkspaceInvitation).toHaveBeenCalledWith('ws-9');
    });
  });

  it('declines a pending invitation via the invitee decline endpoint', async () => {
    api.myInvitations.mockResolvedValue({
      invitations: [
        { id: 'inv-2', workspace_id: 'ws-9', user_id: 'u-1', invited_by: 'u-2', role: 'Developer', status: 'Pending', expires_at: NOW + 6 * 86400, created_at: 1700000000, accepted_at: null },
      ],
    });
    const { container, findByText } = r();
    const invitationsTab = await findByText('Invitations');
    await fireEvent.click(invitationsTab);
    const declineBtn = await waitFor(() => container.querySelector('[data-testid="invitation-decline-inv-2"]'));
    await fireEvent.click(declineBtn);
    await waitFor(() => {
      expect(api.declineWorkspaceInvitation).toHaveBeenCalledWith('ws-9');
    });
  });

  it('filters out non-pending invitation rows (terminal statuses are not actionable)', async () => {
    api.myInvitations.mockResolvedValue({
      invitations: [
        { id: 'inv-3', workspace_id: 'ws-9', user_id: 'u-1', invited_by: 'u-2', role: 'Developer', status: 'Expired', expires_at: NOW - 86400, created_at: 1700000000, accepted_at: null },
      ],
    });
    const { container, findByText } = r();
    const invitationsTab = await findByText('Invitations');
    await fireEvent.click(invitationsTab);
    // The expired row is filtered — the none state renders instead of an
    // actionable invitation.
    await findByText('No pending invitations');
    expect(container.querySelector('[data-testid="invitation-accept-inv-3"]')).toBeNull();
    expect(container.querySelector('[data-testid="invitation-decline-inv-3"]')).toBeNull();
  });

  // ── Sessions tab (user-management.md §UI Pages > Session Management,
  // endpoints from task-111) ───────────────────────────────────────────────
  const SESSIONS = {
    sessions: [
      // task-111 contract: epoch-second timestamps, UA empty for
      // WebSocket-minted sessions, no token hash on the wire.
      { id: 'sess-1', user_agent: 'Mozilla/5.0 (X11; Linux x86_64) Firefox/121.0', ip_address: '10.0.0.4', created_at: 1731500000, last_active_at: 1731590000, expires_at: 1734178000 },
      { id: 'sess-2', user_agent: '', ip_address: '192.168.1.9', created_at: 1731400000, last_active_at: 1731580000, expires_at: 1733998000 },
    ],
  };

  it('lists active sessions from GET /users/me/sessions in the Sessions tab', async () => {
    api.listMySessions.mockResolvedValue(SESSIONS);
    const { container, findByText } = r();
    const sessionsTab = await findByText('Sessions');
    await fireEvent.click(sessionsTab);
    expect(await waitFor(() => container.querySelector('[data-testid="session-sess-1"]'))).toBeTruthy();
    expect(container.querySelector('[data-testid="session-sess-2"]')).toBeTruthy();
    // UA-less session renders the WebSocket device label, not an empty row.
    expect(await findByText('WebSocket / terminal session')).toBeTruthy();
    expect(api.listMySessions).toHaveBeenCalledTimes(1);
  });

  it('revokes one session via DELETE /users/me/sessions/:id and removes the row', async () => {
    api.listMySessions.mockResolvedValue(SESSIONS);
    const { container, findByText } = r();
    const sessionsTab = await findByText('Sessions');
    await fireEvent.click(sessionsTab);
    const revokeBtn = await waitFor(() => container.querySelector('[data-testid="session-revoke-sess-1"]'));
    await fireEvent.click(revokeBtn);
    await waitFor(() => {
      expect(api.revokeMySession).toHaveBeenCalledWith('sess-1');
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="session-sess-1"]')).toBeNull();
    });
    // The other session stays listed.
    expect(container.querySelector('[data-testid="session-sess-2"]')).toBeTruthy();
  });

  it('sign out everywhere calls POST /users/me/sessions/revoke-all and empties the list', async () => {
    api.listMySessions.mockResolvedValue(SESSIONS);
    const { container, findByText } = r();
    const sessionsTab = await findByText('Sessions');
    await fireEvent.click(sessionsTab);
    const revokeAllBtn = await waitFor(() => container.querySelector('[data-testid="sessions-revoke-all"]'));
    await fireEvent.click(revokeAllBtn);
    await waitFor(() => {
      expect(api.revokeAllMySessions).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="session-sess-1"]')).toBeNull();
      expect(container.querySelector('[data-testid="session-sess-2"]')).toBeNull();
    });
    expect(await findByText('No active sessions.')).toBeTruthy();
  });

  it('edit form saves user preferences (theme/density/diff view) via PUT /users/me', async () => {
    api.me.mockResolvedValue({ ...ME, preferences: { theme: 'Dark', ui_density: 'Comfortable', diff_view: 'Unified' } });
    api.updateMe.mockResolvedValue({ ...ME });
    const { findAllByText, findByText, findByDisplayValue, container } = r();
    await findAllByText('James Sell');
    await fireEvent.click(await findByText('Edit'));
    expect(await findByDisplayValue('Dark')).toBeTruthy();
    await fireEvent.change(container.querySelector('[data-testid="prefs-theme"]'), { target: { value: 'System' } });
    await fireEvent.click(container.querySelector('[data-testid="prefs-save-btn"]'));
    await waitFor(() => {
      expect(api.updateMe).toHaveBeenCalledWith(expect.objectContaining({
        preferences: expect.objectContaining({ theme: 'System', diff_view: 'Unified' }),
      }));
    });
  });

  it('edit form saves user profile fields via PUT /users/me exactly once per Save click', async () => {
    api.updateMe.mockResolvedValue({ ...ME });
    const { findAllByText, findByText, findByDisplayValue, container } = r();
    await findAllByText('James Sell');
    await fireEvent.click(await findByText('Edit'));
    const nameInput = await findByDisplayValue('James Sell');
    await fireEvent.input(nameInput, { target: { value: 'James T. Sell' } });
    await fireEvent.click(container.querySelector('.edit-actions .btn-primary'));
    await waitFor(() => {
      expect(api.updateMe).toHaveBeenCalledWith(expect.objectContaining({
        display_name: 'James T. Sell',
      }));
    });
    // The Save button must be type="button": a default-submit button inside
    // the edit form fires saveEdit() via onclick AND via the form's onsubmit,
    // issuing a duplicate PUT /users/me.
    await new Promise((res) => setTimeout(res, 20));
    expect(api.updateMe).toHaveBeenCalledTimes(1);
  });

  it('shows notifications in Notifications tab', async () => {
    const { findByText } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    expect(await findByText('Spec approved')).toBeTruthy();
    expect(await findByText('Gate crashed')).toBeTruthy();
  });

  it('treats resolved/dismissed notifications as read', async () => {
    const { findByText, container } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    await findByText('Spec approved');
    const markBtns = container.querySelectorAll('.mark-read-btn');
    expect(markBtns.length).toBe(1); // n-2 has dismissed_at set
  });

  it('marks notification as read when button is clicked', async () => {
    const { findByText, container } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    await findByText('Spec approved');
    const markBtn = container.querySelector('.mark-read-btn');
    await fireEvent.click(markBtn);
    expect(api.markNotificationRead).toHaveBeenCalledWith('n-1');
  });

  it('shows empty state when no notifications', async () => {
    api.myNotifications.mockResolvedValue({ notifications: [] });
    const { findByText } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    expect(await findByText('No notifications')).toBeTruthy();
  });

  it('shows empty state when no workspaces', async () => {
    api.workspaces.mockResolvedValue([]);
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('No workspaces')).toBeTruthy();
  });

  it('calls all four API endpoints on mount', async () => {
    r();
    await waitFor(() => {
      expect(api.me).toHaveBeenCalledTimes(1);
      expect(api.myNotifications).toHaveBeenCalledTimes(1);
      expect(api.myJudgments).toHaveBeenCalledTimes(1);
      expect(api.workspaces).toHaveBeenCalledTimes(1);
    });
  });
});
