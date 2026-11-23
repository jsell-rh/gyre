<script>
  /** TableCaption — screen-reader-only table caption. */
  import { cn } from '$lib/utils.js';

  let {
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<caption class={cn('sr-only', className)} {...restProps}>
  {@render children?.()}
</caption>
