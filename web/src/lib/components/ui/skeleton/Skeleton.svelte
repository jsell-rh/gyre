<script>
  /**
   * Skeleton — shadcn-svelte skeleton with the design system's shimmer
   * gradient (the keyframes live in the Tailwind theme, app.css).
   * Dimensions come from the caller via inline `style` (width/height/
   * border-radius props of the former bespoke skeleton).
   */
  import { cn } from '$lib/utils.js';

  let {
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<div
  class={cn(
    'shrink-0 bg-linear-to-r from-surface-elevated via-border to-surface-elevated bg-[length:200%_100%] animate-shimmer motion-reduce:animate-none',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</div>
