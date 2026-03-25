<script>
  let {
    tabs = [],
    active = $bindable(''),
    onchange = undefined,
    panelId = undefined,
  } = $props();

  $effect(() => {
    if (!active && tabs.length > 0) active = tabs[0].id;
  });

  function select(id) {
    active = id;
    onchange?.(id);
  }

  function onkeydown(e, id, index) {
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      const next = tabs[(index + 1) % tabs.length];
      if (!next.disabled) select(next.id);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      const prev = tabs[(index - 1 + tabs.length) % tabs.length];
      if (!prev.disabled) select(prev.id);
    } else if (e.key === 'Home') {
      e.preventDefault();
      const first = tabs.find(t => !t.disabled);
      if (first) select(first.id);
    } else if (e.key === 'End') {
      e.preventDefault();
      const last = [...tabs].reverse().find(t => !t.disabled);
      if (last) select(last.id);
    }
  }
</script>

<div class="tabs-bar" role="tablist">
  {#each tabs as tab, i}
    <button
      role="tab"
      id="tab-{tab.id}"
      aria-selected={active === tab.id}
      aria-controls={panelId ?? `tabpanel-${tab.id}`}
      tabindex={active === tab.id ? 0 : -1}
      class="tab-btn"
      class:active={active === tab.id}
      onclick={() => select(tab.id)}
      onkeydown={(e) => onkeydown(e, tab.id, i)}
      disabled={tab.disabled}
      title={tab.title}
    >
      {#if tab.icon}
        <span class="tab-icon" aria-hidden="true">{@html tab.icon}</span>
      {/if}
      {tab.label}
      {#if tab.count !== undefined}
        <span class="tab-count" aria-label="{tab.count} items">{tab.count}</span>
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
