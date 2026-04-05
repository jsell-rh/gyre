<script>
  /**
   * CopyableId — Human-friendly ID/SHA rendering with click-to-copy.
   *
   * Shows a shortened version with full value in tooltip.
   * Click copies full value to clipboard with visual feedback.
   */
  import { toast as showToast } from './toast.svelte.js';

  let {
    /** Full value (UUID, SHA, etc.) */
    value = '',
    /** Display label (overrides auto-truncation) */
    label = undefined,
    /** Number of chars to show (default: 8 for IDs, 7 for SHAs) */
    chars = undefined,
    /** Style variant: 'sha' (mono, 7 chars), 'id' (mono, 8 chars), 'inline' (inherit font) */
    variant = 'id',
    /** Whether to show copy icon */
    icon = false,
    /** What to call it in the toast ("SHA", "ID", etc.) */
    copyLabel = '',
    /** Optional click handler (overrides copy behavior) */
    onclick = undefined,
  } = $props();

  let copied = $state(false);
  let copyTimer = null;

  const defaultChars = variant === 'sha' ? 7 : 8;
  let displayChars = $derived(chars ?? defaultChars);
  let display = $derived(label ?? (value?.length > displayChars + 2 ? value.slice(0, displayChars) : value) ?? '');
  let isTruncated = $derived(display !== value);

  function handleClick(e) {
    if (onclick) {
      onclick(e);
      return;
    }
    e.stopPropagation();
    if (!value) return;
    navigator.clipboard.writeText(value).then(() => {
      copied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => { copied = false; }, 1500);
      const what = copyLabel || (variant === 'sha' ? 'SHA' : 'ID');
      showToast(`${what} copied`, { type: 'success' });
    }).catch(() => {});
  }

  function handleKeydown(e) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleClick(e);
    }
  }
</script>

{#if value}
  <code
    class="copyable-id"
    class:copyable-sha={variant === 'sha'}
    class:copyable-inline={variant === 'inline'}
    class:copied
    title={isTruncated ? `${value}\nClick to copy` : 'Click to copy'}
    onclick={handleClick}
    onkeydown={handleKeydown}
    role="button"
    tabindex="0"
    aria-label="Copy {copyLabel || value}"
  >
    {display}{#if icon && !copied}<span class="copy-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10">
        <rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/>
      </svg>
    </span>{/if}{#if copied}<span class="copy-check" aria-hidden="true">✓</span>{/if}
  </code>
{/if}

<style>
  .copyable-id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: color-mix(in srgb, var(--color-border) 40%, transparent);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
    display: inline-flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
    user-select: none;
    border: 1px solid transparent;
    line-height: 1.4;
  }

  .copyable-id:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border-color: var(--color-border);
  }

  .copyable-id:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .copyable-id.copied {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .copyable-sha {
    letter-spacing: 0.02em;
  }

  .copyable-inline {
    font-family: inherit;
    font-size: inherit;
    background: transparent;
    padding: 0;
    border-radius: 0;
  }

  .copyable-inline:hover {
    background: transparent;
    text-decoration: underline;
  }

  .copy-icon {
    display: inline-flex;
    align-items: center;
    opacity: 0.5;
    transition: opacity var(--transition-fast);
  }

  .copyable-id:hover .copy-icon {
    opacity: 1;
  }

  .copy-check {
    color: var(--color-success);
    font-weight: 700;
    font-size: 10px;
  }
</style>
