<script>
  /**
   * DialogTitle — the dialog's accessible name. Registers its id with the
   * dialog context so Content's aria-labelledby wiring stays correct.
   * Renders role=heading (aria-level, default 3 like the h3 it replaces).
   */
  import { Dialog as DialogPrimitive } from 'bits-ui';
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    level = 3,
    children,
    ...restProps
  } = $props();
</script>

<DialogPrimitive.Title
  bind:ref
  {level}
  class={cn('font-display text-lg font-semibold text-foreground', className)}
  {...restProps}
>
  {@render children?.()}
</DialogPrimitive.Title>
