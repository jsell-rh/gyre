<script>
  /**
   * ConversationalExplorer — system-explorer.md §8 "Conversational
   * Exploration".
   *
   * Chat-style Q&A grounded in the knowledge graph via
   * POST /workspaces/:id/explorer-views/ask:
   *
   * - The server validates every `[entity:<kind>:<id>]` citation against
   *   the real graph before it reaches this component; the parsed refs are
   *   rendered as clickable inline links (node → open detail + focus
   *   canvas, spec → open inline, commit → open in git, agent → highlight
   *   the nodes it modified).
   * - "Show me X" answers carry a view spec → onApplyView renders it on
   *   the canvas (the same apply path as generated views).
   * - "What would happen if…" answers carry a structural prediction →
   *   onPrediction renders ghost overlays (§3).
   * - Node references in a response highlight the referenced nodes on the
   *   canvas (onHighlightNodes).
   * - Conversation history is kept in component state (not persisted) and
   *   replayed on follow-ups so the LLM can build on earlier answers.
   */
  import { t } from 'svelte-i18n';
  import { api } from './api.js';
  import { toastError } from './toast.svelte.js';

  let {
    workspaceId = '',
    repoId = '',
    onApplyView = () => {},
    onOpenNode = () => {},
    onOpenSpec = () => {},
    onOpenCommit = () => {},
    onShowAgent = () => {},
    onHighlightNodes = () => {},
    onPrediction = () => {},
  } = $props();

  const MAX_QUESTION_LENGTH = 2000;
  const MAX_HISTORY = 20;

  // [{ id, role: 'user'|'assistant', content, refs?, spec?, prediction?, isError? }]
  let messages = $state([]);
  let nextMsgId = 0;
  let inputText = $state('');
  let status = $state('ready'); // 'ready' | 'thinking' | 'error'
  let inputEl = $state(null);
  let messagesEl = $state(null);

  function scrollToBottom() {
    requestAnimationFrame(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  }

  /** Split an answer into text and entity-token segments. The token syntax
   *  matches the server's ([entity:<kind>:<id>]); only tokens with ref
   *  metadata render as links — the server already stripped unresolvable
   *  citations, so a token without metadata means a ref the UI cannot
   *  resolve either and stays plain text. */
  function parseSegments(msg) {
    const text = msg.content ?? '';
    const refByKey = new Map(
      (msg.refs ?? []).map((r) => [`${r.type}:${r.id}`, r])
    );
    const segments = [];
    const tokenRe = /\[entity:(node|spec|commit|agent):([^\]]*)\]/g;
    let last = 0;
    let m;
    while ((m = tokenRe.exec(text)) !== null) {
      if (m.index > last) segments.push({ text: text.slice(last, m.index) });
      const ref = refByKey.get(`${m[1]}:${m[2]}`);
      if (ref) {
        segments.push({ ref });
      } else {
        segments.push({ text: m[0] });
      }
      last = m.index + m[0].length;
    }
    if (last < text.length) segments.push({ text: text.slice(last) });
    return segments;
  }

  function clickRef(ref) {
    switch (ref.type) {
      case 'node': onOpenNode(ref.id); break;
      case 'spec': onOpenSpec(ref.id); break;
      case 'commit': onOpenCommit(ref.id); break;
      case 'agent': onShowAgent(ref.id); break;
    }
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
        content: '*' + $t('conversational_explorer.no_workspace') + '*',
        isError: true,
      }];
      return;
    }

    inputText = '';
    messages = [...messages, { id: nextMsgId++, role: 'user', content: question }];
    status = 'thinking';
    scrollToBottom();

    // Follow-up context (§8): replay the conversation (capped at 20 turns
    // — the server rejects more). Error bubbles are local UI, not turns.
    const history = messages
      .slice(0, -1) // everything before this question
      .filter((m) => !m.isError)
      .slice(-MAX_HISTORY)
      .map((m) => ({ role: m.role, content: m.content }));

    let resp;
    try {
      resp = await api.explorerAsk(workspaceId, {
        question,
        ...(repoId ? { repo_id: repoId } : {}),
        history,
      });
    } catch (err) {
      console.error('[ConversationalExplorer] ask request failed:', err);
      status = 'error';
      messages = [...messages, {
        id: nextMsgId++,
        role: 'assistant',
        content: err?.message ?? $t('conversational_explorer.error_occurred'),
        isError: true,
      }];
      scrollToBottom();
      return;
    }

    status = 'ready';
    const nodeIds = (resp.entity_refs ?? [])
      .filter((r) => r.type === 'node')
      .map((r) => r.id);
    messages = [...messages, {
      id: nextMsgId++,
      role: 'assistant',
      content: resp.answer ?? '',
      refs: resp.entity_refs ?? [],
      spec: resp.view_spec ?? null,
      prediction: resp.prediction ?? null,
    }];
    scrollToBottom();

    // Canvas actions (§8): "show me X" → view spec; "what would happen if…"
    // → structural prediction (ghost overlays); node references → the
    // referenced nodes highlight on the canvas.
    if (resp.view_spec) onApplyView(resp.view_spec);
    if (resp.prediction) onPrediction(resp.prediction);
    if (nodeIds.length > 0) onHighlightNodes(nodeIds);
  }

  function onInputKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  let statusLabel = $derived.by(() => {
    switch (status) {
      case 'thinking': return $t('conversational_explorer.status_thinking');
      case 'error':    return $t('conversational_explorer.status_error');
      default:         return $t('conversational_explorer.status_ready');
    }
  });

  let statusColor = $derived(status === 'error' ? 'var(--color-danger)' : status === 'thinking' ? 'var(--color-warning)' : 'var(--color-success)');
