<script>
  /** Sidebar.svelte — Permanent 6-item sidebar (HSI §1.3)
   *
   * Props:
   *   activeItem: string — which sidebar item is active
   *   collapsed: boolean — whether sidebar is in icon-only mode (48px)
   *   onNavigate: (item: string) => void — called when user clicks a sidebar item
   *   onToggleCollapse: () => void — called when user clicks the collapse toggle
   *   decisionsCount: number — badge count for Inbox item
   *   serverVersion: string|null — server version string shown in the footer (HSI ui-layout §1)
   */

  import { t } from 'svelte-i18n';

  let {
    activeItem = 'inbox',
    collapsed = false,
    onNavigate = () => {},
    onToggleCollapse = () => {},
    decisionsCount = 0,
    serverVersion = null,
  } = $props();

  /** The six sidebar items — order and content are fixed per HSI §1.3 */
  const SIDEBAR_ITEMS = [
    { id: 'inbox',      label: 'Inbox',      shortcut: '1' },
    { id: 'briefing',   label: 'Briefing',   shortcut: '2' },
    { id: 'explorer',   label: 'Explorer',   shortcut: '3' },
    { id: 'specs',      label: 'Specs',      shortcut: '4' },
    { id: 'meta-specs', label: 'Meta-specs', shortcut: '5' },
    { id: 'admin',      label: 'Admin',      shortcut: '6' },
  ];
</script>

<aside
  class="sidebar"
  class:collapsed
  aria-label="Main navigation"
  data-testid="sidebar"
>
  <nav class="sidebar-nav">
    <ul class="sidebar-items" role="list">
      {#each SIDEBAR_ITEMS as item (item.id)}
        <li>
          <button
            class="sidebar-item"
            class:active={activeItem === item.id}
            onclick={() => onNavigate(item.id)}
            aria-current={activeItem === item.id ? 'page' : undefined}
            title={collapsed ? `${item.label} (${$t('topbar.search_shortcut').startsWith('⌘') ? '⌘' : 'Ctrl+'}${item.shortcut})` : undefined}
            data-testid={`sidebar-item-${item.id}`}
          >
            <span class="sidebar-icon" aria-hidden="true">
              {#if item.id === 'inbox'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/>
                  <path d="M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z"/>
                </svg>
              {:else if item.id === 'briefing'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
                  <polyline points="14 2 14 8 20 8"/>
                  <line x1="16" y1="13" x2="8" y2="13"/>
                  <line x1="16" y1="17" x2="8" y2="17"/>
                  <polyline points="10 9 9 9 8 9"/>
                </svg>
              {:else if item.id === 'explorer'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <circle cx="12" cy="12" r="10"/>
                  <polygon points="16.24 7.76 14.12 14.12 7.76 16.24 9.88 9.88 16.24 7.76"/>
                </svg>
              {:else if item.id === 'specs'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <path d="M4 19.5A2.5 2.5 0 016.5 17H20"/>
                  <path d="M6.5 2H20v20H6.5A2.5 2.5 0 014 19.5v-15A2.5 2.5 0 016.5 2z"/>
                </svg>
              {:else if item.id === 'meta-specs'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/>
                </svg>
              {:else if item.id === 'admin'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
                  <circle cx="12" cy="12" r="3"/>
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
                </svg>
              {/if}
            </span>
            {#if !collapsed}
              <span class="sidebar-label">{item.label}</span>
              {#if item.id === 'inbox' && decisionsCount > 0}
                <span class="sidebar-badge" aria-label="{decisionsCount} pending">{decisionsCount > 99 ? '99+' : decisionsCount}</span>
              {/if}
            {:else}
              {#if item.id === 'inbox' && decisionsCount > 0}
                <span class="sidebar-badge sidebar-badge-collapsed" aria-label="{decisionsCount} pending">{decisionsCount > 9 ? '9+' : decisionsCount}</span>
              {/if}
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <div class="sidebar-footer">
    <button
      class="sidebar-collapse-btn"
      onclick={onToggleCollapse}
      aria-label={collapsed ? $t('nav.sidebar.expand') : $t('nav.sidebar.collapse')}
      title={collapsed ? $t('nav.sidebar.expand') : $t('nav.sidebar.collapse')}
      data-testid="sidebar-collapse-btn"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
        {#if collapsed}
          <path d="M9 18l6-6-6-6"/>
        {:else}
          <path d="M15 18l-6-6 6-6"/>
        {/if}
      </svg>
    </button>
    {#if serverVersion && !collapsed}
      <span class="sidebar-version" data-testid="sidebar-version" title={$t('nav.sidebar.server_version', { values: { version: serverVersion } })}>
        v{serverVersion}
      </span>
    {/if}
  </div>
</aside>

<style>
  .sidebar {
    width: var(--sidebar-width);
    min-width: var(--sidebar-width);
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border);
    transition: width var(--transition-normal), min-width var(--transition-normal);
    overflow: hidden;
    flex-shrink: 0;
  }

  .sidebar.collapsed {
    width: var(--sidebar-collapsed);
    min-width: var(--sidebar-collapsed);
  }

  .sidebar-nav {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-2) 0;
  }

  .sidebar-items {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .sidebar-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: calc(100% - var(--space-2) * 2);
    margin: 0 var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    text-align: left;
    transition: background var(--transition-fast), color var(--transition-fast);
    position: relative;
    white-space: nowrap;
    overflow: hidden;
  }

  .sidebar.collapsed .sidebar-item {
    justify-content: center;
    padding: var(--space-2);
    gap: 0;
  }

  .sidebar-item:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .sidebar-item.active {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    font-weight: 500;
  }

  .sidebar-item.active::before {
    content: '';
    position: absolute;
    left: 0;
    top: var(--space-1);
    bottom: var(--space-1);
    width: 3px;
    background: var(--color-primary);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  .sidebar.collapsed .sidebar-item.active::before {
    left: 0;
    top: 50%;
    bottom: auto;
    transform: translateY(-50%);
    width: 3px;
    height: 16px;
  }

  .sidebar-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .sidebar-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 18px;
    height: 18px;
  }

  .sidebar-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sidebar-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 var(--space-1);
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
    flex-shrink: 0;
  }

  .sidebar-badge-collapsed {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 14px;
    height: 14px;
    font-size: 9px;
    padding: 0 2px;
  }

  .sidebar-footer {
    padding: var(--space-2);
    border-top: 1px solid var(--color-border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .sidebar-version {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sidebar.collapsed .sidebar-footer {
    justify-content: center;
  }

  .sidebar-collapse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .sidebar-collapse-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .sidebar-collapse-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* Hide sidebar on mobile — the mobile drawer serves as navigation */
  @media (max-width: 768px) {
    .sidebar {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .sidebar,
    .sidebar-item,
    .sidebar-collapse-btn { transition: none; }
  }
</style>
