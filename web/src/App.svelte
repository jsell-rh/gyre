<script>
  import './lib/design-system.css';
  import { createWsStore } from './lib/ws.js';
  import Sidebar from './components/Sidebar.svelte';
  import DashboardHome from './components/DashboardHome.svelte';
  import ActivityFeed from './components/ActivityFeed.svelte';
  import AgentList from './components/AgentList.svelte';
  import TaskBoard from './components/TaskBoard.svelte';
  import ProjectList from './components/ProjectList.svelte';
  import Settings from './components/Settings.svelte';
  import RepoDetail from './components/RepoDetail.svelte';
  import MergeRequestDetail from './components/MergeRequestDetail.svelte';
  import MergeQueueView from './components/MergeQueueView.svelte';
  import AdminPanel from './components/AdminPanel.svelte';
  import McpCatalog from './components/McpCatalog.svelte';
  import ComposeView from './components/ComposeView.svelte';
  import AnalyticsView from './components/AnalyticsView.svelte';
  import CostView from './components/CostView.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import Breadcrumb from './lib/Breadcrumb.svelte';

  let currentView = $state('dashboard');
  let selectedRepo = $state(null);
  let selectedMr = $state(null);
  let wsStatus = $state('disconnected');
  let wsStore = $state(null);

  $effect(() => {
    wsStore = createWsStore();
    const unsub = wsStore.onStatus((s) => (wsStatus = s));
    return () => {
      unsub();
      wsStore.destroy();
    };
  });

  function navigate(view, ctx = {}) {
    currentView = view;
    if (ctx.repo !== undefined) selectedRepo = ctx.repo;
    if (ctx.mr !== undefined) selectedMr = ctx.mr;
  }

  const viewTitles = {
    dashboard:    'Dashboard',
    activity:     'Activity Feed',
    agents:       'Agents',
    tasks:        'Task Board',
    projects:     'Projects',
    'repo-detail': 'Repository',
    'mr-detail':  'Merge Request',
    'merge-queue': 'Merge Queue',
    'mcp-catalog': 'MCP Tool Catalog',
    compose:      'Agent Compose',
    analytics:    'Analytics',
    costs:        'Cost Tracking',
    admin:        'Admin Panel',
    settings:     'Settings',
  };

  let breadcrumbs = $derived(() => {
    if (currentView === 'repo-detail' && selectedRepo) {
      return [
        { label: 'Projects', view: 'projects' },
        { label: selectedRepo.name ?? selectedRepo.id ?? 'Repository' },
      ];
    }
    if (currentView === 'mr-detail' && selectedMr) {
      return [
        { label: 'Projects', view: 'projects' },
        { label: selectedRepo?.name ?? 'Repository', view: 'repo-detail' },
        { label: `MR #${selectedMr.id ?? selectedMr.iid ?? ''}` },
      ];
    }
    return [];
  });
</script>

<div class="app">
  <Sidebar bind:current={currentView} />

  <div class="main">
    <header class="topbar">
      <div class="topbar-left">
        <span class="topbar-title">{viewTitles[currentView] ?? 'Gyre'}</span>
        {#if breadcrumbs().length > 0}
          <Breadcrumb items={breadcrumbs()} onnavigate={navigate} />
        {/if}
      </div>
      <div class="topbar-right">
        <button class="search-trigger" onclick={() => {}} aria-label="Open search (Ctrl+K)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
          </svg>
          <span>Search</span>
          <kbd>⌘K</kbd>
        </button>

        <div class="ws-indicator" class:connected={wsStatus === 'connected'} class:error={wsStatus === 'error' || wsStatus === 'auth-failed'} title="WebSocket: {wsStatus}">
          <span class="ws-dot"></span>
          <span class="ws-label">{wsStatus}</span>
        </div>

        <span class="version">v0.1.0</span>
      </div>
    </header>

    <div class="content">
      {#if currentView === 'dashboard'}
        <DashboardHome {wsStore} onnavigate={(v) => navigate(v)} />
      {:else if currentView === 'activity'}
        <ActivityFeed {wsStore} />
      {:else if currentView === 'agents'}
        <AgentList />
      {:else if currentView === 'tasks'}
        <TaskBoard />
      {:else if currentView === 'projects'}
        <ProjectList onSelectRepo={(repo) => navigate('repo-detail', { repo })} />
      {:else if currentView === 'repo-detail' && selectedRepo}
        <RepoDetail
          repo={selectedRepo}
          onBack={() => navigate('projects')}
          onSelectMr={(mr) => navigate('mr-detail', { mr })}
        />
      {:else if currentView === 'mr-detail' && selectedMr}
        <MergeRequestDetail
          mr={selectedMr}
          repo={selectedRepo}
          onBack={() => navigate('repo-detail')}
        />
      {:else if currentView === 'merge-queue'}
        <MergeQueueView />
      {:else if currentView === 'mcp-catalog'}
        <McpCatalog />
      {:else if currentView === 'compose'}
        <ComposeView />
      {:else if currentView === 'analytics'}
        <AnalyticsView />
      {:else if currentView === 'costs'}
        <CostView />
      {:else if currentView === 'admin'}
        <AdminPanel />
      {:else}
        <Settings {wsStatus} />
      {/if}
    </div>
  </div>
</div>

<SearchBar onnavigate={(v) => navigate(v)} />
<Toast />

<style>
  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  /* Top bar */
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--space-6);
    height: var(--topbar-height);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
  }

  .topbar-left {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    overflow: hidden;
  }

  .topbar-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .topbar-right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  /* Search trigger button */
  .search-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .search-trigger:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text-secondary);
  }

  .search-trigger kbd {
    background: var(--color-border);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    padding: 1px 4px;
    font-size: 0.65rem;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
  }

  /* WS indicator */
  .ws-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .ws-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-text-muted);
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .ws-indicator.connected .ws-dot {
    background: var(--color-success);
    box-shadow: 0 0 5px rgba(99, 153, 61, 0.5);
  }

  .ws-indicator.error .ws-dot {
    background: var(--color-danger);
  }

  .ws-label {
    display: none;
  }

  .version {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Content area */
  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
