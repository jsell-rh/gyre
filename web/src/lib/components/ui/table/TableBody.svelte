<script>
  /** TableBody — tbody rows. */
  import { cn } from '$lib/utils.js';

  let {
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<tbody class={cn('[&_tr:last-child]:border-b-0', className)} {...restProps}>
  {@render children?.()}
</tbody>
