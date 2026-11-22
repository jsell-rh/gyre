<script>
  /**
   * NavRail — ui-navigation.md §1a Collapsible Navigation Rail (Optional)
   *
   * An optional, collapsible, scope-labelled left panel on navigation-heavy
   * screens (workspace home and repo mode only; App.svelte decides whether
   * the current surface is navigation-heavy and simply does not render the
   * rail otherwise, so focused editor/canvas surfaces keep full width).
   *
   * Contract (§1a "Pattern"):
   *   - Collapsed (icon-only, 48px) by default; toggle expands to the
   *     labelled form (240px).
   *   - The expanded/collapsed preference is persisted per user per mode
   *     in localStorage; first use defaults to collapsed.
   *   - Scope-labelled header per mode; the item set is per-mode with a
   *     stable order (items never move, appear, or disappear within a mode).
   *   - Never load-bearing: every destination stays reachable without the
   *     rail (dashboard sections, tab strip, ⌘K, g-shortcuts).
   *   - On narrow viewports (<768px) the rail is not rendered persistently;
   *     its item set is offered through the §8 hamburger drawer instead.
   *
   * The component has no knowledge of destinations: App.svelte passes the
   * per-mode item set (mirroring the §2 dashboard sections / §3 repo tab
   * strip) and an onSelect callback, so the rail introduces no second
   * source of truth for navigation.
   */

  let {
    mode = 'workspace-home',
    scopeLabel = '',
    items = [],
    activeId = null,
    onSelect = undefined,
    userKey = 'anonymous',
  } = $props();

  const STORAGE_PREFIX = 'gyre_nav_rail_expanded';
  // Per user per mode (§1a): the key combines the rail's mode with the
  // signed-in user's id (App passes selfUserId). $derived so a mode or user
  // change re-hydrates the preference below.
  let storageKey = $derived(`${STORAGE_PREFIX}.${mode}.${userKey}`);

  function readPersistedExpanded(key) {
    try {
      return localStorage.getItem(key) === 'expanded';
    } catch {
      return false; // private browsing / storage disabled: default collapsed
    }
  }

  let expanded = $state(false);

  // Hydrate (and re-hydrate on mode/user change) from localStorage; first
  // use defaults to collapsed (§1a "Collapsed by default").
  $effect(() => {
    const key = storageKey;
    expanded = readPersistedExpanded(key);
  });

  function toggleExpanded() {
    expanded = !expanded;
    try {
      localStorage.setItem(storageKey, expanded ? 'expanded' : 'collapsed');
    } catch {
      /* private browsing: preference not persisted, toggle still works */
    }
  }

  // Per-item icons. Ids come from the two §1a item sets: the workspace-home
  // sections (decisions/specs/architecture/repos/briefing/agent-rules/
  // settings) and the repo tab strip (specs/tasks/mrs/agents/architecture/
  // dependencies/decisions/code/settings).
  const ICONS = {
    decisions:    '<path d="M18 8A6 6 0 006 8c0 7-3 9-3 9h18s-3-2-3-9"/><path d="M13.73 21a2 2 0 01-3.46 0"/>',
    specs:        '<path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="15" y2="17"/>',
    architecture: '<circle cx="12" cy="5" r="2.5"/><circle cx="5" cy="19" r="2.5"/><circle cx="19" cy="19" r="2.5"/><path d="M12 7.5V12m0 0L6.5 17M12 12l5.5 5"/>',
    repos:        '<path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>',
    briefing:     '<path d="M4 5h16v11H8l-4 4z"/><line x1="8" y1="9" x2="16" y2="9"/><line x1="8" y1="12" x2="13" y2="12"/>',
    'agent-rules': '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/>',
    settings:     '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
    tasks:        '<path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/>',
    mrs:          '<path d="M8 3H6a2 2 0 00-2 2v4m0 6v6h6m6-18h2a2 2 0 012 2v4m0 6v6h-6"/><path d="M9 12h6"/>',
    agents:       '<circle cx="12" cy="8" r="4"/><path d="M4 20c0-4 3.6-7 8-7s8 3 8 7"/>',
    dependencies: '<circle cx="6" cy="6" r="3"/><circle cx="18" cy="18" r="3"/><path d="M9 6h6a3 3 0 013 3v6"/>',
    code:         '<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>',
  };
  function iconSvg(id) {
    return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18" aria-hidden="true">${ICONS[id] ?? ICONS.specs}</svg>`;
  }
