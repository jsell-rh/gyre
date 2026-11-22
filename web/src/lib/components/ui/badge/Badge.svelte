<script>
  /** Badge — shadcn-svelte badge (cva variants, Tailwind utilities). */
  import { cn } from '$lib/utils.js';
  import { badgeVariants } from './badge.svelte.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    variant = 'default',
    children,
    ...restProps
  } = $props();
</script>

<span bind:this={ref} class={cn(badgeVariants({ variant }), className)} {...restProps}>
  {@render children?.()}
</span>
