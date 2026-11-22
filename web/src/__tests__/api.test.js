import { describe, it, expect, beforeEach, vi } from 'vitest';
import { api, setAuthToken } from '../lib/api.js';

describe('api.js — auth header', () => {
  it('getAuthToken returns default when localStorage is empty', async () => {
    localStorage.clear();
    // Trigger any api call and check the Authorization header used
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.agents();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Authorization']).toMatch(/^Bearer .+/);
  });

  it('request() includes Authorization: Bearer header on every call', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.agents();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers).toBeDefined();
    expect(options.headers['Authorization']).toBeDefined();
    expect(options.headers['Authorization']).toMatch(/^Bearer /);
  });

  it('uses token from localStorage when set', async () => {
    setAuthToken('my-custom-token');
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.agents();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Authorization']).toBe('Bearer my-custom-token');
  });

  it('setAuthToken persists to localStorage', () => {
    setAuthToken('stored-token-123');
    expect(localStorage.getItem('gyre_auth_token')).toBe('stored-token-123');
  });

  it('request() includes Content-Type: application/json', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.tasks();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Content-Type']).toBe('application/json');
  });

  it('api.agents() calls /api/v1/agents', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.agents();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/agents');
  });

  it('api.tasks() calls /api/v1/tasks', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.tasks();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/tasks');
  });

  it('api.repos() calls /api/v1/repos', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
  });

  it('api.createRepo() sends POST with workspace_id in body', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ id: 'r1' }) })
    );
    await api.createRepo({ name: 'test-repo', workspace_id: 'ws-1' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({ name: 'test-repo', workspace_id: 'ws-1' });
  });

  it('api.createTask() sends POST with body', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ id: '2' }) })
    );
    await api.createTask({ title: 'My Task', priority: 'High' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/tasks');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body).title).toBe('My Task');
  });

  it('api.mergeQueue() calls /api/v1/merge-queue', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.mergeQueue();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/merge-queue');
  });

  it('api.repoStatus() calls /api/v1/repos/:id/status', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({}) })
    );
    await api.repoStatus('repo-1');
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/repo-1/status');
  });

  it('api.repoStatus(id, { withGates: false }) skips gate execution', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({}) })
    );
    await api.repoStatus('repo-1', { withGates: false });
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/repo-1/status?gates=false');
  });

  it('api.pauseRepoQueue() PUTs to /api/v1/repos/:id/queue/pause with reason', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({}) })
    );
    await api.pauseRepoQueue('repo-1', 'main is broken');
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/repo-1/queue/pause');
    expect(options.method).toBe('PUT');
    expect(JSON.parse(options.body)).toEqual({ reason: 'main is broken' });
  });

  it('api.resumeRepoQueue() PUTs to /api/v1/repos/:id/queue/resume', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({}) })
    );
    await api.resumeRepoQueue('repo-1');
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/repo-1/queue/resume');
    expect(options.method).toBe('PUT');
  });

  it('api.repos({ workspaceId }) calls /api/v1/repos?workspace_id=<id>', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos({ workspaceId: 'ws-123' });
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos?workspace_id=ws-123');
  });

  it('api.createRepo() sends POST to /api/v1/repos with body', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 201, json: () => Promise.resolve({ id: 'r1' }) })
    );
    await api.createRepo({ name: 'my-repo', workspace_id: 'ws-1', default_branch: 'main' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body).name).toBe('my-repo');
  });

  it('api.allRepos() calls /api/v1/repos without project_id filter', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.allRepos();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
  });

  it('api.repos() with no args calls /api/v1/repos without filter', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
    expect(url).not.toContain('project_id');
  });

  it('api.createMirrorRepo() POSTs to /api/v1/repos/mirror', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 201, json: () => Promise.resolve({ id: 'm1' }) })
    );
    await api.createMirrorRepo({ name: 'mirror', workspace_id: 'ws-p1', url: 'https://github.com/org/repo.git', interval_secs: 300 });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/mirror');
    expect(options.method).toBe('POST');
    const body = JSON.parse(options.body);
    expect(body.workspace_id).toBe('ws-p1');
    expect(body).not.toHaveProperty('project_id');
  });
});


