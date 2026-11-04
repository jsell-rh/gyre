<script>
  /** Test host: accordion group with the single-expansion constraint
   *  (ui-layout.md §3) — parent owns openId, exactly like Inbox/Briefing. */
  import AccordionItem from './AccordionItem.svelte';

  let openId = $state(null);

  function toggle(id) {
    openId = openId === id ? null : id;
  }
</script>

<AccordionItem id="one" open={openId === 'one'} ontoggle={toggle}>
  {#snippet header()}First header{/snippet}
  {#snippet children()}<p>first body</p>{/snippet}
</AccordionItem>
<AccordionItem id="two" open={openId === 'two'} ontoggle={toggle}>
  {#snippet header()}Second header{/snippet}
  {#snippet children()}<p>second body</p>{/snippet}
</AccordionItem>
