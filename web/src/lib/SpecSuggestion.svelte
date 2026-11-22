<script>
  /**
   * SpecSuggestion — one inline "Suggested Change" diff block
   * (ui-layout.md §3 LLM-Assisted Spec Editing, step 3).
   *
   * Renders a single LLM suggestion (diff ops + explanation) with the three
   * curation actions:
   *   - Accept  → `onaccept`   — parent applies the diff to the editor's
   *                              in-memory content (never committed directly)
   *   - Edit    → `onedit`     — parent copies the suggested text into the
   *                              editor for manual refinement
   *   - Dismiss → `ondismiss`  — parent removes the suggestion block
   *
   * Multiple instances coexist, one per suggestion (§3 "Multiple suggestions
   * can be requested. Each appears as a separate inline diff block. The
   * human curates the spec — the LLM assists but never writes directly.").
   *
   * Props:
   *   suggestion — { id, diff: [{op, path, content}], explanation }
   *   onaccept   — () => void
   *   onedit     — () => void
   *   ondismiss  — () => void
   */
  import { t } from 'svelte-i18n';

  let {
    suggestion = { id: '', diff: [], explanation: '' },
    onaccept = undefined,
    onedit = undefined,
    ondismiss = undefined,
  } = $props();

  const hasDiff = $derived(Array.isArray(suggestion.diff) && suggestion.diff.length > 0);
</script>

<div class="suggestion-block" role="region" aria-label={$t('editor_split.llm_suggestion')} data-testid="llm-suggestion">
  <div class="suggestion-hdr">
    <span class="suggestion-lbl">{$t('editor_split.suggested_change')}</span>
    <button class="dismiss-btn" onclick={() => ondismiss?.()} aria-label={$t('editor_split.dismiss_suggestion')}>✕</button>
  </div>
  {#if suggestion.explanation}
    <p class="suggestion-expl">{suggestion.explanation}</p>
  {/if}
  {#if hasDiff}
    <div class="suggestion-diff">
      {#each suggestion.diff as op}
        <div class="diff-op diff-op-{op.op}">
          <span class="diff-badge">{op.op}</span>
          <span class="diff-path">{op.path}</span>
          {#if op.content}
            <pre class="diff-content">{op.content}</pre>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
  <div class="suggestion-btns">
    <button class="accept-btn" onclick={() => onaccept?.()} data-testid="accept-suggestion">{$t('editor_split.accept')}</button>
    {#if hasDiff}
      <button class="dismiss-btn-sm" onclick={() => onedit?.()} data-testid="edit-suggestion">{$t('editor_split.edit')}</button>
    {/if}
    <button class="dismiss-btn-sm" onclick={() => ondismiss?.()} data-testid="dismiss-suggestion">{$t('common.dismiss')}</button>
  </div>
</div>

<style>
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

  .suggestion-diff {
    padding: 0 var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .diff-op {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    flex-wrap: wrap;
  }

  .diff-badge {
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    font-size: 10px;
    text-transform: uppercase;
    font-weight: 600;
    flex-shrink: 0;
  }

  .diff-op-add .diff-badge { background: color-mix(in srgb, var(--color-success, #2e9e5b) 20%, transparent); color: var(--color-success, #2e9e5b); }
  .diff-op-remove .diff-badge { background: color-mix(in srgb, var(--color-danger, #cc3340) 20%, transparent); color: var(--color-danger, #cc3340); }
  .diff-op-replace .diff-badge { background: color-mix(in srgb, var(--color-warning) 20%, transparent); color: var(--color-warning); }

  .diff-path {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .diff-content {
    margin: 0;
    flex: 1;
    min-width: 0;
    white-space: pre-wrap;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
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
</style>
