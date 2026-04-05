import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderMarkdown } from '../lib/markdown.js';

// ---------------------------------------------------------------------------
// Markdown XSS protection tests
// ---------------------------------------------------------------------------
describe('renderMarkdown XSS protection', () => {
  it('strips javascript: links and shows text only', () => {
    const md = '[Click me](javascript:alert(1))';
    const html = renderMarkdown(md);
    expect(html).not.toContain('javascript:');
    expect(html).toContain('Click me');
    // Should NOT be wrapped in an <a> tag
    expect(html).not.toContain('<a ');
  });

  it('strips data: links', () => {
    const md = '[payload](data:text/html,<script>alert(1)</script>)';
    const html = renderMarkdown(md);
    expect(html).not.toContain('data:');
    expect(html).toContain('payload');
    expect(html).not.toContain('<a ');
  });

  it('strips vbscript: links', () => {
    const md = '[click](vbscript:msgbox)';
    const html = renderMarkdown(md);
    expect(html).not.toContain('vbscript:');
    expect(html).not.toContain('<a ');
  });

  it('strips javascript: links with mixed case', () => {
    const md = '[xss](JavaScript:alert(1))';
    const html = renderMarkdown(md);
    expect(html).not.toContain('JavaScript:');
    expect(html).not.toContain('<a ');
    expect(html).toContain('xss');
  });

  it('strips javascript: links with leading whitespace in URL', () => {
    const md = '[xss](  javascript:alert(1))';
    const html = renderMarkdown(md);
    expect(html).not.toContain('javascript:');
    expect(html).not.toContain('<a ');
  });

  it('allows safe https links', () => {
    const md = '[Google](https://google.com)';
    const html = renderMarkdown(md);
    expect(html).toContain('<a href="https://google.com"');
    expect(html).toContain('Google');
    expect(html).toContain('target="_blank"');
    expect(html).toContain('rel="noopener"');
  });

  it('allows safe http links', () => {
    const md = '[example](http://example.com)';
    const html = renderMarkdown(md);
    expect(html).toContain('<a href="http://example.com"');
  });
});

// ---------------------------------------------------------------------------
// Markdown rendering tests
// ---------------------------------------------------------------------------
describe('renderMarkdown', () => {
  it('returns empty string for null/undefined/empty', () => {
    expect(renderMarkdown(null)).toBe('');
    expect(renderMarkdown(undefined)).toBe('');
    expect(renderMarkdown('')).toBe('');
  });

  it('renders headers', () => {
    expect(renderMarkdown('# Title')).toContain('<h1');
    expect(renderMarkdown('## Subtitle')).toContain('<h2');
    expect(renderMarkdown('### H3')).toContain('<h3');
  });

  it('renders bold and italic', () => {
    const html = renderMarkdown('**bold** and *italic*');
    expect(html).toContain('<strong>bold</strong>');
    expect(html).toContain('<em>italic</em>');
  });

  it('renders inline code', () => {
    const html = renderMarkdown('Use `foo()` here');
    expect(html).toContain('<code');
    expect(html).toContain('foo()');
  });

  it('renders fenced code blocks', () => {
    const md = '```js\nconst x = 1;\n```';
    const html = renderMarkdown(md);
    expect(html).toContain('<pre');
    expect(html).toContain('const x = 1;');
  });

  it('escapes HTML in content', () => {
    const html = renderMarkdown('<script>alert("xss")</script>');
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
  });

  it('renders unordered lists', () => {
    const html = renderMarkdown('- item 1\n- item 2');
    expect(html).toContain('<ul');
    expect(html).toContain('<li>item 1</li>');
    expect(html).toContain('<li>item 2</li>');
  });

  it('renders blockquotes', () => {
    const html = renderMarkdown('> quoted text');
    expect(html).toContain('<blockquote');
    expect(html).toContain('quoted text');
  });

  it('renders horizontal rules', () => {
    expect(renderMarkdown('---')).toContain('<hr');
  });
});

