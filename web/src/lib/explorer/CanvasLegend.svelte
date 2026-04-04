<script>
  let {
    items = [], // Array of [label, color] tuples
  } = $props();
</script>

{#if items.length > 0}
  <div class="treemap-legend">
    {#each items as [label, color]}
      <span class="legend-item">
        <span class="legend-swatch" style="background: {color}"></span>
        <span class="legend-label">{label}</span>
      </span>
    {/each}
  </div>
{/if}

<style>
  .treemap-legend { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .legend-item { display: flex; align-items: center; gap: 3px; font-size: 10px; color: #94a3b8; white-space: nowrap; }
  .legend-swatch { width: 10px; height: 10px; border-radius: 2px; flex-shrink: 0; }
  .legend-label { font-size: 10px; }
</style>
