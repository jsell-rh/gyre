<script>
  let {
    breadcrumb = [],
    onNavigate = () => {},
  } = $props();
</script>

{#if breadcrumb.length > 0}
  <div class="treemap-breadcrumb" role="navigation" aria-label="Drill-down path">
    <button class="breadcrumb-item root" onclick={() => onNavigate(-1)} type="button" aria-label="Go to root">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>
    </button>
    {#each breadcrumb as crumb, i}
      <span class="breadcrumb-sep" aria-hidden="true">&rsaquo;</span>
      <button class="breadcrumb-item" class:current={i === breadcrumb.length - 1} onclick={() => onNavigate(i)} type="button">{crumb.name}</button>
    {/each}
  </div>
{/if}

<style>
  .treemap-breadcrumb {
    display: flex; align-items: center; gap: 2px;
    padding: 4px 10px; background: rgba(15,23,42,0.8);
    backdrop-filter: blur(8px); border-radius: 6px;
    font-size: 12px; position: absolute; bottom: 14px; left: 50%;
    transform: translateX(-50%); z-index: 10;
    box-shadow: 0 2px 8px rgba(0,0,0,0.4);
  }
  .breadcrumb-item {
    padding: 3px 8px; border-radius: 4px; cursor: pointer;
    color: #94a3b8; background: transparent; border: none; font-size: 12px;
    white-space: nowrap; max-width: 150px; overflow: hidden; text-overflow: ellipsis;
  }
  .breadcrumb-item:hover { background: #1e293b; color: #e2e8f0; }
  .breadcrumb-item.current { color: #e2e8f0; font-weight: 600; }
  .breadcrumb-item.root { padding: 3px 6px; }
  .breadcrumb-sep { color: #475569; font-size: 14px; }
</style>
