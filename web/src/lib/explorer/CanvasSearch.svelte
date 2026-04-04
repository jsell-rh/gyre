<script>
  import { t } from 'svelte-i18n';

  let {
    open = $bindable(false),
    query = $bindable(''),
    results = [],
    currentIndex = 0,
    onSelectResult = () => {},
    onClose = () => {},
  } = $props();

  let inputEl = $state(null);

  $effect(() => {
    if (open) {
      requestAnimationFrame(() => inputEl?.focus());
    }
  });

  function handleKeydown(e) {
    if (e.key === 'Enter') {
      if (results.length > 0) {
        onSelectResult(results[currentIndex % results.length]);
      }
    } else if (e.key === 'Escape') {
      onClose();
    }
  }
</script>

{#if open}
  <div class="canvas-search">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
    <input
      bind:this={inputEl}
      type="text"
      class="canvas-search-input"
      bind:value={query}
      onkeydown={handleKeydown}
      placeholder="Search nodes... (Esc to close)"
      aria-label="Search nodes"
    />
    {#if query.trim()}
      <span class="canvas-search-count">{results.length} matches</span>
    {/if}
    <button class="canvas-search-close" onclick={onClose} aria-label="Close search" type="button">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
    </button>
    {#if query.trim() && results.length > 0}
      <div class="canvas-search-results" role="listbox" aria-label="Search results">
        {#each results.slice(0, 20) as result, i}
          <button class="search-result-item" role="option"
            class:selected={i === currentIndex}
            onclick={() => onSelectResult(result)}
            type="button"
          >
            <span class="result-type">{result.node_type}</span>
            <span class="result-name">{result.name}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .canvas-search {
    position: absolute; top: 52px; left: 50%; transform: translateX(-50%);
    background: rgba(15,23,42,0.95); backdrop-filter: blur(12px);
    border: 1px solid #334155; border-radius: 8px; padding: 8px 12px;
    display: flex; align-items: center; gap: 8px; z-index: 20;
    box-shadow: 0 4px 16px rgba(0,0,0,0.5); min-width: 300px;
    flex-wrap: wrap;
  }
  .canvas-search-input {
    flex: 1; min-width: 200px; background: transparent; border: none;
    color: #e2e8f0; font-size: 13px; outline: none;
  }
  .canvas-search-input::placeholder { color: #64748b; }
  .canvas-search-count { font-size: 11px; color: #64748b; white-space: nowrap; }
  .canvas-search-close {
    background: none; border: none; color: #64748b; cursor: pointer; padding: 4px;
  }
  .canvas-search-close:hover { color: #e2e8f0; }
  .canvas-search-results {
    width: 100%; max-height: 200px; overflow-y: auto;
    border-top: 1px solid #334155; padding-top: 4px; margin-top: 2px;
    display: flex; flex-direction: column; gap: 1px;
  }
  .search-result-item {
    display: flex; align-items: center; gap: 6px; padding: 4px 8px;
    background: transparent; border: none; border-radius: 4px;
    color: #e2e8f0; font-size: 12px; cursor: pointer; text-align: left; width: 100%;
  }
  .search-result-item:hover, .search-result-item.selected { background: #1e293b; }
  .result-type { font-size: 10px; color: #64748b; min-width: 60px; }
  .result-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
