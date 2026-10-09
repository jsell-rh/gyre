<script>
  /**
   * AccordionItem — reusable single-expansion accordion item.
   *
   * Spec ref: ui-layout.md §3 (Inline Expansion) — "Inbox items and Briefing
   * sections expand inline: click expands below the header, only one item
   * expanded at a time (accordion)."
   *
   * Controlled component: the parent owns the open state so it can enforce
   * the single-expansion constraint across a group of items (openId === id
   * for exactly one item, or null). The header is a real <button> (keyboard
   * accessible by construction); the body is conditionally rendered below it.
   *
   *   headerClass — optional extra class on the header button (styled by caller)
   *   bodyClass   — optional extra class on the body wrapper
   *   class       — optional extra class on the root element
   *
   * Snippets (passed as content snippets inside the component tag):
   *   header   — header row content (title, badges, meta)
   *   children — expanded content, rendered only when open.
   *              Named `children` (not `body`) on purpose: snippet content
   *              resolves identifiers against the callee's prop scope, so a
   *              prop named `body` would shadow callers' local `body`
   *              variables (e.g. Inbox's parsed notification body).
   */
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
    header,
    children,
  } = $props();

  const headerId = $derived(headingId ?? `accordion-header-${id}`);
  const bodyId = $derived(`accordion-body-${id}`);
</script>

<div class={`accordion-item${klass ? ' ' + klass : ''}`} data-open={open}>
  <button
    type="button"
    class={`accordion-header${headerClass ? ' ' + headerClass : ''}`}
    aria-label={ariaLabel}
    data-testid={testId}
    aria-expanded={open}
    aria-controls={bodyId}
    onclick={() => ontoggle?.(id)}
  >
    {@render header?.()}
  </button>
  {#if open}
    <div class={`accordion-body${bodyClass ? ' ' + bodyClass : ''}`} id={bodyId} role="region" aria-labelledby={headerId}>
      {@render children?.()}
    </div>
  {/if}
</div>

<style>
  .accordion-item {
    display: flex;
    flex-direction: column;
  }
  .accordion-header {
    /* Layout-neutral: callers compose their own header styling via the
       `class` prop / descendant selectors. Reset only. */
    all: unset;
    display: block;
    width: 100%;
    cursor: pointer;
    box-sizing: border-box;
  }
  .accordion-header:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
    border-radius: var(--radius-sm);
  }
</style>
