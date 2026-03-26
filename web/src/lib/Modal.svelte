<script>
  import { tick } from 'svelte';

  let {
    open = $bindable(false),
    title = '',
    size = 'md',
    onclose = undefined,
    onsubmit = undefined,
    children,
    footer = undefined,
  } = $props();

  let modalEl = $state(null);
  let previousFocus = $state(null);
  const titleId = 'modal-title-' + Math.random().toString(36).slice(2);

  function close() {
    open = false;
    onclose?.();
  }

  function onkeydown(e) {
    if (e.key === 'Escape') {
      close();
      return;
    }
    // Enter submits if an onsubmit handler is provided (skip textarea and select)
    if (e.key === 'Enter' && onsubmit && e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'SELECT') {
      e.preventDefault();
      onsubmit();
      return;
    }
    // Focus trap: keep Tab within modal
    if (e.key === 'Tab' && modalEl) {
      const focusable = modalEl.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    }
  }

  $effect(() => {
    if (open) {
      previousFocus = document.activeElement;
      // Focus the modal close button (first focusable element) after render
      tick().then(() => {
        const focusable = modalEl?.querySelector(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        );
        focusable?.focus();
      });
    } else {
      previousFocus?.focus();
      previousFocus = null;
    }
  });
</script>

{#if open}
  <div class="modal-backdrop">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="modal-overlay" onclick={close} role="presentation"></div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="modal modal-{size}"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-labelledby={titleId}
      onkeydown={onkeydown}
      bind:this={modalEl}
    >
      <div class="modal-header">
        <h3 class="modal-title" id={titleId}>{title}</h3>
        <button class="modal-close" onclick={close} aria-label="Close {title}">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
        </button>
      </div>
      <div class="modal-body">
        {@render children?.()}
      </div>
      {#if footer}
        <div class="modal-footer">
          {@render footer()}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-4);
  }

  .modal-overlay {
    position: absolute;
    inset: 0;
    z-index: 0;
    background: color-mix(in srgb, var(--color-surface, #0a0a0a) 60%, transparent);
    backdrop-filter: blur(2px);
  }

  .modal {
    position: relative;
    z-index: 10;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    max-height: 85vh;
    width: 100%;
    animation: modal-in 150ms ease;
  }

  @keyframes modal-in {
    from { opacity: 0; transform: scale(0.97) translateY(-8px); }
    to   { opacity: 1; transform: scale(1) translateY(0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .modal {
      animation: none;
    }
  }

  .modal-sm { max-width: 400px; }
  .modal-md { max-width: 560px; }
  .modal-lg { max-width: 800px; }
  .modal-xl { max-width: 1100px; }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .modal-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
  }

  .modal-close {
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius);
    padding: var(--space-1);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .modal-close:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .modal-close:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .modal-body {
    padding: var(--space-6);
    overflow-y: auto;
    flex: 1;
  }

  .modal-footer {
    padding: var(--space-4) var(--space-6);
    border-top: 1px solid var(--color-border);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    flex-shrink: 0;
  }
</style>
