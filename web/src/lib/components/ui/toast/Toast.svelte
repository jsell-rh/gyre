<script>
  /**
   * Toast — a single shadcn-svelte toast item.
   *
   * Presentational: the toast store (dedupe, duration, max count) is owned
   * by the caller. `icon` is a snippet rendered in the accent-colored icon
   * slot; `ondismiss` + `dismissLabel` wire the dismiss button;
   * `transition` accepts a dynamic Svelte transition function applied to
   * the item element.
   */
  import { cn } from '$lib/utils.js';
  import { toastVariants } from './toast.svelte.js';

  let {
    class: className = undefined,
    type = 'info',
    role = undefined,
    ondismiss = undefined,
    dismissLabel = undefined,
    transition = (node, params) => ({ duration: 0 }),
    icon,
    children,
    ...restProps
  } = $props();
</script>

<div class={cn(toastVariants({ type }), 'toast', className)} {role} {transition} {...restProps}>
  <span class="toast-icon mt-px shrink-0 text-(--toast-accent)" aria-hidden="true">
    {@render icon?.()}
  </span>
  <span class="toast-message flex-1 text-sm leading-normal text-foreground">
    {@render children?.()}
  </span>
  <button
    type="button"
    class="toast-dismiss flex shrink-0 items-center rounded-sm bg-transparent border-none p-1 text-muted-foreground cursor-pointer transition-colors duration-150 hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none"
    onclick={ondismiss}
    aria-label={dismissLabel}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
      <path d="M18 6L6 18M6 6l12 12" />
    </svg>
  </button>
</div>
