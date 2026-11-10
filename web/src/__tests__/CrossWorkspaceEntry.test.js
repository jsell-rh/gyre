/**
 * CrossWorkspaceEntry.test.js — ui-navigation.md §10 (Cross-Workspace View)
 * + §7 (URL structure).
 *
 * Regression (review finding, task-083 repair round): a full page load at
 * /all (bookmark, shared deep link, or refresh while at /all) selects no
 * workspace — onMount's cross_workspace branch leaves currentWorkspace null
 * — and goToEntityDetail used to early-return on `if (!currentWorkspace)`,
 * making every entity-item click in the /all dashboard a silent no-op (URL
 * stayed /all, nothing surfaced). The client-side dropdown path to /all was
 * unaffected, so the defect was specific to the page-load path.
 *
 * Contract under test, with the REAL App and the REAL CrossWorkspaceHome
 * (AppShell.test.js stubs WorkspaceHome/RepoMode but renders /all content
 * for real; those stubs are reused here only for the post-navigation view):
 *
 *   1. Clicking a cross-workspace item at /all after a page load enters the
 *      item's actual owning workspace + repo (§10 "opening an item must
 *      enter its actual owning scope"): URL, persisted workspace
 *      selection, and history state all reflect the owning scope.
 *   2. When the owning workspace cannot be determined from the caller's
 *      membership, the failure surfaces as an error toast — not a silent
 *      no-op and not a fabricated scope identity (task Required
 *      Behavior 4: lookup errors must surface as errors).
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
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    notificationCount: vi.fn().mockResolvedValue(0),
    tokenInfo: vi.fn().mockResolvedValue({ kind: 'global' }),
    me: vi.fn().mockResolvedValue(null),
    version: vi.fn().mockResolvedValue({ version: '0.1.0', commit: 'abc1234', milestone: 'M35' }),
    myNotifications: vi.fn().mockResolvedValue({ notifications: [] }),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    repo: vi.fn().mockResolvedValue(null),
    task: vi.fn().mockResolvedValue(null),
    agent: vi.fn().mockResolvedValue(null),
    agents: vi.fn().mockResolvedValue([]),
    mergeRequest: vi.fn().mockResolvedValue(null),
    mergeRequests: vi.fn().mockResolvedValue([]),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../components/WorkspaceHome.svelte', () => ({ default: function WorkspaceHomeStub() {} }));
vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));

import { api } from '../lib/api.js';
import { getToasts } from '../lib/toast.svelte.js';
import App from '../App.svelte';

// jsdom lacks the Web Animations API — Svelte 5's transition:fly calls
// element.animate when a toast mounts (the error toast asserted below).
// Polyfill the minimum surface so the real <Toast /> renders.
if (typeof Element !== 'undefined' && !Element.prototype.animate) {
  Element.prototype.animate = function () {
    return { finished: Promise.resolve(), cancel: () => {}, finished: Promise.resolve() };
  };
}

const WS_PAYMENTS = { id: 'ws-1', name: 'Payments', slug: 'payments' };
const WS_BILLING = { id: 'ws-2', name: 'Billing', slug: 'billing' };

/** Notification attributed to ws-2/repo-2 carrying a task reference in its
 *  body — the same shape users.rs get_my_notifications returns (`body` is a
 *  JSON string; the decision-item click handler reads body.task_id). */
function notifForBillingRepo() {
  return {
    id: 'n-1',
    notification_type: 'task_completed',
    message: 'Task t-1 completed in billing-svc',
    repo_id: 'repo-2',
    workspace_id: 'ws-2',
    body: JSON.stringify({ task_id: 't-1' }),
  };
}

describe('Cross-workspace item entry after a full page load at /all (ui-navigation.md §10)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/all');
    localStorage.clear();
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    // Membership: the caller can access Payments and Billing, but nothing is
    // selected — a page load at /all leaves currentWorkspace null.
    api.workspaces.mockResolvedValue([WS_PAYMENTS, WS_BILLING]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.myNotifications.mockResolvedValue({ notifications: [notifForBillingRepo()] });
    api.specsForWorkspace.mockResolvedValue([]);
    api.getWorkspaceBriefing.mockResolvedValue({ narrative: '' });
    api.getMetaSpecs.mockResolvedValue([]);
    // The item's repo lives in the Billing workspace (not the first member
    // of the list) — the owner must be resolved from the repo, not assumed.
    api.repo.mockImplementation(async (id) => {
      if (id === 'repo-2') return { id: 'repo-2', name: 'billing-svc', workspace_id: 'ws-2' };
      return null;
    });
  });

  it('decision item click enters the owning workspace and repo (not a silent no-op)', async () => {
    const { container } = render(App);
    // The cross-workspace dashboard rendered from the page-load path at /all
    // with the attributed decision item.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="cwh-decision-item"]')).toBeTruthy();
    }, { timeout: 5000 });
    expect(window.location.pathname).toBe('/all');

    // Click the decision body — must navigate into the item's owning scope.
    await fireEvent.click(container.querySelector('.decision-body-clickable'));

    // URL enters the owning workspace's repo mode at the task's parent tab
    // (§10 + §7 entity detail route), not stay at /all.
    await waitFor(() => {
      expect(window.location.pathname).toBe('/workspaces/billing/r/billing-svc/tasks/t-1');
    }, { timeout: 5000 });

    // The owning workspace became the selected scope and was persisted.
    expect(localStorage.getItem('gyre_workspace_id')).toBe('ws-2');
    // History state records the owning scope (deep link + back nav, §7).
    expect(window.history.state?.wsId).toBe('ws-2');
    expect(window.history.state?.repoName).toBe('billing-svc');
    expect(window.history.state?.repoTab).toBe('tasks');
    expect(window.history.state?.entityId).toBe('t-1');
  });

  it('undeterminable owning workspace surfaces an error, stays at /all, fabricates nothing', async () => {
    // The item's repo resolves to a workspace outside the caller's
    // membership — the owning scope cannot be determined.
    api.repo.mockImplementation(async (id) => {
      if (id === 'repo-2') return { id: 'repo-2', name: 'billing-svc', workspace_id: 'ws-x' };
      return null;
    });

    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="cwh-decision-item"]')).toBeTruthy();
    }, { timeout: 5000 });

    await fireEvent.click(container.querySelector('.decision-body-clickable'));

    // The failure surfaces as an error toast (Required Behavior 4) — both
    // in the toast store and rendered by the real <Toast /> (role=alert)…
    await waitFor(() => {
      expect(
        getToasts().some(t => t.type === 'error' && /workspace not found/i.test(t.message))
      ).toBe(true);
    }, { timeout: 5000 });
    await waitFor(() => {
      const alert = container.querySelector('.toast-container [role="alert"]');
      expect(alert).toBeTruthy();
      expect(alert.textContent).toMatch(/workspace not found/i);
    }, { timeout: 5000 });
    // …the URL stays at /all, and no scope identity was fabricated or
    // persisted (no silent fallback to Payments/ws-1 either).
    await new Promise(r => setTimeout(r, 100));
    expect(window.location.pathname).toBe('/all');
    expect(localStorage.getItem('gyre_workspace_id')).toBeNull();
  });
});
