<script>
  /** TableHead — th cell (display font, uppercase column label). */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<th
  bind:this={ref}
  scope="col"
  class={cn(
    'px-4 py-4 text-left font-display text-xs font-semibold uppercase tracking-[0.06em] text-muted-foreground whitespace-nowrap select-none',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</th>
