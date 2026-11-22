<script>
  /**
   * Toast — compatibility facade over the shadcn-svelte toast parts
   * (lib/components/ui/toast/). The toast store contract
   * (toast/toastSuccess/toastError/toastWarning/toastInfo/dismiss,
   * dedupe, max 5, auto-dismiss) is unchanged in ./toast.svelte.js; this
   * component renders the store's items through the ui parts, keeping the
   * icons, localized dismiss labels, role=alert on errors, and the
   * reduced-motion-aware fly transition.
   */
  import { fly } from 'svelte/transition';
  import { t as i18n } from 'svelte-i18n';
  import { getToasts, dismiss } from './toast.svelte.js';
  import { Toaster, Toast } from './components/ui/toast/index.js';

  const prefersReducedMotion = typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false;

  /** Svelte JS transitions bypass CSS reduced-motion rules — apply duration 0 manually */
  const flyDuration = prefersReducedMotion ? 0 : 200;

  // Dynamic transition (Svelte transitions are functions): the item
  // element animates in with the same fly parameters as before.
  const toastTransition = (node, params) => fly(node, { y: 8, duration: flyDuration, ...params });

  let toasts = $derived(getToasts());
</script>

<Toaster>
  {#each toasts as t (t.id)}
    <Toast
      type={t.type}
      role={t.type === 'error' ? 'alert' : undefined}
      ondismiss={() => dismiss(t.id)}
      dismissLabel={$i18n('common.dismiss_notification', { values: { message: t.message } })}
      transition={toastTransition}
    >
      {#snippet icon()}
        {#if t.type === 'success'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><path d="M20 6L9 17l-5-5"/></svg>
        {:else if t.type === 'error'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
        {:else if t.type === 'warning'}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
        {/if}
      {/snippet}
      {t.message}
    </Toast>
  {/each}
</Toaster>
