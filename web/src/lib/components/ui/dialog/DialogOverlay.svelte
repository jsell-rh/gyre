<script>
  /** DialogOverlay — the dimmed backdrop layer behind the dialog content. */
  import { Dialog as DialogPrimitive } from 'bits-ui';
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    forceMount = false,
    children,
    ...restProps
  } = $props();
</script>

<DialogPrimitive.Overlay
  bind:ref
  {forceMount}
  class={cn('absolute inset-0 z-0 bg-background/60 backdrop-blur-[2px]', className)}
  {...restProps}
>
  {@render children?.()}
</DialogPrimitive.Overlay>