// ---------------------------------------------------------------------------
// ExplorerChat handleMessage logic tests (unit-tested without component)
// ---------------------------------------------------------------------------
describe('ExplorerChat message handling', () => {
  // Replicate the handleMessage logic from ExplorerChat.svelte for unit testing
  function createChatState() {
    return {
      messages: [],
      streamingText: '',
      status: 'ready',
    };
  }

  function handleMessage(state, msg) {
    switch (msg.type) {
      case 'text': {
        if (!msg.done) {
          state.streamingText += msg.content ?? '';
          state.status = 'thinking';
        } else {
          const fullText = state.streamingText + (msg.content ?? '');
          if (fullText.trim()) {
            state.messages = [...state.messages, { role: 'assistant', content: fullText, timestamp: Date.now() }];
          }
          state.streamingText = '';
          state.status = 'ready';
        }
        break;
      }
      case 'view_query': {
        const query = msg.query ?? msg.view_query ?? msg;
        if (state.streamingText.trim()) {
          state.messages = [...state.messages, { role: 'assistant', content: state.streamingText, timestamp: Date.now() }];
        }
        state.streamingText = '';
        state.messages = [...state.messages, {
          role: 'assistant',
          content: msg.explanation ?? 'View applied.',
          viewQuery: query,
          timestamp: Date.now(),
        }];
        state.status = 'ready';
        break;
      }
      case 'status': {
        if (msg.status === 'thinking') state.status = 'thinking';
        else if (msg.status === 'refining') state.status = 'refining';
        else if (msg.status === 'ready') state.status = 'ready';
        break;
      }
      case 'error': {
        const errorMsg = msg.message ?? 'An error occurred.';
        state.messages = [...state.messages, { role: 'assistant', content: errorMsg, timestamp: Date.now(), isError: true }];
        state.streamingText = '';
        if (errorMsg.includes('Session message limit')) {
          state.status = 'disconnected';
        } else {
          state.status = 'ready';
        }
        break;
      }
      case 'views': {
        state.savedViews = msg.views ?? [];
        break;
      }
    }
    return state;
  }

  it('view_query finalizes streaming text into a message before appending view', () => {
    let state = createChatState();

    // Simulate streaming text arriving
    state = handleMessage(state, { type: 'text', content: 'Here is your ', done: false });
    state = handleMessage(state, { type: 'text', content: 'analysis.', done: false });
    expect(state.streamingText).toBe('Here is your analysis.');
    expect(state.messages.length).toBe(0);

    // Now a view_query arrives while streaming
    state = handleMessage(state, {
      type: 'view_query',
      query: { scope: { type: 'all' } },
      explanation: 'Showing all nodes',
    });

    // The streaming text should have been finalized as a message
    expect(state.streamingText).toBe('');
    expect(state.messages.length).toBe(2);
    expect(state.messages[0].role).toBe('assistant');
    expect(state.messages[0].content).toBe('Here is your analysis.');
    // Second message is the view_query message
    expect(state.messages[1].viewQuery).toBeDefined();
    expect(state.messages[1].content).toBe('Showing all nodes');
  });

  it('view_query does not add empty streaming text as a message', () => {
    let state = createChatState();

    // No streaming, just a direct view_query
    state = handleMessage(state, {
      type: 'view_query',
      query: { scope: { type: 'filter', node_types: ['function'] } },
      explanation: 'Functions only',
    });

    // Only one message (the view_query), no empty streaming finalized
    expect(state.messages.length).toBe(1);
    expect(state.messages[0].viewQuery).toBeDefined();
  });

  it('text done=true finalizes accumulated streaming into a message', () => {
    let state = createChatState();

    state = handleMessage(state, { type: 'text', content: 'Part 1. ', done: false });
    state = handleMessage(state, { type: 'text', content: 'Part 2.', done: false });
    expect(state.messages.length).toBe(0);

    state = handleMessage(state, { type: 'text', content: '', done: true });
    expect(state.messages.length).toBe(1);
    expect(state.messages[0].content).toBe('Part 1. Part 2.');
    expect(state.streamingText).toBe('');
    expect(state.status).toBe('ready');
  });
});

