import { describe, it, expect, vi } from 'vitest';
import { sendEditingPresence, specEditingEntity } from '../lib/presence.js';

function fakeStore() {
  return { send: vi.fn(), sessionId: 'sess-1' };
}

describe('sendEditingPresence', () => {
  it('sends a UserPresence heartbeat with editing_entity when editing', () => {
    const store = fakeStore();
    sendEditingPresence(store, { workspaceId: 'ws1', editingEntity: 'spec:specs/a.md' });
    expect(store.send).toHaveBeenCalledTimes(1);
    const msg = store.send.mock.calls[0][0];
    expect(msg.type).toBe('UserPresence');
    expect(msg.session_id).toBe('sess-1');
    expect(msg.workspace_id).toBe('ws1');
    expect(msg.editing_entity).toBe('spec:specs/a.md');
    expect(typeof msg.timestamp).toBe('number');
  });

  it('omits editing_entity when clearing (null) so other banners disappear', () => {
    const store = fakeStore();
    sendEditingPresence(store, { workspaceId: 'ws1', editingEntity: null });
    expect(store.send).toHaveBeenCalledTimes(1);
    const msg = store.send.mock.calls[0][0];
    expect('editing_entity' in msg).toBe(false);
    // Still a valid presence heartbeat for the workspace.
    expect(msg.workspace_id).toBe('ws1');
  });

  it('no-ops when the store is missing send/sessionId or workspace is absent', () => {
    const store = fakeStore();
    sendEditingPresence(null, { workspaceId: 'ws1', editingEntity: 'spec:a.md' });
    sendEditingPresence({ sessionId: 's' }, { workspaceId: 'ws1', editingEntity: 'spec:a.md' });
    sendEditingPresence(store, { workspaceId: '', editingEntity: 'spec:a.md' });
    expect(store.send).not.toHaveBeenCalled();
  });
});

describe('specEditingEntity', () => {
  it('prefixes a spec path with the spec: entity scheme', () => {
    expect(specEditingEntity('specs/system/auth.md')).toBe('spec:specs/system/auth.md');
  });

  it('returns null for an empty path', () => {
    expect(specEditingEntity('')).toBeNull();
    expect(specEditingEntity(null)).toBeNull();
  });
});
