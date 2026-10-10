<script>
  /** Test host: AccordionItem with an actions snippet — the interactive
   *  controls live in a SIBLING row of the header button (HTML content
   *  model forbids interactive descendants of <button>). */
  import AccordionItem from './AccordionItem.svelte';

  let open = $state(false);
  let actionClicks = $state(0);
  let toggles = $state(0);

  function toggle() {
    toggles += 1;
    open = !open;
  }
</script>

<AccordionItem id="with-actions" {open} ontoggle={toggle}>
  {#snippet header()}Header text{/snippet}
  {#snippet actions()}
    <button class="probe-action" onclick={() => (actionClicks += 1)}>act</button>
  {/snippet}
  {#snippet children()}<p>body</p>{/snippet}
</AccordionItem>
