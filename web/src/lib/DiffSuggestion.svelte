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
</script>

<div class="diff-suggestion" role="region" aria-label="Suggested change">
  <div class="diff-header">
    <span class="diff-label">Suggested Change</span>
    <span class="diff-hint">Review before accepting</span>
  </div>
  <pre class="diff-content">{suggestion.content}</pre>
  <div class="diff-actions">
    <Button variant="primary" size="sm" onclick={onaccept}>Accept</Button>
    <Button variant="secondary" size="sm" onclick={onedit}>Edit</Button>
    <Button variant="secondary" size="sm" onclick={ondismiss}>Dismiss</Button>
  </div>
</div>

<style>
  .diff-suggestion {
    border: 1px solid var(--color-primary, #ee0000);
    border-radius: var(--radius, 6px);
    overflow: hidden;
    background: color-mix(in srgb, var(--color-primary, #ee0000) 6%, transparent);
  }

  .diff-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-2, 0.5rem) var(--space-3, 0.75rem);
    background: color-mix(in srgb, var(--color-primary, #ee0000) 12%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-primary, #ee0000) 25%, transparent);
  }

  .diff-label {
    font-size: var(--text-xs, 0.75rem);
    font-weight: 600;
    color: var(--color-primary, #ee0000);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .diff-hint {
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text-muted, #888);
  }

  .diff-content {
    margin: 0;
    padding: var(--space-3, 0.75rem);
    font-family: var(--font-mono, monospace);
    font-size: var(--text-sm, 0.875rem);
    line-height: 1.6;
    color: var(--color-text, #eee);
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .diff-actions {
    display: flex;
    gap: var(--space-2, 0.5rem);
    padding: var(--space-2, 0.5rem) var(--space-3, 0.75rem);
    border-top: 1px solid var(--color-border, #333);
    background: var(--color-surface-elevated, #1a1a1a);
  }
</style>
