/**
 * TEMPORARY REVIEW PROBE (task-083 review) — deleted after evidence capture.
 *
 * Question: does the `?subTab=briefing` Architecture deep link survive a
 * fresh page load? App's init computes a canonical URL via urlFor() (path
 * only, no query string) and replaceState()s it — which would strip the
 * query param BEFORE ExplorerView's mount effect can read it.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';
import App from '../App.svelte';
import { api } from '../lib/api.js';

vi.mock('../lib/ws.js', () => ({
  wsClient: {
    onStatus: vi.fn().mockReturnValue(() => {}),
    onMessage: vi.fn().mockReturnValue(() => {}),
    connect: vi.fn(),
  },
}));
vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn(),
    workspaceRepos: vi.fn(),
    workspaceBudget: vi.fn(),
    notificationCount: vi.fn(),
    tokenInfo: vi.fn().mockResolvedValue({ kind: 'global' }),
    me: vi.fn().mockResolvedValue(null),
    version: vi.fn().mockResolvedValue({ version: '0.1.0', commit: 'abc1234', milestone: 'M35' }),
  },
  setAuthToken: vi.fn(),
}));
vi.mock('../components/WorkspaceHome.svelte', () => ({ default: function WorkspaceHomeStub() {} }));
vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));

describe('PROBE: ?subTab=briefing deep link on fresh load', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments', slug: 'payments' }]);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'core' }]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.version.mockResolvedValue({ version: '0.1.0' });
  });

  it('fresh load of /workspaces/payments/r/core/architecture?subTab=briefing keeps the param', async () => {
    window.history.pushState({}, '', '/workspaces/payments/r/core/architecture?subTab=briefing');
    const { container } = render(App);
    // Wait for repo mode to initialize (document title proves it).
    await waitFor(() => expect(document.title).toBe('core — Payments | Gyre'), { timeout: 3000 });
    // Give effects a moment to settle.
    await new Promise(r => setTimeout(r, 300));
    // The URL must still carry ?subTab=briefing for ExplorerView's mount
    // effect (and for a refresh / shared link) to land on the Briefing
    // sub-tab.
    // eslint-disable-next-line no-console
    console.log('PROBE URL AFTER INIT:', window.location.pathname + window.location.search);
    expect(window.location.search).toBe('?subTab=briefing');
  });
});
