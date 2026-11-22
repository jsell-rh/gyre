<script>
  /**
   * Dialog — shadcn-svelte dialog Root (bits-ui Dialog.Root).
   *
   * Renderless: provides the open-state context for Overlay/Content/Title/
   * Close. `open` is bindable; `onOpenChange` fires on any state change
   * (Escape key, close button, programmatic).
   */
  import { Dialog as DialogPrimitive } from 'bits-ui';

  let {
    open = $bindable(false),
    onOpenChange = undefined,
    onOpenChangeComplete = undefined,
    children,
  } = $props();
</script>

<DialogPrimitive.Root bind:open {onOpenChange} {onOpenChangeComplete}>
  {@render children?.()}
</DialogPrimitive.Root>
