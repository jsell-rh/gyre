<script>
  import './lib/design-system.css';
  import { isLoading } from 'svelte-i18n';
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
  import TaskDetail from './components/TaskDetail.svelte';
  import SpecApprovalsView from './components/SpecApprovalsView.svelte';
  import SpecDashboard from './components/SpecDashboard.svelte';
  import AuditView from './components/AuditView.svelte';
  import WorkspaceList from './components/WorkspaceList.svelte';
  import WorkspaceDetail from './components/WorkspaceDetail.svelte';
  import PersonaCatalog from './components/PersonaCatalog.svelte';
  import BudgetDashboard from './components/BudgetDashboard.svelte';
  import DependencyGraph from './components/DependencyGraph.svelte';
  import SpecGraph from './components/SpecGraph.svelte';
  import UserProfile from './components/UserProfile.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import Breadcrumb from './lib/Breadcrumb.svelte';
  import Modal from './lib/Modal.svelte';
  import { onMount } from 'svelte';
  import { setAuthToken, api } from './lib/api.js';

  let currentView = $state('dashboard');
  let selectedRepo = $state(null);
  let selectedMr = $state(null);
  let selectedTask = $state(null);
  let selectedWorkspace = $state(null);
  let wsStatus = $state('disconnected');
  let wsStore = $state(null);
  let tokenModalOpen = $state(false);
  let tokenInput = $state(localStorage.getItem('gyre_auth_token') || 'gyre-dev-token');
  let hasToken = $state(!!localStorage.getItem('gyre_auth_token'));
  let tokenInfo = $state(null);
  let searchOpen = $state(false);

  async function openTokenModal() {
    tokenInput = localStorage.getItem('gyre_auth_token') || 'test-token';
    tokenModalOpen = true;
    tokenInfo = null;
    try { tokenInfo = await api.tokenInfo(); } catch { /* ignore */ }
  }

  function saveToken() {
    const t = tokenInput.trim() || 'gyre-dev-token';
    setAuthToken(t);
    tokenInput = t;
    hasToken = true;
    tokenModalOpen = false;
    tokenInfo = null;
    // Reconnect WS with new token — capture local ref so we own the lifecycle
    if (wsStore) {
      const old = wsStore;
      wsStore = createWsStore();
      wsStore.onStatus((s) => (wsStatus = s));
      old.destroy();
    }
  }

  $effect(() => {
    // Capture the store in a local variable so the cleanup closes over this
    // specific instance rather than the reactive proxy (which may point to a
    // newer store after saveToken() runs).
    const store = createWsStore();
    wsStore = store;
    const unsub = store.onStatus((s) => (wsStatus = s));
    return () => {
      unsub();
      store.destroy();
    };
  });

  // Token kind → human-readable label
  const TOKEN_KIND_LABELS = {
    global:      'Global admin token',
    agent_jwt:   'Agent JWT (EdDSA, scoped)',
    uuid_token:  'Per-agent UUID token (legacy)',
    api_key:     'API key',
  };

  function navigate(view, ctx = {}) {
    currentView = view;
    if (ctx.repo !== undefined) selectedRepo = ctx.repo;
    if (ctx.mr !== undefined) selectedMr = ctx.mr;
    if (ctx.task !== undefined) selectedTask = ctx.task;
    // Push to browser history so back button works
    window.history.pushState(
      { view, selectedRepo, selectedMr, selectedTask },
      '',
      '#' + view,
    );
    if (ctx.workspace !== undefined) selectedWorkspace = ctx.workspace;
  }

  // Sync browser history ↔ app state using onMount to avoid reactive loops
  onMount(() => {
    const initHash = window.location.hash.slice(1);
    if (initHash && initHash in viewTitles) {
      currentView = initHash;
    }
    window.history.replaceState(
      { view: currentView, selectedRepo, selectedMr, selectedTask },
      '',
      '#' + currentView,
    );

    function handlePopstate(e) {
      if (e.state?.view) {
        currentView  = e.state.view;
        selectedRepo = e.state.selectedRepo ?? null;
        selectedMr   = e.state.selectedMr   ?? null;
        selectedTask = e.state.selectedTask  ?? null;
      } else {
        currentView = 'dashboard';
      }
    }
    window.addEventListener('popstate', handlePopstate);
    return () => window.removeEventListener('popstate', handlePopstate);
  });

  const viewTitles = {
    dashboard:          'Dashboard',
    activity:           'Activity Feed',
    agents:             'Agents',
    tasks:              'Task Board',
    'task-detail':      'Task Detail',
    projects:           'Projects',
    'repo-detail':      'Repository',
    'mr-detail':        'Merge Request',
    'merge-queue':      'Merge Queue',
    'mcp-catalog':      'MCP Tool Catalog',
    compose:            'Agent Compose',
    analytics:          'Analytics',
    costs:              'Cost Tracking',
    audit:              'Audit Events',
    'spec-approvals':   'Spec Approvals',
    specs:              'Spec Registry',
    'spec-graph':       'Spec Link Graph',
    admin:              'Admin Panel',
    settings:           'Settings',
    workspaces:         'Workspaces',
    'workspace-detail': 'Workspace',
    personas:           'Persona Catalog',
    budget:             'Budget Dashboard',
    dependencies:       'Dependency Graph',
    profile:            'My Profile',
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

<a href="#main-content" class="skip-to-content">Skip to main content</a>

{#if !$isLoading}
<div class="app">
  <Sidebar bind:current={currentView} onnavigate={(v) => navigate(v)} />

  <div class="main">
    <header class="topbar">
      <div class="topbar-left">
        <span class="topbar-title" aria-live="polite" aria-atomic="true">{viewTitles[currentView] ?? 'Gyre'}</span>
        {#if breadcrumbs().length > 0}
          <Breadcrumb items={breadcrumbs()} onnavigate={navigate} />
        {/if}
      </div>
      <div class="topbar-right">
        <button class="search-trigger" onclick={() => (searchOpen = true)} aria-label="Open search (Ctrl+K)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
          </svg>
          <span>Search</span>
          <kbd aria-hidden="true">⌘K</kbd>
        </button>

        <div
          class="ws-indicator"
          class:connected={wsStatus === 'connected'}
          class:error={wsStatus === 'error' || wsStatus === 'auth-failed'}
          role="status"
          aria-label="WebSocket status: {wsStatus}"
        >
          <span class="ws-dot" aria-hidden="true"></span>
          <span class="ws-label">{wsStatus}</span>
        </div>

        <button
          class="auth-btn"
          class:auth-active={hasToken}
          onclick={openTokenModal}
          aria-label={hasToken ? 'Authenticated — configure API token' : 'No token — configure API token'}
        >
          <span class="auth-dot" aria-hidden="true"></span>
          <span aria-hidden="true">{hasToken ? 'Authenticated' : 'No Token'}</span>
        </button>

        <span class="version" aria-label="Version 0.1.0">v0.1.0</span>
      </div>
    </header>

    <main class="content" id="main-content" tabindex="-1">
      {#if currentView === 'dashboard'}
        <DashboardHome {wsStore} onnavigate={(v) => navigate(v)} />
      {:else if currentView === 'activity'}
        <ActivityFeed {wsStore} />
      {:else if currentView === 'agents'}
        <AgentList />
      {:else if currentView === 'tasks'}
        <TaskBoard onSelectTask={(task) => navigate('task-detail', { task })} />
      {:else if currentView === 'task-detail' && selectedTask}
        <TaskDetail
          task={selectedTask}
          onBack={() => navigate('tasks')}
        />
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
      {:else if currentView === 'audit'}
        <AuditView />
      {:else if currentView === 'spec-approvals'}
        <SpecApprovalsView />
      {:else if currentView === 'specs'}
        <SpecDashboard />
      {:else if currentView === 'spec-graph'}
        <SpecGraph />
      {:else if currentView === 'workspaces'}
        <WorkspaceList onSelect={(ws) => navigate('workspace-detail', { workspace: ws })} />
      {:else if currentView === 'workspace-detail' && selectedWorkspace}
        <WorkspaceDetail
          workspace={selectedWorkspace}
          onBack={() => navigate('workspaces')}
        />
      {:else if currentView === 'personas'}
        <PersonaCatalog />
      {:else if currentView === 'budget'}
        <BudgetDashboard />
      {:else if currentView === 'dependencies'}
        <DependencyGraph />
      {:else if currentView === 'profile'}
        <UserProfile />
      {:else if currentView === 'admin'}
        <AdminPanel />
      {:else}
        <Settings {wsStatus} />
      {/if}
    </main>
  </div>
</div>
{/if}

<SearchBar bind:open={searchOpen} onnavigate={(v) => navigate(v)} />
<Toast />

<Modal bind:open={tokenModalOpen} title="API Token" size="sm">
  <div class="token-modal">
    <p class="token-desc">Set the Bearer token used for all API and WebSocket requests. Leave blank to use the default <code>test-token</code>.</p>
    {#if tokenInfo}
      <div class="token-info-box">
        <div class="token-info-row">
          <span class="token-info-label">Kind</span>
          <span class="token-info-val">{TOKEN_KIND_LABELS[tokenInfo.kind] ?? tokenInfo.kind ?? '—'}</span>
        </div>
        {#if tokenInfo.agent_id}
          <div class="token-info-row">
            <span class="token-info-label">Agent ID</span>
            <span class="token-info-val mono">{tokenInfo.agent_id}</span>
          </div>
        {/if}
        {#if tokenInfo.task_id}
          <div class="token-info-row">
            <span class="token-info-label">Task ID</span>
            <span class="token-info-val mono">{tokenInfo.task_id}</span>
          </div>
        {/if}
        {#if tokenInfo.scope}
          <div class="token-info-row">
            <span class="token-info-label">Scope</span>
            <span class="token-info-val">{tokenInfo.scope}</span>
          </div>
        {/if}
        {#if tokenInfo.exp}
          <div class="token-info-row">
            <span class="token-info-label">Expires</span>
            <span class="token-info-val">{new Date(tokenInfo.exp * 1000).toLocaleString()}</span>
          </div>
        {/if}
      </div>
    {/if}
    <label class="token-label" for="token-input">Token</label>
    <input
      id="token-input"
      class="token-input"
      type="text"
      bind:value={tokenInput}
      placeholder="test-token"
      onkeydown={(e) => e.key === 'Enter' && saveToken()}
    />
    <div class="token-actions">
      <button class="btn-secondary" onclick={() => (tokenModalOpen = false)}>Cancel</button>
      <button class="btn-primary" onclick={saveToken}>Save</button>
    </div>
  </div>
</Modal>

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

  /* Auth indicator button */
  .auth-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .auth-btn:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text-secondary);
  }

  .auth-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-danger);
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .auth-btn.auth-active .auth-dot {
    background: var(--color-success);
    box-shadow: 0 0 5px rgba(99, 153, 61, 0.5);
  }

  /* Token modal */
  .token-modal {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .token-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .token-desc code {
    font-family: var(--font-mono);
    background: var(--color-surface-elevated);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  .token-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .token-input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    box-sizing: border-box;
  }

  .token-input:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .token-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .btn-primary:hover { opacity: 0.85; }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .btn-secondary:hover { border-color: var(--color-text-muted); }

  /* Token info box */
  .token-info-box {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .token-info-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .token-info-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    width: 70px;
    flex-shrink: 0;
  }

  .token-info-val {
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .token-info-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  /* Content area */
  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* Remove default outline on main — focus is only programmatic (skip link) */
  main:focus {
    outline: none;
  }
</style>
