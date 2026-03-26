<script>
  let {
    currentNav = $bindable('inbox'),
    onnavigate = undefined,
    inboxBadge = 0,
    wsStatus = 'disconnected',
  } = $props();

  let collapsed = $state(false);

  const NAV_ITEMS = [
    { id: 'briefing',   label: 'Briefing',   shortcut: '1' },
    { id: 'explorer',   label: 'Explorer',   shortcut: '2' },
    { id: 'specs',      label: 'Specs',      shortcut: '3' },
    { id: 'meta-specs', label: 'Meta-specs', shortcut: '4' },
    { id: 'admin',      label: 'Admin',      shortcut: '5' },
    { id: 'inbox',      label: 'Inbox',      shortcut: '6' },
  ];

  function nav(id) {
    currentNav = id;
    onnavigate?.(id);
  }

  // SVG icons for each nav item
  const ICONS = {
    inbox: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z"/></svg>',
    briefing: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>',
    explorer: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/><circle cx="11" cy="11" r="3"/></svg>',
    specs: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>',
    'meta-specs': '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="12" cy="12" r="9"/><path d="M12 8v4"/><circle cx="12" cy="16" r="0.5" fill="currentColor"/><path d="M8 12h1.5M14.5 12H16"/></svg>',
    admin: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>',
  };
</script>

<nav
  class="sidebar"
  class:collapsed
  aria-label="Main navigation"
  data-testid="sidebar"
>
  <!-- Logo + collapse toggle -->
  <div class="logo">
    {#if !collapsed}
      <div class="logo-mark" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none" width="22" height="22" aria-hidden="true">
          <circle cx="12" cy="12" r="10" stroke="var(--color-primary)" stroke-width="2"/>
          <path d="M8 12l3 3 5-5" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </div>
      <span class="logo-text">Gyre</span>
    {:else}
      <span class="sr-only">Gyre</span>
    {/if}
    <button
      class="collapse-btn"
      onclick={() => (collapsed = !collapsed)}
      aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-expanded={!collapsed}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        {#if collapsed}
          <path d="M9 18l6-6-6-6"/>
        {:else}
          <path d="M15 18l-6-6 6-6"/>
        {/if}
      </svg>
    </button>
  </div>

  <!-- 6 fixed nav items -->
  <ul class="nav-list" role="list" aria-label="Navigation">
    {#each NAV_ITEMS as item}
      <li>
        <button
          class="nav-item"
          class:active={currentNav === item.id}
          onclick={() => nav(item.id)}
          aria-label={collapsed ? item.label : undefined}
          aria-current={currentNav === item.id ? 'page' : undefined}
          title={collapsed ? `${item.label} (⌘${item.shortcut})` : undefined}
        >
          <span class="nav-icon" aria-hidden="true">{@html ICONS[item.id]}</span>
          {#if !collapsed}
            <span class="nav-label">{item.label}</span>
            {#if item.id === 'inbox' && inboxBadge > 0}
              <span class="nav-badge" aria-label="{inboxBadge} unresolved">{inboxBadge > 99 ? '99+' : inboxBadge}</span>
            {/if}
            <span class="nav-shortcut" aria-hidden="true">⌘{item.shortcut}</span>
          {:else if item.id === 'inbox' && inboxBadge > 0}
            <span class="nav-badge-dot" aria-label="{inboxBadge} unresolved"></span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <!-- Bottom: server version -->
  <div class="sidebar-footer">
    <div class="version-indicator" role="status" aria-label="Server version">
      <span class="version-dot" class:connected={wsStatus === 'connected'} class:error={wsStatus === 'error' || wsStatus === 'auth-failed'} aria-hidden="true"></span>
      {#if !collapsed}
        <span class="version-text">v0.1.0</span>
      {/if}
    </div>
  </div>
</nav>

<style>
  .sidebar {
    width: var(--sidebar-width);
    min-width: var(--sidebar-width);
    background: var(--color-surface);
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    transition: width var(--transition-normal), min-width var(--transition-normal);
    overflow: hidden;
    flex-shrink: 0;
  }

  .sidebar.collapsed {
    width: var(--sidebar-collapsed);
    min-width: var(--sidebar-collapsed);
  }

  /* Logo */
  .logo {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    height: var(--topbar-height);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .sidebar.collapsed .logo {
    padding: 0 var(--space-1);
    justify-content: center;
  }

  .sidebar.collapsed .collapse-btn {
    margin-left: 0;
  }

  .logo-mark {
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .logo-text {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    flex: 1;
    white-space: nowrap;
  }

  .collapse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius);
    padding: var(--space-1);
    margin-left: auto;
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .collapse-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  /* Nav list */
  .nav-list {
    flex: 1;
    list-style: none;
    padding: var(--space-2) 0;
    margin: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .nav-list li {
    padding: 0 var(--space-2);
    margin-bottom: var(--space-2);
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3) var(--space-2);
    border: none;
    border-left: 2px solid transparent;
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: 0 var(--radius) var(--radius) 0;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    text-align: left;
    white-space: nowrap;
    transition: background var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast);
    position: relative;
    margin-left: calc(-1 * var(--space-2));
    width: calc(100% + var(--space-2));
  }

  .nav-item:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .nav-item.active {
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    color: var(--color-text);
    border-left-color: var(--color-primary);
    font-weight: 500;
  }

  .nav-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 20px;
  }

  .nav-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .nav-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    background: var(--color-primary);
    color: var(--color-text-inverse, #fff);
    border-radius: 999px;
    font-size: 0.6rem;
    font-weight: 700;
    flex-shrink: 0;
  }

  /* Small dot badge when collapsed */
  .nav-badge-dot {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-primary);
    flex-shrink: 0;
  }

  .nav-shortcut {
    font-size: 0.6rem;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    opacity: 0;
    transition: opacity var(--transition-fast);
    flex-shrink: 0;
  }

  .nav-item:hover .nav-shortcut {
    opacity: 1;
  }

  /* Footer */
  .sidebar-footer {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3) var(--space-4);
    flex-shrink: 0;
  }

  .version-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .version-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-warning);
    box-shadow: none;
    flex-shrink: 0;
  }

  .version-dot.connected {
    background: var(--color-success);
    box-shadow: 0 0 5px color-mix(in srgb, var(--color-success) 60%, transparent);
  }

  .version-dot.error {
    background: var(--color-danger);
    box-shadow: none;
  }

  .version-text {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .nav-item:focus-visible,
  .collapse-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    white-space: nowrap;
    border: 0;
  }
</style>
