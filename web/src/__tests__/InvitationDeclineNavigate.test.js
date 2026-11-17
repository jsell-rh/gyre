/**
 * InvitationDeclineNavigate.test.js — user-management.md §UI Pages
 * (Pending Invitation Accept Page, declined state).
 *
 * Regression (review finding, task-127 repair round): after declining a
 * pending workspace invitation, the "Go to all workspaces" button
 * (data-testid="invite-go-all") was a silent no-op in production —
 * InvitationAccept.svelte reads getContext('goToCrossWorkspace'), but
 * App.svelte never provided that context, so the click did nothing and
 * the user was stranded on the declined screen. Component-level tests
 * with an injected context map cannot catch a missing wiring in the real
 * ancestor, so this test renders the REAL App at /invite/{ws} and
 * asserts the navigation contract end to end:
 *
 *   1. Declining the invitation calls the invitee self-decline endpoint
 *      (POST /workspaces/{id}/invite/decline) and shows the
 *      declined state.
 *   2. Clicking "Go to all workspaces" then navigates to /all (the
 *      cross-workspace dashboard), not a silent no-op.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/ws.js', () => ({
  createWsStore: () => ({
    onStatus: vi.fn().mockReturnValue(() => {}),
    destroy: vi.fn(),
    onMessage: vi.fn().mockReturnValue(() => {}),
    subscribe: vi.fn(),
    send: vi.fn(),
    sessionId: 'test-session',
  }),
}));

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn(),
    workspaceRepos: vi.fn(),
    workspaceBudget: vi.fn(),
    notificationCount: vi.fn(),
    tokenInfo: vi.fn(),
    me: vi.fn(),
    version: vi.fn(),
    myNotifications: vi.fn(),
    specsForWorkspace: vi.fn(),
    getMetaSpecs: vi.fn(),
    getWorkspaceBriefing: vi.fn(),
    repo: vi.fn(),
    task: vi.fn(),
    agent: vi.fn(),
    agents: vi.fn(),
    mergeRequest: vi.fn(),
    mergeRequests: vi.fn(),
    // InvitationAccept surface (task-110 WorkspaceInvitation model)
    workspace: vi.fn(),
    myInvitations: vi.fn(),
    acceptWorkspaceInvitation: vi.fn(),
    declineWorkspaceInvitation: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../components/WorkspaceHome.svelte', () => ({ default: function WorkspaceHomeStub() {} }));
vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));

import { api } from '../lib/api.js';
import App from '../App.svelte';

// jsdom lacks the Web Animations API — Svelte 5's transition:fly calls
// element.animate when a toast mounts (the decline info toast below).
if (typeof Element !== 'undefined' && !Element.prototype.animate) {
  Element.prototype.animate = function () {
    return { finished: Promise.resolve(), cancel: () => {}, finished: Promise.resolve() };
  };
}

const WS = { id: 'ws-9', name: 'Payments', slug: 'payments', description: 'Payment processing' };
const ME = { id: 'u-1', username: 'jsell' };
const MY_INVITATIONS = {
  invitations: [
    {
      id: 'inv-1', workspace_id: 'ws-9', user_id: 'u-1', invited_by: 'u-owner',
      role: 'Developer', status: 'Pending',
      expires_at: Math.floor(Date.now() / 1000) + 6 * 86400,
      created_at: 1700000000, accepted_at: null,
    },
  ],
};

describe('Declined invitation "Go to all workspaces" navigation (user-management.md §UI Pages)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/invite/ws-9');
    localStorage.clear();
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    api.workspaces.mockResolvedValue([WS]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.tokenInfo.mockResolvedValue({ kind: 'global' });
    api.me.mockResolvedValue(ME);
    api.version.mockResolvedValue({ version: '0.1.0', commit: 'abc1234', milestone: 'M35' });
    api.myNotifications.mockResolvedValue({ notifications: [] });
    api.specsForWorkspace.mockResolvedValue([]);
    api.getMetaSpecs.mockResolvedValue([]);
    api.getWorkspaceBriefing.mockResolvedValue({ narrative: '' });
    api.repo.mockResolvedValue(null);
    api.workspace.mockResolvedValue({ ...WS });
    api.myInvitations.mockResolvedValue(JSON.parse(JSON.stringify(MY_INVITATIONS)));
    api.acceptWorkspaceInvitation.mockResolvedValue(null);
    api.declineWorkspaceInvitation.mockResolvedValue(null);
  });

  it('declining then clicking "Go to all workspaces" navigates to /all, not a silent no-op', async () => {
    const { container } = render(App);

    // The invitation card renders from the page-load path at /invite/ws-9.
    const declineBtn = await waitFor(() => {
      const el = container.querySelector('[data-testid="invite-decline-btn"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 5000 });
    expect(window.location.pathname).toBe('/invite/ws-9');

    // Decline — the invitee self-decline endpoint is called and the
    // declined state (with the go-to-all button) appears.
    await fireEvent.click(declineBtn);
    await waitFor(() => {
      expect(api.declineWorkspaceInvitation).toHaveBeenCalledWith('ws-9');
    }, { timeout: 5000 });
    const goAll = await waitFor(() => {
      const el = container.querySelector('[data-testid="invite-go-all"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 5000 });

    // Click "Go to all workspaces" — must navigate to the cross-workspace
    // dashboard (/all). This was the silent no-op: the component read a
    // context App.svelte never provided.
    await fireEvent.click(goAll);
    await waitFor(() => {
      expect(window.location.pathname).toBe('/all');
    }, { timeout: 5000 });
    expect(window.history.state?.mode).toBe('cross_workspace');
  });
});
