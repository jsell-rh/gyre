<script>
  /** Card — shadcn-svelte card root (surface + border + brand radius). */
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
  class={cn('flex flex-col overflow-hidden rounded-md border border-border bg-card text-card-foreground', className)}
  {...restProps}
>
  {@render children?.()}
</div>
