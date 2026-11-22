<script>
  /** TableRow — table row (hover surface; cursor when clickable). */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<tr
  bind:this={ref}
  class={cn('border-b border-border transition-colors hover:bg-surface-elevated', className)}
  {...restProps}
>
  {@render children?.()}
</tr>
