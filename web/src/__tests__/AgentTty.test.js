import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// api.agentTtyUrl must be the single source of the TTY socket URL — the
// test mocks it with a tracked implementation and asserts the component
// actually consumes its return value (agentTtyUrl is no longer dead code).
const agentTtyUrlMock = vi.fn((id) => `ws://testserver/ws/agents/${id}/tty`);
vi.mock('../lib/api.js', () => ({
  api: {
    agentTtyUrl: (...args) => agentTtyUrlMock(...args),
  },
}));

import AgentTty from '../lib/AgentTty.svelte';

// WebSocket mock (same shape as ws.test.js).
class MockWebSocket {
  static instances = [];
  constructor(url) {
    this.url = url;
    this.readyState = 0;
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
    this.readyState = 3;
    this.onclose?.();
  }
  _simulateOpen() {
    this.readyState = 1;
    this.onopen?.();
  }
  _simulateText(text) {
    this.onmessage?.({ data: text });
  }
}

describe('AgentTty — TTY attach from the product UI', () => {
  let originalWebSocket;

  beforeEach(() => {
    MockWebSocket.instances = [];
    agentTtyUrlMock.mockClear();
    originalWebSocket = global.WebSocket;
    global.WebSocket = MockWebSocket;
    localStorage.clear();
  });

  afterEach(() => {
    global.WebSocket = originalWebSocket;
  });

  it('renders an Attach-TTY control', () => {
    const { getByText } = render(AgentTty, { props: { agentId: 'agent-42' } });
    expect(getByText('Attach TTY')).toBeTruthy();
  });

  it('opens the /ws/agents/:id/tty socket URL from agentTtyUrl and authenticates', async () => {
    const { getByText } = render(AgentTty, { props: { agentId: 'agent-42' } });
    await fireEvent.click(getByText('Attach TTY'));

    // agentTtyUrl was consumed — the socket was constructed with EXACTLY
    // the URL it returned (it is no longer dead code).
    expect(agentTtyUrlMock).toHaveBeenCalledWith('agent-42');
    expect(MockWebSocket.instances).toHaveLength(1);
    expect(MockWebSocket.instances[0].url).toBe('ws://testserver/ws/agents/agent-42/tty');
    expect(MockWebSocket.instances[0].url).toContain('/ws/agents/agent-42/tty');

    // First frame on the socket is the Auth message (tty.rs protocol).
    const socket = MockWebSocket.instances[0];
    socket._simulateOpen();
    expect(socket.sentMessages).toHaveLength(1);
    const auth = JSON.parse(socket.sentMessages[0]);
    expect(auth.type).toBe('Auth');
    expect(typeof auth.token).toBe('string');
  });

  it('displays replayed then live lines from the stream', async () => {
    const { getByText, container } = render(AgentTty, { props: { agentId: 'agent-42' } });
    await fireEvent.click(getByText('Attach TTY'));
    const socket = MockWebSocket.instances[0];
    socket._simulateOpen();
    // Auth verdict, then the banner + replayed buffer, then a live line —
    // plain text frames in arrival order (tty.rs).
    socket._simulateText(JSON.stringify({ type: 'AuthResult', success: true, message: 'authenticated' }));
    socket._simulateText('[tty] agent agent-42 — log stream\r\n');
    socket._simulateText('replayed line 1\r\n');
    socket._simulateText('replayed line 2\r\n');
    socket._simulateText('live line 3\r\n');

    const view = container.querySelector('.agent-tty-view');
    expect(view).toBeTruthy();
    await waitFor(() => {
      expect(view.textContent).toContain('replayed line 1');
      expect(view.textContent).toContain('replayed line 2');
      expect(view.textContent).toContain('live line 3');
    });
    // Auth failure path surfaces as a message, not a silent dead socket.
    expect(getByText('Detach TTY')).toBeTruthy();
  });

  it('auth failure is reported and detaches', async () => {
    const { getByText, container } = render(AgentTty, { props: { agentId: 'agent-42' } });
    await fireEvent.click(getByText('Attach TTY'));
    const socket = MockWebSocket.instances[0];
    socket._simulateOpen();
    socket._simulateText(JSON.stringify({ type: 'AuthResult', success: false, message: 'invalid token' }));
    await waitFor(() => {
      expect(container.querySelector('.agent-tty-view').textContent).toContain('authentication failed');
    });
    // Back to the attach affordance after the failure.
    expect(getByText('Attach TTY')).toBeTruthy();
  });

  it('detach closes the socket', async () => {
    const { getByText } = render(AgentTty, { props: { agentId: 'agent-42' } });
    await fireEvent.click(getByText('Attach TTY'));
    const socket = MockWebSocket.instances[0];
    socket._simulateOpen();
    await fireEvent.click(getByText('Detach TTY'));
    expect(socket.readyState).toBe(3);
    expect(getByText('Attach TTY')).toBeTruthy();
  });
});
