<script>
  /**
   * InlineChat — contextual chat input with recipient indicator.
   *
   * Spec ref: ui-layout.md §3 (Contextual Chat)
   *           HSI §1 (Scoped communication)
   *
   * Props:
   *   recipient     — string, e.g. "worker-12", "workspace orchestrator"
   *   recipientType — 'agent' | 'llm-qa' | 'spec-edit'
   *   placeholder   — override input placeholder text
   *   onmessage     — async (text: string) => void | Response
   *                   If it returns a Response, InlineChat handles SSE streaming.
   *   streaming     — bool, true when SSE is in flight
   */
  let {
    recipient = '',
    recipientType = 'agent',
    placeholder = undefined,
    onmessage = undefined,
    streaming = $bindable(false),
  } = $props();

  import { onDestroy } from 'svelte';

  let inputEl = $state(null);
  let historyEl = $state(null);
  let text = $state('');
  let messages = $state([]);
  let streamBuffer = $state('');
  let error = $state(null);

  // Track active SSE reader so we can cancel it on component destroy
  let activeReader = $state(null);
  let destroyed = false;
  onDestroy(() => {
    destroyed = true;
    // Cancel any in-flight SSE stream to prevent background reads
    activeReader?.cancel().catch(() => {});
    activeReader = null;
  });

  // Scroll chat history to bottom whenever messages or streaming buffer updates.
  $effect(() => {
    messages; streamBuffer;
    if (historyEl) historyEl.scrollTop = historyEl.scrollHeight;
  });

  // Recipient display text per spec.
  let recipientLabel = $derived(buildRecipientLabel(recipient, recipientType));
  let defaultPlaceholder = $derived(buildPlaceholder(recipientType));

  function buildRecipientLabel(r, type) {
    if (!r) return '';
    switch (type) {
      case 'llm-qa':
        return `Ask about ${r} ▸`;
      case 'spec-edit':
        return `Edit spec: "${r}" ▸`;
      default:
        return `Message to ${r} ▸`;
    }
  }

  function buildPlaceholder(type) {
    switch (type) {
      case 'llm-qa':   return 'Ask a question…';
      case 'spec-edit': return 'Describe a change…';
      default:          return 'Type a message…';
    }
  }

  async function send() {
    const msg = text.trim();
    if (!msg || streaming) return;

    text = '';
    error = null;
    messages = [...messages, { role: 'user', content: msg }];

    if (!onmessage) return;

    streaming = true;
    streamBuffer = '';

    try {
      const result = await onmessage(msg);

      if (result instanceof Response) {
        // SSE streaming response
        await handleSse(result);
      } else if (typeof result === 'string') {
        messages = [...messages, { role: 'assistant', content: result }];
      }
    } catch (e) {
      error = e?.message ?? 'Send failed';
    } finally {
      streaming = false;
      streamBuffer = '';
    }
  }

  async function handleSse(response) {
    const reader = response.body?.getReader();
    if (!reader) {
      error = 'No response body';
      return;
    }
    activeReader = reader;
    const decoder = new TextDecoder();
    let partial = '';
    let done = false;

    while (!done) {
      if (destroyed) { reader.cancel().catch(() => {}); return; }
      const { value, done: streamDone } = await reader.read();
      done = streamDone;
      if (value) {
        partial += decoder.decode(value, { stream: true });
        const lines = partial.split('\n');
        partial = lines.pop() ?? '';
        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const raw = line.slice(6);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              if (parsed.type === 'partial') {
                streamBuffer += parsed.text ?? '';
              } else if (parsed.type === 'complete') {
                const final = parsed.text ?? streamBuffer;
                messages = [...messages, { role: 'assistant', content: final }];
                streamBuffer = '';
                done = true;
                break;
              } else if (parsed.type === 'error') {
                error = parsed.message ?? 'Streaming error';
                done = true;
                break;
              }
            } catch {
              // Not JSON — raw text chunk
              streamBuffer += raw;
            }
          }
        }
      }
    }

    activeReader = null;

    // If we got partial content but no complete event, commit what we have.
    if (!destroyed && streamBuffer) {
      messages = [...messages, { role: 'assistant', content: streamBuffer }];
      streamBuffer = '';
    }
  }

  function onkeydown(e) {
    // Cmd/Ctrl+Enter sends message (Enter alone is a newline in textarea).
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      send();
    }
  }

  function clearHistory() {
    messages = [];
    error = null;
  }
</script>

