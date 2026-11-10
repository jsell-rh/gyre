<script>
  /**
   * GeneratedViewChat — HSI §3 "Generated Views (LLM-Powered, On-Demand)".
   *
   * Natural-language question → LLM-generated view definition (ViewSpec:
   * graph query + layout + explanation) via POST /workspaces/:id/explorer-views/generate.
   *
   * - The generated view is applied to the ExplorerCanvas by the parent
   *   (onApplyView) — including "flow" layouts, which animate trace
   *   particles. The LLM's explanation renders both here and as the canvas
   *   query annotation (sidebar annotation alongside the graph).
   * - "Save this view" persists the generated spec via
   *   POST /workspaces/:id/explorer-views (query + layout + explanation).
   * - Follow-up questions refine the current view: the previous view
   *   definition is sent as previous_spec and the server passes it to the
   *   LLM as refinement context.
   * - The LLM behind the endpoint is a read-only query translator — this
   *   component only ever sends it a question.
   */
  import { t } from 'svelte-i18n';
  import { api } from './api.js';
  import { readGenerateSse } from './generated-view.js';
  import { toastSuccess, toastError } from './toast.svelte.js';

  let {
    workspaceId = '',
    repoId = '',
    onApplyView = () => {},
  } = $props();

  const MAX_QUESTION_LENGTH = 2000;

  let messages = $state([]); // [{ id, role, content, spec?, explanation?, isError? }]
  let nextMsgId = 0;
  let inputText = $state('');
  let status = $state('ready'); // 'ready' | 'thinking' | 'error'
  let currentSpec = $state(null); // last generated view definition (refinement context)
  let savedViewKeys = $state(new Set()); // "name|created_at" keys of saved views
  let inputEl = $state(null);
  let messagesEl = $state(null);

  function scrollToBottom() {
    requestAnimationFrame(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  }

  async function send() {
    const question = inputText.trim();
    if (!question || status === 'thinking') return;
    if (question.length > MAX_QUESTION_LENGTH) {
      messages = [...messages, {
        id: nextMsgId++,
        role: 'assistant',
        content: `*Question too long (${question.length.toLocaleString()} / ${MAX_QUESTION_LENGTH.toLocaleString()} characters).*`,
        isError: true,
      }];
      return;
    }
    if (!workspaceId) {
      messages = [...messages, {
        id: nextMsgId++,
        role: 'assistant',
        content: '*' + $t('generated_view.no_workspace') + '*',
        isError: true,
      }];
      return;
    }

    inputText = '';
    messages = [...messages, { id: nextMsgId++, role: 'user', content: question }];
    status = 'thinking';
    scrollToBottom();

    let resp;
    try {
      resp = await api.generateExplorerView(workspaceId, {
        question,
        ...(repoId ? { repo_id: repoId } : {}),
        // Refinement (HSI §3): follow-up questions carry the previous view
        // definition so the LLM refines it instead of starting over.
        ...(currentSpec ? { previous_spec: currentSpec } : {}),
      });
    } catch (err) {
      console.error('[GeneratedViewChat] generate request failed:', err);
      status = 'error';
      messages = [...messages, {
        id: nextMsgId++,
        role: 'assistant',
        content: $t('generated_view.error_occurred'),
        isError: true,
      }];
      scrollToBottom();
      return;
    }

    if (!resp.ok) {
      status = 'ready';
      const detail = resp.status === 429
        ? $t('generated_view.rate_limited')
        : resp.status === 400
          ? $t('generated_view.invalid_previous')
          : $t('generated_view.error_occurred');
      messages = [...messages, { id: nextMsgId++, role: 'assistant', content: detail, isError: true }];
      scrollToBottom();
      return;
    }

    const events = await readGenerateSse(resp);
    const complete = events.complete;
    status = 'ready';

    if (!complete) {
      messages = [...messages, {
        id: nextMsgId++,
        role: 'assistant',
        content: $t('generated_view.error_occurred'),
        isError: true,
      }];
      scrollToBottom();
      return;
    }

    const spec = complete.view_spec ?? complete.fallback ?? null;
    const explanation = complete.explanation ?? null;
    if (!spec) {
      messages = [...messages, {
        id: nextMsgId++, role: 'assistant',
        content: explanation ?? $t('generated_view.error_occurred'),
        isError: true,
      }];
      scrollToBottom();
      return;
    }

    currentSpec = spec;
    messages = [...messages, {
      id: nextMsgId++,
      role: 'assistant',
      content: explanation ?? spec.description ?? '',
      spec,
      explanation,
    }];
    // HSI §3: the generated view renders immediately, with the LLM's
    // explanation as a sidebar annotation. The parent owns the canvas.
    onApplyView(spec);
    scrollToBottom();
  }

  function viewKey(msg) {
    return `${msg.spec?.name ?? ''}|${msg.id}`;
  }

  async function saveView(msg) {
    if (!msg.spec || !workspaceId) return;
    const key = viewKey(msg);
    try {
      await api.saveExplorerView(workspaceId, {
        name: msg.spec.name ?? $t('generated_view.untitled'),
        // Preserve the generated explanation as the saved description
        // (HSI §3: the saved view preserves query + layout + explanation —
        // the explanation also lives inside the spec itself).
        description: msg.explanation ?? msg.spec.description ?? null,
        spec: msg.spec,
      });
      savedViewKeys = new Set([...savedViewKeys, key]);
      toastSuccess($t('generated_view.saved'));
    } catch (err) {
      console.error('[GeneratedViewChat] save view failed:', err);
      toastError($t('generated_view.save_failed'));
    }
  }

  function onInputKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  let statusLabel = $derived.by(() => {
    switch (status) {
      case 'thinking': return $t('generated_view.status_thinking');
      case 'error':    return $t('generated_view.status_error');
      default:         return $t('generated_view.status_ready');
    }
  });

  let statusColor = $derived(status === 'error' ? 'var(--color-danger)' : status === 'thinking' ? 'var(--color-warning)' : 'var(--color-success)');

  let canRefine = $derived(currentSpec !== null);
</script>

<div class="gen-chat-container">
  <div class="gen-chat-header">
    <div class="gen-chat-header-left">
      <span class="gen-chat-title">{$t('generated_view.title')}</span>
      <span class="gen-chat-status" style="--status-color: {statusColor}">
        <span class="status-dot" aria-hidden="true"></span>
        {statusLabel}
      </span>
    </div>
    {#if canRefine}
      <span class="gen-refine-hint" title={$t('generated_view.refine_hint_title')}>
        {$t('generated_view.refine_hint')}
      </span>
    {/if}
  </div>

  <div class="gen-messages" bind:this={messagesEl} role="log" aria-live="polite">
    {#if messages.length === 0}
      <div class="gen-welcome">
        <p class="welcome-title">{$t('generated_view.welcome_title')}</p>
        <p class="welcome-desc">{$t('generated_view.welcome_desc')}</p>
        <div class="welcome-suggestions">
          {#each [$t('generated_view.suggestion_structure', { default: 'How is the codebase structured?' }), $t('generated_view.suggestion_flow', { default: 'How does a request flow through the system?' }), $t('generated_view.suggestion_auth', { default: 'How does authentication work?' })] as suggestion}
            <button
              class="suggestion-btn"
              onclick={() => { inputText = suggestion; inputEl?.focus(); }}
              type="button"
            >{suggestion}</button>
          {/each}
        </div>
      </div>
    {:else}
      {#each messages as msg (msg.id)}
        <div class="gen-message {msg.role}" class:error={msg.isError}>
          <div class="message-meta">
            <span class="message-role">{msg.role === 'user' ? $t('explorer_chat.you') : $t('explorer_chat.assistant')}</span>
          </div>
          <div class="message-content">{msg.content}</div>
          {#if msg.spec}
            <div class="gen-view-card" data-testid="generated-view-card">
              <span class="gen-view-name">{msg.spec.name}</span>
              <span class="gen-view-layout" data-testid="generated-view-layout">{msg.spec.layout}</span>
              <div class="gen-view-actions">
                <button class="gen-view-btn" onclick={() => onApplyView(msg.spec)} type="button">
                  {$t('explorer_chat.apply_view')}
                </button>
                <button
                  class="gen-view-btn gen-view-save"
                  onclick={() => saveView(msg)}
                  disabled={savedViewKeys.has(viewKey(msg))}
                  type="button"
                >
                  {savedViewKeys.has(viewKey(msg)) ? $t('generated_view.saved_label') : $t('generated_view.save_view')}
                </button>
              </div>
            </div>
          {/if}
        </div>
      {/each}
      {#if status === 'thinking'}
        <div class="gen-thinking" role="status">
          <span class="dot"></span><span class="dot"></span><span class="dot"></span>
        </div>
      {/if}
    {/if}
  </div>

  <div class="gen-input-area">
    <textarea
      bind:this={inputEl}
      class="gen-input"
      placeholder={canRefine ? $t('generated_view.refine_placeholder') : $t('generated_view.input_placeholder')}
      bind:value={inputText}
      onkeydown={onInputKeydown}
      disabled={status === 'thinking'}
      rows="1"
      aria-label={$t('generated_view.input_placeholder')}
    ></textarea>
    <button
      class="gen-send-btn"
      onclick={send}
      disabled={!inputText.trim() || status === 'thinking'}
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

<style>
  .gen-chat-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    overflow: hidden;
  }

  .gen-chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .gen-chat-header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .gen-chat-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .gen-chat-status {
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

  .gen-refine-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .gen-messages {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .gen-welcome {
    padding: var(--space-6) var(--space-2);
    text-align: center;
  }

  .welcome-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-1);
  }

  .welcome-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0 0 var(--space-4);
  }

  .welcome-suggestions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
  }

  .suggestion-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-full);
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .suggestion-btn:hover { border-color: var(--color-primary); color: var(--color-text); }
  .suggestion-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .gen-message {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 100%;
  }

  .gen-message.user { align-items: flex-end; }

  .message-meta {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .message-content {
    font-size: var(--text-sm);
    line-height: 1.5;
    color: var(--color-text);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .gen-message.user .message-content {
    background: var(--color-primary-soft, rgba(59, 130, 246, 0.12));
  }

  .gen-message.error .message-content {
    color: var(--color-danger);
  }

  .gen-view-card {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    flex-wrap: wrap;
  }

  .gen-view-name {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .gen-view-layout {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: 0 var(--space-2);
  }

  .gen-view-actions {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }

  .gen-view-btn {
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .gen-view-btn:hover { border-color: var(--color-primary); color: var(--color-text); }
  .gen-view-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .gen-view-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .gen-thinking {
    display: flex;
    gap: 4px;
    padding: var(--space-2);
  }

  .gen-thinking .dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-full);
    background: var(--color-text-muted);
    animation: gen-dot-pulse 1.2s infinite ease-in-out;
  }

  .gen-thinking .dot:nth-child(2) { animation-delay: 0.15s; }
  .gen-thinking .dot:nth-child(3) { animation-delay: 0.3s; }

  @keyframes gen-dot-pulse {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.8); }
    40% { opacity: 1; transform: scale(1); }
  }

  .gen-input-area {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .gen-input {
    flex: 1;
    resize: none;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2);
    max-height: 120px;
  }

  .gen-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 1px; }
  .gen-input:disabled { opacity: 0.6; }

  .gen-send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    border: 1px solid var(--color-border-strong);
    background: var(--color-surface);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: border-color var(--transition-fast);
    flex-shrink: 0;
  }

  .gen-send-btn:hover:not(:disabled) { border-color: var(--color-primary); color: var(--color-text); }
  .gen-send-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .gen-send-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
</style>
