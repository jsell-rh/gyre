import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createWsStore } from '../lib/ws.js';

// --- WebSocket mock ---
class MockWebSocket {
  static instances = [];
  constructor(url) {
    this.url = url;
    this.readyState = 0; // CONNECTING
    this.sentMessages = [];
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    MockWebSocket.instances.push(this);
  }
  send(data) {
    this.sentMessages.push(data);
  }
  close() {
    this.readyState = 3; // CLOSED
    this.onclose?.();
  }
  // Test helpers
  _simulateOpen() {
    this.readyState = 1; // OPEN
    this.onopen?.();
  }
  _simulateMessage(data) {
    this.onmessage?.({ data: JSON.stringify(data) });
  }
  _simulateClose() {
    this.readyState = 3;
    this.onclose?.();
  }
  _simulateError() {
    this.onerror?.();
  }
}

describe('ws.js — createWsStore', () => {
  let originalWebSocket;

  beforeEach(() => {
    MockWebSocket.instances = [];
    originalWebSocket = global.WebSocket;
    global.WebSocket = MockWebSocket;
    vi.useFakeTimers();
    localStorage.clear();
  });

  afterEach(() => {
    global.WebSocket = originalWebSocket;
    vi.useRealTimers();
  });

  it('creates a WebSocket on construction', () => {
    const store = createWsStore();
    expect(MockWebSocket.instances.length).toBe(1);
    store.destroy();
  });

  it('connects to ws:// protocol based on location', () => {
    const store = createWsStore();
    const ws = MockWebSocket.instances[0];
    expect(ws.url).toContain('ws:');
    expect(ws.url).toContain('/ws');
    store.destroy();
  });

  it('sends Auth message on WebSocket open', () => {
    const store = createWsStore();
    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();

    expect(ws.sentMessages.length).toBe(1);
    const authMsg = JSON.parse(ws.sentMessages[0]);
    expect(authMsg.type).toBe('Auth');
    expect(authMsg.token).toBe('gyre-dev-token');
    store.destroy();
  });

  it('uses token from localStorage when available', () => {
    localStorage.setItem('gyre_auth_token', 'custom-token-xyz');
    const store = createWsStore();
    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();

    const authMsg = JSON.parse(ws.sentMessages[0]);
    expect(authMsg.token).toBe('custom-token-xyz');
    store.destroy();
  });

  it('sets status to "connected" on successful AuthResult', () => {
    const statusCb = vi.fn();
    const store = createWsStore();
    store.onStatus(statusCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: true });

    expect(statusCb).toHaveBeenCalledWith('connected');
    store.destroy();
  });

  it('sets status to "auth-failed" on failed AuthResult', () => {
    const statusCb = vi.fn();
    const store = createWsStore();
    store.onStatus(statusCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: false });

    expect(statusCb).toHaveBeenCalledWith('auth-failed');
    store.destroy();
  });

  it('dispatches non-auth messages to listeners', () => {
    const msgCb = vi.fn();
    const store = createWsStore();
    store.onMessage(msgCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: true });
    ws._simulateMessage({ type: 'TaskUpdated', id: 't1' });

    // AuthResult should NOT be dispatched to message listeners
    expect(msgCb).toHaveBeenCalledTimes(1);
    expect(msgCb).toHaveBeenCalledWith({ type: 'TaskUpdated', id: 't1' });
    store.destroy();
  });

  it('sets status to "disconnected" on close', () => {
    const statusCb = vi.fn();
    const store = createWsStore();
    store.onStatus(statusCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateClose();

    expect(statusCb).toHaveBeenCalledWith('disconnected');
    store.destroy();
  });

  it('sets status to "error" on error', () => {
    const statusCb = vi.fn();
    const store = createWsStore();
    store.onStatus(statusCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateError();

    expect(statusCb).toHaveBeenCalledWith('error');
    store.destroy();
  });

  it('schedules reconnect after close (3s delay)', () => {
    const store = createWsStore();
    expect(MockWebSocket.instances.length).toBe(1);

    const ws = MockWebSocket.instances[0];
    ws._simulateClose();

    // Before timer fires, no new WebSocket
    expect(MockWebSocket.instances.length).toBe(1);

    // Advance 3000ms (RECONNECT_DELAY_MS)
    vi.advanceTimersByTime(3000);

    expect(MockWebSocket.instances.length).toBe(2);
    store.destroy();
  });

  it('does not schedule multiple reconnects if close fires repeatedly', () => {
    const store = createWsStore();
    const ws = MockWebSocket.instances[0];
    ws._simulateClose();
    ws._simulateClose(); // second close before timer fires

    vi.advanceTimersByTime(3000);
    // Should only have created one new WebSocket
    expect(MockWebSocket.instances.length).toBe(2);
    store.destroy();
  });

  it('onMessage returns an unsubscribe function', () => {
    const msgCb = vi.fn();
    const store = createWsStore();
    const unsub = store.onMessage(msgCb);

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: true });

    unsub();
    ws._simulateMessage({ type: 'TaskUpdated', id: 't1' });

    expect(msgCb).not.toHaveBeenCalled();
    store.destroy();
  });

  it('onStatus returns an unsubscribe function', () => {
    const statusCb = vi.fn();
    const store = createWsStore();
    const unsub = store.onStatus(statusCb);

    // onStatus immediately calls with current status
    expect(statusCb).toHaveBeenCalledWith('disconnected');

    unsub();
    statusCb.mockClear();

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: true });

    // Should NOT have been called after unsubscribe
    expect(statusCb).not.toHaveBeenCalled();
    store.destroy();
  });

  it('onStatus immediately calls callback with current status', () => {
    const store = createWsStore();
    const statusCb = vi.fn();
    store.onStatus(statusCb);
    // Initial status is "disconnected" (WebSocket hasn't connected yet in jsdom)
    expect(statusCb).toHaveBeenCalledWith('disconnected');
    store.destroy();
  });

  it('destroy() closes WebSocket and clears listeners', () => {
    const msgCb = vi.fn();
    const statusCb = vi.fn();
    const store = createWsStore();
    store.onMessage(msgCb);
    store.onStatus(statusCb);

    store.destroy();

    const ws = MockWebSocket.instances[0];
    expect(ws.readyState).toBe(3); // CLOSED
  });

  it('destroy() cancels pending reconnect timer', () => {
    const store = createWsStore();
    const ws = MockWebSocket.instances[0];
    ws._simulateClose();

    store.destroy();
    vi.advanceTimersByTime(5000);

    // Should not have created a new WebSocket after destroy
    expect(MockWebSocket.instances.length).toBe(1);
  });

  it('supports multiple message listeners', () => {
    const cb1 = vi.fn();
    const cb2 = vi.fn();
    const store = createWsStore();
    store.onMessage(cb1);
    store.onMessage(cb2);

    const ws = MockWebSocket.instances[0];
    ws._simulateOpen();
    ws._simulateMessage({ type: 'AuthResult', success: true });
    ws._simulateMessage({ type: 'AgentStatus', id: 'a1', status: 'running' });

    expect(cb1).toHaveBeenCalledWith({ type: 'AgentStatus', id: 'a1', status: 'running' });
    expect(cb2).toHaveBeenCalledWith({ type: 'AgentStatus', id: 'a1', status: 'running' });
    store.destroy();
  });
});
