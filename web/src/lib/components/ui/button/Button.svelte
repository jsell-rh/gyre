<script>
  /**
   * Button — shadcn-svelte button on the bits-ui Button primitive.
   *
   * All element attributes (onclick, aria-*, data-testid, …) are forwarded
   * to the underlying <button> via restProps.
   */
  import { Button as ButtonPrimitive } from 'bits-ui';
  import { cn } from '$lib/utils.js';
  import { buttonVariants } from './button.svelte.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    variant = 'primary',
    size = 'md',
    type = 'button',
    children,
    ...restProps
  } = $props();
</script>

<ButtonPrimitive.Root
  bind:ref
  {type}
  class={cn(buttonVariants({ variant, size }), className)}
  {...restProps}
>
  {@render children?.()}
</ButtonPrimitive.Root>
