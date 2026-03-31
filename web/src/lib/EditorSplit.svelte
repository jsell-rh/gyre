<script>
  import ArchPreviewCanvas from './ArchPreviewCanvas.svelte';
  import { api } from './api.js';
  import { toastError, toastSuccess } from './toast.svelte.js';
  import { t } from 'svelte-i18n';

  /**
   * EditorSplit — full-width editor + architecture preview split layout.
   *
   * Used by DetailPanel (spec editing pop-out) and MetaSpecs (meta-spec editing).
   * Left panel: spec textarea + inline LLM chat.
   * Right panel: ArchPreviewCanvas with ghost overlays.
   *
   * Spec ref: ui-navigation.md §3 Specs tab (Editor Split),
   *           §3 Architecture tab (Ghost Overlays),
   *           §4 Meta-Spec Management (Agent Rules → Architecture)
   *
   * Props:
   *   content       — string — current text content
   *   onChange      — (newContent: string) => void — called on every change
   *   repoId        — string | null
   *   specPath      — string | null — e.g. 'specs/system/auth.md'
   *   ghostOverlays — array of { nodeId, type: 'new'|'modified'|'removed' }
   *   onClose       — () => void — called when user dismisses (Back or Esc)
   *   context       — 'spec' | 'meta-spec' — display label
   */
  let {
    content = $bindable(''),
    onChange = undefined,
    repoId = null,
    specPath = null,
    ghostOverlays = [],
    onClose = undefined,
    context = 'spec',
  } = $props();

  // ── Graph data (lazy-loaded from graphPredict) ─────────────────────────────
  let graphNodes = $state([]);
  let graphEdges = $state([]);
  let graphLoading = $state(false);
  let graphLoaded = $state(false);

  $effect(() => {
    // Load graph once when repoId is available
    if (repoId && !graphLoaded && !graphLoading) {
      loadGraph();
    }
  });

  async function loadGraph() {
    if (!repoId) return;
    graphLoading = true;
    try {
      const result = await api.graphPredict(repoId, {
        spec_path: specPath,
        overlays: ghostOverlays,
      });
      graphNodes = result?.nodes ?? [];
      graphEdges = result?.edges ?? [];
      graphLoaded = true;
    } catch {
      // graceful: show empty canvas; user can still edit
      graphLoaded = true;
    } finally {
      graphLoading = false;
    }
  }

  // ── LLM chat ───────────────────────────────────────────────────────────────
  let llmInstruction = $state('');
  let llmStreaming = $state(false);
  let llmExplanation = $state('');
  let llmSuggestion = $state(null); // { diff: [...], explanation: string } | null
  let saving = $state(false);

  async function sendLlmInstruction() {
    if (!llmInstruction.trim() || llmStreaming) return;
    if (!repoId) return;
    const instruction = llmInstruction.trim();
    llmInstruction = '';
    llmStreaming = true;
    llmExplanation = '';
    llmSuggestion = null;

    try {
      const resp = await api.specsAssist(repoId, {
        spec_path: specPath,
        instruction,
        draft_content: content || undefined,
      });
      if (!resp.ok) throw new Error(`LLM request failed: ${resp.status}`);

      const reader = resp.body?.getReader();
      if (!reader) throw new Error('No response body');
      const decoder = new TextDecoder();
      let buf = '';
      let done = false;

      while (!done) {
        const { value, done: streamDone } = await reader.read();
        done = streamDone;
        if (value) {
          buf += decoder.decode(value, { stream: true });
          const lines = buf.split('\n');
          buf = lines.pop() ?? '';
          for (const line of lines) {
            if (!line.startsWith('data: ')) continue;
            const raw = line.slice(6);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              if (parsed.event === 'partial' || parsed.type === 'partial') {
                llmExplanation += parsed.text ?? parsed.explanation ?? '';
              } else if (parsed.event === 'complete' || parsed.type === 'complete') {
                llmSuggestion = {
                  diff: parsed.diff ?? [],
                  explanation: parsed.explanation ?? llmExplanation,
                };
                done = true; break;
              } else if (parsed.event === 'error' || parsed.type === 'error') {
                throw new Error(parsed.message ?? 'LLM error');
              }
            } catch (pe) {
              if (pe.message && !pe.message.startsWith('Unexpected token')) throw pe;
            }
          }
        }
      }
    } catch (e) {
      toastError($t('editor_split.llm_assist_failed', { values: { error: e.message } }));
    } finally {
      llmStreaming = false;
    }
  }

  function acceptSuggestion() {
    if (!llmSuggestion) return;
    let c = content;
    for (const op of llmSuggestion.diff) {
      if (op.op === 'add') {
        const idx = c.indexOf(op.path);
        if (idx !== -1) {
          const lineEnd = c.indexOf('\n', idx + op.path.length);
          const insertAt = lineEnd !== -1 ? lineEnd + 1 : c.length;
          c = c.slice(0, insertAt) + op.content + '\n' + c.slice(insertAt);
        } else {
          c += '\n' + op.content;
        }
      } else if (op.op === 'replace') {
        const idx = c.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(c, idx + op.path.length);
          c = c.slice(0, idx) + op.path + '\n' + op.content + c.slice(end);
        }
      } else if (op.op === 'remove') {
        const idx = c.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(c, idx + op.path.length);
          c = c.slice(0, idx) + c.slice(end);
        }
      }
    }
    content = c;
    onChange?.(c);
    llmSuggestion = null;
  }

  function dismissSuggestion() { llmSuggestion = null; }

  function findSectionEnd(c, from) {
    const rest = c.slice(from);
    const match = rest.match(/\n(#{1,6} )/);
    if (match?.index !== undefined) return from + match.index + 1;
    return c.length;
  }

  async function saveSpec() {
    if (!repoId || !specPath || saving) return;
    saving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: specPath,
        content,
        message: `Update ${specPath} via editor split`,
      });
      toastSuccess($t('editor_split.spec_saved', { values: { mr_id: result.mr_id } }));
    } catch (e) {
      toastError($t('editor_split.save_failed', { values: { error: e.message } }));
    } finally {
      saving = false;
    }
  }

  function handleContentInput(e) {
    content = e.target.value;
    onChange?.(content);
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose?.();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="editor-split" role="region" aria-label={$t('editor_split.editor_split_view')}>
  <!-- Back button -->
  <div class="split-header">
    <button class="back-btn" onclick={() => onClose?.()} aria-label={$t('editor_split.close_editor')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
      {$t('editor_split.back')}
    </button>
    <span class="split-label">{context === 'meta-spec' ? $t('editor_split.meta_spec_editor') : $t('editor_split.spec_editor')}{specPath ? ` — ${specPath}` : ''}</span>
    {#if repoId && specPath}
      <button
        class="save-btn"
        onclick={saveSpec}
        disabled={saving || !content.trim()}
        aria-busy={saving}
      >
        {saving ? $t('editor_split.saving') : $t('editor_split.save_create_mr')}
      </button>
    {/if}
  </div>

  <!-- Split panes -->
  <div class="split-panes">
    <!-- Left: Editor + LLM chat -->
    <div class="pane pane-left">
      <textarea
        class="split-textarea"
        value={content}
        oninput={handleContentInput}
        placeholder={$t('editor_split.spec_placeholder')}
        aria-label={$t('editor_split.spec_editor')}
        spellcheck="false"
        data-testid="editor-split-textarea"
      ></textarea>

      {#if llmSuggestion}
        <div class="suggestion-block" role="region" aria-label={$t('editor_split.llm_suggestion')} data-testid="llm-suggestion">
          <div class="suggestion-hdr">
            <span class="suggestion-lbl">{$t('editor_split.suggested_change')}</span>
            <button class="dismiss-btn" onclick={dismissSuggestion} aria-label={$t('editor_split.dismiss_suggestion')}>✕</button>
          </div>
          {#if llmSuggestion.explanation}
            <p class="suggestion-expl">{llmSuggestion.explanation}</p>
          {/if}
          <div class="suggestion-btns">
            <button class="accept-btn" onclick={acceptSuggestion}>{$t('editor_split.accept')}</button>
            <button class="dismiss-btn-sm" onclick={dismissSuggestion}>{$t('common.dismiss')}</button>
          </div>
        </div>
      {/if}

      {#if llmStreaming && llmExplanation}
        <div class="llm-streaming" aria-live="polite">
          <span class="streaming-lbl">{$t('editor_split.thinking')}</span>
          <p class="streaming-txt">{llmExplanation}<span class="blink-cursor" aria-hidden="true"></span></p>
        </div>
      {/if}

      <div class="llm-input-area">
        <div class="recipient-line">
          {$t('editor_split.edit_label')} {context === 'meta-spec' ? $t('editor_split.meta_spec') : $t('editor_split.spec')}{specPath ? `: "${specPath}" ▸` : ' ▸'}
        </div>
        <div class="llm-row">
          <textarea
            class="llm-textarea"
            bind:value={llmInstruction}
            placeholder={$t('editor_split.llm_placeholder')}
            rows="2"
            disabled={llmStreaming || !repoId}
            onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); sendLlmInstruction(); } }}
            aria-label={$t('editor_split.llm_instruction')}
            data-testid="llm-input"
          ></textarea>
          <button
            class="llm-send"
            onclick={sendLlmInstruction}
            disabled={!llmInstruction.trim() || llmStreaming || !repoId}
            aria-label={$t('editor_split.send_to_llm')}
            aria-busy={llmStreaming}
            data-testid="llm-send-btn"
          >
            {#if llmStreaming}
              <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
              </svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                <line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/>
              </svg>
            {/if}
            <span class="sr-only">{$t('editor_split.send')}</span>
          </button>
        </div>
        {#if !repoId}
          <p class="llm-hint warn">{$t('editor_split.llm_requires_repo')}</p>
        {:else}
          <p class="llm-hint">{$t('editor_split.llm_hint')}</p>
        {/if}
      </div>
    </div>

    <!-- Divider -->
    <div class="split-divider" aria-hidden="true"></div>

    <!-- Right: Architecture preview -->
    <div class="pane pane-right" data-testid="arch-preview-pane">
      <div class="pane-header">
        <span class="pane-title">{$t('editor_split.architecture_preview')}</span>
        {#if graphLoading}
          <span class="loading-chip" aria-live="polite">{$t('editor_split.loading_graph')}</span>
        {:else if ghostOverlays.length}
          <span class="overlay-chip">{$t('editor_split.predicted_changes', { values: { count: ghostOverlays.length } })}</span>
        {/if}
      </div>
      <div class="canvas-wrap">
        <ArchPreviewCanvas
          nodes={graphNodes}
          edges={graphEdges}
          {ghostOverlays}
          size="full"
        />
      </div>
    </div>
  </div>
</div>

<style>
  .editor-split {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    overflow: hidden;
  }

  .split-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    min-height: 44px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-label {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .save-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .save-btn:hover:not(:disabled) { background: var(--color-primary-hover); }
  .save-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .save-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-panes {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .pane-left {
    flex: 1;
    border-right: none;
  }

  .pane-right {
    flex: 1;
    background: var(--color-surface, #0f172a);
  }

  .split-divider {
    width: 1px;
    background: var(--color-border);
    flex-shrink: 0;
  }

  .split-textarea {
    flex: 1;
    width: 100%;
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: none;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    resize: none;
    box-sizing: border-box;
    min-height: 200px;
  }

  .split-textarea:focus:not(:focus-visible) { outline: none; }
  .split-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* LLM suggestion block */
  .suggestion-block {
    margin: var(--space-2) var(--space-3);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  .suggestion-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .suggestion-lbl {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-primary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .suggestion-expl {
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
  }

  .suggestion-btns {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }

  .accept-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
  }

  .accept-btn:hover { opacity: 0.9; }
  .accept-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .dismiss-btn {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0 var(--space-1);
    font-size: var(--text-xs);
  }

  .dismiss-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .dismiss-btn-sm {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
  }

  .dismiss-btn-sm:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* LLM streaming */
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

  /* LLM input */
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

  /* Right pane */
  .pane-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .pane-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .loading-chip {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .overlay-chip {
    font-size: 10px;
    padding: 1px 5px;
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-warning);
    font-family: var(--font-mono);
  }

  .canvas-wrap {
    flex: 1;
    overflow: hidden;
    min-height: 0;
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
    .spin { animation: none; }
    .blink-cursor { animation: none; }
    .back-btn, .save-btn, .llm-send, .llm-textarea { transition: none; }
  }
</style>
