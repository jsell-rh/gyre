<script>
  /**
   * DiffSuggestion — shows an LLM-suggested change with Accept/Edit/Dismiss actions.
   *
   * Props:
   *   suggestion  — { id, content: string }
   *   onaccept    — () => void
   *   onedit      — () => void
   *   ondismiss   — () => void
   */
  import Button from './Button.svelte';

  let {
    suggestion = { id: '', content: '' },
    onaccept = undefined,
    onedit = undefined,
    ondismiss = undefined,
  } = $props();

  let acting = $state(false);

  async function handleAction(fn) {
    if (acting || !fn) return;
    acting = true;
    try {
      await fn();
    } finally {
      acting = false;
    }
  }
</script>

<div class="diff-suggestion" role="region" aria-label="Suggested change {suggestion.id}" aria-live="polite">
  <div class="diff-header">
    <span class="diff-label">Suggested Change</span>
    <span class="diff-hint">Review before accepting</span>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <pre class="diff-content" tabindex="0">{suggestion.content}</pre>
  <div class="diff-actions">
    <Button variant="primary" size="sm" onclick={() => handleAction(onaccept)} disabled={acting}>Accept</Button>
    <Button variant="secondary" size="sm" onclick={() => handleAction(onedit)} disabled={acting}>Edit</Button>
    <Button variant="secondary" size="sm" onclick={() => handleAction(ondismiss)} disabled={acting}>Dismiss</Button>
  </div>
</div>

<style>
  .diff-suggestion {
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    overflow: hidden;
    background: color-mix(in srgb, var(--color-primary) 6%, transparent);
  }

  .diff-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-primary) 25%, transparent);
  }

  .diff-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-primary-text);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .diff-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .diff-content {
    margin: 0;
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    color: var(--color-text);
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .diff-content:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .diff-actions {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }
</style>
