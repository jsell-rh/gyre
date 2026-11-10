/**
 * ExplorerChat-streaming.test.js — task-036 acceptance criteria
 * (explorer-implementation.md §Phase 4: Polish + §WebSocket Protocol)
 *
 * Unlike the mirrored-logic unit suites, these tests mount the REAL
 * ExplorerChat component with a controllable WebSocket mock and drive the
 * spec's server→client wire frames (`{"type":"text",...,"done":false}`,
 * `{"type":"status","status":"thinking"|"refining"|"ready"}`) through the
 * component's actual handleMessage path, then assert on the rendered DOM:
 *
 *  - text renders progressively as done:false chunks arrive
 *  - "Thinking..." / "Refining query..." indicators appear for their statuses
 *  - indicators hide when status is "ready" or streaming text is on screen
 *  - the final message is the concatenation of all chunks after done:true
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

import ExplorerChat from '../lib/ExplorerChat.svelte';

// ── Controllable WebSocket mock ─────────────────────────────────────────────
// The component constructs exactly one socket per connection; tests grab the
// instance the component created, flip it open, and push server frames
// through the same onmessage handler the real socket would use.
class MockWebSocket {
  static instances = [];
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  constructor(url) {
    this.url = url;
    this.readyState = MockWebSocket.CONNECTING;
    this.sent = []; // parsed outbound frames
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    MockWebSocket.instances.push(this);
  }

  send(data) {
    this.sent.push(JSON.parse(data));
  }

  close() {
    if (this.readyState === MockWebSocket.OPEN || this.readyState === MockWebSocket.CONNECTING) {
      this.readyState = MockWebSocket.CLOSED;
      // Component nulls onclose before closing on teardown; only an
      // unsolicited server-side close should trigger reconnect logic.
      if (this.onclose) {
        const cb = this.onclose;
        this.onclose = null;
        cb();
      }
    }
  }
}

/** Mount the component and complete the ticket + WebSocket handshake. */
async function mountChat() {
  localStorage.setItem('gyre_auth_token', 'test-token');
  const view = render(ExplorerChat, { props: { repoId: 'repo-1' } });

  // connect() awaits the ws-ticket fetch before constructing the socket.
  let socket;
  await waitFor(() => {
    socket = MockWebSocket.instances[MockWebSocket.instances.length - 1];
    expect(socket).toBeDefined();
    expect(global.fetch).toHaveBeenCalled();
  });

  socket.readyState = MockWebSocket.OPEN;
  socket.onopen();
  // onopen requests the saved-views list and transitions to "ready"
  await waitFor(() => {
    expect(view.container.querySelector('.chat-status')).toHaveTextContent('Ready');
  });
  expect(socket.sent.some((m) => m.type === 'list_views')).toBe(true);

  return { ...view, socket };
}

/** Push a server→client frame through the component's real onmessage path. */
function serverSend(socket, obj) {
  socket.onmessage({ data: JSON.stringify(obj) });
}

beforeEach(() => {
  MockWebSocket.instances = [];
  vi.stubGlobal('WebSocket', MockWebSocket);
  // Ticket exchange for the mocked auth token
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ ticket: 'test-ticket' }),
    })
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('ExplorerChat streaming (mounted component)', () => {
  it('renders text progressively as done:false chunks arrive and finalizes on done:true', async () => {
    const { container, socket } = await mountChat();

    serverSend(socket, { type: 'status', status: 'thinking' });
    await waitFor(() => {
      expect(container.querySelector('.thinking-indicator')).toBeInTheDocument();
    });

    // First chunk: streaming bubble appears with partial text
    serverSend(socket, { type: 'text', content: 'Space is ', done: false });
    await waitFor(() => {
      const streaming = container.querySelector('.chat-message.streaming .message-content');
      expect(streaming).toBeInTheDocument();
      expect(streaming).toHaveTextContent('Space is');
    });

    // Indicator hides once streaming text is on screen
    expect(container.querySelector('.thinking-indicator')).not.toBeInTheDocument();

    // Second chunk: buffer grows — progressive rendering
    serverSend(socket, { type: 'text', content: 'the core domain entity', done: false });
    await waitFor(() => {
      const streaming = container.querySelector('.chat-message.streaming .message-content');
      expect(streaming).toHaveTextContent('Space is the core domain entity');
    });

    // Not finalized yet: only the streaming bubble exists, status still thinking
    expect(container.querySelectorAll('.chat-message')).toHaveLength(1);
    expect(container.querySelector('.chat-status')).toHaveTextContent('Thinking...');

    // Final frame: message finalized with the full concatenated text
    serverSend(socket, { type: 'text', content: '', done: true });
    await waitFor(() => {
      const finalMsg = container.querySelector('.chat-message:not(.streaming) .message-content');
      expect(finalMsg).toHaveTextContent('Space is the core domain entity');
    });
    expect(container.querySelector('.chat-message.streaming')).not.toBeInTheDocument();
    expect(container.querySelector('.thinking-indicator')).not.toBeInTheDocument();
    expect(container.querySelector('.chat-status')).toHaveTextContent('Ready');
  });

  it('final message includes a trailing chunk delivered with done:true', async () => {
    const { container, socket } = await mountChat();

    serverSend(socket, { type: 'text', content: 'Part 1. ', done: false });
    serverSend(socket, { type: 'text', content: 'Part 2. ', done: false });
    serverSend(socket, { type: 'text', content: 'Part 3.', done: true });

    await waitFor(() => {
      const finalMsg = container.querySelector('.chat-message:not(.streaming) .message-content');
      expect(finalMsg).toHaveTextContent('Part 1. Part 2. Part 3.');
    });
    expect(container.querySelector('.chat-message.streaming')).not.toBeInTheDocument();
    expect(container.querySelector('.chat-status')).toHaveTextContent('Ready');
  });
});