describe('api.js — invitation + role management surface (task-127)', () => {
  const TOKEN = 'ab'.repeat(32);

  function lastCall() {
    return global.fetch.mock.calls[global.fetch.mock.calls.length - 1];
  }

  it('invitationPreview() GETs /invite/{token}/preview WITHOUT an Authorization header', async () => {
    setAuthToken('some-session-token');
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ kind: 'tenant' }) })
    );
    await api.invitationPreview(TOKEN);
    const [url, options] = lastCall();
    expect(url).toBe(`/api/v1/invite/${TOKEN}/preview`);
    expect(options.method).toBeUndefined();
    // The magic-link token is the auth factor: a logged-in session's
    // Bearer identity must not ride along (the server rejects a
    // mismatched authenticated invitee).
    expect(options.headers['Authorization']).toBeUndefined();
  });

  it('acceptTenantInvitationByToken() POSTs the token accept without a Bearer header', async () => {
    setAuthToken('some-session-token');
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ user_id: 'u1' }) })
    );
    await api.acceptTenantInvitationByToken(TOKEN, { username: 'alice' });
    const [url, options] = lastCall();
    expect(url).toBe(`/api/v1/invite/${TOKEN}/accept`);
    expect(options.method).toBe('POST');
    expect(options.headers['Authorization']).toBeUndefined();
    expect(JSON.parse(options.body)).toEqual({ username: 'alice' });
  });

  it('declineWorkspaceInvitationByToken() POSTs the workspace token decline without a Bearer header', async () => {
    setAuthToken('some-session-token');
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 204, json: () => Promise.resolve(null) })
    );
    await api.declineWorkspaceInvitationByToken(TOKEN);
    const [url, options] = lastCall();
    expect(url).toBe(`/api/v1/workspaces/invitations/${TOKEN}/decline`);
    expect(options.method).toBe('POST');
    expect(options.headers['Authorization']).toBeUndefined();
  });

  it('tenantInvitations() lists with an optional status filter and a Bearer header', async () => {
    setAuthToken('admin-token');
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ invitations: [] }) })
    );
    await api.tenantInvitations('Pending');
    const [url, options] = lastCall();
    expect(url).toBe('/api/v1/tenant/invitations?status=Pending');
    expect(options.headers['Authorization']).toBe('Bearer admin-token');
    await api.tenantInvitations();
    expect(lastCall()[0]).toBe('/api/v1/tenant/invitations');
  });

  it('inviteToTenant() and bulkInviteToTenant() POST their payloads', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 201, json: () => Promise.resolve({ id: 'i1' }) })
    );
    await api.inviteToTenant({ email: 'a@example.com', role: 'Member' });
    let [url, options] = lastCall();
    expect(url).toBe('/api/v1/tenant/invite');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({ email: 'a@example.com', role: 'Member' });

    await api.bulkInviteToTenant({ invitations: [{ email: 'b@example.com' }] });
    [url, options] = lastCall();
    expect(url).toBe('/api/v1/tenant/invite/bulk');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({ invitations: [{ email: 'b@example.com' }] });
  });

  it('revokeTenantInvitation() DELETEs by id', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 204, json: () => Promise.resolve(null) })
    );
    await api.revokeTenantInvitation('inv-42');
    const [url, options] = lastCall();
    expect(url).toBe('/api/v1/tenant/invitations/inv-42');
    expect(options.method).toBe('DELETE');
  });

  it('updateUserGlobalRole() PUTs the GlobalRole to /users/{id}/role', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ id: 'u-1' }) })
    );
    await api.updateUserGlobalRole('u-1', { role: 'TenantAdmin', reason: 'promotion' });
    const [url, options] = lastCall();
    expect(url).toBe('/api/v1/users/u-1/role');
    expect(options.method).toBe('PUT');
    expect(JSON.parse(options.body)).toEqual({ role: 'TenantAdmin', reason: 'promotion' });
  });
});
