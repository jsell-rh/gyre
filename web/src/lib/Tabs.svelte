<script>
  let {
    tabs = [],
    active = $bindable(''),
    onchange = undefined,
  } = $props();

  $effect(() => {
    if (!active && tabs.length > 0) active = tabs[0].id;
  });

  function select(id) {
    active = id;
    onchange?.(id);
  }
</script>

<div class="tabs-bar" role="tablist">
  {#each tabs as tab}
    <button
      role="tab"
      aria-selected={active === tab.id}
      class="tab-btn"
      class:active={active === tab.id}
      onclick={() => select(tab.id)}
      disabled={tab.disabled}
    >
      {#if tab.icon}
        <span class="tab-icon">{@html tab.icon}</span>
      {/if}
      {tab.label}
      {#if tab.count !== undefined}
        <span class="tab-count">{tab.count}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .tabs-bar {
    display: flex;
    border-bottom: 1px solid var(--color-border);
    gap: 0;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover:not(:disabled) {
    color: var(--color-text);
  }

  .tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
  }

  .tab-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .tab-count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 var(--space-1);
    background: var(--color-surface-elevated);
    border-radius: 9px;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-weight: 600;
  }

  .tab-btn.active .tab-count {
    background: rgba(238, 0, 0, 0.12);
    color: var(--color-primary);
  }

  .tab-icon {
    display: flex;
    align-items: center;
  }
</style>
