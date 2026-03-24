<script>
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';

  let {
    current = $bindable('dashboard'),
    onnavigate = undefined,
    selectedWorkspace = null,
    selectedRepo = null,
  } = $props();

  let collapsed = $state(false);
  let inboxCount = $state(0);
  let adminExpanded = $state(false);
  let isAdmin = $state(false);

  // Load inbox badge count (review MRs + pending specs)
  async function loadInboxCount() {
    try {
      const [mrs, specs] = await Promise.allSettled([
        api.mergeRequests({ status: 'review' }),
        api.getPendingSpecs(),
      ]);
      const mrCount = mrs.status === 'fulfilled' ? (mrs.value || []).length : 0;
      const specCount = specs.status === 'fulfilled' ? (specs.value || []).length : 0;
      inboxCount = mrCount + specCount;
    } catch { /* ignore */ }
  }

  async function loadRole() {
    try {
      const info = await api.tokenInfo();
      // Global token = admin; also check JWT role claim
      isAdmin = info.kind === 'global' || info.role === 'Admin';
    } catch { /* ignore */ }
  }

  onMount(() => {
    loadInboxCount();
    loadRole();
    const interval = setInterval(loadInboxCount, 60000);
    return () => clearInterval(interval);
  });

  function isActive(id) {
    if (id === 'projects') {
      return current === 'projects' || current === 'repo-detail' || current === 'mr-detail';
    }
    if (id === 'workspaces') {
      return current === 'workspaces' || current === 'workspace-detail';
    }
    return current === id;
  }

  function nav(id, ctx) {
    current = id;
    onnavigate?.(id, ctx);
  }

  // SVG icon helpers — inline SVG strings
  function inboxIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11L2 12v6a2 2 0 002 2h16a2 2 0 002-2v-6l-3.45-6.89A2 2 0 0016.76 4H7.24a2 2 0 00-1.79 1.11z"/></svg>'; }
  function briefingIcon()     { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>'; }
  function explorerIcon()     { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.35-4.35"/><circle cx="11" cy="11" r="3"/></svg>'; }
  function workspaceIcon()    { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><rect x="2" y="7" width="20" height="14" rx="2"/><path d="M16 7V5a2 2 0 00-2-2h-4a2 2 0 00-2 2v2"/><path d="M12 12v4"/><path d="M8 12h8"/></svg>'; }
  function budgetIcon()       { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><rect x="2" y="5" width="20" height="14" rx="2"/><path d="M2 10h20"/><circle cx="12" cy="15" r="2"/></svg>'; }
  function membersIcon()      { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87"/><path d="M16 3.13a4 4 0 010 7.75"/></svg>'; }
  function reposIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/></svg>'; }
  function branchIcon()       { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><line x1="6" y1="3" x2="6" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 01-9 9"/></svg>'; }
  function mrIcon()           { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 21V9a9 9 0 009 9"/></svg>'; }
  function gatesIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>'; }
  function graphIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/><path d="M7 12h10M17 7l-10 4M17 17L7 13"/></svg>'; }
  function adminIcon()        { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>'; }
  function settingsIcon()     { return '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>'; }
  function chevronIcon(down) {
    return down
      ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><path d="M6 9l6 6 6-6"/></svg>'
      : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><path d="M9 18l6-6-6-6"/></svg>';
  }
</script>

<nav class="sidebar" class:collapsed aria-label="Main navigation">
  <!-- Logo -->
  <div class="logo">
    <div class="logo-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" width="22" height="22" aria-hidden="true">
        <circle cx="12" cy="12" r="10" stroke="var(--color-primary)" stroke-width="2"/>
        <path d="M8 12l3 3 5-5" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </div>
    {#if !collapsed}
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

  <!-- Nav body -->
  <div class="nav-body">

    <!-- Primary journeys (always visible) -->
    <div class="nav-section">
      {#if !collapsed}
        <div class="section-label" aria-hidden="true">Journeys</div>
      {/if}
      <ul role="list" aria-label={collapsed ? 'Journeys' : undefined}>
        <li>
          <button
            class="nav-item"
            class:active={isActive('inbox')}
            onclick={() => nav('inbox')}
            aria-label="Inbox"
            aria-current={isActive('inbox') ? 'page' : undefined}
          >
            <span class="nav-icon" aria-hidden="true">{@html inboxIcon()}</span>
            {#if !collapsed}
              <span class="nav-label">Inbox</span>
              {#if inboxCount > 0}
                <span class="nav-badge" aria-label="{inboxCount} pending">{inboxCount}</span>
              {/if}
            {/if}
          </button>
        </li>
        <li>
          <button
            class="nav-item"
            class:active={isActive('briefing')}
            onclick={() => nav('briefing')}
            aria-label="Briefing"
            aria-current={isActive('briefing') ? 'page' : undefined}
          >
            <span class="nav-icon" aria-hidden="true">{@html briefingIcon()}</span>
            {#if !collapsed}<span class="nav-label">Briefing</span>{/if}
          </button>
        </li>
        <li>
          <button
            class="nav-item"
            class:active={isActive('explorer')}
            onclick={() => nav('explorer')}
            aria-label="Explorer"
            aria-current={isActive('explorer') ? 'page' : undefined}
          >
            <span class="nav-icon" aria-hidden="true">{@html explorerIcon()}</span>
            {#if !collapsed}<span class="nav-label">Explorer</span>{/if}
          </button>
        </li>
      </ul>
    </div>

    <!-- Context: repo takes priority over workspace -->
    {#if selectedRepo}
      <div class="nav-section context-section">
        {#if !collapsed}
          <div class="section-label context-label" aria-hidden="true">
            <span class="context-icon" aria-hidden="true">{@html reposIcon()}</span>
            <span class="context-name" title={selectedRepo.name}>{selectedRepo.name}</span>
          </div>
        {/if}
        <ul role="list" aria-label={collapsed ? 'Repository' : undefined}>
          <li>
            <button class="nav-item" class:active={isActive('repo-detail')} onclick={() => nav('repo-detail')}
              aria-label="Branches" aria-current={isActive('repo-detail') ? 'page' : undefined}>
              <span class="nav-icon" aria-hidden="true">{@html branchIcon()}</span>
              {#if !collapsed}<span class="nav-label">Branches</span>{/if}
            </button>
          </li>
          <li>
            <button class="nav-item" class:active={isActive('merge-queue')} onclick={() => nav('merge-queue')}
              aria-label="Merge Requests" aria-current={isActive('merge-queue') ? 'page' : undefined}>
              <span class="nav-icon" aria-hidden="true">{@html mrIcon()}</span>
              {#if !collapsed}<span class="nav-label">Merge Requests</span>{/if}
            </button>
          </li>
          <li>
            <button class="nav-item" class:active={false} onclick={() => nav('repo-detail')}
              aria-label="Gates">
              <span class="nav-icon" aria-hidden="true">{@html gatesIcon()}</span>
              {#if !collapsed}<span class="nav-label">Gates</span>{/if}
            </button>
          </li>
          <li>
            <button class="nav-item" class:active={false} onclick={() => nav('repo-detail')}
              aria-label="Knowledge Graph">
              <span class="nav-icon" aria-hidden="true">{@html graphIcon()}</span>
              {#if !collapsed}<span class="nav-label">Knowledge Graph</span>{/if}
            </button>
          </li>
        </ul>
      </div>
    {:else if selectedWorkspace}
      <div class="nav-section context-section">
        {#if !collapsed}
          <div class="section-label context-label" aria-hidden="true">
            <span class="context-icon" aria-hidden="true">{@html workspaceIcon()}</span>
            <span class="context-name" title={selectedWorkspace.name}>{selectedWorkspace.name}</span>
          </div>
        {/if}
        <ul role="list" aria-label={collapsed ? 'Workspace' : undefined}>
          <li>
            <button class="nav-item" class:active={isActive('workspace-detail')} onclick={() => nav('workspace-detail')}
              aria-label="Workspace Repos" aria-current={isActive('workspace-detail') ? 'page' : undefined}>
              <span class="nav-icon" aria-hidden="true">{@html reposIcon()}</span>
              {#if !collapsed}<span class="nav-label">Repos</span>{/if}
            </button>
          </li>
          <li>
            <button class="nav-item" class:active={isActive('workspace-detail')} onclick={() => nav('workspace-detail')}
              aria-label="Workspace Members">
              <span class="nav-icon" aria-hidden="true">{@html membersIcon()}</span>
              {#if !collapsed}<span class="nav-label">Members</span>{/if}
            </button>
          </li>
          <li>
            <button class="nav-item" class:active={isActive('budget')} onclick={() => nav('budget')}
              aria-label="Workspace Budget" aria-current={isActive('budget') ? 'page' : undefined}>
              <span class="nav-icon" aria-hidden="true">{@html budgetIcon()}</span>
              {#if !collapsed}<span class="nav-label">Budget</span>{/if}
            </button>
          </li>
        </ul>
      </div>
    {/if}

    <!-- Admin accordion (Admin users only) -->
    {#if isAdmin}
      <div class="nav-section admin-section">
        <button
          class="admin-toggle"
          class:expanded={adminExpanded}
          onclick={() => { adminExpanded = !adminExpanded; }}
          aria-expanded={adminExpanded}
          aria-label="Admin"
        >
          <span class="nav-icon" aria-hidden="true">{@html adminIcon()}</span>
          {#if !collapsed}
            <span class="nav-label">Admin</span>
            <span class="admin-chevron" aria-hidden="true">{@html chevronIcon(adminExpanded)}</span>
          {/if}
        </button>
        {#if adminExpanded && !collapsed}
          <ul role="list" class="admin-items" aria-label="Admin">
            <li>
              <button class="nav-item nav-item-sub" class:active={isActive('admin')} onclick={() => nav('admin')}
                aria-label="Admin Panel" aria-current={isActive('admin') ? 'page' : undefined}>
                <span class="nav-label">Admin Panel</span>
              </button>
            </li>
            <li>
              <button class="nav-item nav-item-sub" class:active={isActive('settings')} onclick={() => nav('settings')}
                aria-label="Settings" aria-current={isActive('settings') ? 'page' : undefined}>
                <span class="nav-label">Settings</span>
              </button>
            </li>
          </ul>
        {/if}
        {#if adminExpanded && collapsed}
          <!-- In collapsed mode, show admin and settings icons -->
          <ul role="list" aria-label="Admin">
            <li>
              <button class="nav-item" class:active={isActive('admin')} onclick={() => nav('admin')} aria-label="Admin Panel">
                <span class="nav-icon" aria-hidden="true">{@html settingsIcon()}</span>
              </button>
            </li>
          </ul>
        {/if}
      </div>
    {/if}

  </div>

  <!-- Bottom status -->
  <div class="sidebar-footer">
    <div class="server-status" role="status" aria-label="Server connected">
      <span class="status-dot" aria-hidden="true"></span>
      {#if !collapsed}
        <span class="status-text" aria-hidden="true">Connected</span>
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

  /* Context section label with icon + name */
  .context-section {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-2);
    margin-top: var(--space-1);
  }

  .context-label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3) var(--space-1);
  }

  .context-icon {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    opacity: 0.6;
  }

  .context-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.65rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--color-text-muted);
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

  /* Sub-items (indented, no icon) */
  .nav-item-sub {
    padding-left: var(--space-6);
    font-size: var(--text-xs);
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
    color: #fff;
    border-radius: 999px;
    font-size: 0.6rem;
    font-weight: 700;
    flex-shrink: 0;
  }

  /* Admin accordion toggle */
  .admin-section {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-2);
    margin-top: var(--space-1);
  }

  .admin-toggle {
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
    transition: background var(--transition-fast), color var(--transition-fast);
    margin-left: 0;
    padding-left: var(--space-4);
  }

  .admin-toggle:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .admin-toggle.expanded {
    color: var(--color-text);
  }

  .admin-chevron {
    display: flex;
    align-items: center;
    margin-left: auto;
    flex-shrink: 0;
    opacity: 0.6;
  }

  .admin-items {
    padding: 0 var(--space-2);
    margin: 0;
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