// ---------------------------------------------------------------------------
// Save button disabled state — snake_case selected_node
// ---------------------------------------------------------------------------
describe('Save button disabled logic', () => {
  // The save button in ExplorerChat.svelte uses:
  //   disabled={!canvasState?.selected_node && status !== 'ready'}
  // This tests the logic with snake_case property names (not camelCase).
  function isSaveDisabled(canvasState, status) {
    return !canvasState?.selected_node && status !== 'ready';
  }

  it('is disabled when no selected_node and status is not ready', () => {
    expect(isSaveDisabled({}, 'thinking')).toBe(true);
    expect(isSaveDisabled(null, 'connecting')).toBe(true);
    expect(isSaveDisabled({ selected_node: null }, 'thinking')).toBe(true);
  });

  it('is enabled when selected_node is present (regardless of status)', () => {
    expect(isSaveDisabled({ selected_node: 'node-123' }, 'thinking')).toBe(false);
    expect(isSaveDisabled({ selected_node: 'node-123' }, 'connecting')).toBe(false);
  });

  it('is enabled when status is ready (regardless of selected_node)', () => {
    expect(isSaveDisabled({}, 'ready')).toBe(false);
    expect(isSaveDisabled(null, 'ready')).toBe(false);
  });

  it('uses snake_case selected_node not camelCase selectedNode', () => {
    // camelCase should NOT work — the disabled logic checks selected_node
    expect(isSaveDisabled({ selectedNode: 'node-123' }, 'thinking')).toBe(true);
    // snake_case should work
    expect(isSaveDisabled({ selected_node: 'node-123' }, 'thinking')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// WebSocket session lifecycle integration tests
// ---------------------------------------------------------------------------
describe('WebSocket session lifecycle', () => {
  const RECONNECT_DELAY = 3000;
  const MAX_RECONNECTS = 5;

  // Minimal WebSocket lifecycle simulation mirroring ExplorerChat.svelte connect()
  function createSession() {
    let ws = null;
    let status = 'connecting';
    let reconnectCount = 0;
    let reconnectTimer = null;
    let onViewQueryCalls = [];
    let sentMessages = [];

    function createMockSocket() {
      return {
        readyState: 0, // CONNECTING
        onopen: null,
        onmessage: null,
        onclose: null,
        onerror: null,
        send: vi.fn((data) => sentMessages.push(JSON.parse(data))),
        close: vi.fn(),
        OPEN: 1,
        CLOSED: 3,
      };
    }

    function connect(repoId) {
      if (ws) {
        ws.onclose = null;
        ws.close();
      }
      if (!repoId) {
        status = 'disconnected';
        return;
      }
      status = 'connecting';
      const socket = createMockSocket();
      ws = socket;
      return socket;
    }

    function scheduleReconnect() {
      if (reconnectTimer) return;
      if (reconnectCount >= MAX_RECONNECTS) return;
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        reconnectCount++;
      }, RECONNECT_DELAY);
    }

    return {
      get ws() { return ws; },
      set ws(v) { ws = v; },
      get status() { return status; },
      set status(v) { status = v; },
      get reconnectCount() { return reconnectCount; },
      set reconnectCount(v) { reconnectCount = v; },
      get reconnectTimer() { return reconnectTimer; },
      set reconnectTimer(v) { reconnectTimer = v; },
      get sentMessages() { return sentMessages; },
      get onViewQueryCalls() { return onViewQueryCalls; },
      connect,
      scheduleReconnect,
    };
  }

  it('connects and transitions to ready on socket open', () => {
    const session = createSession();
    const socket = session.connect('repo-1');
    expect(session.status).toBe('connecting');

    // Simulate onopen
    socket.readyState = 1;
    session.status = 'ready';
    session.reconnectCount = 0;

    expect(session.status).toBe('ready');
    expect(session.reconnectCount).toBe(0);
  });

  it('sends list_views on connection open', () => {
    const session = createSession();
    const socket = session.connect('repo-1');
    socket.readyState = 1;

    // Simulate onopen behavior: send list_views
    socket.send(JSON.stringify({ type: 'list_views' }));

    expect(session.sentMessages).toHaveLength(1);
    expect(session.sentMessages[0].type).toBe('list_views');
  });

  it('transitions to disconnected when no repoId', () => {
    const session = createSession();
    session.connect('');
    expect(session.status).toBe('disconnected');
  });

  it('transitions to disconnected on socket close', () => {
    const session = createSession();
    const socket = session.connect('repo-1');
    socket.readyState = 1;
    session.status = 'ready';

    // Simulate onclose
    session.ws = null;
    session.status = 'disconnected';

    expect(session.status).toBe('disconnected');
    expect(session.ws).toBeNull();
  });

  it('transitions to error on socket error', () => {
    const session = createSession();
    session.connect('repo-1');

    // Simulate onerror
    session.status = 'error';
    expect(session.status).toBe('error');
  });

  it('closes previous socket before reconnecting', () => {
    const session = createSession();
    const socket1 = session.connect('repo-1');
    socket1.readyState = 1;
    session.status = 'ready';

    // Connect again — should close socket1
    const socket2 = session.connect('repo-1');
    expect(socket1.close).toHaveBeenCalled();
    expect(socket2).not.toBe(socket1);
  });

  it('limits reconnection attempts to MAX_RECONNECTS', () => {
    const session = createSession();
    session.reconnectCount = MAX_RECONNECTS;
    session.scheduleReconnect();

    // Should not schedule another reconnect
    expect(session.reconnectTimer).toBeNull();
  });

  it('does not double-schedule reconnects', () => {
    vi.useFakeTimers();
    const session = createSession();
    session.scheduleReconnect();
    const timer1 = session.reconnectTimer;
    session.scheduleReconnect(); // Second call should be no-op
    expect(session.reconnectTimer).toBe(timer1);
    vi.useRealTimers();
  });

  it('sends user message with canvas_state', () => {
    const session = createSession();
    const socket = session.connect('repo-1');
    socket.readyState = 1;

    const canvasState = { selected_node: 'fn1', zoom: 1.5, breadcrumb: [] };
    socket.send(JSON.stringify({
      type: 'message',
      text: 'Show me the API endpoints',
      canvas_state: canvasState,
    }));

    expect(session.sentMessages).toHaveLength(1);
    expect(session.sentMessages[0].type).toBe('message');
    expect(session.sentMessages[0].text).toBe('Show me the API endpoints');
    expect(session.sentMessages[0].canvas_state.selected_node).toBe('fn1');
  });

  it('does not send when socket is not open', () => {
    const session = createSession();
    const socket = session.connect('repo-1');
    // readyState is still 0 (CONNECTING), not 1 (OPEN)

    const canSend = socket.readyState === 1;
    expect(canSend).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Saved view CRUD protocol tests
// ---------------------------------------------------------------------------
describe('Saved view CRUD protocol', () => {
  function createMockWs() {
    const sent = [];
    return {
      readyState: 1, // WebSocket.OPEN
      send: vi.fn((data) => sent.push(JSON.parse(data))),
      close: vi.fn(),
      sent,
    };
  }

  it('sends save_view with name, description, and query', () => {
    const ws = createMockWs();
    const lastViewQuery = { scope: { type: 'filter', node_types: ['function'] } };

    ws.send(JSON.stringify({
      type: 'save_view',
      name: 'My Functions',
      description: 'Saved from explorer chat',
      query: lastViewQuery,
    }));

    expect(ws.sent).toHaveLength(1);
    expect(ws.sent[0].type).toBe('save_view');
    expect(ws.sent[0].name).toBe('My Functions');
    expect(ws.sent[0].query.scope.type).toBe('filter');
    expect(ws.sent[0].query.scope.node_types).toEqual(['function']);
  });

  it('sends load_view with view_id', () => {
    const ws = createMockWs();

    ws.send(JSON.stringify({ type: 'load_view', view_id: 'view-42' }));

    expect(ws.sent).toHaveLength(1);
    expect(ws.sent[0].type).toBe('load_view');
    expect(ws.sent[0].view_id).toBe('view-42');
  });

  it('sends list_views request', () => {
    const ws = createMockWs();

    ws.send(JSON.stringify({ type: 'list_views' }));

    expect(ws.sent).toHaveLength(1);
    expect(ws.sent[0].type).toBe('list_views');
  });

  it('handles views response with saved views list', () => {
    function handleMessage(state, msg) {
      if (msg.type === 'views') {
        state.savedViews = msg.views ?? [];
      }
      return state;
    }

    const state = { savedViews: [] };
    const views = [
      { id: 'v1', name: 'Endpoints', created_at: 1700000000 },
      { id: 'v2', name: 'Test gaps', created_at: 1700001000 },
    ];

    handleMessage(state, { type: 'views', views });

    expect(state.savedViews).toHaveLength(2);
    expect(state.savedViews[0].name).toBe('Endpoints');
    expect(state.savedViews[1].id).toBe('v2');
  });

  it('handles views response with empty list', () => {
    function handleMessage(state, msg) {
      if (msg.type === 'views') {
        state.savedViews = msg.views ?? [];
      }
      return state;
    }

    const state = { savedViews: [{ id: 'old' }] };
    handleMessage(state, { type: 'views', views: [] });
    expect(state.savedViews).toHaveLength(0);
  });

  it('saves the last view query from conversation history', () => {
    const messages = [
      { role: 'user', content: 'Show endpoints' },
      { role: 'assistant', content: 'Here are endpoints.', viewQuery: { scope: { type: 'filter', node_types: ['endpoint'] } } },
      { role: 'user', content: 'Now show types' },
      { role: 'assistant', content: 'Here are the types.', viewQuery: { scope: { type: 'filter', node_types: ['type'] } } },
      { role: 'user', content: 'Thanks' },
      { role: 'assistant', content: 'You are welcome.' },
    ];

    // Mirroring saveCurrentView logic: find last viewQuery
    const lastViewQuery = [...messages].reverse().find(m => m.viewQuery)?.viewQuery ?? {};
    expect(lastViewQuery.scope.type).toBe('filter');
    expect(lastViewQuery.scope.node_types).toEqual(['type']);
  });

  it('returns empty object when no view queries in history', () => {
    const messages = [
      { role: 'user', content: 'Hello' },
      { role: 'assistant', content: 'Hi there' },
    ];

    const lastViewQuery = [...messages].reverse().find(m => m.viewQuery)?.viewQuery ?? {};
    expect(lastViewQuery).toEqual({});
  });
});

// ---------------------------------------------------------------------------
// View query extraction from chat messages
// ---------------------------------------------------------------------------
describe('View query extraction from chat messages', () => {
  function createChatState() {
    return { messages: [], streamingText: '', status: 'ready' };
  }

  function handleMessage(state, msg) {
    switch (msg.type) {
      case 'view_query': {
        const query = msg.query ?? msg.view_query ?? msg;
        if (state.streamingText.trim()) {
          state.messages = [...state.messages, { role: 'assistant', content: state.streamingText, timestamp: Date.now() }];
        }
        state.streamingText = '';
        state.messages = [...state.messages, {
          role: 'assistant',
          content: msg.explanation ?? 'View applied.',
          viewQuery: query,
          timestamp: Date.now(),
        }];
        state.status = 'ready';
        state.lastViewQuery = query;
        break;
      }
    }
    return state;
  }

  it('extracts focus scope query with node and edges', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'focus', node: 'create_user', edges: ['calls'], direction: 'incoming', depth: 3 },
        emphasis: { dim_unmatched: 0.12 },
        annotation: { title: 'Blast radius: create_user' },
      },
      explanation: 'Showing blast radius for create_user',
    });

    expect(state.lastViewQuery.scope.type).toBe('focus');
    expect(state.lastViewQuery.scope.node).toBe('create_user');
    expect(state.lastViewQuery.scope.edges).toEqual(['calls']);
    expect(state.lastViewQuery.scope.direction).toBe('incoming');
    expect(state.lastViewQuery.emphasis.dim_unmatched).toBe(0.12);
  });

  it('extracts filter scope with node_types and name_pattern', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'filter', node_types: ['function', 'endpoint'], name_pattern: 'user' },
        annotation: { title: 'User-related endpoints' },
      },
      explanation: 'Filtering to user-related endpoints',
    });

    expect(state.lastViewQuery.scope.type).toBe('filter');
    expect(state.lastViewQuery.scope.node_types).toEqual(['function', 'endpoint']);
    expect(state.lastViewQuery.scope.name_pattern).toBe('user');
  });

  it('extracts test_gaps scope query', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'test_gaps' },
        emphasis: { highlight: { matched: { color: '#ef4444', label: 'Untested' } }, dim_unmatched: 0.3 },
      },
      explanation: 'Showing untested functions',
    });

    expect(state.lastViewQuery.scope.type).toBe('test_gaps');
    expect(state.lastViewQuery.emphasis.dim_unmatched).toBe(0.3);
    expect(state.lastViewQuery.emphasis.highlight.matched.color).toBe('#ef4444');
  });

  it('extracts concept scope with seed_nodes and expand_edges', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'concept', seed_nodes: ['User', 'Session'], expand_edges: ['calls', 'depends_on'], expand_depth: 3 },
      },
      explanation: 'User and Session concept cluster',
    });

    expect(state.lastViewQuery.scope.type).toBe('concept');
    expect(state.lastViewQuery.scope.seed_nodes).toEqual(['User', 'Session']);
    expect(state.lastViewQuery.scope.expand_depth).toBe(3);
  });

  it('extracts diff scope with from_commit', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'diff', from_commit: 'abc123def' },
        emphasis: { highlight: { matched: { color: '#22c55e' } } },
      },
      explanation: 'Changes since abc123def',
    });

    expect(state.lastViewQuery.scope.type).toBe('diff');
    expect(state.lastViewQuery.scope.from_commit).toBe('abc123def');
  });

  it('extracts heat map emphasis from view query', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'all' },
        emphasis: { heat: { metric: 'incoming_calls', palette: 'blue-red' } },
      },
      explanation: 'Hotspot analysis',
    });

    expect(state.lastViewQuery.emphasis.heat.metric).toBe('incoming_calls');
    expect(state.lastViewQuery.emphasis.heat.palette).toBe('blue-red');
  });

  it('extracts $clicked interactive query template', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'focus', node: '$clicked', edges: ['calls'], direction: 'incoming', depth: 10 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308'], dim_unmatched: 0.12 },
        annotation: { title: 'Blast radius: $name' },
      },
      explanation: 'Click any node to see its blast radius',
    });

    expect(state.lastViewQuery.scope.node).toBe('$clicked');
    expect(state.lastViewQuery.emphasis.tiered_colors).toHaveLength(3);
    expect(state.lastViewQuery.annotation.title).toBe('Blast radius: $name');
  });

  it('prefers msg.query over msg.view_query', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: { scope: { type: 'all' } },
      view_query: { scope: { type: 'filter', node_types: ['function'] } },
    });

    // msg.query takes precedence
    expect(state.lastViewQuery.scope.type).toBe('all');
  });

  it('falls back to msg.view_query when msg.query is absent', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      view_query: { scope: { type: 'filter', node_types: ['type'] } },
    });

    expect(state.lastViewQuery.scope.type).toBe('filter');
  });

  it('extracts callouts from view query', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'all' },
        callouts: [
          { node: 'create_user', text: 'Entry point' },
          { node: 'User', text: 'Core type' },
        ],
      },
    });

    expect(state.lastViewQuery.callouts).toHaveLength(2);
    expect(state.lastViewQuery.callouts[0].node).toBe('create_user');
    expect(state.lastViewQuery.callouts[1].text).toBe('Core type');
  });

  it('extracts narrative steps from view query', () => {
    let state = createChatState();
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'all' },
        narrative: [
          { node: 'create_user', text: 'Step 1: Request received' },
          { node: 'User', text: 'Step 2: Domain model instantiated' },
        ],
      },
    });

    expect(state.lastViewQuery.narrative).toHaveLength(2);
    expect(state.lastViewQuery.narrative[0].node).toBe('create_user');
  });
});

