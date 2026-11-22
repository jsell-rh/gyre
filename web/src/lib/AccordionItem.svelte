<script>
  /**
   * AccordionItem — compatibility facade over the shadcn-svelte
   * accordion item (lib/components/ui/accordion/). Consumed contract
   * unchanged (see the ui component's doc block).
   */
  import { AccordionItem } from './components/ui/accordion/index.js';

  let {
    id,
    open = false,
    ontoggle,
    ariaLabel = undefined,
    headingId = undefined,
    testId = undefined,
    class: klass = undefined,
    headerClass = undefined,
    bodyClass = undefined,
    actionsClass = undefined,
    header,
    actions,
    children,
  } = $props();
</script>

<AccordionItem
  {id}
  {open}
  {ontoggle}
  {ariaLabel}
  {headingId}
  {testId}
  class={klass}
  {headerClass}
  {bodyClass}
  {actionsClass}
  {header}
  {actions}
>
  {@render children?.()}
</AccordionItem>

<style>
  /* No scoped styles: all accordion styling lives in the shadcn-svelte
     component (./components/ui/accordion/) as Tailwind utilities. This
     block is intentionally kept so CSS-scoping probes that compile this
     file (see AccordionScoping.test.js) still receive a stylesheet AST —
     the guarantee they check (no blanket reset that would erase parent
     :global rules) holds structurally: there are no scoped declarations
     at all. */
</style>
