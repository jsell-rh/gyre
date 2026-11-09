<script>
  /**
   * SideBySideView — CSS Grid two-pane composite layout
   * (ui-layout.md §10 Layout Engines: `side-by-side` → CSS Grid 50/50 or 60/40).
   *
   * Two sub-views render independently in a CSS Grid: the left pane is the
   * SVG graph canvas, the right pane is the virtualized node list. The split
   * ratio toggles between 50/50 and 60/40. Both panes' interactive elements
   * emit standardized ViewEvents handled uniformly by the Explorer shell.
   */
  import SvgGraphCanvas from './SvgGraphCanvas.svelte';
  import VirtualNodeList from './VirtualNodeList.svelte';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    onSelectNode = undefined,
  } = $props();

  let split = $state('50/50'); // '50/50' | '60/40'

  let columns = $derived(split === '50/50' ? '1fr 1fr' : '3fr 2fr');
</script>

<div class="sbs-view">
  <div class="sbs-toolbar" role="group" aria-label="Split ratio">
    <button
      type="button"
      class:active={split === '50/50'}
      aria-pressed={split === '50/50'}
      onclick={() => (split = '50/50')}
    >50 / 50</button>
    <button
      type="button"
      class:active={split === '60/40'}
      aria-pressed={split === '60/40'}
      onclick={() => (split = '60/40')}
    >60 / 40</button>
  </div>
  <!-- CSS Grid two-pane layout (ui-layout.md §10 side-by-side) -->
  <div class="sbs-grid" style="grid-template-columns: {columns}">
    <div class="sbs-pane" role="region" aria-label="Left view: graph">
      <SvgGraphCanvas {nodes} {edges} {repoId} engine="hierarchical" {onSelectNode} />
    </div>
    <div class="sbs-pane" role="region" aria-label="Right view: node list">
      <VirtualNodeList {nodes} {onSelectNode} viewportHeight={520} />
    </div>
  </div>
</div>

<style>
  .sbs-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 320px;
    gap: 6px;
  }
  .sbs-toolbar {
    display: flex;
    gap: 6px;
  }
  .sbs-toolbar button {
    background: var(--color-bg, #0f172a);
    color: #cbd5e1;
    border: 1px solid #334155;
    border-radius: 6px;
    font-size: 12px;
    padding: 2px 10px;
    cursor: pointer;
  }
  .sbs-toolbar button.active {
    border-color: #22d3ee;
    color: #22d3ee;
  }
  .sbs-grid {
    display: grid;
    flex: 1 1 auto;
    min-height: 0;
    gap: 8px;
  }
  .sbs-pane {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