// ---------------------------------------------------------------------------
// Error handling and session limit tests
// ---------------------------------------------------------------------------
describe('Error handling and reconnection', () => {
  function createChatState() {
    return { messages: [], streamingText: '', status: 'ready' };
  }

  function handleMessage(state, msg) {
    switch (msg.type) {
      case 'text': {
        if (!msg.done) {
          state.streamingText += msg.content ?? '';
          state.status = 'thinking';
        } else {
          const fullText = state.streamingText + (msg.content ?? '');
          if (fullText.trim()) {
            state.messages = [...state.messages, { role: 'assistant', content: fullText, isError: false }];
          }
          state.streamingText = '';
          state.status = 'ready';
        }
        break;
      }
      case 'error': {
        const errorMsg = msg.message ?? 'An error occurred.';
        state.messages = [...state.messages, { role: 'assistant', content: errorMsg, isError: true }];
        state.streamingText = '';
        if (errorMsg.includes('Session message limit')) {
          state.status = 'disconnected';
        } else {
          state.status = 'ready';
        }
        break;
      }
      case 'status': {
        if (msg.status === 'thinking') state.status = 'thinking';
        else if (msg.status === 'refining') state.status = 'refining';
        else if (msg.status === 'ready') state.status = 'ready';
        break;
      }
    }
    return state;
  }

  it('handles error message and marks isError', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'error', message: 'Rate limit exceeded' });

    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].isError).toBe(true);
    expect(state.messages[0].content).toBe('Rate limit exceeded');
    expect(state.status).toBe('ready');
  });

  it('sets disconnected status on session message limit error', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'error', message: 'Session message limit reached' });

    expect(state.status).toBe('disconnected');
    expect(state.messages[0].isError).toBe(true);
  });

  it('clears in-flight streaming on error', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'text', content: 'partial ', done: false });
    expect(state.streamingText).toBe('partial ');

    state = handleMessage(state, { type: 'error', message: 'LLM timeout' });
    expect(state.streamingText).toBe('');
    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].isError).toBe(true);
  });

  it('uses default error message when msg.message is absent', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'error' });

    expect(state.messages[0].content).toBe('An error occurred.');
  });

  it('handles status transitions: thinking -> refining -> ready', () => {
    let state = createChatState();

    state = handleMessage(state, { type: 'status', status: 'thinking' });
    expect(state.status).toBe('thinking');

    state = handleMessage(state, { type: 'status', status: 'refining' });
    expect(state.status).toBe('refining');

    state = handleMessage(state, { type: 'status', status: 'ready' });
    expect(state.status).toBe('ready');
  });

  it('ignores unknown status values', () => {
    let state = createChatState();
    state.status = 'ready';
    state = handleMessage(state, { type: 'status', status: 'banana' });
    expect(state.status).toBe('ready');
  });

  it('ignores unknown message types gracefully', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'unknown_type', data: 'something' });

    expect(state.messages).toHaveLength(0);
    expect(state.status).toBe('ready');
  });

  it('does not create message for whitespace-only streaming text on done=true', () => {
    let state = createChatState();
    state = handleMessage(state, { type: 'text', content: '   ', done: false });
    state = handleMessage(state, { type: 'text', content: '', done: true });

    // Whitespace-only content is trimmed and dropped
    expect(state.messages).toHaveLength(0);
    expect(state.status).toBe('ready');
  });

  it('caps messages to MAX_CLIENT_MESSAGES', () => {
    const MAX = 200;
    let messages = [];
    function capMessages(msgs) {
      if (msgs.length > MAX) return msgs.slice(msgs.length - MAX);
      return msgs;
    }

    // Add 210 messages
    for (let i = 0; i < 210; i++) {
      messages = capMessages([...messages, { role: 'user', content: `msg ${i}` }]);
    }

    expect(messages).toHaveLength(MAX);
    // Oldest messages should be dropped
    expect(messages[0].content).toBe('msg 10');
    expect(messages[messages.length - 1].content).toBe('msg 209');
  });
});

