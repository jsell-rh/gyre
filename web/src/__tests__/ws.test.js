import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock WebSocket before importing the module
let mockWsInstances = [];

class MockWebSocket {
  constructor(url) {
    this.url = url;
    this.readyState = 0;
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    this.sent = [];
    mockWsInstances.push(this);
  }
  send(data) { this.sent.push(data); }
  close() { this.onclose?.(); }

  // Test helpers
  _triggerOpen() { this.readyState = 1; this.onopen?.(); }
  _triggerMessage(data) { this.onmessage?.({ data: JSON.stringify(data) }); }
  _triggerClose() { this.readyState = 3; this.onclose?.(); }
  _triggerError() { this.onerror?.(); }
}

vi.stubGlobal('WebSocket', MockWebSocket);

// Mock window.location for ws URL construction
Object.defineProperty(window, 'location', {
  value: { protocol: 'http:', host: 'localhost:3000' },
  writable: true,
});

const { createWsStore } = await import('../lib/ws.js');

describe('createWsStore', () => {
  beforeEach(() => {
    mockWsInstances = [];
    localStorage.clear();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('creates a WebSocket connection on init', () => {
    const store = createWsStore();
    expect(mockWsInstances.length).toBeGreaterThanOrEqual(1);
    const ws = mockWsInstances[mockWsInstances.length - 1];
    expect(ws.url).toBe('ws://localhost:3000/ws');
    store.destroy();
  });

  it('sends Auth message on open', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    ws._triggerOpen();
    expect(ws.sent.length).toBe(1);
    const msg = JSON.parse(ws.sent[0]);
    expect(msg.type).toBe('Auth');
    expect(msg.token).toBeTruthy();
    store.destroy();
  });

  it('reports connected status after successful AuthResult', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const statuses = [];
    store.onStatus(s => statuses.push(s));
    ws._triggerOpen();
    ws._triggerMessage({ type: 'AuthResult', success: true });
    expect(statuses).toContain('connected');
    store.destroy();
  });

  it('reports auth-failed status on failed AuthResult', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const statuses = [];
    store.onStatus(s => statuses.push(s));
    ws._triggerOpen();
    ws._triggerMessage({ type: 'AuthResult', success: false });
    expect(statuses).toContain('auth-failed');
    store.destroy();
  });

  it('reports error status on WebSocket error', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const statuses = [];
    store.onStatus(s => statuses.push(s));
    ws._triggerError();
    expect(statuses).toContain('error');
    store.destroy();
  });

  it('delivers non-Auth messages to onMessage listeners', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const messages = [];
    store.onMessage(m => messages.push(m));
    ws._triggerOpen();
    ws._triggerMessage({ type: 'TaskUpdated', id: '123' });
    expect(messages).toHaveLength(1);
    expect(messages[0].type).toBe('TaskUpdated');
    store.destroy();
  });

  it('does not deliver AuthResult to onMessage listeners', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const messages = [];
    store.onMessage(m => messages.push(m));
    ws._triggerOpen();
    ws._triggerMessage({ type: 'AuthResult', success: true });
    expect(messages).toHaveLength(0);
    store.destroy();
  });

  it('unsubscribes onMessage listener when returned function is called', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const messages = [];
    const unsub = store.onMessage(m => messages.push(m));
    ws._triggerOpen();
    ws._triggerMessage({ type: 'Event', data: 1 });
    expect(messages).toHaveLength(1);
    unsub();
    ws._triggerMessage({ type: 'Event', data: 2 });
    expect(messages).toHaveLength(1);
    store.destroy();
  });

  it('reports disconnected on close and schedules reconnect', () => {
    const store = createWsStore();
    const ws = mockWsInstances[mockWsInstances.length - 1];
    const statuses = [];
    store.onStatus(s => statuses.push(s));
    ws._triggerClose();
    expect(statuses).toContain('disconnected');
    // After reconnect delay, a new WebSocket should be created
    const countBefore = mockWsInstances.length;
    vi.advanceTimersByTime(3500);
    expect(mockWsInstances.length).toBeGreaterThan(countBefore);
    store.destroy();
  });

  it('onStatus fires immediately with current status', () => {
    const store = createWsStore();
    const statuses = [];
    store.onStatus(s => statuses.push(s));
    // Should have received the initial status immediately
    expect(statuses.length).toBeGreaterThanOrEqual(1);
    store.destroy();
  });

  it('destroy cleans up without errors', () => {
    const store = createWsStore();
    expect(() => store.destroy()).not.toThrow();
  });
});
