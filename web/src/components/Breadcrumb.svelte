<script>
  let {
    breadcrumb = [],
    onNavigate = () => {},
    onReset = () => {},
  } = $props();
</script>

{#if breadcrumb.length > 0}
  <div class="treemap-breadcrumb" role="navigation" aria-label="Drill-down path">
    <button class="breadcrumb-item root" onclick={onReset} type="button" aria-label="Go to root">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>
      Root
    </button>
    {#each breadcrumb as crumb, i}
      <span class="breadcrumb-sep" aria-hidden="true">&rsaquo;</span>
      <button class="breadcrumb-item" class:current={i === breadcrumb.length - 1} onclick={() => { onNavigate(i); }} type="button">{crumb.name}</button>
    {/each}
  </div>
{/if}

<style>
  .treemap-breadcrumb {
    display: flex; align-items: center; gap: 4px;
    padding: 6px 12px; border-top: 1px solid #1e293b;
    background: rgba(15,15,26,0.95); flex-shrink: 0; overflow-x: auto;
  }
  .breadcrumb-item {
    display: flex; align-items: center; gap: 4px;
    padding: 3px 10px; background: transparent; border: none; border-radius: 4px;
    color: #60a5fa; font-size: 12px; font-family: 'SF Mono', Menlo, monospace;
    cursor: pointer; transition: background 0.15s; white-space: nowrap;
  }
  .breadcrumb-item:hover { background: #1e293b; }
  .breadcrumb-item.current { color: #f1f5f9; font-weight: 600; }
  .breadcrumb-item.root { color: #94a3b8; }
  .breadcrumb-sep { color: #475569; font-size: 14px; user-select: none; }

  @media (prefers-reduced-motion: reduce) {
    .breadcrumb-item { transition: none; }
  }
</style>