</script>

<div class="conv-chat-container">
  <div class="conv-chat-header">
    <div class="conv-chat-header-left">
      <span class="conv-chat-title">{$t('conversational_explorer.title')}</span>
      <span class="conv-chat-status" style="--status-color: {statusColor}">
        <span class="status-dot" aria-hidden="true"></span>
        {statusLabel}
      </span>
    </div>
    <span class="conv-chat-hint" title={$t('conversational_explorer.grounded_hint_title')}>
      {$t('conversational_explorer.grounded_hint')}
    </span>
  </div>

  <div class="conv-messages" bind:this={messagesEl} role="log" aria-live="polite">
    {#if messages.length === 0}
      <div class="conv-welcome">
        <p class="welcome-title">{$t('conversational_explorer.welcome_title')}</p>
        <p class="welcome-desc">{$t('conversational_explorer.welcome_desc')}</p>
        <div class="welcome-suggestions">
          {#each [$t('conversational_explorer.suggestion_why'), $t('conversational_explorer.suggestion_show'), $t('conversational_explorer.suggestion_whatif')] as suggestion}
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
        <div class="conv-message {msg.role}" class:error={msg.isError}>
          <div class="message-meta">
            <span class="message-role">{msg.role === 'user' ? $t('explorer_chat.you') : $t('explorer_chat.assistant')}</span>
          </div>
          <div class="message-content" data-testid="conv-answer">
            {#each parseSegments(msg) as seg, i}
              {#if seg.ref}
                <button
                  class="entity-ref entity-ref-{seg.ref.type}"
                  data-testid="entity-ref"
                  data-entity-type={seg.ref.type}
                  title={seg.ref.type === 'node'
                    ? $t('conversational_explorer.ref_node_title')
                    : seg.ref.type === 'spec'
                      ? $t('conversational_explorer.ref_spec_title')
                      : seg.ref.type === 'commit'
                        ? $t('conversational_explorer.ref_commit_title')
                        : $t('conversational_explorer.ref_agent_title')}
                  onclick={() => clickRef(seg.ref)}
                  type="button"
                >{seg.ref.label}</button>
              {:else}
                <span>{seg.text}</span>
              {/if}
            {/each}
          </div>
          {#if msg.spec}
            <div class="conv-view-card" data-testid="conv-view-card">
              <span class="conv-view-name">{msg.spec.name}</span>
              <span class="conv-view-layout">{msg.spec.layout}</span>
              <div class="conv-view-actions">
                <button class="conv-view-btn" onclick={() => onApplyView(msg.spec)} type="button">
                  {$t('explorer_chat.apply_view')}
                </button>
              </div>
            </div>
          {/if}
          {#if msg.prediction}
            <div class="conv-prediction-chip" data-testid="conv-prediction-chip">
              <span class="conv-prediction-count">
                {$t('conversational_explorer.prediction_applied', { values: { count: (msg.prediction.predictions ?? []).length } })}
              </span>
            </div>
          {/if}
        </div>
      {/each}
      {#if status === 'thinking'}
        <div class="conv-thinking" role="status">
          <span class="dot"></span><span class="dot"></span><span class="dot"></span>
        </div>
      {/if}
    {/if}
  </div>

  <div class="conv-input-area">
    <textarea
      bind:this={inputEl}
      class="conv-input"
      placeholder={$t('conversational_explorer.input_placeholder')}
      bind:value={inputText}
      onkeydown={onInputKeydown}
      disabled={status === 'thinking'}
      rows="1"
      aria-label={$t('conversational_explorer.input_placeholder')}
    ></textarea>
    <button
      class="conv-send-btn"
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
  .conv-chat-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    overflow: hidden;
  }

  .conv-chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .conv-chat-header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .conv-chat-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .conv-chat-status {
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

  .conv-chat-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .conv-messages {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
  }

  .conv-welcome {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-2);
  }

  .welcome-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .welcome-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    line-height: 1.5;
  }

  .welcome-suggestions {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-2);
  }

  .suggestion-btn {
    text-align: left;
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
  }

  .suggestion-btn:hover {
    color: var(--color-text);
    border-color: var(--color-primary, #818cf8);
  }

  .conv-message {
    margin-bottom: var(--space-3);
  }

  .conv-message.user .message-content {
    background: rgba(129, 140, 248, 0.12);
    border-radius: var(--radius-sm, 6px);
    padding: var(--space-2);
  }

  .conv-message.error .message-content {
    color: var(--color-danger);
  }

  .message-meta {
    margin-bottom: var(--space-1);
  }

  .message-role {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
  }

  .message-content {
    font-size: var(--text-sm);
    color: var(--color-text);
    line-height: 1.55;
    word-break: break-word;
  }

  /* Entity references — clickable, styled distinctly from prose. */
  .entity-ref {
    display: inline;
    padding: 0 2px;
    font: inherit;
    font-weight: 600;
    background: none;
    border: none;
    border-bottom: 1px dashed currentColor;
    cursor: pointer;
    text-decoration: none;
  }

  .entity-ref-node   { color: #a78bfa; }
  .entity-ref-spec   { color: #34d399; }
  .entity-ref-commit { color: #fbbf24; }
  .entity-ref-agent  { color: #f472b6; }

  .entity-ref:hover { filter: brightness(1.2); }

  .entity-ref:focus-visible {
    outline: 2px solid currentColor;
    outline-offset: 1px;
    border-bottom-style: solid;
  }

  .conv-view-card {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm, 6px);
    background: var(--color-surface-elevated);
    flex-wrap: wrap;
  }

  .conv-view-name {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .conv-view-layout {
    font-size: 10px;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full, 999px);
    padding: 0 var(--space-2);
  }

  .conv-view-actions {
    margin-left: auto;
    display: flex;
    gap: var(--space-1);
  }

  .conv-view-btn {
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
    background: transparent;
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
  }

  .conv-view-btn:hover { border-color: var(--color-primary, #818cf8); }

  .conv-prediction-chip {
    display: inline-flex;
    margin-top: var(--space-2);
    padding: 2px var(--space-2);
    font-size: 10px;
    color: #fbbf24;
    border: 1px dashed rgba(251, 191, 36, 0.5);
    border-radius: var(--radius-full, 999px);
  }

  .conv-thinking {
    display: flex;
    gap: 4px;
    padding: var(--space-2);
  }

  .conv-thinking .dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-full, 999px);
    background: var(--color-text-muted);
    animation: conv-dot-pulse 1.2s ease-in-out infinite;
  }

  .conv-thinking .dot:nth-child(2) { animation-delay: 0.2s; }
  .conv-thinking .dot:nth-child(3) { animation-delay: 0.4s; }

  @keyframes conv-dot-pulse {
    0%, 100% { opacity: 0.3; }
    50% { opacity: 1; }
  }

  .conv-input-area {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .conv-input {
    flex: 1;
    resize: none;
    font-size: var(--text-sm);
    color: var(--color-text);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm, 6px);
    padding: var(--space-2);
    max-height: 96px;
    font-family: inherit;
  }

  .conv-input:focus {
    outline: none;
    border-color: var(--color-primary, #818cf8);
  }

  .conv-send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: rgba(129, 140, 248, 0.15);
    color: #a5b4fc;
    border: none;
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
    flex-shrink: 0;
  }

  .conv-send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
