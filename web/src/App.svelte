<script>
  import { createWsStore } from './lib/ws.js';
  import Sidebar from './components/Sidebar.svelte';
  import ActivityFeed from './components/ActivityFeed.svelte';
  import AgentList from './components/AgentList.svelte';
  import TaskBoard from './components/TaskBoard.svelte';
  import ProjectList from './components/ProjectList.svelte';
  import Settings from './components/Settings.svelte';
  import RepoDetail from './components/RepoDetail.svelte';
  import MergeRequestDetail from './components/MergeRequestDetail.svelte';
  import MergeQueueView from './components/MergeQueueView.svelte';
  import AdminPanel from './components/AdminPanel.svelte';

  let currentView = $state('activity');
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
    activity: 'Activity Feed',
    agents: 'Agents',
    tasks: 'Task Board',
    projects: 'Projects',
    'repo-detail': 'Repository',
    'mr-detail': 'Merge Request',
    'merge-queue': 'Merge Queue',
    admin: 'Admin Panel',
    settings: 'Settings',
  };
</script>

<div class="app">
  <Sidebar bind:current={currentView} />

  <div class="main">
    <header class="topbar">
      <span class="topbar-title">{viewTitles[currentView] ?? 'Gyre'}</span>
      <div class="topbar-right">
        <span class="ws-indicator {wsStatus}" title="WebSocket: {wsStatus}">
          <span class="ws-dot"></span>
          {wsStatus}
        </span>
        <span class="version">v0.1.0</span>
      </div>
    </header>

    <div class="content">
      {#if currentView === 'activity'}
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
      {:else if currentView === 'admin'}
        <AdminPanel />
      {:else}
        <Settings {wsStatus} />
      {/if}
    </div>
  </div>
</div>

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }

  :global(:root) {
    --bg:            #0f1117;
    --surface:       #161b27;
    --surface-hover: #1e2536;
    --border:        #2a3148;
    --border-subtle: #1e2536;
    --text:          #e2e8f0;
    --text-muted:    #94a3b8;
    --text-dim:      #4b5c7a;
    --accent:        #60a5fa;
    --accent-muted:  #60a5fa18;
    font-family: 'Inter', system-ui, -apple-system, sans-serif;
    font-size: 14px;
    line-height: 1.5;
  }

  :global(body) {
    background: var(--bg);
    color: var(--text);
    min-height: 100vh;
    overflow: hidden;
  }

  :global(html, body) { height: 100%; }

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

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.6rem 1.25rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .topbar-title {
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text);
  }

  .topbar-right {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .ws-indicator {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.78rem;
    color: var(--text-dim);
  }

  .ws-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-dim);
    flex-shrink: 0;
  }

  .ws-indicator.connected .ws-dot    { background: #4ade80; box-shadow: 0 0 5px #22c55e88; }
  .ws-indicator.disconnected .ws-dot { background: #f97316; }
  .ws-indicator.error .ws-dot        { background: #f87171; }
  .ws-indicator.auth-failed .ws-dot  { background: #f87171; }

  .version {
    font-size: 0.78rem;
    color: var(--text-dim);
    font-family: 'Courier New', monospace;
  }

  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
