<script>
  /** CardContent — the card body. */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<div bind:this={ref} class={cn('p-6', className)} {...restProps}>
  {@render children?.()}
</div>
