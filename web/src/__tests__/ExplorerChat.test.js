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
