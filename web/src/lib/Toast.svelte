<script>
  import { fly } from 'svelte/transition';
  import { getToasts, dismiss } from './toast.svelte.js';

  let toasts = $derived(getToasts());
</script>

<div class="toast-container" aria-live="polite" aria-atomic="false">
  {#each toasts as t (t.id)}
    <div class="toast toast-{t.type}" role={t.type === 'error' ? 'alert' : undefined} transition:fly={{ y: 8, duration: 200 }}>
      <span class="toast-icon" aria-hidden="true">
        {#if t.type === 'success'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><path d="M20 6L9 17l-5-5"/></svg>
        {:else if t.type === 'error'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
        {:else if t.type === 'warning'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
        {/if}
      </span>
      <span class="toast-message">{t.message}</span>
      <button class="toast-dismiss" onclick={() => dismiss(t.id)} aria-label="Dismiss notification: {t.message}">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M18 6L6 18M6 6l12 12"/></svg>
      </button>
    </div>
  {/each}
</div>

<style>
  .toast-container {
    position: fixed;
    bottom: var(--space-6);
    right: var(--space-6);
    z-index: 2000;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 380px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-md);
    pointer-events: all;
  }

  .toast-icon {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .toast-success .toast-icon { color: var(--color-success); }
  .toast-error   .toast-icon { color: var(--color-danger);  }
  .toast-warning .toast-icon { color: var(--color-warning); }
  .toast-info    .toast-icon { color: var(--color-link);    }

  .toast-success { border-left: 3px solid var(--color-success); }
  .toast-error   { border-left: 3px solid var(--color-danger);  }
  .toast-warning { border-left: 3px solid var(--color-warning); }
  .toast-info    { border-left: 3px solid var(--color-link);    }

  .toast-message {
    flex: 1;
    font-size: var(--text-sm);
    color: var(--color-text);
    line-height: 1.4;
  }

  .toast-dismiss {
    flex-shrink: 0;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: var(--space-0, 2px);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    transition: color var(--transition-fast);
  }

  .toast-dismiss:hover {
    color: var(--color-text);
  }

  .toast-dismiss:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }
</style>