describe('ExplorerChat status indicators (mounted component)', () => {
  it('shows "Thinking..." then "Refining query..." and hides on ready', async () => {
    const { container, socket } = await mountChat();

    serverSend(socket, { type: 'status', status: 'thinking' });
    await waitFor(() => {
      const indicator = container.querySelector('.thinking-indicator');
      expect(indicator).toBeInTheDocument();
      expect(indicator.getAttribute('role')).toBe('status');
      expect(indicator).toHaveTextContent('Thinking...');
    });

    serverSend(socket, { type: 'status', status: 'refining' });
    await waitFor(() => {
      const indicator = container.querySelector('.thinking-indicator');
      expect(indicator).toBeInTheDocument();
      expect(indicator).toHaveTextContent('Refining query...');
    });

    serverSend(socket, { type: 'status', status: 'ready' });
    await waitFor(() => {
      expect(container.querySelector('.thinking-indicator')).not.toBeInTheDocument();
    });
    expect(container.querySelector('.chat-status')).toHaveTextContent('Ready');
  });

  it('hides the thinking indicator while streaming text is on screen', async () => {
    const { container, socket } = await mountChat();

    serverSend(socket, { type: 'status', status: 'thinking' });
    await waitFor(() => {
      expect(container.querySelector('.thinking-indicator')).toBeInTheDocument();
    });

    // Text starts arriving — the streaming bubble replaces the indicator
    serverSend(socket, { type: 'text', content: 'Answering now', done: false });
    await waitFor(() => {
      expect(container.querySelector('.chat-message.streaming')).toBeInTheDocument();
    });
    expect(container.querySelector('.thinking-indicator')).not.toBeInTheDocument();

    // Status stays "thinking" (chunks keep it alive) but the indicator only
    // renders when there is no streaming text to show instead.
    expect(container.querySelector('.chat-status')).toHaveTextContent('Thinking...');
  });

  it('finalizes in-flight streaming text as a thinking message when a new turn starts', async () => {
    const { container, socket } = await mountChat();

    serverSend(socket, { type: 'status', status: 'thinking' });
    serverSend(socket, { type: 'text', content: 'internal reasoning', done: false });
    await waitFor(() => {
      expect(container.querySelector('.chat-message.streaming')).toBeInTheDocument();
    });

    // A refining turn begins: prior partial text is preserved as a
    // visually-distinct "(thinking)" message, not silently dropped.
    serverSend(socket, { type: 'status', status: 'refining' });
    await waitFor(() => {
      const thinkingMsg = container.querySelector('.chat-message.thinking');
      expect(thinkingMsg).toBeInTheDocument();
      expect(thinkingMsg).toHaveTextContent('internal reasoning');
      expect(thinkingMsg).toHaveTextContent('(thinking)');
    });
    expect(container.querySelector('.chat-message.streaming')).not.toBeInTheDocument();

    // Indicator is back (no streaming text) with the refining label
    await waitFor(() => {
      expect(container.querySelector('.thinking-indicator')).toHaveTextContent('Refining query...');
    });

    // The refined turn completes with the final answer
    serverSend(socket, { type: 'text', content: 'Final answer.', done: true });
    await waitFor(() => {
      const finalMsg = container.querySelector(
        '.chat-message:not(.streaming):not(.thinking) .message-content'
      );
      expect(finalMsg).toHaveTextContent('Final answer.');
    });
    expect(container.querySelector('.thinking-indicator')).not.toBeInTheDocument();
  });
});

describe('ExplorerChat user round trip (mounted component)', () => {
  it('sends the user message over the WebSocket and renders the user bubble', async () => {
    const { container, socket } = await mountChat();

    const textarea = container.querySelector('.chat-input');
    await fireEvent.input(textarea, { target: { value: 'What are the main endpoints?' } });

    const sendBtn = container.querySelector('.send-btn');
    expect(sendBtn).not.toBeDisabled();
    await fireEvent.click(sendBtn);

    await waitFor(() => {
      const last = socket.sent[socket.sent.length - 1];
      expect(last.type).toBe('message');
      expect(last.text).toBe('What are the main endpoints?');
    });

    await waitFor(() => {
      const userMsg = container.querySelector('.chat-message.user');
      expect(userMsg).toHaveTextContent('What are the main endpoints?');
    });
    // Input cleared and status moved to thinking after the send
    expect(container.querySelector('.chat-input').value).toBe('');
    expect(container.querySelector('.chat-status')).toHaveTextContent('Thinking...');
  });
});
