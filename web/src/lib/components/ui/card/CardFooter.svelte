<script>
  /** CardFooter — the card's action band (elevated surface, top divider). */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<div
  bind:this={ref}
  class={cn('flex items-center gap-2 border-t border-border bg-surface-elevated px-6 py-4', className)}
  {...restProps}
>
  {@render children?.()}
</div>
