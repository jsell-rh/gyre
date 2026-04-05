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
  const RECONNECT_BASE_DELAY = 1000;
  const MAX_RECONNECTS = 5;
  const MAX_CLIENT_MESSAGES = 200;
  // Align with server MAX_USER_MESSAGE_LENGTH — enforce client-side to avoid
  // composing a long message only to have it rejected after a round trip.
  const MAX_USER_MESSAGE_LENGTH = 10000;
  // Align with server MAX_SESSION_MESSAGES — tracks all client-sent messages
  // (not just rendered bubbles) to warn users before hitting the server limit.
  const MAX_SESSION_MESSAGES = 200;
  // Server keeps MAX_CONVERSATION_HISTORY=20 messages (20 user + 20 assistant = 40 total)
  // in the LLM's context window. Older messages are summarized (lossy).
  const CONTEXT_WINDOW_SIZE = 40;

  // ── State ────────────────────────────────────────────────────────────────
  let messages = $state([]); // [{ id: number, role: 'user'|'assistant', content: string, viewQuery?: object, timestamp: number }]
  let nextMsgId = 0;
  let sessionMessageCount = $state(0); // Tracks all messages sent to server this session

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
  let streamingTimeout = null; // Auto-finalize after 65s of no chunks (server LLM timeout is 60s)
  let saveViewInputOpen = $state(false);
  let saveViewInputValue = $state('');
  let savedViewsDropdownOpen = $state(false);
  let activeViewId = $state(null);

  // ── Auth ─────────────────────────────────────────────────────────────────
  function getAuthToken() {
    const token = localStorage.getItem(AUTH_TOKEN_KEY);
    if (!token) {
      // No auth token found — return null to trigger "please log in" UX
      // instead of silently using a hardcoded dev token in production.
      return null;
    }
    return token;
  }

  // ── WebSocket connection ─────────────────────────────────────────────────
  async function connect() {
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
    if (!token) {
      status = 'error';
      messages = capMessages([...messages, {
        id: nextMsgId++, role: 'assistant',
        content: 'Not authenticated. Please log in to use the explorer.',
        timestamp: Date.now(),
      }]);
      return;
    }
    // Ticket-based WebSocket auth: exchange Bearer token for a short-lived,
    // single-use ticket to avoid leaking the real token in WebSocket URLs
    // (which appear in server logs, proxy logs, and browser history).
    const base = new URL('/api/v1/', document.baseURI || window.location.href);
    let ticket;
    try {
      const ticketResp = await fetch(`${base.href}ws-ticket`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!ticketResp.ok) throw new Error(`Ticket request failed: ${ticketResp.status}`);
      const ticketData = await ticketResp.json();
      ticket = ticketData.ticket;
    } catch (err) {
      console.error('[ExplorerChat] Failed to obtain WS ticket:', err);
      status = 'error';
      messages = capMessages([...messages, {
        id: nextMsgId++, role: 'assistant',
        content: 'Failed to authenticate WebSocket connection. Please try again.',
        timestamp: Date.now(),
      }]);
      return;
    }
    const wsBase = `${protocol}//${base.host}${base.pathname}`;
    const url = `${wsBase}repos/${repoId}/explorer?ticket=${encodeURIComponent(ticket)}`;
    const socket = new WebSocket(url);

    socket.onopen = () => {
      // Request saved views list.
      socket.send(JSON.stringify({ type: 'list_views' }));
      status = 'ready';
      reconnectCount = 0;
      sessionMessageCount = 0; // Reset on fresh connection
    };

    socket.onmessage = (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch (e) {
        console.warn('[ExplorerChat] Malformed WebSocket JSON:', e.message);
        // Surface parse error to user instead of silently dropping
        messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: '*Received malformed response from server. The explorer may need to reconnect.*', timestamp: Date.now(), isError: true }]);
        return;
      }
      handleMessage(msg);
    };

    socket.onclose = () => {
      ws = null;
      // Clear streaming timeout to prevent stale "[Response timed out]" injection
      // into a future reconnected session.
      if (streamingTimeout) { clearTimeout(streamingTimeout); streamingTimeout = null; }
      // Finalize any orphaned streaming text on disconnect
      // (prevents indefinite accumulation if server crashes mid-stream)
      if (streamingText.trim()) {
        messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: streamingText, timestamp: Date.now() }]);
        streamingText = '';
      }
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
    if (reconnectCount >= MAX_RECONNECTS) {
      status = 'error';
      return;
    }
    // Exponential backoff: 1s, 2s, 4s, 8s, 16s
    const delay = RECONNECT_BASE_DELAY * Math.pow(2, reconnectCount);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      reconnectCount++;
      connect();
    }, delay);
  }

  // Manual reconnect for session limit or error states
  function manualReconnect() {
    if (ws) { ws.onclose = null; ws.close(); ws = null; }
    reconnectCount = 0;
    messages = [];
    streamingText = '';
    status = 'connecting';
    connect();
  }

  function handleMessage(msg) {
    switch (msg.type) {
      case 'text': {
        if (!msg.done) {
          // Streaming chunk: accumulate text (coerce to string for safety)
          const chunk = typeof msg.content === 'string' ? msg.content : String(msg.content ?? '');
          streamingText += chunk;
          status = 'thinking';
          // Reset streaming timeout: auto-finalize if no chunks for 65s
          // (server-side LLM timeout is 60s; 65s = server timeout + 5s buffer)
          if (streamingTimeout) clearTimeout(streamingTimeout);
          streamingTimeout = setTimeout(() => {
            if (streamingText.trim()) {
              messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: streamingText + '\n\n*[Response timed out]*', timestamp: Date.now() }]);
              streamingText = '';
              status = 'ready';
              scrollToBottom();
            }
          }, 65000);
        } else {
          // Final text message (done=true)
          if (streamingTimeout) { clearTimeout(streamingTimeout); streamingTimeout = null; }
          const doneChunk = typeof msg.content === 'string' ? msg.content : String(msg.content ?? '');
          const fullText = streamingText + doneChunk;
          if (fullText.trim()) {
            messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: fullText, timestamp: Date.now() }]);
          }
          streamingText = '';
          status = 'ready';
          scrollToBottom();
        }
        break;
      }
      case 'view_query': {
        const query = msg.query ?? msg.view_query;
        if (!query || typeof query !== 'object' || !query.scope) break;
        // Finalize any in-flight streaming text before clearing
        if (streamingText.trim()) {
          messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: streamingText, timestamp: Date.now() }]);
        }
        streamingText = '';
        // Add as assistant message with view query
        messages = capMessages([...messages, {
          id: nextMsgId++,
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
        messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: errorMsg, timestamp: Date.now(), isError: true }]);
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
        console.warn('[ExplorerChat] Unknown message type:', msg.type);
        break;
    }
  }

  // ── Send message ─────────────────────────────────────────────────────────
  function sendMessage() {
    if (!inputText.trim() || !ws || ws.readyState !== WebSocket.OPEN) return;

    // Client-side message length enforcement (mirrors server MAX_USER_MESSAGE_LENGTH)
    if (inputText.length > MAX_USER_MESSAGE_LENGTH) {
      messages = capMessages([...messages, {
        id: nextMsgId++, role: 'assistant',
        content: `*Message too long (${inputText.length.toLocaleString()} / ${MAX_USER_MESSAGE_LENGTH.toLocaleString()} characters). Please shorten your message.*`,
        timestamp: Date.now(), isError: true,
      }]);
      return;
    }

    // Warn before hitting server session limit
    if (sessionMessageCount >= MAX_SESSION_MESSAGES - 5) {
      const remaining = MAX_SESSION_MESSAGES - sessionMessageCount;
      if (remaining <= 0) {
        messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: '*Session message limit reached. Please reconnect for a fresh session.*', timestamp: Date.now() }]);
        return;
      }
      // Show a soft warning at 5 messages remaining
      messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: `*${remaining} messages remaining in this session.*`, timestamp: Date.now() }]);
    }

    const text = inputText.trim();
    inputText = '';

    messages = capMessages([...messages, { id: nextMsgId++, role: 'user', content: text, timestamp: Date.now() }]);
    scrollToBottom();

    try {
      ws.send(JSON.stringify({
        type: 'message',
        text: text,
        canvas_state: canvasState,
      }));
      // Increment AFTER successful send so failed sends don't count toward
      // the session limit (which would eventually block the client).
      sessionMessageCount++;
      status = 'thinking';
    } catch (err) {
      console.error('[ExplorerChat] WebSocket send failed:', err);
      messages = capMessages([...messages, { id: nextMsgId++, role: 'assistant', content: '*Failed to send message. Please check your connection.*', timestamp: Date.now(), isError: true }]);
    }
  }

  // ── Saved views ──────────────────────────────────────────────────────────
  function loadView(view) {
    savedViewsDropdownOpen = false;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    activeViewId = view.id;
    ws.send(JSON.stringify({ type: 'load_view', view_id: view.id }));
  }

  function saveCurrentView() {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    // Show inline input instead of blocking prompt()
    saveViewInputValue = '';
    saveViewInputOpen = true;
  }

  function confirmSaveView() {
    const name = saveViewInputValue.trim();
    saveViewInputOpen = false;
    saveViewInputValue = '';
    if (!name || !ws || ws.readyState !== WebSocket.OPEN) return;
    // Find the last view query from conversation, or fall back to the active canvas query
    const lastViewQuery = [...messages].reverse().find(m => m.viewQuery)?.viewQuery
      ?? canvasState?.active_query
      ?? null;
    if (!lastViewQuery || (typeof lastViewQuery === 'object' && Object.keys(lastViewQuery).length === 0)) {
      return; // Nothing to save
    }
    ws.send(JSON.stringify({
      type: 'save_view',
      name: name,
      description: `Saved from explorer chat`,
      query: lastViewQuery,
    }));
  }

  function cancelSaveView() {
    saveViewInputOpen = false;
    saveViewInputValue = '';
  }

  function onSaveViewKeydown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      confirmSaveView();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelSaveView();
    }
  }

  function deleteView(view, event) {
    event.stopPropagation();
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (view.is_system) return; // Cannot delete system views
    ws.send(JSON.stringify({ type: 'delete_view', view_id: view.id }));
    // Optimistically remove from local list
    onSavedViewsUpdate(savedViews.filter(v => v.id !== view.id));
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
  // Index of the first message that is still in the LLM's context window.
  // Messages before this index are "out of context" (summarized server-side).
  let contextCutoffIndex = $derived(Math.max(0, messages.length - CONTEXT_WINDOW_SIZE));

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

  // Keyboard handler for Escape to dismiss dropdown (WCAG 2.1)
  function onKeyDown(e) {
    if (e.key === 'Escape' && savedViewsDropdownOpen) {
      savedViewsDropdownOpen = false;
      e.stopPropagation();
    }
  }

  // Auto-focus save view input when dialog opens
  $effect(() => {
    if (saveViewInputOpen) {
      requestAnimationFrame(() => {
        const el = document.getElementById('save-view-input');
        el?.focus();
      });
    }
  });

  $effect(() => {
    if (savedViewsDropdownOpen) {
      document.addEventListener('keydown', onKeyDown);
      return () => document.removeEventListener('keydown', onKeyDown);
    }
  });

  onDestroy(() => {
    if (ws && ws.readyState !== WebSocket.CLOSED && ws.readyState !== WebSocket.CLOSING) {
      ws.onclose = null;
      ws.close();
    }
    ws = null;
    if (reconnectTimer) clearTimeout(reconnectTimer);
    if (streamingTimeout) clearTimeout(streamingTimeout);
    // Safety: $effect cleanup handles this, but onDestroy may fire first in edge cases
    document.removeEventListener('keydown', onKeyDown);
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
                <div class="saved-view-row">
                  <button
                    class="saved-view-item"
                    role="option"
                    aria-selected={activeViewId === view.id}
                    onclick={() => loadView(view)}
                    type="button"
                  >
                    <span class="saved-view-name">{view.name}</span>
                    {#if view.created_at}
                      <span class="saved-view-date">{formatTime(view.created_at)}</span>
                    {/if}
                  </button>
                  {#if !view.is_system}
                    <button class="saved-view-delete" onclick={(e) => deleteView(view, e)} type="button"
                      title="Delete view" aria-label="Delete view {view.name}">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12">
                        <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                      </svg>
                    </button>
                  {/if}
                </div>
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
      {#each messages as msg, i (msg.id ?? msg.timestamp + '-' + i)}
        {#if contextCutoffIndex > 0 && i === contextCutoffIndex}
          <div class="context-divider" role="separator">
            <span class="context-divider-text">Messages above this line are summarized in the AI's memory</span>
          </div>
        {/if}
        <div class="chat-message {msg.role}" class:error={msg.isError} class:out-of-context={i < contextCutoffIndex}>
          {#if i < contextCutoffIndex}
            <span class="out-of-context-label">out of context</span>
          {/if}
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
                    {path.split('/').pop()} <span class="spec-ref-action">Edit in Explorer</span>
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

  <!-- Reconnect bar (shown when disconnected or error) -->
  {#if status === 'disconnected' || status === 'error'}
    <div class="reconnect-bar">
      <span class="reconnect-text">{status === 'disconnected' ? 'Session ended' : 'Connection lost'}</span>
      <button class="reconnect-btn" onclick={manualReconnect} type="button">Reconnect</button>
    </div>
  {/if}

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
    {#if inputText.length > MAX_USER_MESSAGE_LENGTH * 0.8}
      <span class="char-counter" class:over-limit={inputText.length > MAX_USER_MESSAGE_LENGTH}>
        {inputText.length.toLocaleString()} / {MAX_USER_MESSAGE_LENGTH.toLocaleString()}
      </span>
    {/if}
    <button
      class="send-btn"
      onclick={sendMessage}
      disabled={!inputText.trim() || status === 'connecting' || status === 'disconnected' || (ws?.readyState !== WebSocket.OPEN) || inputText.length > MAX_USER_MESSAGE_LENGTH}
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

<!-- Save view inline input overlay -->
{#if saveViewInputOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="save-view-overlay" onclick={cancelSaveView}>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="save-view-dialog" onclick={(e) => e.stopPropagation()}>
      <label class="save-view-label" for="save-view-input">{$t('explorer_chat.save_view_name', { default: 'View name' })}</label>
      <input
        id="save-view-input"
        class="save-view-input"
        type="text"
        bind:value={saveViewInputValue}
        onkeydown={onSaveViewKeydown}
        placeholder="My view..."
      />
      <div class="save-view-actions">
        <button class="save-view-cancel" onclick={cancelSaveView} type="button">Cancel</button>
        <button class="save-view-confirm" onclick={confirmSaveView} disabled={!saveViewInputValue.trim()} type="button">Save</button>
      </div>
    </div>
  </div>
{/if}

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

  .saved-view-row {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .saved-view-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: 1;
    min-width: 0;
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

  .saved-view-delete {
    flex-shrink: 0;
    padding: 4px;
    margin-right: 4px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: 4px;
    opacity: 0;
    transition: opacity var(--transition-fast), color var(--transition-fast);
  }
  .saved-view-row:hover .saved-view-delete { opacity: 1; }
  .saved-view-delete:hover { color: #ef4444; background: rgba(239, 68, 68, 0.1); }

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
    z-index: 99;
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

  /* ── Context window divider ──────────────────────────────────────── */
  .context-divider {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
  }

  .context-divider::before,
  .context-divider::after {
    content: '';
    flex: 1;
    height: 1px;
    background: #334155;
  }

  .context-divider-text {
    font-size: var(--text-xs);
    color: #64748b;
    white-space: nowrap;
    font-style: italic;
  }

  /* ── Out-of-context messages ────────────────────────────────────── */
  .chat-message.out-of-context {
    opacity: 0.5;
    position: relative;
  }

  .out-of-context-label {
    font-size: 10px;
    color: #64748b;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    align-self: flex-start;
    padding: 0 var(--space-1);
  }

  .chat-message.user.out-of-context .out-of-context-label {
    align-self: flex-end;
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

  .spec-ref-action {
    font-family: var(--font-sans);
    font-weight: 600;
    font-size: 10px;
    opacity: 0.7;
    margin-left: 2px;
  }

  .spec-ref-btn:hover .spec-ref-action {
    opacity: 1;
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

  .reconnect-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    background: rgba(239, 68, 68, 0.08);
    border-top: 1px solid rgba(239, 68, 68, 0.2);
  }
  .reconnect-text {
    font-size: var(--font-sm);
    color: var(--color-text-muted);
  }
  .reconnect-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary, #3b82f6);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: var(--font-sm);
    font-weight: 500;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  .reconnect-btn:hover { opacity: 0.85; }

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

  .char-counter {
    position: absolute; right: 48px; bottom: 6px;
    font-size: 10px; color: #64748b;
    font-family: 'SF Mono', Menlo, monospace;
    pointer-events: none;
  }
  .char-counter.over-limit { color: #ef4444; font-weight: 700; }

  /* ── Save view dialog ─────────────────────────────────────────── */
  .save-view-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .save-view-dialog {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: var(--space-4);
    min-width: 260px;
    max-width: 360px;
    box-shadow: 0 8px 24px color-mix(in srgb, black 40%, transparent);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .save-view-label {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .save-view-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    outline: none;
  }

  .save-view-input:focus {
    border-color: var(--color-focus);
    box-shadow: 0 0 0 2px var(--color-focus);
  }

  .save-view-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .save-view-cancel {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    cursor: pointer;
  }

  .save-view-confirm {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary, #3b82f6);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    font-size: var(--text-sm);
    font-family: var(--font-body);
    font-weight: 500;
    cursor: pointer;
  }

  .save-view-confirm:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  @media (prefers-reduced-motion: reduce) {
    .dot { animation: none; opacity: 0.6; }
    .suggestion-btn, .saved-views-btn, .save-view-btn,
    .saved-view-item, .apply-view-btn, .chat-input, .send-btn {
      transition: none;
    }
  }
</style>
