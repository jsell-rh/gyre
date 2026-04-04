<script>
  import { onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { renderMarkdown } from './markdown.js';

  let {
    repoId = '',
    canvasState = {},
    onViewQuery = () => {},
    onOpenSpec = null,
    savedViews = [],
    onSavedViewsUpdate = () => {},
  } = $props();

  // ── Spec path detection ─────────────────────────────────────────────────
  // Extracts spec paths (specs/.../*.md) from message content
  const SPEC_PATH_RE = /\b(specs\/[\w\-\/]+\.md)\b/g;

  function extractSpecPaths(content) {
    if (!content) return [];
    const matches = [...content.matchAll(SPEC_PATH_RE)];
    // Deduplicate
    return [...new Set(matches.map(m => m[1]))];
  }

  // ── Constants ────────────────────────────────────────────────────────────
  const AUTH_TOKEN_KEY = 'gyre_auth_token';
  const RECONNECT_DELAY = 3000;
  const MAX_RECONNECTS = 5;
  const MAX_CLIENT_MESSAGES = 200;

  // ── State ────────────────────────────────────────────────────────────────
  let messages = $state([]); // [{ role: 'user'|'assistant', content: string, viewQuery?: object, timestamp: number }]

  /** Cap the messages array to MAX_CLIENT_MESSAGES, keeping the newest entries. */
  function capMessages(msgs) {
    if (msgs.length > MAX_CLIENT_MESSAGES) {
      return msgs.slice(msgs.length - MAX_CLIENT_MESSAGES);
    }
    return msgs;
  }

  let inputText = $state('');
  let status = $state('connecting'); // 'connecting' | 'ready' | 'thinking' | 'refining' | 'error' | 'disconnected'
  let ws = $state(null);
  let reconnectCount = $state(0);
  let reconnectTimer = null;
  let messagesEl = $state(null);
  let inputEl = $state(null);
  let streamingText = $state('');
  let savedViewsDropdownOpen = $state(false);

  // ── Auth ─────────────────────────────────────────────────────────────────
  function getAuthToken() {
    return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
  }

  // ── WebSocket connection ─────────────────────────────────────────────────
  function connect() {
    if (ws) {
      ws.onclose = null;
      ws.close();
    }

    if (!repoId) {
      status = 'disconnected';
      return;
    }

    status = 'connecting';
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const token = getAuthToken();
    const url = `${protocol}//${window.location.host}/api/v1/repos/${repoId}/explorer?token=${encodeURIComponent(token)}`;
    const socket = new WebSocket(url);

    socket.onopen = () => {
      // Auth is handled via HTTP header (Bearer token in cookie/header),
      // no separate auth message needed.
      // Request saved views list.
      socket.send(JSON.stringify({ type: 'list_views' }));
      status = 'ready';
      reconnectCount = 0;
    };

    socket.onmessage = (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return;
      }
      handleMessage(msg);
    };

    socket.onclose = () => {
      ws = null;
      if (status !== 'error') {
        status = 'disconnected';
      }
      scheduleReconnect();
    };

    socket.onerror = () => {
      status = 'error';
    };

    ws = socket;
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    if (reconnectCount >= MAX_RECONNECTS) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      reconnectCount++;
      connect();
    }, RECONNECT_DELAY);
  }

  function handleMessage(msg) {
    switch (msg.type) {
      case 'text': {
        if (!msg.done) {
          // Streaming chunk: accumulate text
          streamingText += msg.content ?? '';
          status = 'thinking';
        } else {
          // Final text message (done=true)
          const fullText = streamingText + (msg.content ?? '');
          if (fullText.trim()) {
            messages = capMessages([...messages, { role: 'assistant', content: fullText, timestamp: Date.now() }]);
          }
          streamingText = '';
          status = 'ready';
          scrollToBottom();
        }
        break;
      }
      case 'view_query': {
        const query = msg.query ?? msg.view_query ?? msg;
        // Finalize any in-flight streaming text before clearing
        if (streamingText.trim()) {
          messages = capMessages([...messages, { role: 'assistant', content: streamingText, timestamp: Date.now() }]);
        }
        streamingText = '';
        // Add as assistant message with view query
        messages = capMessages([...messages, {
          role: 'assistant',
          content: msg.explanation ?? $t('explorer_chat.view_applied'),
          viewQuery: query,
          timestamp: Date.now(),
        }]);
        status = 'ready';
        onViewQuery(query);
        scrollToBottom();
        break;
      }
      case 'status': {
        if (msg.status === 'thinking') status = 'thinking';
        else if (msg.status === 'refining') status = 'refining';
        else if (msg.status === 'ready') status = 'ready';
        break;
      }
      case 'views': {
        // Notify parent via callback to avoid Svelte 5 prop shadow
        if (msg.views) {
          onSavedViewsUpdate(msg.views);
        }
        break;
      }
      case 'error': {
        const errorMsg = msg.message ?? $t('explorer_chat.error_occurred');
        messages = capMessages([...messages, { role: 'assistant', content: errorMsg, timestamp: Date.now(), isError: true }]);
        streamingText = '';
        // Session limit reached — mark as disconnected so user knows to reconnect
        if (errorMsg.includes('Session message limit')) {
          status = 'disconnected';
          if (ws) { ws.onclose = null; ws.close(); ws = null; }
        } else {
          status = 'ready';
        }
        scrollToBottom();
        break;
      }
      default:
        break;
    }
  }

  // ── Send message ─────────────────────────────────────────────────────────
  function sendMessage() {
    if (!inputText.trim() || !ws || ws.readyState !== WebSocket.OPEN) return;

    const text = inputText.trim();
    inputText = '';

    messages = capMessages([...messages, { role: 'user', content: text, timestamp: Date.now() }]);
    scrollToBottom();

    ws.send(JSON.stringify({
      type: 'message',
      text: text,
      canvas_state: canvasState,
    }));

    status = 'thinking';
  }

  // ── Saved views ──────────────────────────────────────────────────────────
  function loadView(view) {
    savedViewsDropdownOpen = false;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: 'load_view', view_id: view.id }));
  }

  function saveCurrentView() {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    const name = prompt($t('explorer_chat.save_view_name'));
    if (!name?.trim()) return;
    // Find the last view query from conversation
    const lastViewQuery = [...messages].reverse().find(m => m.viewQuery)?.viewQuery ?? {};
    ws.send(JSON.stringify({
      type: 'save_view',
      name: name.trim(),
      description: `Saved from explorer chat`,
      query: lastViewQuery,
    }));
  }

  // ── Keyboard ─────────────────────────────────────────────────────────────
  function onInputKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    } else if (e.key === 'Escape') {
      if (inputText) {
        inputText = '';
      } else {
        // Blur so the global Escape handler can cascade
        inputEl?.blur();
      }
    }
  }

  // ── Spec link click delegation ────────────────────────────────────────────
  function onMessagesClick(e) {
    const specLink = e.target.closest('.md-spec-link');
    if (specLink) {
      e.preventDefault();
      const specPath = specLink.dataset.specPath;
      if (specPath && onOpenSpec) {
        onOpenSpec(specPath);
      }
    }
  }

  // ── Scroll ───────────────────────────────────────────────────────────────
  function scrollToBottom() {
    requestAnimationFrame(() => {
      if (messagesEl) {
        messagesEl.scrollTop = messagesEl.scrollHeight;
      }
    });
  }

  // ── Timestamp formatting ─────────────────────────────────────────────────
  function formatTime(ts) {
    if (!ts) return '';
    const d = new Date(ts);
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }

  // ── Status display ───────────────────────────────────────────────────────
  let statusLabel = $derived.by(() => {
    switch (status) {
      case 'connecting':   return $t('explorer_chat.status_connecting');
      case 'thinking':     return $t('explorer_chat.status_thinking');
      case 'refining':     return $t('explorer_chat.status_refining');
      case 'ready':        return $t('explorer_chat.status_ready');
      case 'error':        return $t('explorer_chat.status_error');
      case 'disconnected': return $t('explorer_chat.status_disconnected');
      default:             return '';
    }
  });

  let statusColor = $derived.by(() => {
    switch (status) {
      case 'ready':        return 'var(--color-success)';
      case 'thinking':
      case 'refining':     return 'var(--color-warning)';
      case 'error':        return 'var(--color-danger)';
      default:             return 'var(--color-text-muted)';
    }
  });

  // ── Lifecycle ────────────────────────────────────────────────────────────
  $effect(() => {
    const id = repoId;
    if (id) {
      connect();
    }
    return () => {
      if (ws) { ws.onclose = null; ws.close(); ws = null; }
      if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
    };
  });

  onDestroy(() => {
    if (ws) { ws.onclose = null; ws.close(); }
    if (reconnectTimer) clearTimeout(reconnectTimer);
  });
