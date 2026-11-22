<script>
  /**
   * Tabs — compatibility facade over the shadcn-svelte Tabs
   * (lib/components/ui/tabs/). Consumed contract unchanged: `tabs` array,
   * `active` (bindable), `onchange(id)`, `panelId`, `ariaLabel`.
   */
  import { Tabs } from './components/ui/tabs/index.js';

  let {
    tabs = [],
    active = $bindable(''),
    onchange = undefined,
    panelId = undefined,
    ariaLabel = 'Tab navigation',
  } = $props();
</script>

<Tabs {tabs} bind:active {onchange} {panelId} {ariaLabel} />
