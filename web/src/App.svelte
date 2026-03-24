<script>
  import './lib/design-system.css';
  import { isLoading } from 'svelte-i18n';
  import { createWsStore } from './lib/ws.js';
  import Sidebar from './components/Sidebar.svelte';
  import DashboardHome from './components/DashboardHome.svelte';
  import ActivityFeed from './components/ActivityFeed.svelte';
  import AgentList from './components/AgentList.svelte';
  import TaskBoard from './components/TaskBoard.svelte';
  import RepoList from './components/RepoList.svelte';
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
  import TenantList from './components/TenantList.svelte';
  import WorkspaceList from './components/WorkspaceList.svelte';
  import WorkspaceDetail from './components/WorkspaceDetail.svelte';
  import PersonaCatalog from './components/PersonaCatalog.svelte';
  import BudgetDashboard from './components/BudgetDashboard.svelte';
  import DependencyGraph from './components/DependencyGraph.svelte';
  import SpecGraph from './components/SpecGraph.svelte';
  import MetaSpecs from './components/MetaSpecs.svelte';
  import UserProfile from './components/UserProfile.svelte';
  import Inbox from './components/Inbox.svelte';
  import Briefing from './components/Briefing.svelte';
  import ExplorerView from './components/ExplorerView.svelte';
  import Toast from './lib/Toast.svelte';
  import SearchBar from './lib/SearchBar.svelte';
  import Modal from './lib/Modal.svelte';
  import { onMount, setContext } from 'svelte';
  import { setAuthToken, api } from './lib/api.js';

  const savedWorkspace = localStorage.getItem('gyre_workspace_id');
  let currentView = $state(savedWorkspace ? 'explorer' : 'inbox');
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

  // Tenant selector state
  let tenants = $state([]);
  let selectedTenantId = $state(localStorage.getItem('gyre_tenant_id') || '');
  let selectedTenant = $state(null);

  // Workspace selector state
  let workspaces = $state([]);
  let selectedWorkspaceId = $state(localStorage.getItem('gyre_workspace_id') || '');

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

  // Generate a pretty URL for a view + context
  function urlFor(view, ctx) {
    const repo = ctx.repo ?? selectedRepo;
    const mr = ctx.mr ?? selectedMr;
    const task = ctx.task ?? selectedTask;
    const workspace = ctx.workspace ?? selectedWorkspace;
    if (view === 'repo-detail' && repo?.id) return `/repos/${repo.id}`;
    if (view === 'task-detail' && task?.id) return `/tasks/${task.id}`;
    if (view === 'mr-detail' && mr?.id) return `/merge-requests/${mr.id}`;
    if (view === 'workspace-detail' && workspace?.id) return `/workspaces/${workspace.id}`;
    return '/' + view;
  }

  function navigate(view, ctx = {}) {
    currentView = view;
    if (ctx.repo !== undefined) selectedRepo = ctx.repo;
    if (ctx.mr !== undefined) selectedMr = ctx.mr;
    if (ctx.task !== undefined) selectedTask = ctx.task;
    if (ctx.workspace !== undefined) selectedWorkspace = ctx.workspace;
    // Push plain (non-proxy) objects — history.pushState uses structuredClone
    // which cannot serialize Svelte 5 reactive Proxy objects.
    const snap = (o) => { try { return JSON.parse(JSON.stringify(o ?? null)); } catch { return null; } };
    const url = urlFor(view, ctx);
    window.history.pushState(
      { view, selectedRepo: snap(selectedRepo), selectedMr: snap(selectedMr), selectedTask: snap(selectedTask), selectedWorkspace: snap(selectedWorkspace) },
      '',
      url,
    );
  }

  // Expose navigate via context so child components can navigate without prop drilling
  setContext('navigate', navigate);

  // Sync browser history ↔ app state using onMount to avoid reactive loops
  onMount(async () => {
    // Fetch tenants and workspaces for selectors
    try { tenants = await api.tenants(); } catch { /* ignore */ }
    try { workspaces = await api.workspaces(); } catch { /* ignore */ }

    // Support both path-based URLs and legacy hash-based URLs
    const path = window.location.pathname;
    const parts = path.split('/').filter(Boolean);
    const hashView = window.location.hash.slice(1);

    let initView = null;
    let initCtx = {};

    if (parts.length === 2) {
      // Detail view URLs: /repos/:id, /tasks/:id, /merge-requests/:id, /workspaces/:id
      const [segment, id] = parts;
      if (segment === 'repos') {
        initView = 'repo-detail';
        try {
          const allRepos = await api.allRepos();
          const repo = allRepos.find(r => r.id === id);
          if (repo) initCtx = { repo };
        } catch { /* fallback to repos view */ initView = 'repos'; }
      } else if (segment === 'tasks') {
        initView = 'task-detail';
        try {
          const task = await api.task(id);
          initCtx = { task };
        } catch { initView = 'tasks'; }
      } else if (segment === 'merge-requests') {
        initView = 'mr-detail';
        try {
          const mr = await api.mergeRequest(id);
          initCtx = { mr };
        } catch { initView = 'projects'; }
      } else if (segment === 'workspaces') {
        initView = 'workspace-detail';
        try {
          const workspace = await api.workspace(id);
          initCtx = { workspace };
          // Update the workspace in selector too
          if (workspace) {
            selectedWorkspace = workspace;
            selectedWorkspaceId = workspace.id;
            localStorage.setItem('gyre_workspace_id', workspace.id);
          }
        } catch { initView = 'workspaces'; }
      } else if (segment === 'agents') {
        initView = 'agents';
      }
    } else if (parts.length === 1) {
      const pathView = parts[0];
      initView = (pathView && pathView in viewTitles) ? pathView : null;
    }

    // Fall back to hash-based for legacy support
    if (!initView && hashView && hashView in viewTitles) {
      initView = hashView;
    }

    if (initView) {
      currentView = initView;
      if (initCtx.repo !== undefined) selectedRepo = initCtx.repo;
      if (initCtx.mr !== undefined) selectedMr = initCtx.mr;
      if (initCtx.task !== undefined) selectedTask = initCtx.task;
      if (initCtx.workspace !== undefined) selectedWorkspace = initCtx.workspace;
    }

    // Restore selected workspace from localStorage (if not set by URL)
    if (!selectedWorkspace && selectedWorkspaceId && workspaces.length > 0) {
      const ws = workspaces.find(w => w.id === selectedWorkspaceId);
      if (ws) selectedWorkspace = ws;
    }

    const snap = (o) => { try { return JSON.parse(JSON.stringify(o ?? null)); } catch { return null; } };
    window.history.replaceState(
      { view: currentView, selectedRepo: snap(selectedRepo), selectedMr: snap(selectedMr), selectedTask: snap(selectedTask), selectedWorkspace: snap(selectedWorkspace) },
      '',
      urlFor(currentView, {}),
    );

    function handlePopstate(e) {
      if (e.state?.view) {
        currentView       = e.state.view;
        selectedRepo      = e.state.selectedRepo      ?? null;
        selectedMr        = e.state.selectedMr        ?? null;
        selectedTask      = e.state.selectedTask       ?? null;
        selectedWorkspace = e.state.selectedWorkspace  ?? null;
      } else {
        currentView = 'dashboard';
      }
    }
    window.addEventListener('popstate', handlePopstate);

    function handleJourneyKeys(e) {
      if (e.metaKey || e.ctrlKey) {
        if (e.key === 'i') { e.preventDefault(); navigate('inbox'); }
        if (e.key === 'b') { e.preventDefault(); navigate('briefing'); }
      }
    }
    window.addEventListener('keydown', handleJourneyKeys);

    return () => {
      window.removeEventListener('popstate', handlePopstate);
      window.removeEventListener('keydown', handleJourneyKeys);
    };
  });

  function onTenantChange(e) {
    const id = e.target.value;
    selectedTenantId = id;
    localStorage.setItem('gyre_tenant_id', id);
    selectedTenant = id ? (tenants.find(t => t.id === id) ?? null) : null;
  }

  function onWorkspaceChange(e) {
    const id = e.target.value;
    selectedWorkspaceId = id;
    localStorage.setItem('gyre_workspace_id', id);
    if (id) {
      const ws = workspaces.find(w => w.id === id);
      selectedWorkspace = ws ?? null;
    } else {
      selectedWorkspace = null;
    }
  }

  const viewTitles = {
    inbox:              'Inbox',
    briefing:           'Briefing',
    dashboard:          'Dashboard',
    activity:           'Activity Feed',
    agents:             'Agents',
    tasks:              'Task Board',
    'task-detail':      'Task Detail',
    repos:              'Repositories',
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
    'meta-specs':       'Meta-Specs',
    admin:              'Admin Panel',
    settings:           'Settings',
    tenants:            'Tenants',
    workspaces:         'Workspaces',
    'workspace-detail': 'Workspace',
    personas:           'Persona Catalog',
    budget:             'Budget Dashboard',
    dependencies:       'Dependency Graph',
    profile:            'My Profile',
    explorer:           'System Explorer',
  };

  // Scope breadcrumb: Gyre › Workspace › Repo (shown in topbar)
  let scopeCrumbs = $derived(() => {
    const crumbs = [{ label: 'Gyre', view: 'dashboard' }];
    if (selectedWorkspace) {
      crumbs.push({ label: selectedWorkspace.name, view: 'workspace-detail', ctx: { workspace: selectedWorkspace } });
    }
    if ((currentView === 'repo-detail' || currentView === 'mr-detail') && selectedRepo) {
      crumbs.push({ label: selectedRepo.name ?? 'Repository', view: 'repo-detail', ctx: { repo: selectedRepo } });
    }
    return crumbs;
  });
