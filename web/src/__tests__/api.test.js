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
    await api.createMirrorRepo({ name: 'mirror', project_id: 'p1', url: 'https://github.com/org/repo.git' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos/mirror');
    expect(options.method).toBe('POST');
  });
});
