<script>
  /**
   * Toaster — the toast region container (aria-live polite). Fixed to the
   * bottom-right; children are the Toast items.
   */
  import { cn } from '$lib/utils.js';

  let {
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<div
  class={cn('toast-container fixed bottom-6 right-6 z-[2000] flex max-w-[380px] flex-col gap-2 pointer-events-none', className)}
  aria-live="polite"
  aria-atomic="false"
  {...restProps}
>
  {@render children?.()}
</div>
