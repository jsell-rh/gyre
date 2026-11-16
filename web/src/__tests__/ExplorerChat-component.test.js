/**
 * ExplorerChat-component.test.js — explorer-implementation.md §18, §26
 *
 * Mounted-component tests for ExplorerChat.svelte. Unlike the protocol
 * mirror tests in ExplorerChat.test.js, these render the real component
 * with a mocked WebSocket + ticket-auth fetch and drive it through the
 * specced behaviors:
 *
 * - WS connection to /api/v1/repos/:repo_id/explorer (ticket via Bearer)
 * - list_views on open; Ready/Thinking/Refining status display
 * - streamed text rendering (incremental, markdown) before done=true
 * - view_query forwarding to the canvas via onViewQuery
 * - user messages carrying the §10 wire canvas_state
 * - saved views dropdown, load_view and save_view protocol
 * - reconnection after disconnect (including a fast drop inside the
 *   connect-debounce window, which must not kill auto-reconnect)
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor, cleanup } from '@testing-library/svelte';

import ExplorerChat from '../lib/ExplorerChat.svelte';

// ── Mock WebSocket ────────────────────────────────────────────────────────
let sockets;

class MockWebSocket {
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSING = 2;
  static CLOSED = 3;

  constructor(url) {
    this.url = url;
    this.readyState = MockWebSocket.CONNECTING;
    this.sent = [];
    this.send = vi.fn((data) => this.sent.push(JSON.parse(data)));
    this.close = vi.fn(() => { this.readyState = MockWebSocket.CLOSED; });
    this.onopen = null;
    this.onmessage = null;
    this.onclose = null;
    this.onerror = null;
    sockets.push(this);
  }

  // Test helpers: simulate server-side socket events.
  simulateOpen() {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }
  simulateMessage(obj) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
  simulateClose() {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.();
  }
}

// Expected explorer WS endpoint for the default repo, derived from the
// jsdom document base URI the same way the component derives it.
function explorerWsUrl(repoId = 'repo-1') {
  const base = new URL('/api/v1/', document.baseURI);
  return `ws://${base.host}${base.pathname}repos/${repoId}/explorer?ticket=ticket-abc-123`;
}

// Flush pending microtasks ($effect scheduling + awaited fetch in connect()).
async function flushMicrotasks(ticks = 25) {
  for (let i = 0; i < ticks; i++) await Promise.resolve();
}

beforeEach(() => {
  sockets = [];
  global.WebSocket = MockWebSocket;
  localStorage.setItem('gyre_auth_token', 'bearer-token-1');
  // Ticket-exchange endpoint mock: POST /api/v1/ws-ticket with Bearer auth.
  global.fetch = vi.fn(() =>
    Promise.resolve({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ ticket: 'ticket-abc-123' }),
    })
  );
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllTimers();
  vi.restoreAllMocks();
});

// Render the component, wait for the WS to be created, and open it.
async function renderChat(props = {}) {
  const rendered = render(ExplorerChat, {
    props: {
      repoId: 'repo-1',
      onViewQuery: vi.fn(),
      onSavedViewsUpdate: vi.fn(),
      onLoadView: vi.fn(),
      ...props,
    },
  });
  await waitFor(() => expect(sockets.length).toBe(1));
  sockets[0].simulateOpen();
  await waitFor(() => expect(rendered.getByText('Ready')).toBeTruthy());
  return rendered;
}

// ── Connection (§18: WebSocket to /api/v1/repos/:repo_id/explorer) ───────
describe('ExplorerChat connection', () => {
  it('connects to /api/v1/repos/:repo_id/explorer with a ticket obtained via Bearer auth', async () => {
    await renderChat();

    // The Bearer token is exchanged for a single-use ticket (browser WS
    // upgrades cannot carry Authorization headers).
    expect(global.fetch).toHaveBeenCalledTimes(1);
    const [url, opts] = global.fetch.mock.calls[0];
    expect(url).toContain('/api/v1/ws-ticket');
    expect(opts.headers.Authorization).toBe('Bearer bearer-token-1');

    expect(sockets[0].url).toBe(explorerWsUrl());
  });

  it('requests the saved views list when the socket opens', async () => {
    await renderChat();
    const listMsg = sockets[0].sent.find((m) => m.type === 'list_views');
    expect(listMsg).toBeDefined();
  });

  it('shows Connecting... before the socket opens and Ready after', async () => {
    const rendered = render(ExplorerChat, { props: { repoId: 'repo-1' } });
    await waitFor(() => expect(sockets.length).toBe(1));
    // Socket has not fired onopen yet.
    expect(rendered.getByText('Connecting...')).toBeTruthy();
    sockets[0].simulateOpen();
    await waitFor(() => expect(rendered.getByText('Ready')).toBeTruthy());
  });

  it('shows an error and does not connect without an auth token', async () => {
    localStorage.removeItem('gyre_auth_token');
    const rendered = render(ExplorerChat, { props: { repoId: 'repo-1' } });
    await waitFor(() =>
      expect(rendered.getByText(/Not authenticated/i)).toBeTruthy()
    );
    expect(sockets.length).toBe(0);
  });
});

// ── Streaming + status (§18: streamed text, refinement status) ───────────
describe('ExplorerChat streamed responses and status', () => {
  it('renders streamed text incrementally with markdown before done=true', async () => {
    const rendered = await renderChat();
    const { container } = rendered;

    sockets[0].simulateMessage({ type: 'text', content: 'Space is the core ', done: false });
    await waitFor(() => expect(container.textContent).toContain('Space is the core'));
    // Not finalized yet: only the streaming block exists, no settled message.
    expect(container.querySelectorAll('.chat-message:not(.streaming)')).toHaveLength(0);
    expect(container.querySelector('.chat-message.streaming')).toBeTruthy();

    sockets[0].simulateMessage({ type: 'text', content: '**domain entity**.', done: false });
    await waitFor(() =>
      expect(container.innerHTML).toContain('<strong>domain entity</strong>')
    );

    sockets[0].simulateMessage({ type: 'text', content: '', done: true });
    await waitFor(() => expect(container.querySelector('.chat-message')).toBeTruthy());
    // Streaming block is gone; the finalized message keeps markdown rendering.
    expect(container.querySelector('.chat-message.streaming')).toBeFalsy();
    expect(container.innerHTML).toContain('<strong>domain entity</strong>');
  });

  it('displays Thinking..., Refining query..., and Ready from server status messages', async () => {
    const rendered = await renderChat();

    sockets[0].simulateMessage({ type: 'status', status: 'thinking' });
    await waitFor(() => {
      // Shown both in the header status and the thinking indicator.
      expect(rendered.getAllByText('Thinking...').length).toBeGreaterThan(0);
      expect(rendered.getByRole('status')).toBeTruthy();
    });

    sockets[0].simulateMessage({ type: 'status', status: 'refining' });
    await waitFor(() =>
      expect(rendered.getAllByText('Refining query...').length).toBeGreaterThan(0)
    );

    sockets[0].simulateMessage({ type: 'status', status: 'ready' });
    await waitFor(() => expect(rendered.getByText('Ready')).toBeTruthy());
    expect(rendered.queryByRole('status')).toBeFalsy();
  });

  it('shows an error bubble from server error messages', async () => {
    const rendered = await renderChat();
    sockets[0].simulateMessage({ type: 'error', message: 'Agent unavailable.' });
    await waitFor(() => expect(rendered.getByText('Agent unavailable.')).toBeTruthy());
  });
});

// ── View query forwarding (§18: forward view queries to ExplorerCanvas) ──
describe('ExplorerChat view query forwarding', () => {
  it('forwards a valid server view_query to onViewQuery and renders an Apply view button', async () => {
    const onViewQuery = vi.fn();
    const rendered = await renderChat({ onViewQuery });

    const query = { scope: { type: 'focus', node: 'Space', depth: 1 } };
    sockets[0].simulateMessage({
      type: 'view_query',
      query,
      explanation: 'Focusing on Space',
    });

    await waitFor(() => expect(onViewQuery).toHaveBeenCalledTimes(1));
    expect(onViewQuery).toHaveBeenCalledWith(query);
    expect(rendered.getByText('Focusing on Space')).toBeTruthy();
    expect(rendered.getByRole('button', { name: /apply view/i })).toBeTruthy();
  });

  it('rejects an invalid view query instead of applying it to the canvas', async () => {
    const onViewQuery = vi.fn();
    const rendered = await renderChat({ onViewQuery });

    // Unknown scope type fails validateViewQuery.
    sockets[0].simulateMessage({
      type: 'view_query',
      query: { scope: { type: 'bogus_scope' } },
    });

    await waitFor(() =>
      expect(rendered.container.textContent).toContain('invalid')
    );
    // The error detail names the offending scope type.
    expect(rendered.container.textContent).toContain('bogus_scope');
    expect(onViewQuery).not.toHaveBeenCalled();
  });

  it('re-applies a view query from a previous message via the Apply view button', async () => {
    const onViewQuery = vi.fn();
    const rendered = await renderChat({ onViewQuery });

    const query = { scope: { type: 'all' } };
    sockets[0].simulateMessage({ type: 'view_query', query, explanation: 'All nodes' });
    await waitFor(() => expect(onViewQuery).toHaveBeenCalledTimes(1));

    await fireEvent.click(rendered.getByRole('button', { name: /apply view/i }));
    expect(onViewQuery).toHaveBeenCalledTimes(2);
    expect(onViewQuery).toHaveBeenLastCalledWith(query);
  });
});

// ── Outgoing messages (§18: send user messages with canvas state) ────────
describe('ExplorerChat outgoing messages', () => {
  it('sends user messages with the §10 wire canvas_state', async () => {
    const rendered = await renderChat({
      // Internal (camelCase) canvas state shape owned by ExplorerView.
      canvasState: {
        selectedNode: { id: 'n-1', name: 'Space', node_type: 'type', qualified_name: 'domain.Space' },
        zoom: 1.8,
        visibleGroups: ['coordinator', 'domain', 'db'],
        active_filter: 'endpoints',
        active_query: { scope: { type: 'filter', node_types: ['endpoint'] } },
        breadcrumb: [],
      },
    });

    const textarea = rendered.getByRole('textbox');
    await fireEvent.input(textarea, { target: { value: 'What would break if I change Space?' } });
    await fireEvent.click(rendered.getByRole('button', { name: /send/i }));

    const sent = sockets[0].sent.filter((m) => m.type === 'message');
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toBe('What would break if I change Space?');
    // §10 wire format: snake_case fields the server's CanvasState expects.
    expect(sent[0].canvas_state).toEqual({
      zoom_level: 1.8,
      visible_tree_groups: ['coordinator', 'domain', 'db'],
      recent_interactions: [],
      selected_node: { id: 'n-1', name: 'Space', node_type: 'type', qualified_name: 'domain.Space' },
      active_filter: 'endpoints',
      active_query: { scope: { type: 'filter', node_types: ['endpoint'] } },
    });
    // camelCase-only fields must not leak onto the wire.
    expect(sent[0].canvas_state.selectedNode).toBeUndefined();
    expect(sent[0].canvas_state.breadcrumb).toBeUndefined();
  });

  it('renders the sent message as a user bubble', async () => {
    const rendered = await renderChat();
    const textarea = rendered.getByRole('textbox');
    await fireEvent.input(textarea, { target: { value: 'Show me the endpoints' } });
    await fireEvent.click(rendered.getByRole('button', { name: /send/i }));
    await waitFor(() => expect(rendered.getByText('Show me the endpoints')).toBeTruthy());
  });
});

// ── Saved views (§18: dropdown + save button over WebSocket) ─────────────
describe('ExplorerChat saved views', () => {
  const VIEWS = [
    { id: 'v-1', name: 'Endpoints view', created_at: 1700000000000 },
    { id: 'v-2', name: 'Test gaps view', created_at: 1700001000000 },
  ];

  it('lists saved views in the dropdown and sends load_view on selection', async () => {
    const onLoadView = vi.fn();
    const rendered = await renderChat({ savedViews: VIEWS, onLoadView });

    await fireEvent.click(rendered.getByRole('button', { name: /saved views/i }));
    const listbox = rendered.getByRole('listbox');
    expect(listbox.textContent).toContain('Endpoints view');
    expect(listbox.textContent).toContain('Test gaps view');

    await fireEvent.click(rendered.getByText('Endpoints view'));
    const loadMsg = sockets[0].sent.find((m) => m.type === 'load_view');
    expect(loadMsg).toEqual({ type: 'load_view', view_id: 'v-1' });
    expect(onLoadView).toHaveBeenCalledWith(VIEWS[0]);
  });

  it('sends save_view with name, description, and the last view query', async () => {
    const rendered = await renderChat();

    // A view query must exist in the conversation for the save button to enable.
    const query = { scope: { type: 'test_gaps' } };
    sockets[0].simulateMessage({ type: 'view_query', query, explanation: 'Untested functions' });
    await waitFor(() => expect(rendered.getByRole('button', { name: /apply view/i })).toBeTruthy());

    const saveBtn = rendered.getByRole('button', { name: /save this view/i });
    expect(saveBtn.disabled).toBeFalsy();
    await fireEvent.click(saveBtn);

    const input = rendered.getByLabelText(/view name/i);
    await fireEvent.input(input, { target: { value: 'My test gaps' } });
    await fireEvent.click(rendered.getByRole('button', { name: /^Save$/ }));

    const saveMsg = sockets[0].sent.find((m) => m.type === 'save_view');
    expect(saveMsg).toBeDefined();
    expect(saveMsg.name).toBe('My test gaps');
    expect(saveMsg.description).toBe('Untested functions');
    expect(saveMsg.query).toEqual(query);
  });

  it('keeps the save button disabled with no view query in the conversation', async () => {
    const rendered = await renderChat();
    expect(rendered.getByRole('button', { name: /save this view/i }).disabled).toBe(true);
  });

  it('updates saved views from the server views message', async () => {
    const onSavedViewsUpdate = vi.fn();
    await renderChat({ onSavedViewsUpdate });

    sockets[0].simulateMessage({ type: 'views', views: VIEWS });
    await waitFor(() => expect(onSavedViewsUpdate).toHaveBeenCalledWith(VIEWS));
  });
});

// ── Reconnection (§18/AC: reconnection handles disconnects) ──────────────
describe('ExplorerChat reconnection', () => {
  it('shows the reconnect bar when disconnected', async () => {
    const rendered = await renderChat();
    sockets[0].simulateClose();
    await waitFor(() => {
      expect(rendered.getByText('Session ended')).toBeTruthy();
      expect(rendered.getByRole('button', { name: /reconnect/i })).toBeTruthy();
    });
  });

  it('auto-reconnects after a disconnect that lands inside the connect-debounce window', async () => {
    // A socket that drops shortly after connect must not permanently kill
    // auto-reconnect: the reconnect timer fires inside CONNECT_DEBOUNCE_MS,
    // connect() is debounced, and the attempt must be rescheduled rather
    // than silently swallowed.
    vi.useFakeTimers();
    const rendered = render(ExplorerChat, { props: { repoId: 'repo-1' } });
    await flushMicrotasks();
    expect(sockets.length).toBe(1);
    sockets[0].simulateOpen();
    await waitFor(() => expect(rendered.getByText('Ready')).toBeTruthy());

    // Drop the socket immediately (well inside the 2s debounce window).
    sockets[0].simulateClose();
    await waitFor(() => expect(rendered.getByText('Session ended')).toBeTruthy());

    // Backoff timer (1s) fires, but the debounce window is still active —
    // the reconnect must be rescheduled, not dropped.
    await vi.advanceTimersByTimeAsync(1000);
    expect(sockets.length).toBe(1);

    // Debounce window lapses; the next attempt proceeds.
    await vi.advanceTimersByTimeAsync(1100);
    await flushMicrotasks();
    expect(sockets.length).toBe(2);
    expect(sockets[1].url).toBe(explorerWsUrl());
    sockets[1].simulateOpen();
    await waitFor(() => expect(rendered.getByText('Ready')).toBeTruthy());
  });

  it('stops auto-reconnecting after MAX_RECONNECTS attempts', async () => {
    vi.useFakeTimers();
    render(ExplorerChat, { props: { repoId: 'repo-1' } });
    await flushMicrotasks();
    sockets[0].simulateOpen();

    // Five failed reconnect cycles (backoff 1s, 2s, 4s, 8s, 16s),
    // each new socket dropping immediately inside the debounce window.
    for (let i = 0; i < 5; i++) {
      sockets[sockets.length - 1].simulateClose();
      // Advance up to 60s in 1s steps until a real reconnect happens
      // (attempts swallowed by the debounce window are rescheduled).
      for (let step = 0; step < 60 && sockets.length === i + 1; step++) {
        await vi.advanceTimersByTimeAsync(1000);
        await flushMicrotasks();
      }
      expect(sockets.length).toBe(i + 2);
    }

    // Sixth drop: no further automatic attempts.
    const count = sockets.length;
    sockets[sockets.length - 1].simulateClose();
    await vi.advanceTimersByTimeAsync(60000);
    await flushMicrotasks();
    expect(sockets.length).toBe(count);
  });
});