</script>

<nav
  class="nav-rail"
  class:expanded
  data-testid="nav-rail"
  data-rail-mode={mode}
  aria-label={scopeLabel ? `${scopeLabel} navigation rail` : 'navigation rail'}
>
  <!-- Scope label (§1a "Scope-labelled"): workspace name at workspace
       home; workspace / repo path in repo mode. Collapsed shows a scope
       glyph with the full label as tooltip. -->
  <div class="nav-rail-scope" data-testid="nav-rail-scope" title={scopeLabel}>
    {#if expanded}
      <span class="nav-rail-scope-label">{scopeLabel}</span>
    {:else}
      <span class="nav-rail-scope-glyph" aria-hidden="true">◆</span>
    {/if}
  </div>

  <button
    class="nav-rail-toggle"
    onclick={toggleExpanded}
    aria-expanded={expanded}
    aria-label={expanded ? 'Collapse navigation rail' : 'Expand navigation rail'}
    title={expanded ? 'Collapse navigation rail' : 'Expand navigation rail'}
    data-testid="nav-rail-toggle"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
      {#if expanded}
        <path d="M15 6l-6 6 6 6"/>
      {:else}
        <path d="M9 6l6 6-6 6"/>
      {/if}
    </svg>
  </button>

  <ul class="nav-rail-items" role="list">
    {#each items as item (item.id)}
      <li>
        <button
          class="nav-rail-item"
          class:active={activeId === item.id}
          onclick={() => onSelect?.(item.id)}
          data-testid={`nav-rail-item-${item.id}`}
          aria-current={activeId === item.id ? 'page' : undefined}
          title={item.label}
        >
          <span class="nav-rail-icon">{@html iconSvg(item.id)}</span>
          {#if expanded}
            <span class="nav-rail-label">{item.label}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  /* Collapsed: icon-only 48px (§1a). Expanded: labelled 240px. */
  .nav-rail {
    width: 48px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border);
    overflow: hidden;
    transition: width var(--transition-fast, 150ms ease);
  }

  .nav-rail.expanded { width: 240px; }

  .nav-rail-scope {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 40px;
    padding: var(--space-1, 4px) var(--space-2, 8px);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font-size: var(--text-xs, 11px);
    white-space: nowrap;
  }

  .nav-rail-scope-label {
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
    color: var(--color-text);
  }

  .nav-rail-scope-glyph { font-size: 10px; line-height: 1; }

  .nav-rail-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 28px;
    border: none;
    border-bottom: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .nav-rail-toggle:hover { color: var(--color-text); background: var(--color-surface-hover, rgba(0,0,0,0.04)); }
  .nav-rail-toggle:focus-visible { outline: 2px solid var(--color-primary); outline-offset: -2px; }

  .nav-rail-items {
    list-style: none;
    margin: 0;
    padding: var(--space-1, 4px) 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }

  .nav-rail-item {
    display: flex;
    align-items: center;
    gap: var(--space-3, 12px);
    height: 36px;
    padding: 0 14px;
    border: none;
    border-radius: 6px;
    margin: 0 var(--space-1, 4px);
    background: transparent;
    color: var(--color-text-muted);
    font-size: var(--text-sm, 13px);
    cursor: pointer;
    white-space: nowrap;
    text-align: left;
  }

  .nav-rail.expanded .nav-rail-item { padding: 0 12px; }

  .nav-rail-icon { display: inline-flex; flex-shrink: 0; }

  .nav-rail-label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .nav-rail-item:hover { color: var(--color-text); background: var(--color-surface-hover, rgba(0,0,0,0.04)); }

  .nav-rail-item:focus-visible { outline: 2px solid var(--color-primary); outline-offset: -2px; }

  /* Active destination is highlighted exactly as in the tab strip /
     dashboard section (§1a). */
  .nav-rail-item.active {
    color: var(--color-primary);
    background: var(--color-primary-muted, rgba(59,130,246,0.12));
    font-weight: 600;
  }

  /* Narrow viewports (<768px): no persistent rail (§1a "Narrow viewports").
     The per-mode item set is offered through the §8 hamburger drawer. */
  @media (max-width: 767px) {
    .nav-rail { display: none; }
  }
</style>