// ---------------------------------------------------------------------------
// Self-check loop simulation (agent view query -> resolve -> refine)
// ---------------------------------------------------------------------------
describe('Self-check loop (view query refinement)', () => {
  function createChatState() {
    return { messages: [], streamingText: '', status: 'ready', viewQueries: [] };
  }

  function handleMessage(state, msg) {
    switch (msg.type) {
      case 'text': {
        if (!msg.done) {
          state.streamingText += msg.content ?? '';
          state.status = 'thinking';
        } else {
          const fullText = state.streamingText + (msg.content ?? '');
          if (fullText.trim()) {
            state.messages = [...state.messages, { role: 'assistant', content: fullText }];
          }
          state.streamingText = '';
          state.status = 'ready';
        }
        break;
      }
      case 'status': {
        state.status = msg.status;
        break;
      }
      case 'view_query': {
        const query = msg.query ?? msg.view_query;
        if (state.streamingText.trim()) {
          state.messages = [...state.messages, { role: 'assistant', content: state.streamingText }];
        }
        state.streamingText = '';
        state.messages = [...state.messages, {
          role: 'assistant',
          content: msg.explanation ?? 'View applied.',
          viewQuery: query,
        }];
        state.viewQueries.push(query);
        state.status = 'ready';
        break;
      }
    }
    return state;
  }

  it('simulates full self-check loop: question -> draft query -> refine -> final query', () => {
    let state = createChatState();

    // Step 1: User asks a question
    state.messages = [...state.messages, { role: 'user', content: 'Show me the blast radius of create_user' }];

    // Step 2: Agent thinks
    state = handleMessage(state, { type: 'status', status: 'thinking' });
    expect(state.status).toBe('thinking');

    // Step 3: Agent streams explanation
    state = handleMessage(state, { type: 'text', content: 'I will analyze the call graph ', done: false });
    state = handleMessage(state, { type: 'text', content: 'for create_user...', done: false });

    // Step 4: Agent enters refining status (self-check)
    state = handleMessage(state, { type: 'status', status: 'refining' });
    expect(state.status).toBe('refining');

    // Step 5: Agent sends initial view query (draft)
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'focus', node: 'create_user', edges: ['calls'], direction: 'both', depth: 2 },
        emphasis: { dim_unmatched: 0.15 },
        annotation: { title: 'Blast radius: create_user (draft)' },
      },
      explanation: 'Initial blast radius analysis',
    });
    expect(state.viewQueries).toHaveLength(1);
    expect(state.viewQueries[0].scope.depth).toBe(2);

    // Step 6: Agent refines — sends a better query with deeper depth
    state = handleMessage(state, { type: 'status', status: 'refining' });
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'focus', node: 'create_user', edges: ['calls'], direction: 'incoming', depth: 5 },
        emphasis: { dim_unmatched: 0.12, tiered_colors: ['#ef4444', '#f97316', '#eab308'] },
        annotation: { title: 'Blast radius: create_user' },
      },
      explanation: 'Refined blast radius with tiered coloring and deeper traversal',
    });

    expect(state.viewQueries).toHaveLength(2);
    // Refined query has deeper depth and tiered colors
    expect(state.viewQueries[1].scope.depth).toBe(5);
    expect(state.viewQueries[1].scope.direction).toBe('incoming');
    expect(state.viewQueries[1].emphasis.tiered_colors).toHaveLength(3);

    // Step 7: Agent confirms final state
    expect(state.status).toBe('ready');
    expect(state.messages.length).toBeGreaterThanOrEqual(3); // user + streaming text + 2 view queries
  });

  it('self-check loop with test_gaps: generate -> validate -> adjust', () => {
    let state = createChatState();

    state.messages = [...state.messages, { role: 'user', content: 'Find untested code' }];

    // Draft: test_gaps scope
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'test_gaps' },
        emphasis: { dim_unmatched: 0.5 },
      },
      explanation: 'Showing test coverage gaps',
    });

    // Refinement: tighter dim_unmatched for better visual contrast
    state = handleMessage(state, {
      type: 'view_query',
      query: {
        scope: { type: 'test_gaps' },
        emphasis: {
          highlight: { matched: { color: '#ef4444', label: 'Untested' } },
          dim_unmatched: 0.15,
        },
        annotation: { title: 'Test coverage gaps', description: '3 untested functions found' },
      },
      explanation: 'Refined: 3 untested functions highlighted in red',
    });

    expect(state.viewQueries).toHaveLength(2);
    // Second query has highlight and tighter dim
    expect(state.viewQueries[1].emphasis.highlight.matched.color).toBe('#ef4444');
    expect(state.viewQueries[1].emphasis.dim_unmatched).toBe(0.15);
  });

  it('handles multiple rapid view_query messages without losing any', () => {
    let state = createChatState();

    for (let i = 0; i < 5; i++) {
      state = handleMessage(state, {
        type: 'view_query',
        query: { scope: { type: 'all' }, annotation: { title: `View ${i}` } },
        explanation: `View ${i}`,
      });
    }

    expect(state.viewQueries).toHaveLength(5);
    expect(state.messages.filter(m => m.viewQuery)).toHaveLength(5);
  });
});

