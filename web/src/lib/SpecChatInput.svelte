<script>
  /**
   * SpecChatInput — the "Edit spec: ..." chat input for LLM-assisted spec
   * editing (ui-layout.md §3 LLM-Assisted Spec Editing, step 1).
   *
   * Shared by the DetailPanel Spec/Edit tab and the EditorSplit left pane
   * (task-185): one production implementation of the §3 chat input instead
   * of two diverging inline copies.
   *
   * Owns the recipient indicator line, the streaming explanation display
   * (`partial` events rendered progressively), the instruction textarea
   * with Ctrl/Cmd+Enter submit, the send button with its loading state,
   * and the hint line. All display strings are passed by the parent — the
   * two surfaces use different i18n namespaces (`editor_split.*` /
   * `detail_panel.*`).
   *
   * Props:
   *   recipient       — string — full recipient indicator line text
   *                   ("Edit spec: \"specs/auth.md\" ▸")
   *   enabled         — bool — false disables input + send (no repo context)
   *   streaming       — bool — true while the SSE request is in flight
   *   streamingText   — string — accumulated `partial` explanation text
   *   placeholder     — string — textarea placeholder
   *   instructionAria — string — textarea aria-label
   *   sendAria        — string — send button aria-label
   *   thinking        — string — streaming label ("Thinking…")
   *   hint            — string | null — normal hint line (null hides)
   *   warnHint        — string | null — warning hint line (null hides)
   *   onsubmit        — (instruction: string) => void — fired with the
   *                   trimmed instruction on send; the input is cleared
   */
  let {
    recipient = '',
    enabled = true,
    streaming = false,
    streamingText = '',
    placeholder = '',
    instructionAria = 'LLM instruction',
    sendAria = 'Send to LLM',
    thinking = 'Thinking…',
    hint = null,
    warnHint = null,
    onsubmit = undefined,
  } = $props();

  let instruction = $state('');

  const canSend = $derived(Boolean(instruction.trim()) && !streaming && enabled);

  function submit() {
    if (!canSend) return;
    const text = instruction.trim();
    instruction = '';
    onsubmit?.(text);
  }

  function handleKeydown(e) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if streaming}
  <div class="llm-streaming" aria-live="polite">
    <span class="streaming-lbl">{thinking}</span>
    {#if streamingText}
      <p class="streaming-txt">{streamingText}<span class="blink-cursor" aria-hidden="true"></span></p>
    {:else}
      <p class="streaming-txt"><span class="blink-cursor" aria-hidden="true"></span></p>
    {/if}
  </div>
{/if}

<div class="llm-input-area">
  <div class="recipient-line">{recipient}</div>
  <div class="llm-row">
    <textarea
      class="llm-textarea"
      bind:value={instruction}
      {placeholder}
      rows="2"
      disabled={streaming || !enabled}
      onkeydown={handleKeydown}
      aria-label={instructionAria}
      data-testid="llm-input"
    ></textarea>
    <button
      class="llm-send"
      onclick={submit}
      disabled={!canSend}
      aria-label={sendAria}
      aria-busy={streaming}
      data-testid="llm-send-btn"
    >
      {#if streaming}
        <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
          <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
          <line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/>
        </svg>
      {/if}
      <span class="sr-only">{sendAria}</span>
    </button>
  </div>
  {#if warnHint}
    <p class="llm-hint warn">{warnHint}</p>
  {:else if hint}
    <p class="llm-hint">{hint}</p>
  {/if}
</div>

<style>
  .llm-streaming {
    margin: var(--space-2) var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .streaming-lbl {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    display: block;
    margin-bottom: var(--space-1);
  }

  .streaming-txt {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .blink-cursor {
    display: inline-block;
    width: 2px;
    height: 1em;
    background: var(--color-primary);
    margin-left: 2px;
    vertical-align: text-bottom;
    animation: blink 1s step-end infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0; }
  }

  .llm-input-area {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .recipient-line {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .llm-row {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .llm-textarea {
    flex: 1;
    min-height: 44px;
    max-height: 90px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    resize: vertical;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .llm-textarea:focus:not(:focus-visible) { outline: none; }
  .llm-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .llm-textarea:disabled { opacity: 0.6; cursor: not-allowed; }

  .llm-send {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    padding: 0;
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .llm-send:hover:not(:disabled) { background: var(--color-primary-hover); }
  .llm-send:disabled { opacity: 0.4; cursor: not-allowed; }
  .llm-send:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .llm-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .llm-hint.warn { color: var(--color-warning); }

  .spin { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .spin { animation: none; }
    .blink-cursor { animation: none; }
    .llm-send, .llm-textarea { transition: none; }
  }
</style>
