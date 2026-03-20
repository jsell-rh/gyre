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

  it('api.projects() calls /api/v1/projects', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.projects();
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/projects');
  });

  it('api.createProject() sends POST with body', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ id: '1' }) })
    );
    await api.createProject({ name: 'test', description: 'desc' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/projects');
    expect(options.method).toBe('POST');
    expect(JSON.parse(options.body)).toEqual({ name: 'test', description: 'desc' });
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
});

// ── Repo URL correctness (TASK-097 regression) ────────────────────────────────
// The bug: api.repos(projectId) was calling /projects/{id}/repos instead of
// /repos?project_id={id}, causing the SPA catch-all to return HTML with status
// 200, which failed with "JSON.parse: unexpected character at line 1 column 1".

describe('api.js — repo URL correctness', () => {
  it('api.repos(projectId) calls /api/v1/repos?project_id=...', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos('proj-123');
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos?project_id=proj-123');
  });

  it('api.repos() URL-encodes the project ID', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos('proj/with-slash');
    const [url] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos?project_id=proj%2Fwith-slash');
  });

  it('api.repos() does NOT call /projects/{id}/repos (the old wrong path)', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    await api.repos('my-project');
    const [url] = global.fetch.mock.calls[0];
    expect(url).not.toContain('/projects/');
    expect(url).not.toContain('/my-project/repos');
  });

  it('api.createRepo() POSTs to /api/v1/repos and returns a JSON object', async () => {
    const mockRepo = { id: 'repo-1', name: 'my-repo', project_id: 'proj-1', default_branch: 'main' };
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 201, json: () => Promise.resolve(mockRepo) })
    );
    const result = await api.createRepo({ name: 'my-repo', project_id: 'proj-1' });
    const [url, options] = global.fetch.mock.calls[0];
    expect(url).toBe('/api/v1/repos');
    expect(options.method).toBe('POST');
    expect(result).not.toBeNull();
    expect(result).toHaveProperty('id');
    expect(result).toHaveProperty('name', 'my-repo');
  });

  it('request() propagates SyntaxError when API returns non-JSON with status 200 (SPA HTML fallback)', async () => {
    // Simulates the SPA catch-all returning HTML for an unknown API path:
    // the server route does not exist, returns index.html with 200,
    // and res.json() throws a SyntaxError (JSON.parse error).
    global.fetch = vi.fn(() =>
      Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.reject(new SyntaxError('JSON.parse: unexpected character at line 1 column 1 of the JSON data')),
      })
    );
    await expect(api.repos('proj-1')).rejects.toThrow(SyntaxError);
  });
});
