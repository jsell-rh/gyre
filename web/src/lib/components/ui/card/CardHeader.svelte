<script>
  /** CardHeader — the card's titled band (display font, bottom divider). */
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
  class={cn('flex items-center justify-between gap-4 border-b border-border px-6 py-4 font-display text-base font-semibold', className)}
  {...restProps}
>
  {@render children?.()}
</div>
