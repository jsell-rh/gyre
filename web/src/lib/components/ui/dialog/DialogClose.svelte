<script>
  /** DialogClose — closes the dialog (bits-ui Dialog.Close button). */
  import { Dialog as DialogPrimitive } from 'bits-ui';
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    children,
    ...restProps
  } = $props();
</script>

<DialogPrimitive.Close
  bind:ref
  type="button"
  class={cn(
    'flex items-center justify-center rounded-md bg-transparent border-none p-1 text-muted-foreground cursor-pointer transition-colors duration-150 hover:text-foreground hover:bg-surface-elevated focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</DialogPrimitive.Close>
