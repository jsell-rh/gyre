<script>
  /**
   * AccordionItem — shadcn-svelte accordion item (single-expansion inline
   * expansion, ui-layout.md §3).
   *
   * Controlled component: the parent owns the open state so it can enforce
   * the single-expansion constraint across a group of items (openId === id
   * for exactly one item, or null). The header is a real <button> (keyboard
   * accessible by construction); the body is conditionally rendered below it.
   *
   * bits-ui's Accordion primitives are not used here: the consumed contract
   * is a standalone controlled item whose header/body ids are caller-visible
   * (`headingId` override, `accordion-header-{id}` / `accordion-body-{id}`
   * wiring to caller-rendered regions), while bits-ui's Item/Trigger/Content
   * derive their ids and open state from a shared Accordion.Root context
   * that these callers do not render.
   *
   * Props and DOM hooks (consumed by Inbox, Briefing, and their scoped CSS):
   *   headerClass — extra class on the header button (authored here, so
   *                 parent <style> rules targeting it must be :global())
   *   bodyClass   — extra class on the body wrapper
   *   class       — extra class on the root element
   *
   * Snippets:
   *   header   — header row content (title, badges, meta)
   *   actions  — interactive controls rendered as a SIBLING of the header
   *              button, never inside it (HTML forbids interactive
   *              descendants of <button>); the actions row does not toggle.
   *   children — expanded content, rendered only when open.
   */
  import { cn } from '$lib/utils.js';
  import { accordionTriggerVariants } from './accordion.svelte.js';

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

  const headerId = $derived(headingId ?? `accordion-header-${id}`);
  const bodyId = $derived(`accordion-body-${id}`);
</script>

<div class={cn('accordion-item flex flex-col', klass)} data-open={open}>
  <div class={cn('accordion-row flex w-full items-start', actions && 'has-actions items-stretch')}>
    <button
      type="button"
      id={headerId}
      class={cn(accordionTriggerVariants(), headerClass)}
      aria-label={ariaLabel}
      data-testid={testId}
      aria-expanded={open}
      aria-controls={bodyId}
      onclick={() => ontoggle?.(id)}
    >
      {@render header?.()}
    </button>
    {#if actions}
      <!-- Sibling of the header button, NOT inside it: HTML content model
           forbids nested interactive elements; clicks here never toggle. -->
      <div class={cn('accordion-actions flex items-center gap-2 shrink-0', actionsClass)}>
        {@render actions?.()}
      </div>
    {/if}
  </div>
  {#if open}
    <div class={cn('accordion-body', bodyClass)} id={bodyId} role="region" aria-labelledby={headerId}>
      {@render children?.()}
    </div>
  {/if}
</div>