</script>

<div class="chat-container">
  <!-- Header with status and saved views -->
  <div class="chat-header">
    <div class="chat-header-left">
      <span class="chat-title">{$t('explorer_chat.title')}</span>
      <span class="chat-status" style="--status-color: {statusColor}">
        <span class="status-dot" aria-hidden="true"></span>
        {statusLabel}
      </span>
    </div>
    <div class="chat-header-right">
      <!-- Saved views dropdown -->
      <div class="saved-views-wrap">
        <button
          class="saved-views-btn"
          onclick={() => { savedViewsDropdownOpen = !savedViewsDropdownOpen; }}
          aria-expanded={savedViewsDropdownOpen}
          aria-haspopup="listbox"
          type="button"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <path d="M19 21l-7-5-7 5V5a2 2 0 012-2h10a2 2 0 012 2z"/>
          </svg>
          {$t('explorer_chat.saved_views')}
        </button>
        {#if savedViewsDropdownOpen}
          <div class="saved-views-dropdown" role="listbox" aria-label={$t('explorer_chat.saved_views')}>
            {#if savedViews.length === 0}
              <div class="saved-views-empty">{$t('explorer_chat.no_saved_views')}</div>
            {:else}
              {#each savedViews as view}
                <button
                  class="saved-view-item"
                  role="option"
                  aria-selected={false}
                  onclick={() => loadView(view)}
                  type="button"
                >
                  <span class="saved-view-name">{view.name}</span>
                  {#if view.created_at}
                    <span class="saved-view-date">{formatTime(view.created_at)}</span>
                  {/if}
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      <!-- Save current view button -->
      <button
        class="save-view-btn"
        onclick={saveCurrentView}
        disabled={status !== 'ready' || !messages.some(m => m.viewQuery)}
        title={$t('explorer_chat.save_this_view')}
        aria-label={$t('explorer_chat.save_this_view')}
        type="button"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
          <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
          <polyline points="17 21 17 13 7 13 7 21"/>
          <polyline points="7 3 7 8 15 8"/>
        </svg>
      </button>
    </div>
  </div>

  <!-- Messages area -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="chat-messages" bind:this={messagesEl} role="log" aria-live="polite" onclick={onMessagesClick}>
    {#if messages.length === 0 && !streamingText}
      <div class="chat-welcome">
        <div class="welcome-icon" aria-hidden="true">
          <svg viewBox="0 0 48 48" fill="none" width="40" height="40">
            <circle cx="24" cy="24" r="18" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 3"/>
            <path d="M16 20h16M16 28h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" opacity="0.6"/>
          </svg>
        </div>
        <p class="welcome-title">{$t('explorer_chat.welcome_title')}</p>
        <p class="welcome-desc">{$t('explorer_chat.welcome_desc')}</p>
        <div class="welcome-suggestions">
          {#each [
            { key: 'explorer_chat.suggestion_endpoints', fallback: 'What are the main API endpoints?' },
            { key: 'explorer_chat.suggestion_dependencies', fallback: 'Show me the dependency graph' },
            { key: 'explorer_chat.suggestion_complexity', fallback: 'Which types are most complex?' },
          ] as suggestion}
            <button
              class="suggestion-btn"
              onclick={() => { inputText = $t(suggestion.key, { default: suggestion.fallback }); inputEl?.focus(); }}
              type="button"
            >{$t(suggestion.key, { default: suggestion.fallback })}</button>
          {/each}
        </div>
      </div>
    {:else}
      {#each messages as msg, i (i)}
        <div class="chat-message {msg.role}" class:error={msg.isError}>
          <div class="message-meta">
            <span class="message-role">{msg.role === 'user' ? $t('explorer_chat.you') : $t('explorer_chat.assistant')}</span>
            <span class="message-time">{formatTime(msg.timestamp)}</span>
          </div>
          <div class="message-content">
            {#if msg.role === 'assistant'}
              {@html renderMarkdown(msg.content)}
            {:else}
              <p>{msg.content}</p>
            {/if}
          </div>
          {#if msg.viewQuery}
            <button
              class="apply-view-btn"
              onclick={() => onViewQuery(msg.viewQuery)}
              type="button"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
                <circle cx="12" cy="12" r="3"/>
              </svg>
              {$t('explorer_chat.apply_view')}
            </button>
          {/if}
          {#if msg.role === 'assistant' && onOpenSpec}
            {@const specPaths = extractSpecPaths(msg.content)}
            {#if specPaths.length > 0}
              <div class="message-spec-refs">
                <span class="spec-refs-label">Specs mentioned:</span>
                {#each specPaths as path}
                  <button
                    class="spec-ref-btn"
                    onclick={() => onOpenSpec(path)}
                    title="Open {path} in spec editor"
                    type="button"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="11" height="11" aria-hidden="true">
                      <path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/>
                      <path d="M18.5 2.5a2.121 2.121 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>
                    </svg>
                    {path.split('/').pop()}
                  </button>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/each}

      <!-- Streaming text (in progress) -->
      {#if streamingText}
        <div class="chat-message assistant streaming">
          <div class="message-meta">
            <span class="message-role">{$t('explorer_chat.assistant')}</span>
            <span class="streaming-indicator">
              <span class="dot"></span>
              <span class="dot"></span>
              <span class="dot"></span>
            </span>
          </div>
          <div class="message-content">
            {@html renderMarkdown(streamingText)}
          </div>
        </div>
      {/if}
    {/if}

    <!-- Status indicator while thinking -->
    {#if status === 'thinking' && !streamingText}
      <div class="thinking-indicator" role="status">
        <span class="thinking-dots">
          <span class="dot"></span>
          <span class="dot"></span>
          <span class="dot"></span>
        </span>
        <span class="thinking-label">{statusLabel}</span>
      </div>
    {/if}
  </div>

  <!-- Input area -->
  <div class="chat-input-area">
    <textarea
      bind:this={inputEl}
      class="chat-input"
      placeholder={$t('explorer_chat.input_placeholder')}
      bind:value={inputText}
      onkeydown={onInputKeydown}
      disabled={status === 'connecting' || status === 'disconnected'}
      rows="1"
      aria-label={$t('explorer_chat.input_placeholder')}
    ></textarea>
    <button
      class="send-btn"
      onclick={sendMessage}
      disabled={!inputText.trim() || status === 'connecting' || status === 'disconnected' || (ws?.readyState !== WebSocket.OPEN)}
      aria-label={$t('explorer_chat.send')}
      type="button"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <line x1="22" y1="2" x2="11" y2="13"/>
        <polygon points="22 2 15 22 11 13 2 9 22 2"/>
      </svg>
    </button>
  </div>
</div>

<!-- Click-away for dropdown -->
{#if savedViewsDropdownOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="click-away" onclick={() => { savedViewsDropdownOpen = false; }}></div>
{/if}

<style>
  .chat-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    overflow: hidden;
  }

  /* ── Header ──────────────────────────────────────────────────────── */
  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    gap: var(--space-2);
  }

  .chat-header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .chat-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .chat-status {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-full);
    background: var(--status-color);
    flex-shrink: 0;
  }

  .chat-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  /* ── Saved views ─────────────────────────────────────────────────── */
  .saved-views-wrap {
    position: relative;
  }

  .saved-views-btn, .save-view-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: border-color var(--transition-fast);
    white-space: nowrap;
  }

  .saved-views-btn:hover, .save-view-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }

  .saved-views-btn:focus-visible, .save-view-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .save-view-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .saved-views-dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: var(--space-1);
    min-width: 200px;
    max-height: 240px;
    overflow-y: auto;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px color-mix(in srgb, black 40%, transparent);
    z-index: 100;
    padding: var(--space-1) 0;
  }

  .saved-views-empty {
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
    font-style: italic;
  }

  .saved-view-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .saved-view-item:hover {
    background: var(--color-surface);
  }

  .saved-view-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .saved-view-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .saved-view-date {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
    margin-left: var(--space-2);
  }

  .click-away {
    position: fixed;
    inset: 0;
    z-index: 50;
  }

  /* ── Messages ────────────────────────────────────────────────────── */
  .chat-messages {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .chat-welcome {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: var(--space-8) var(--space-4);
    gap: var(--space-3);
    flex: 1;
  }

  .welcome-icon {
    color: var(--color-text-muted);
    opacity: 0.6;
  }

  .welcome-title {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .welcome-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
    max-width: 300px;
    line-height: 1.5;
  }

  .welcome-suggestions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-2);
    width: 100%;
    max-width: 300px;
  }

  .suggestion-btn {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    cursor: pointer;
    text-align: left;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .suggestion-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }

  .suggestion-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Message bubbles ─────────────────────────────────────────────── */
  .chat-message {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .chat-message.user {
    align-items: flex-end;
  }

  .chat-message.user .message-content {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius) var(--radius) 0 var(--radius);
    max-width: 85%;
  }

  .chat-message.assistant .message-content {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius) var(--radius) var(--radius) 0;
    max-width: 95%;
  }

  .chat-message.error .message-content {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
    color: var(--color-danger);
  }

  .message-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-1);
  }

  .message-role {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .message-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .message-content {
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    color: var(--color-text);
    line-height: 1.6;
    word-break: break-word;
  }

  .message-content p {
    margin: 0;
  }

  .message-content :global(code) {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--color-text) 8%, transparent);
    padding: 1px 4px;
    border-radius: 3px;
  }

  .message-content :global(pre) {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    overflow-x: auto;
    font-size: var(--text-xs);
    margin: var(--space-2) 0;
  }

  .message-content :global(a) {
    color: var(--color-link);
    text-decoration: underline;
  }

  .message-content :global(a.md-spec-link) {
    color: var(--color-warning);
    text-decoration: none;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 1px 4px;
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 25%, transparent);
    border-radius: 3px;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .message-content :global(a.md-spec-link:hover) {
    background: color-mix(in srgb, var(--color-warning) 20%, transparent);
    border-color: var(--color-warning);
  }

  .message-content :global(strong) {
    font-weight: 600;
    color: var(--color-text);
  }

  .apply-view-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
    align-self: flex-start;
    margin-top: var(--space-1);
  }

  .apply-view-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .apply-view-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Spec path references ────────────────────────────────────────── */
  .message-spec-refs {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    margin-top: var(--space-1);
  }

  .spec-refs-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .spec-ref-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--space-2);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-warning);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    white-space: nowrap;
  }

  .spec-ref-btn:hover {
    background: color-mix(in srgb, var(--color-warning) 20%, transparent);
    border-color: var(--color-warning);
  }

  .spec-ref-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Streaming indicator ─────────────────────────────────────────── */
  .streaming-indicator, .thinking-dots {
    display: inline-flex;
    gap: 3px;
    align-items: center;
  }

  .dot {
    width: 4px;
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--color-text-muted);
    animation: dotPulse 1.4s infinite ease-in-out both;
  }

  .dot:nth-child(2) { animation-delay: 0.16s; }
  .dot:nth-child(3) { animation-delay: 0.32s; }

  @keyframes dotPulse {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
    40% { opacity: 1; transform: scale(1); }
  }

  .thinking-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .thinking-label {
    font-size: var(--text-xs);
  }

  /* ── Input area ──────────────────────────────────────────────────── */
  .chat-input-area {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .chat-input {
    flex: 1;
    min-height: 36px;
    max-height: 100px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    resize: none;
    outline: none;
    transition: border-color var(--transition-fast);
    line-height: 1.4;
  }

  .chat-input::placeholder {
    color: var(--color-text-muted);
  }

  .chat-input:focus {
    border-color: var(--color-focus);
    box-shadow: 0 0 0 2px var(--color-focus);
  }

  .chat-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .send-btn {
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-primary, #3b82f6);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .send-btn:hover:not(:disabled) {
    opacity: 0.85;
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .send-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .dot { animation: none; opacity: 0.6; }
    .suggestion-btn, .saved-views-btn, .save-view-btn,
    .saved-view-item, .apply-view-btn, .chat-input, .send-btn {
      transition: none;
    }
  }
</style>
