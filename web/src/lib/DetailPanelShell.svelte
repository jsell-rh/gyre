<script>
  import DetailPanel from './DetailPanel.svelte';

  /**
   * DetailPanelShell — layout wrapper for the Split layout pattern.
   *
   * When detail panel is open:
   *   main content → 60%
   *   detail panel → 40%
   * When closed: main content → 100%.
   *
   * Spec ref: ui-layout.md §2 (Split layout), §3 (Drill-Down)
   *
   * Props:
   *   entity    — { type, id, data } | null — current entity to show in panel
   *   onclose   — () => void — called when panel is closed
   *   onentity  — (entity) => void — called when another entity is clicked (replaces panel content)
   *   children  — main content slot
   */
  let {
    entity = $bindable(null),
    onclose = undefined,
    onentity = undefined,
    children,
  } = $props();

  let expanded = $state(false);

  function handleClose() {
    entity = null;
    expanded = false;
    onclose?.();
  }

  function handlePopout() {
    // expanded toggled inside DetailPanel
  }

  let panelOpen = $derived(!!entity);
</script>

<div class="shell" class:split={panelOpen && !expanded}>
  <div
    class="main-area"
    class:compressed={panelOpen && !expanded}
    class:hidden={panelOpen && expanded}
  >
    {@render children?.()}
  </div>

  <DetailPanel
    {entity}
    bind:expanded
    onclose={handleClose}
    onpopout={handlePopout}
  />
</div>

<style>
  .shell {
    display: flex;
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .main-area {
    flex: 1;
    overflow: auto;
    transition: width var(--transition-normal) ease-out;
    min-width: 0;
  }

  /* When split: main takes 60%, panel takes 40% (panel handles its own width) */
  .main-area.compressed {
    flex: none;
    width: 60%;
    overflow: auto;
  }

  /* When popped out: hide main, panel takes full width */
  .main-area.hidden {
    display: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .main-area { transition: none; }
  }
</style>
