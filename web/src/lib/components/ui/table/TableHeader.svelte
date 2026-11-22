<script>
  /** TableHeader — thead band on the elevated surface. */
  import { cn } from '$lib/utils.js';

  let {
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<thead class={cn('bg-surface-elevated', className)} {...restProps}>
  {@render children?.()}
</thead>
