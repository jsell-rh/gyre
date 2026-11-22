<script>
  /**
   * Modal — compatibility facade composing the shadcn-svelte Dialog parts
   * (lib/components/ui/dialog/, built on bits-ui Dialog).
   *
   * Consumed contract unchanged:
   *   - `open` (bindable), `title`, `size` (sm/md/lg/xl), `onclose`,
   *     `onsubmit`, `footer` snippet, children snippet
   *   - role=dialog + aria-modal + aria-labelledby wiring (bits-ui)
   *   - Escape closes (bits-ui EscapeLayer, document keydown)
   *   - focus trap + focus restore (bits-ui FocusScope); opening focuses
   *     the first form field, else the first focusable element
   *   - overlay click closes (exactly once: bits-ui's pointerdown-outside
   *     dismissal is disabled via onInteractOutside preventDefault and the
   *     overlay's own onclick is the single close path, so jsdom-driven
   *     clicks and real pointer interactions behave identically)
   *   - Enter submits when `onsubmit` is provided (not from textarea/select)
   *   - body scroll lock (bits-ui ScrollLock)
   *
   * The legacy `.modal*` class names are kept as inert DOM hooks.
   */
  import { tick } from 'svelte';
  import { t } from 'svelte-i18n';
  import {
    Dialog, DialogOverlay, DialogContent, DialogTitle, DialogClose,
  } from './components/ui/dialog/index.js';

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
  let previousFocus = null;

  // max-width per size — mirrors the former .modal-sm/md/lg/xl rules
  const SIZE_CLASSES = {
    sm: 'max-w-[400px]',
    md: 'max-w-[560px]',
    lg: 'max-w-[800px]',
    xl: 'max-w-[1100px]',
  };
  let sizeClass = $derived(SIZE_CLASSES[size] ?? SIZE_CLASSES.md);

  function close() {
    open = false;
    onclose?.();
  }

  function onOpenChange(next) {
    if (next) {
      open = true;
      return;
    }
    // bits-ui closes (Escape key): surface the same onclose contract the
    // overlay/close-button paths fire.
    open = false;
    onclose?.();
  }

  function onkeydown(e) {
    // Enter submits if an onsubmit handler is provided (skip textarea and select)
    if (e.key === 'Enter' && onsubmit && e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'SELECT') {
      e.preventDefault();
      onsubmit();
    }
    // Tab is trapped by the bits-ui FocusScope inside DialogContent.
  }

  function onOpenAutoFocus(e) {
    // Keep the former focus behavior: first form field, else first
    // focusable element (bits-ui's default would focus the first
    // focusable — the close button).
    e.preventDefault();
    tick().then(() => {
      const formField = modalEl?.querySelector(
        '.modal-body input, .modal-body select, .modal-body textarea'
      );
      if (formField) {
        formField.focus();
      } else {
        const focusable = modalEl?.querySelector(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
        );
        focusable?.focus();
      }
    });
  }

  function onCloseAutoFocus(e) {
    // Focus restore is handled below (previousFocus), mirroring the
    // former behavior of restoring the element focused before opening.
    e.preventDefault();
  }

  // Capture the pre-modal focus before the dialog content mounts and
  // bits-ui's FocusScope can move focus into the dialog.
  $effect.pre(() => {
    if (open) {
      previousFocus = document.activeElement;
    }
  });

  $effect(() => {
    if (!open && previousFocus) {
      previousFocus?.focus();
      previousFocus = null;
    }
  });
</script>

<Dialog bind:open {onOpenChange}>
  {#if open}
    <div class="modal-backdrop fixed inset-0 z-[1000] flex items-center justify-center p-4">
      <DialogOverlay
        class="modal-overlay"
        onclick={close}
        role="presentation"
        aria-hidden="true"
      />
      <DialogContent
        bind:ref={modalEl}
        class="modal modal-{size} {sizeClass}"
        onkeydown={onkeydown}
        onInteractOutside={(e) => e.preventDefault()}
        {onOpenAutoFocus}
        {onCloseAutoFocus}
      >
        <div class="modal-header flex shrink-0 items-center justify-between border-b border-border px-6 py-4">
          <DialogTitle class="modal-title">{title}</DialogTitle>
          <DialogClose class="modal-close" aria-label={$t('common.close_title', { values: { title } })}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </DialogClose>
        </div>
        <div class="modal-body flex-1 overflow-y-auto p-6">
          {@render children?.()}
        </div>
        {#if footer}
          <div class="modal-footer flex shrink-0 items-center justify-end gap-2 border-t border-border px-6 py-4">
            {@render footer()}
          </div>
        {/if}
      </DialogContent>
    </div>
  {/if}
</Dialog>
