import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import ConcurrentEditBanner from '../lib/ConcurrentEditBanner.svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspacePresence: vi.fn().mockResolvedValue([]),
  },
}));

// Fake ws store whose onMessage/onStatus handlers we can drive directly.
function makeWsStore(sessionId = 's-self') {
  let cb = null;
  let statusCb = null;
  return {
    sessionId,
    onMessage: (fn) => {
      cb = fn;
      return () => {
        cb = null;
      };
    },
    emit: (msg) => cb && cb(msg),
    // Mirrors the real store: onStatus fires synchronously with the current
    // status at registration (here: already connected), then on transitions.
    onStatus: (fn) => {
      statusCb = fn;
      fn('connected');
      return () => {
        statusCb = null;
      };
    },
    emitStatus: (s) => statusCb && statusCb(s),
  };
}

describe('ConcurrentEditBanner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows a warning when another user is editing the same spec (initial fetch)', async () => {
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-maria', user_id: 'maria', editing_entity: 'spec:specs/a.md' },
    ]);
    const wsStore = makeWsStore();
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore, selfUserId: null },
    });

    await waitFor(() => {
      expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy();
    });
    expect(document.body.textContent).toContain('maria is also editing this spec');
  });

  it('does not warn about a different spec', async () => {
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-maria', user_id: 'maria', editing_entity: 'spec:specs/other.md' },
    ]);
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore: makeWsStore(), selfUserId: null },
    });
    // Give the fetch effect time to resolve.
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();
  });

  it('excludes this tab (same session_id) from the warning', async () => {
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-self', user_id: 'jsell', editing_entity: 'spec:specs/a.md' },
    ]);
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore: makeWsStore('s-self'), selfUserId: null },
    });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();
  });

  it('excludes the same user in another tab (different session, same user_id) when selfUserId is set', async () => {
    // F1: a second tab of the *same* user editing the same spec has a different
    // session_id but the same user_id. With selfUserId wired, the banner must
    // stay hidden (no false "you are also editing this spec" warning).
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-other-tab', user_id: 'jsell', editing_entity: 'spec:specs/a.md' },
    ]);
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore: makeWsStore('s-self'), selfUserId: 'jsell' },
    });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();
  });

  it('still warns about a genuinely different user when selfUserId is set', async () => {
    // Guard against over-broad exclusion: a different user must still trigger the banner.
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-maria', user_id: 'maria', editing_entity: 'spec:specs/a.md' },
    ]);
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore: makeWsStore('s-self'), selfUserId: 'jsell' },
    });
    await waitFor(() => {
      expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy();
    });
    expect(document.body.textContent).toContain('maria is also editing this spec');
  });

  it('appears and then disappears as another editor arrives and leaves via live updates', async () => {
    const wsStore = makeWsStore();
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore, selfUserId: null },
    });
    // Initially no one else editing.
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();

    // Another editor starts editing the same spec.
    wsStore.emit({
      type: 'UserPresence',
      session_id: 's-maria',
      user_id: 'maria',
      view: 'specs',
      editing_entity: 'spec:specs/a.md',
    });
    await waitFor(() => {
      expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy();
    });

    // That editor leaves the editor (editing_entity cleared).
    wsStore.emit({
      type: 'UserPresence',
      session_id: 's-maria',
      user_id: 'maria',
      view: 'specs',
    });
    await waitFor(() => {
      expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();
    });
  });

  it('removes a warning when the editor session is evicted', async () => {
    const wsStore = makeWsStore();
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore, selfUserId: null },
    });
    wsStore.emit({
      type: 'UserPresence',
      session_id: 's-maria',
      user_id: 'maria',
      view: 'specs',
      editing_entity: 'spec:specs/a.md',
    });
    await waitFor(() => expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy());

    wsStore.emit({ type: 'PresenceEvicted', session_id: 's-maria' });
    await waitFor(() => expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull());
  });

  it('re-seeds presence from the endpoint after a WebSocket reconnect', async () => {
    const { api } = await import('../lib/api.js');
    // Initial fetch (default []) shows nothing.
    const wsStore = makeWsStore();
    render(ConcurrentEditBanner, {
      props: { specPath: 'specs/a.md', workspaceId: 'ws1', wsStore, selfUserId: null },
    });
    await waitFor(() => expect(api.workspacePresence).toHaveBeenCalledTimes(1));
    // The synchronous 'connected' at registration must NOT trigger a second
    // fetch — only a later transition counts as a reconnect.
    expect(api.workspacePresence).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId('concurrent-edit-banner')).toBeNull();

    // Maria started editing while the socket was down — no live UserPresence
    // reached us. Reconnect must re-fetch the authoritative snapshot.
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-maria', user_id: 'maria', editing_entity: 'spec:specs/a.md' },
    ]);
    wsStore.emitStatus('disconnected');
    wsStore.emitStatus('connected');

    await waitFor(() => {
      expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy();
    });
    expect(api.workspacePresence).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).toContain('maria is also editing this spec');
  });
});
