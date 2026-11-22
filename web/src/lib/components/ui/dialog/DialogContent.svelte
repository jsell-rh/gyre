<script>
  /**
   * DialogContent — the dialog surface (bits-ui Dialog.Content).
   *
   * Provides the modal behaviors: Escape-to-close (document keydown),
   * focus trap + focus restore (FocusScope), scroll lock, and outside-
   * interaction dismissal. Callers that own their overlay's click handling
   * pass `onInteractOutside={(e) => e.preventDefault()}` so a single close
   * path fires.
   */
  import { Dialog as DialogPrimitive } from 'bits-ui';
  import { cn } from '$lib/utils.js';

  let {
    ref = $bindable(null),
    class: className = undefined,
    forceMount = false,
    trapFocus = true,
    preventScroll = true,
    onOpenAutoFocus = undefined,
    onCloseAutoFocus = undefined,
    onEscapeKeydown = undefined,
    onInteractOutside = undefined,
    children,
    ...restProps
  } = $props();
</script>

<DialogPrimitive.Content
  bind:ref
  {forceMount}
  {trapFocus}
  {preventScroll}
  {onOpenAutoFocus}
  {onCloseAutoFocus}
  {onEscapeKeydown}
  {onInteractOutside}
  class={cn(
    'relative z-10 flex w-full max-h-[85vh] flex-col rounded-lg border border-border-strong bg-card text-card-foreground shadow-lg animate-in fade-in zoom-in-95 slide-in-from-top-2 duration-150 motion-reduce:animate-none',
    className
  )}
  {...restProps}
>
  {@render children?.()}
</DialogPrimitive.Content>