<div class="inline-chat">
  {#if messages.length > 0 || streamBuffer}
    <div class="chat-history" aria-live="polite" aria-label="Chat history" bind:this={historyEl}>
      {#each messages as msg}
        <div class="chat-msg chat-msg-{msg.role}">
          <span class="msg-role" aria-label={msg.role === 'user' ? 'You' : recipient}>
            {msg.role === 'user' ? 'You' : recipient}
          </span>
          <p class="msg-content">{msg.content}</p>
        </div>
      {/each}

      {#if streamBuffer}
        <div class="chat-msg chat-msg-assistant streaming">
          <span class="msg-role">{recipient}</span>
          <p class="msg-content">{streamBuffer}<span class="cursor" aria-hidden="true"></span></p>
        </div>
      {/if}
    </div>

    <button class="clear-btn" onclick={clearHistory} aria-label="Clear conversation">
      Clear
    </button>
  {/if}

  {#if error}
    <div class="chat-error" role="alert">
      <span class="chat-error-msg">{error}</span>
      <button class="chat-error-dismiss" onclick={() => error = null} aria-label="Dismiss error">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M18 6L6 18M6 6l12 12"/></svg>
      </button>
    </div>
  {/if}

  <div class="chat-input-area" aria-busy={streaming}>
    {#if recipientLabel}
      <div class="recipient-line" id="inline-chat-recipient" aria-label="Sending to: {recipient}">
        {recipientLabel}
      </div>
    {/if}

    <div class="input-row">
      <textarea
        class="chat-input"
        bind:value={text}
        bind:this={inputEl}
        placeholder={placeholder ?? defaultPlaceholder}
        rows="1"
        disabled={streaming}
        aria-label="Message input"
        aria-describedby={recipientLabel ? 'inline-chat-recipient' : undefined}
        onkeydown={onkeydown}
      ></textarea>

      <button
        class="send-btn"
        onclick={send}
        disabled={!text.trim() || streaming}
        aria-label="Send message"
      >
        {#if streaming}
          <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <line x1="22" y1="2" x2="11" y2="13"/>
            <polygon points="22 2 15 22 11 13 2 9 22 2"/>
          </svg>
        {/if}
        <span class="sr-only">Send</span>
      </button>
    </div>

    {#if recipientType === 'agent'}
      <p class="input-hint">Ctrl+Enter to send · Messages are signed and persisted via message bus</p>
    {:else if recipientType === 'llm-qa'}
      <p class="input-hint">Ctrl+Enter to send · Read-only Q&A — cannot trigger actions</p>
    {:else if recipientType === 'spec-edit'}
      <p class="input-hint">Ctrl+Enter to send · Produces draft suggestions — you accept before saving</p>
    {/if}
  </div>
</div>

<style>
  .inline-chat {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-3);
  }

  /* Conversation history */
  .chat-history {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-height: 240px;
    overflow-y: auto;
    padding: var(--space-2) 0;
  }

  .chat-msg {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .chat-msg-user {
    align-items: flex-end;
  }

  .chat-msg-assistant {
    align-items: flex-start;
  }

  .msg-role {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
  }

  .msg-content {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    line-height: 1.5;
    max-width: 85%;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .chat-msg-user .msg-content {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-text);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .chat-msg-assistant .msg-content {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border: 1px solid var(--color-border);
  }

  .streaming .msg-content {
    border-color: var(--color-focus);
  }

  /* Blinking cursor for streaming */
  .cursor {
    display: inline-block;
    width: 2px;
    height: 1em;
    background: var(--color-focus);
    margin-left: 2px;
    vertical-align: text-bottom;
    animation: blink 1s step-end infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0; }
  }

  .clear-btn {
    align-self: flex-end;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
    transition: color var(--transition-fast);
  }

  .clear-btn:hover { color: var(--color-text-secondary); }

  .chat-error {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .chat-error-msg { flex: 1; }

  .chat-error-dismiss {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--color-danger);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }

  .chat-error-dismiss:hover { opacity: 1; }
  .chat-error-dismiss:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* Input area */
  .chat-input-area {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .recipient-line {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    padding: 0 var(--space-1);
  }

  .input-row {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .chat-input {
    flex: 1;
    min-height: 36px;
    max-height: 120px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    resize: vertical;
    transition: border-color var(--transition-fast);
    box-sizing: border-box;
  }

  .chat-input:focus:not(:focus-visible) {
    outline: none;
  }

  .chat-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -1px;
  }

  .send-btn:focus-visible,
  .clear-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .chat-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .send-btn:hover:not(:disabled) {
    background: var(--color-primary-hover);
  }

  .send-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .input-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    padding: 0 var(--space-1);
  }

  .spin {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .cursor { animation: none; }
    .spin { animation: none; }
    .chat-input,
    .clear-btn,
    .send-btn { transition: none; }
  }
</style>