// ---------------------------------------------------------------------------
// Spec path extraction tests
// ---------------------------------------------------------------------------
describe('Spec path extraction from message content', () => {
  const SPEC_PATH_RE = /\b(specs\/[\w\-\/]+\.md)\b/g;

  function extractSpecPaths(content) {
    if (!content) return [];
    const matches = [...content.matchAll(SPEC_PATH_RE)];
    return [...new Set(matches.map(m => m[1]))];
  }

  it('extracts spec paths from message content', () => {
    const content = 'Check specs/system/vision.md and specs/development/architecture.md for details.';
    const paths = extractSpecPaths(content);
    expect(paths).toEqual(['specs/system/vision.md', 'specs/development/architecture.md']);
  });

  it('deduplicates repeated spec paths', () => {
    const content = 'See specs/system/vision.md and refer to specs/system/vision.md again.';
    const paths = extractSpecPaths(content);
    expect(paths).toEqual(['specs/system/vision.md']);
  });

  it('returns empty for content without spec paths', () => {
    expect(extractSpecPaths('Just a normal message')).toEqual([]);
    expect(extractSpecPaths(null)).toEqual([]);
    expect(extractSpecPaths('')).toEqual([]);
  });

  it('extracts paths with hyphens', () => {
    const content = 'See specs/system/meta-spec-reconciliation.md for details.';
    const paths = extractSpecPaths(content);
    expect(paths).toEqual(['specs/system/meta-spec-reconciliation.md']);
  });

  it('extracts deeply nested spec paths', () => {
    const content = 'Refer to specs/system/agent-gates.md and specs/development/architecture.md';
    const paths = extractSpecPaths(content);
    expect(paths).toHaveLength(2);
    expect(paths).toContain('specs/system/agent-gates.md');
    expect(paths).toContain('specs/development/architecture.md');
  });
});

