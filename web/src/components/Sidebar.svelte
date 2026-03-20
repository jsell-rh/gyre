<script>
  let { current = $bindable('dashboard') } = $props();

  let collapsed = $state(false);

  const sections = [
    {
      label: 'Overview',
      items: [
        { id: 'dashboard', label: 'Dashboard', icon: dashboardIcon() },
        { id: 'activity',  label: 'Activity',  icon: activityIcon() },
      ],
    },
    {
      label: 'Source Control',
      items: [
        { id: 'projects',    label: 'Projects',    icon: projectsIcon() },
        { id: 'merge-queue', label: 'Merge Queue', icon: queueIcon() },
      ],
    },
    {
      label: 'Agents',
      items: [
        { id: 'agents',  label: 'Agents',  icon: agentsIcon() },
        { id: 'tasks',   label: 'Tasks',   icon: tasksIcon() },
        { id: 'compose', label: 'Compose', icon: composeIcon() },
      ],
    },
    {
      label: 'Operations',
      items: [
        { id: 'analytics',   label: 'Analytics',   icon: analyticsIcon() },
        { id: 'costs',       label: 'Costs',        icon: costsIcon() },
        { id: 'mcp-catalog', label: 'MCP Tools',    icon: mcpIcon() },
      ],
    },
    {
      label: 'Admin',
      items: [
        { id: 'admin',    label: 'Admin Panel', icon: adminIcon() },
        { id: 'settings', label: 'Settings',    icon: settingsIcon() },
      ],
    },
  ];

  function isActive(id) {
    if (id === 'projects') {
      return current === 'projects' || current === 'repo-detail' || current === 'mr-detail';
    }
    return current === id;
  }

  // SVG icon helpers — inline SVG strings
  function dashboardIcon()  { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>'; }
  function activityIcon()   { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>'; }
  function projectsIcon()   { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/></svg>'; }
  function queueIcon()      { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="12" cy="12" r="9"/><path d="M12 8v4l3 3"/></svg>'; }
  function agentsIcon()     { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0110 0v4"/><circle cx="12" cy="16" r="1" fill="currentColor"/></svg>'; }
  function tasksIcon()      { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>'; }
  function composeIcon()    { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 013 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>'; }
  function analyticsIcon()  { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/></svg>'; }
  function costsIcon()      { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 100 7h5a3.5 3.5 0 110 7H6"/></svg>'; }
  function mcpIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M14.7 6.3a1 1 0 000 1.4l1.6 1.6a1 1 0 001.4 0l3.77-3.77a6 6 0 01-7.94 7.94l-6.91 6.91a2.12 2.12 0 01-3-3l6.91-6.91a6 6 0 017.94-7.94l-3.76 3.76z"/></svg>'; }
  function adminIcon()      { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>'; }
  function settingsIcon()   { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>'; }
</script>

<nav class="sidebar" class:collapsed>
  <!-- Logo -->
  <div class="logo">
    <div class="logo-mark">
      <svg viewBox="0 0 24 24" fill="none" width="22" height="22">
        <circle cx="12" cy="12" r="10" stroke="var(--color-primary)" stroke-width="2"/>
        <path d="M8 12l3 3 5-5" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    {#if !collapsed}
      <span class="logo-text">Gyre</span>
    {/if}
    <button
      class="collapse-btn"
      onclick={() => (collapsed = !collapsed)}
      title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
        {#if collapsed}
          <path d="M9 18l6-6-6-6"/>
        {:else}
          <path d="M15 18l-6-6 6-6"/>
        {/if}
      </svg>
    </button>
  </div>

  <!-- Nav sections -->
  <div class="nav-body">
    {#each sections as section}
      <div class="nav-section">
        {#if !collapsed}
          <div class="section-label">{section.label}</div>
        {/if}
        <ul>
          {#each section.items as item}
            <li>
              <button
                class="nav-item"
                class:active={isActive(item.id)}
                onclick={() => (current = item.id)}
                title={collapsed ? item.label : ''}
                aria-current={isActive(item.id) ? 'page' : undefined}
              >
                <span class="nav-icon">{@html item.icon}</span>
                {#if !collapsed}
                  <span class="nav-label">{item.label}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </div>

  <!-- Bottom status -->
  <div class="sidebar-footer">
    <div class="server-status" title="Server status">
      <span class="status-dot"></span>
      {#if !collapsed}
        <span class="status-text">Connected</span>
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

  /* Nav body */
  .nav-body {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: var(--space-2) 0;
  }

  .nav-section {
    margin-bottom: var(--space-2);
  }

  .section-label {
    padding: var(--space-2) var(--space-4) var(--space-1);
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--color-text-muted);
    white-space: nowrap;
    user-select: none;
  }

  ul {
    list-style: none;
    padding: 0 var(--space-2);
    margin: 0;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-2);
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
    background: rgba(238, 0, 0, 0.08);
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
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Footer */
  .sidebar-footer {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3) var(--space-4);
    flex-shrink: 0;
  }

  .server-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-success);
    box-shadow: 0 0 5px rgba(99, 153, 61, 0.6);
    flex-shrink: 0;
  }

  .status-text {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }
</style>
