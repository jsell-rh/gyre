<script>
  import { untrack } from 'svelte';

  let {
    tabs = [],
    active = $bindable(''),
    onchange = undefined,
    panelId = undefined,
    ariaLabel = 'Tab navigation',
  } = $props();

  $effect(() => {
    // Read tabs reactively so this re-runs when tabs change,
    // but read active via untrack to avoid circular dependency
    // that causes state_unsafe_mutation during derived recalculation.
    const currentTabs = tabs;
    const currentActive = untrack(() => active);
    if ((!currentActive || !currentTabs.some(t => t.id === currentActive)) && currentTabs.length > 0) {
      active = currentTabs[0].id;
    }
  });

  function select(id) {
    active = id;
    onchange?.(id);
  }

  function focusTab(id) {
    document.querySelector('[role="tab"][data-id="' + id + '"]')?.focus();
  }

  function findNext(index) {
    for (let step = 1; step < tabs.length; step++) {
      const candidate = tabs[(index + step) % tabs.length];
      if (!candidate.disabled) return candidate;
    }
    return null;
  }

  function findPrev(index) {
    for (let step = 1; step < tabs.length; step++) {
      const candidate = tabs[(index - step + tabs.length) % tabs.length];
      if (!candidate.disabled) return candidate;
    }
    return null;
  }

  function onkeydown(e, id, index) {
    let target = null;
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      target = findNext(index);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      target = findPrev(index);
    } else if (e.key === 'Home') {
      e.preventDefault();
      target = tabs.find(t => !t.disabled) ?? null;
    } else if (e.key === 'End') {
      e.preventDefault();
      target = [...tabs].reverse().find(t => !t.disabled) ?? null;
    }
    if (target) {
      select(target.id);
      focusTab(target.id);
    }
  }
</script>

<div class="tabs-bar" role="tablist" aria-label={ariaLabel || undefined}>
  {#each tabs as tab, i}
    <button
      role="tab"
      id="tab-{tab.id}"
      data-id={tab.id}
      aria-selected={active === tab.id}
      aria-controls={panelId ?? `tabpanel-${tab.id}`}
      tabindex={active === tab.id ? 0 : -1}
      class="tab-btn"
      class:active={active === tab.id}
      onclick={() => select(tab.id)}
      onkeydown={(e) => onkeydown(e, tab.id, i)}
      disabled={tab.disabled}
    >
      {#if tab.icon}
        <span class="tab-icon" aria-hidden="true">{@html tab.icon}</span>
      {/if}
      {tab.label}
      {#if tab.count !== undefined}
        <span class="tab-count" aria-hidden="true">{tab.count}</span>
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
    border-bottom-color: var(--color-link);
  }

  .tab-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .tab-count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 var(--space-1);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-weight: 600;
  }

  .tab-btn.active .tab-count {
    background: color-mix(in srgb, var(--color-focus) 12%, transparent);
    color: var(--color-focus);
  }

  .tab-icon {
    display: flex;
    align-items: center;
  }

  @media (prefers-reduced-motion: reduce) {
    .tab-btn {
      transition: none;
    }
  }
</style>