</script>

<a href="#main-content" class="skip-to-content">Skip to main content</a>

{#if !$isLoading}
<div class="app">
  <Sidebar bind:current={currentView} onnavigate={(v) => navigate(v)} {selectedWorkspace} {selectedRepo} />

  <div class="main">
    <header class="topbar">
      <div class="topbar-left">
        <span class="topbar-title" aria-live="polite" aria-atomic="true">{viewTitles[currentView] ?? 'Gyre'}</span>
        <nav class="scope-crumb" aria-label="Scope breadcrumb">
          {#each scopeCrumbs() as crumb, i}
            {#if i > 0}<span class="scope-sep" aria-hidden="true">›</span>{/if}
            {#if i < scopeCrumbs().length - 1}
              <button class="scope-crumb-link" onclick={() => navigate(crumb.view, crumb.ctx ?? {})}>
                {crumb.label}
              </button>
            {:else}
              <span class="scope-crumb-current">{crumb.label}</span>
            {/if}
          {/each}
        </nav>
      </div>
      <div class="topbar-right">
        <!-- Tenant selector -->
        {#if tenants.length > 1}
          <div class="ws-selector-wrap">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" class="ws-icon" aria-hidden="true">
              <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>
            </svg>
            <select
              class="ws-selector"
              value={selectedTenantId}
              onchange={onTenantChange}
              aria-label="Select tenant"
            >
              <option value="">All Tenants</option>
              {#each tenants as t}
                <option value={t.id}>{t.name}</option>
              {/each}
            </select>
          </div>
        {/if}
        <!-- Workspace selector -->
        {#if workspaces.length > 0}
          <div class="ws-selector-wrap">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" class="ws-icon" aria-hidden="true">
              <rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>
            </svg>
            <select
              class="ws-selector"
              value={selectedWorkspaceId}
              onchange={onWorkspaceChange}
              aria-label="Select workspace"
            >
              <option value="">All Workspaces</option>
              {#each workspaces as ws}
                <option value={ws.id}>{ws.name}</option>
              {/each}
            </select>
          </div>
        {:else if selectedWorkspace}
          <button
            class="scope-chip"
            onclick={() => navigate('workspace-detail', { workspace: selectedWorkspace })}
            aria-label="Current workspace: {selectedWorkspace.name}"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
              <rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>
            </svg>
            <span>{selectedWorkspace.name}</span>
          </button>
        {/if}

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
      {#if currentView === 'inbox'}
        <Inbox />
      {:else if currentView === 'briefing'}
        <Briefing />
      {:else if currentView === 'dashboard'}
        <DashboardHome {wsStore} onnavigate={(v) => navigate(v)} />
      {:else if currentView === 'activity'}
        <ActivityFeed {wsStore} />
      {:else if currentView === 'agents'}
        <AgentList workspaceId={selectedWorkspaceId} />
      {:else if currentView === 'tasks'}
        <TaskBoard workspaceId={selectedWorkspaceId} onSelectTask={(task) => navigate('task-detail', { task })} />
      {:else if currentView === 'task-detail' && selectedTask}
        <TaskDetail
          task={selectedTask}
          onBack={() => navigate('tasks')}
        />
      {:else if currentView === 'repos'}
        <RepoList workspaceId={selectedWorkspaceId} onSelectRepo={(repo) => navigate('repo-detail', { repo })} />
      {:else if currentView === 'repo-detail' && selectedRepo}
        <RepoDetail
          repo={selectedRepo}
          onBack={() => navigate('repos')}
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
      {:else if currentView === 'meta-specs'}
        <MetaSpecs />
      {:else if currentView === 'tenants'}
        <TenantList />
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
      {:else if currentView === 'explorer'}
        <ExplorerView />
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

  /* Scope breadcrumb: Gyre › Workspace › Repo */
  .scope-crumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
  }

  .scope-sep {
    color: var(--color-text-muted);
    user-select: none;
    font-size: var(--text-xs);
  }

  .scope-crumb-link {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
    transition: color var(--transition-fast);
  }

  .scope-crumb-link:hover {
    color: var(--color-text-secondary);
  }

  .scope-crumb-current {
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  /* Workspace selector */
  .ws-selector-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-radius: var(--radius);
  }

  .ws-icon {
    color: var(--color-primary);
    flex-shrink: 0;
  }

  .ws-selector {
    background: transparent;
    border: none;
    color: var(--color-primary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    cursor: pointer;
    padding: 0;
    max-width: 140px;
    appearance: none;
    outline: none;
  }

  .ws-selector option {
    background: var(--color-surface);
    color: var(--color-text);
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

  /* Workspace scope chip (fallback when dropdown not shown) */
  .scope-chip {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    white-space: nowrap;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .scope-chip:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-color: var(--color-primary);
  }

  .scope-chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
