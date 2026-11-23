<script>
  /**
   * Tabs — shadcn-svelte tabs (WAI-ARIA Tabs pattern, manual activation).
   *
   * The consumed contract is kept exactly:
   *   - `tabs` array ({ id, label, icon?, count?, disabled? })
   *   - `active` (bindable) + `onchange(id)`
   *   - triggers render as <button role="tab" id="tab-{id}"> with
   *     aria-controls pointing at the caller-rendered panel
   *     (`panelId` or `tabpanel-{id}`) — panels are authored by the
   *     consuming view, not by this component
   *   - roving tabindex, ArrowLeft/ArrowRight/Home/End with wrap-around
   *     and disabled-skip, handled on the tablist (keyboard events reach
   *     it from the focused trigger by bubbling)
   *
   * bits-ui's Tabs primitives are not used here: its Trigger derives
   * aria-controls from Tabs.Content instances that must live inside the
   * same Tabs.Root, while this component's callers own the panels. The
   * aria/id contract above (fixed `tab-{id}` / `tabpanel-{id}` wiring) is
   * part of the consumed accessibility contract and cannot be satisfied
   * by the bits-ui content-registration model.
   */
  import { untrack } from 'svelte';
  import { cn } from '$lib/utils.js';
  import { tabTriggerVariants } from './tabs.svelte.js';

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

  function onkeydown(e) {
    let target = null;
    const index = tabs.findIndex(t => t.id === active);
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

<div class="tabs-bar flex border-b border-border" role="tablist" aria-label={ariaLabel || undefined} onkeydown={onkeydown}>
  {#each tabs as tab (tab.id)}
    <button
      role="tab"
      id="tab-{tab.id}"
      data-id={tab.id}
      data-state={active === tab.id ? 'active' : 'inactive'}
      aria-selected={active === tab.id}
      aria-controls={panelId ?? `tabpanel-${tab.id}`}
      tabindex={active === tab.id ? 0 : -1}
      class={cn('tab-btn', tabTriggerVariants({ active: active === tab.id }))}
      onclick={() => select(tab.id)}
      disabled={tab.disabled}
    >
      {#if tab.icon}
        <span class="tab-icon flex items-center" aria-hidden="true">{@html tab.icon}</span>
      {/if}
      {tab.label}
      {#if tab.count !== undefined}
        <span
          class={cn(
            'tab-count inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded-full text-xs font-semibold',
            active === tab.id ? 'bg-ring/15 text-ring' : 'bg-surface-elevated text-muted-foreground'
          )}
          aria-hidden="true"
        >{tab.count}</span>
      {/if}
    </button>
  {/each}
</div>