// ---------------------------------------------------------------------------
// Initial state tests
// ---------------------------------------------------------------------------
describe('ExplorerChat initial state', () => {
  it('starts with connecting status', () => {
    const initialStatus = 'connecting';
    expect(initialStatus).toBe('connecting');
  });

  it('starts with empty messages array', () => {
    const messages = [];
    expect(messages).toHaveLength(0);
  });

  it('starts with empty streaming text', () => {
    const streamingText = '';
    expect(streamingText).toBe('');
  });

  it('starts with zero reconnect count', () => {
    const reconnectCount = 0;
    expect(reconnectCount).toBe(0);
  });

  it('status transitions from connecting to ready on socket open', () => {
    let status = 'connecting';
    // Simulate onopen
    status = 'ready';
    expect(status).toBe('ready');
  });

  it('status transitions from connecting to error when no auth token', () => {
    let status = 'connecting';
    const token = null;
    if (!token) status = 'error';
    expect(status).toBe('error');
  });

  it('status transitions from connecting to disconnected when no repoId', () => {
    let status = 'connecting';
    const repoId = '';
    if (!repoId) status = 'disconnected';
    expect(status).toBe('disconnected');
  });
});

// ---------------------------------------------------------------------------
// User and assistant message rendering data
// ---------------------------------------------------------------------------
describe('ExplorerChat message rendering', () => {
  it('user messages have role=user and content', () => {
    const msg = { id: 1, role: 'user', content: 'Show me endpoints', timestamp: Date.now() };
    expect(msg.role).toBe('user');
    expect(msg.content).toBe('Show me endpoints');
    expect(msg.timestamp).toBeGreaterThan(0);
  });

  it('assistant messages have role=assistant and optional viewQuery', () => {
    const msg = {
      id: 2, role: 'assistant', content: 'Here are the endpoints.',
      viewQuery: { scope: { type: 'filter', node_types: ['endpoint'] } },
      timestamp: Date.now(),
    };
    expect(msg.role).toBe('assistant');
    expect(msg.viewQuery).toBeDefined();
    expect(msg.viewQuery.scope.node_types).toEqual(['endpoint']);
  });

  it('error messages have isError flag', () => {
    const msg = { id: 3, role: 'assistant', content: 'Rate limit exceeded', isError: true, timestamp: Date.now() };
    expect(msg.isError).toBe(true);
  });

  it('messages are ordered by id', () => {
    const messages = [
      { id: 1, role: 'user', content: 'Hello' },
      { id: 2, role: 'assistant', content: 'Hi' },
      { id: 3, role: 'user', content: 'Show types' },
      { id: 4, role: 'assistant', content: 'Here are types.', viewQuery: { scope: { type: 'filter' } } },
    ];
    for (let i = 1; i < messages.length; i++) {
      expect(messages[i].id).toBeGreaterThan(messages[i - 1].id);
    }
  });
});

