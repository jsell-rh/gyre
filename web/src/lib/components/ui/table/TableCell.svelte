<script>
  /** TableCell — td cell. */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<td bind:this={ref} class={cn('px-4 py-4 text-foreground align-middle', className)} {...restProps}>
  {@render children?.()}
</td>
