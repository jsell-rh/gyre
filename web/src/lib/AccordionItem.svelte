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
   *   headerClass — optional extra class on the header button. The button is
   *                 authored HERE, so parent <style> rules targeting it must
   *                 be :global()-wrapped in the parent (a plain parent-scoped
   *                 selector cannot match a child-authored element — Svelte
   *                 scopes CSS with the authoring component's hash class).
   *                 The header's *content* stays parent-authored, so parent
   *                 rules on inner elements work normally.
   *   bodyClass   — optional extra class on the body wrapper
   *   class       — optional extra class on the root element
   *
   * Snippets (passed as content snippets inside the component tag):
   *   header   — header row content (title, badges, meta)
   *   actions  — interactive controls rendered as a SIBLING of the header
   *              button, never inside it. HTML forbids interactive
   *              descendants of <button>; quick links etc. go here. The
   *              actions row does not toggle the accordion.
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
    actionsClass = undefined,
    header,
    actions,
    children,
  } = $props();

  const headerId = $derived(headingId ?? `accordion-header-${id}`);
  const bodyId = $derived(`accordion-body-${id}`);
</script>

<div class={`accordion-item${klass ? ' ' + klass : ''}`} data-open={open}>
  <div class={`accordion-row${actions ? ' has-actions' : ''}`}>
    <button
      type="button"
      id={headerId}
      class={`accordion-header${headerClass ? ' ' + headerClass : ''}`}
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
      <div class={`accordion-actions${actionsClass ? ' ' + actionsClass : ''}`}>
        {@render actions?.()}
      </div>
    {/if}
  </div>
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
  /* The actions row (flex row) keeps the header button as the flexing
     child; the button itself keeps layout-neutral defaults only — callers
     compose its look via headerClass + :global() rules in their <style>. */
  .accordion-row {
    display: flex;
    align-items: flex-start;
    width: 100%;
  }
  .accordion-row.has-actions {
    align-items: stretch;
  }
  .accordion-header {
    display: block;
    width: 100%;
    flex: 1 1 auto;
    min-width: 0;
    cursor: pointer;
    box-sizing: border-box;
    font: inherit;
    color: inherit;
    text-align: inherit;
    background: none;
    border: none;
    padding: 0;
    margin: 0;
  }
  .accordion-actions {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
  }
  .accordion-header:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
    border-radius: var(--radius-sm);
  }
</style>