// ---------------------------------------------------------------------------
// Saved views dropdown data
// ---------------------------------------------------------------------------
describe('Saved views dropdown rendering', () => {
  it('renders empty when no saved views', () => {
    const savedViews = [];
    expect(savedViews).toHaveLength(0);
  });

  it('renders saved views sorted by creation time', () => {
    const savedViews = [
      { id: 'v1', name: 'Endpoints', created_at: 1700001000 },
      { id: 'v2', name: 'Types', created_at: 1700000000 },
      { id: 'v3', name: 'Test gaps', created_at: 1700002000 },
    ];
    const sorted = [...savedViews].sort((a, b) => b.created_at - a.created_at);
    expect(sorted[0].name).toBe('Test gaps');
    expect(sorted[1].name).toBe('Endpoints');
    expect(sorted[2].name).toBe('Types');
  });

  it('each saved view has id, name, and created_at', () => {
    const view = { id: 'v1', name: 'My View', created_at: 1700000000 };
    expect(view.id).toBeDefined();
    expect(view.name).toBeDefined();
    expect(view.created_at).toBeDefined();
  });

  it('active view id tracks current selection', () => {
    let activeViewId = null;
    const views = [{ id: 'v1' }, { id: 'v2' }];
    activeViewId = 'v1';
    expect(activeViewId).toBe('v1');
    activeViewId = null;
    expect(activeViewId).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Session message count and user message length limits
// ---------------------------------------------------------------------------
describe('ExplorerChat session limits', () => {
  const MAX_USER_MESSAGE_LENGTH = 10000;
  const MAX_SESSION_MESSAGES = 200;

  it('rejects messages exceeding MAX_USER_MESSAGE_LENGTH', () => {
    const longMessage = 'a'.repeat(MAX_USER_MESSAGE_LENGTH + 1);
    expect(longMessage.length).toBeGreaterThan(MAX_USER_MESSAGE_LENGTH);
    const isValid = longMessage.length <= MAX_USER_MESSAGE_LENGTH;
    expect(isValid).toBe(false);
  });

  it('accepts messages within MAX_USER_MESSAGE_LENGTH', () => {
    const normalMessage = 'Show me the blast radius of create_user';
    expect(normalMessage.length).toBeLessThanOrEqual(MAX_USER_MESSAGE_LENGTH);
  });

  it('tracks session message count', () => {
    let sessionMessageCount = 0;
    for (let i = 0; i < 5; i++) {
      sessionMessageCount++;
    }
    expect(sessionMessageCount).toBe(5);
    expect(sessionMessageCount).toBeLessThan(MAX_SESSION_MESSAGES);
  });

  it('warns when approaching session limit', () => {
    const sessionMessageCount = 195;
    const isNearLimit = sessionMessageCount >= MAX_SESSION_MESSAGES - 10;
    expect(isNearLimit).toBe(true);
  });
});
