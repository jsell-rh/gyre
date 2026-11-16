/**
 * StaleScopeResponse.test.js — ui-navigation.md §4 scope transitions:
 * "After switching scopes, a delayed response from the old scope must not
 * overwrite the new scope's content."
 *
 * Regression for the generation guards in the three scoped loaders that
 * render user-visible content (WorkspaceHome, Inbox, Briefing). Each test
 * renders the REAL component, switches scope the same way App.svelte does
 * (a workspace prop change driving the component's reload effect), lets the
 * NEW scope's response render, then resolves the OLD scope's still-pending
 * promise and asserts the new scope's rendered content is preserved and the
 * old scope's never appears.
 *
 * Mutation probe (verified, see /tmp/stage/review-evidence): neutralizing
 * the guards — `stale()` forced to `false` in WorkspaceHome.svelte, the
 * `if (gen !== loadGen) return;` checks removed in Inbox.svelte and
 * Briefing.svelte — fails every test below; with the guards intact all
 * pass. Without a guard, the late old-scope response replaces the new
 * scope's state: Inbox/Briefing render the old workspace's rows, and
 * WorkspaceHome's Decisions section is wiped (the stale snapshot no longer
 * matches the new workspace filter), destroying the new scope's content.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => {
  const methods = {
    // Controlled per-test: the scoped loaders under test.
    myNotifications: vi.fn(),
    getWorkspaceBriefing: vi.fn(),
    // Workspace home panels unrelated to the stale-response rule.
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  };
  // Any other panel loader gets an empty successful fixture.
  return { api: new Proxy(methods, {
    get(target, name) {
      if (!(name in target)) target[name] = vi.fn().mockResolvedValue([]);
      return target[name];
    },
  }) };
});

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';
import Inbox from '../components/Inbox.svelte';
import Briefing from '../components/Briefing.svelte';

const WS_A = { id: 'ws-a', name: 'Workspace A', slug: 'a', trust_level: 'Guided' };
const WS_B = { id: 'ws-b', name: 'Workspace B', slug: 'b', trust_level: 'Guided' };

/** A notification the Decisions/Inbox loaders treat as actionable. */
const decisionNotif = (id, wsId, title) => ({
  id,
  notification_type: 'spec_approval',
  priority: 1,
  title,
  body: JSON.stringify({ spec_path: `specs/system/${wsId}.md`, diff_summary: '+10 lines' }),
  entity_ref: `specs/system/${wsId}.md`,
  workspace_id: wsId,
  repo_id: null,
  resolved_at: null,
  dismissed_at: null,
  created_at: new Date().toISOString(),
});

/** Let every pending promise continuation (and the Svelte update it
 *  triggers) settle before asserting final rendered ownership. */
const flush = () => new Promise(resolve => setTimeout(resolve, 0));

beforeEach(() => {
  vi.clearAllMocks();
  api.getMetaSpecs.mockResolvedValue([]);
  api.workspaces.mockResolvedValue([]);
});

describe('ui-navigation.md §4 — delayed old-scope responses cannot replace current-scope content', () => {
  it('WorkspaceHome: late old-workspace notifications do not wipe the new workspace\'s decisions', async () => {
    const notifA = decisionNotif('notif-a', 'ws-a', 'Workspace A spec needs approval');
    const notifB = decisionNotif('notif-b', 'ws-b', 'Workspace B spec needs approval');
    let resolveOld;
    const oldResponse = new Promise(resolve => { resolveOld = resolve; });
    // loadDecisions() calls myNotifications() with NO arguments (it filters
    // client-side by workspace_id); loadActivity()'s synthesis path calls it
    // with {workspace_id, limit}. Only the no-arg Decisions call is deferred.
    let decisionsCalls = 0;
    api.myNotifications.mockImplementation(args => {
      if (args) return Promise.resolve({ notifications: [] });
      decisionsCalls += 1;
      return decisionsCalls === 1 ? oldResponse : Promise.resolve([notifB]);
    });

    const { getByText, queryByText, rerender } = render(WorkspaceHome, { workspace: WS_A });
    // Real API argument of the Decisions loader: unscoped myNotifications().
    await waitFor(() => expect(api.myNotifications).toHaveBeenCalledWith());

    await rerender({ workspace: WS_B });
    // New scope's content is rendered from its own scoped response.
    await waitFor(() => expect(getByText('Workspace B spec needs approval')).toBeTruthy());

    // The old workspace's response finally settles — after the switch.
    resolveOld([notifA]);
    await flush();
    await flush();

    expect(getByText('Workspace B spec needs approval')).toBeTruthy();
    expect(queryByText('Workspace A spec needs approval')).toBeNull();
  });

  it('Inbox: late old-workspace notifications do not replace the new workspace\'s inbox rows', async () => {
    const notifA = decisionNotif('notif-a', 'ws-a', 'Workspace A spec needs approval');
    const notifB = decisionNotif('notif-b', 'ws-b', 'Workspace B spec needs approval');
    let resolveOld;
    const oldResponse = new Promise(resolve => { resolveOld = resolve; });
    api.myNotifications.mockImplementation(params =>
      params?.workspace_id === 'ws-a' ? oldResponse : Promise.resolve([notifB])
    );

    const { getByText, queryByText, rerender } = render(Inbox, { scope: 'workspace', workspaceId: 'ws-a' });
    // Real API arguments: the workspace filter is server-side (?workspace_id=).
    await waitFor(() => expect(api.myNotifications).toHaveBeenCalledWith({ workspace_id: 'ws-a' }));

    await rerender({ workspaceId: 'ws-b' });
    await waitFor(() => expect(getByText('Workspace B spec needs approval')).toBeTruthy());
    expect(api.myNotifications).toHaveBeenCalledWith({ workspace_id: 'ws-b' });

    resolveOld([notifA]);
    await flush();
    await flush();

    expect(getByText('Workspace B spec needs approval')).toBeTruthy();
    expect(queryByText('Workspace A spec needs approval')).toBeNull();
  });

  it('Briefing: late old-workspace briefing does not replace the new workspace\'s briefing', async () => {
    const briefingA = { completed: [{ id: 'c-a', title: 'Workspace A completed work', spec_ref: 'a.md' }], in_progress: [], cross_workspace: [], exceptions: [], metrics: null };
    const briefingB = { completed: [{ id: 'c-b', title: 'Workspace B completed work', spec_ref: 'b.md' }], in_progress: [], cross_workspace: [], exceptions: [], metrics: null };
    let resolveOld;
    const oldResponse = new Promise(resolve => { resolveOld = resolve; });
    api.getWorkspaceBriefing.mockImplementation(id =>
      id === 'ws-a' ? oldResponse : Promise.resolve(briefingB)
    );

    const { getByText, queryByText, rerender } = render(Briefing, { scope: 'workspace', workspaceId: 'ws-a' });
    // Real API arguments: (workspaceId, since) — since is null for the
    // default 'last_visit' range (server uses stored last_seen_at).
    await waitFor(() => expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-a', null));

    await rerender({ workspaceId: 'ws-b' });
    await waitFor(() => expect(getByText('Workspace B completed work')).toBeTruthy());
    expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-b', null);

    resolveOld(briefingA);
    await flush();
    await flush();

    expect(getByText('Workspace B completed work')).toBeTruthy();
    expect(queryByText('Workspace A completed work')).toBeNull();
  });
});
