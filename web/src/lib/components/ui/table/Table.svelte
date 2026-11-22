<script>
  /** Table — shadcn-svelte table (scroll wrapper + table element). */
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<div bind:this={ref} class={cn('overflow-x-auto rounded-md border border-border', className)}>
  <table class="w-full border-collapse font-sans text-sm" {...restProps}>
    {@render children?.()}
  </table>
</div>
