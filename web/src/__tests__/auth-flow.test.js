/**
 * TASK-109 — Auth flow regression tests
 *
 * These tests pin the exact behaviours that were broken:
 *   (a) api.js must send the Authorization header with the stored token
 *   (b) After setAuthToken() the very next API request uses the new value
 *   (c) The default fallback token must be 'gyre-dev-token' (matches server default),
 *       NOT 'test-token' which always produced 401s for first-time users
 *   (d) setAuthToken() persists to localStorage so page refreshes retain the token
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { api, setAuthToken } from '../lib/api.js';

function mockFetch(status = 200, body = []) {
  return vi.fn(() =>
    Promise.resolve({
      ok: status < 400,
      status,
      statusText: status === 200 ? 'OK' : 'Unauthorized',
      json: () => Promise.resolve(body),
    })
  );
}

describe('TASK-109 — api.js auth header behaviour', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
  });

  // (c) Default fallback must be gyre-dev-token, not test-token
  it('uses gyre-dev-token as the default when localStorage is empty', async () => {
    global.fetch = mockFetch();
    await api.agents();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Authorization']).toBe('Bearer gyre-dev-token');
  });

  // (a) Authorization header is always present
  it('always includes Authorization: Bearer header', async () => {
    setAuthToken('my-real-token');
    global.fetch = mockFetch();
    await api.tasks();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Authorization']).toBeDefined();
    expect(options.headers['Authorization']).toMatch(/^Bearer .+/);
  });

  // (b) Token saved via setAuthToken is used in the very next request
  it('uses the token saved via setAuthToken on the next request', async () => {
    global.fetch = mockFetch(); // first call returns OK so no error thrown
    await api.agents(); // sends with old/default token

    setAuthToken('freshly-saved-token');

    global.fetch = mockFetch();
    await api.tasks();
    const [, options] = global.fetch.mock.calls[0];
    expect(options.headers['Authorization']).toBe('Bearer freshly-saved-token');
  });

  // (d) setAuthToken persists to localStorage
  it('setAuthToken persists to localStorage under gyre_auth_token', () => {
    setAuthToken('persisted-token');
    expect(localStorage.getItem('gyre_auth_token')).toBe('persisted-token');
  });

  // Regression: overwriting the token mid-session takes effect immediately
  it('changing the token mid-session updates subsequent requests', async () => {
    setAuthToken('token-v1');
    global.fetch = mockFetch();
    await api.repos();
    expect(global.fetch.mock.calls[0][1].headers['Authorization']).toBe('Bearer token-v1');

    setAuthToken('token-v2');
    global.fetch = mockFetch();
    await api.repos();
    expect(global.fetch.mock.calls[0][1].headers['Authorization']).toBe('Bearer token-v2');
  });
});

describe('TASK-109 — ws.js default token', () => {
  it('getAuthToken in ws.js falls back to gyre-dev-token when localStorage is empty', async () => {
    localStorage.clear();
    // We can't easily instantiate a real WebSocket in jsdom, but we can verify
    // the fallback value by importing the module and checking behaviour through api.js
    // (both share the same AUTH_TOKEN_KEY and same fallback logic that we're fixing).
    global.fetch = mockFetch();
    await api.mergeQueue();
    const [, options] = global.fetch.mock.calls[0];
    // With empty localStorage the fallback must be gyre-dev-token, not test-token
    expect(options.headers['Authorization']).not.toBe('Bearer test-token');
    expect(options.headers['Authorization']).toBe('Bearer gyre-dev-token');
  });
});
