<script>
  import { getContext, onDestroy } from 'svelte';
  import { api } from '../lib/api.js';
  import MoldableView from '../lib/MoldableView.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import WorkspaceCards from './WorkspaceCards.svelte';
  import ExplorerCodeTab from './ExplorerCodeTab.svelte';
  import ExplorerFilterPanel from './ExplorerFilterPanel.svelte';
  import Briefing from './Briefing.svelte';

  const navigate = getContext('navigate');
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings');

  // scope: { type: 'tenant' | 'workspace' | 'repo', workspaceId?, repoId? }
  // Defaults to tenant scope for backwards compatibility with old App.svelte.
  let { scope = { type: 'tenant' }, onSelectWorkspace = null, workspaceName = null } = $props();

  let scopeType = $derived(scope?.type ?? 'tenant');

  // ── Workspace-scope repo list ──────────────────────────────────────────
  let wsRepos = $state([]);
  let wsReposLoading = $state(true);
  let wsReposError = $state(null);

  // ── Repo-scope graph state ─────────────────────────────────────────────
  let repos = $state([]);
  let selectedRepoId = $state('');
  let graph = $state(null);
  let loading = $state(false);
  let reposLoading = $state(true);
  let selectedNode = null;
  let graphError = $state(null);

  // Repo-scope tab: 'architecture' | 'code' | 'briefing'
  let explorerTab = $state('architecture');

  // Filter panel state
  let filterVisible = $state(false);
  let activeFilters = $state(null);

  function onFilterChange(filters) {
    activeFilters = filters;
  }

  // Ask input state (conversational exploration, §3 Architecture tab)
  let askQuery = $state('');
  let askLoading = $state(false);
  let askExplanation = $state('');
  let askError = $state('');
  let askAbortController = null;

  async function submitAsk() {
    const wid = scope.workspaceId;
    if (!askQuery.trim() || !wid || askLoading) return;
    askAbortController?.abort();
    const controller = new AbortController();
    askAbortController = controller;
    askLoading = true;
    askExplanation = '';
    askError = '';
    try {
      const res = await api.generateExplorerView(wid, {
        question: askQuery.trim(),
        ...(selectedRepoId ? { repo_id: selectedRepoId } : {}),
      });
      if (!res.ok) {
        askError = `Request failed: ${res.status}`;
        askLoading = false;
        return;
      }
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const blocks = buffer.split('\n\n');
        buffer = blocks.pop() ?? '';
        for (const block of blocks) {
          if (!block.trim()) continue;
          const lines = block.split('\n');
          let eventType = 'message';
          let dataPayload = '';
          for (const line of lines) {
            if (line.startsWith('event: ')) eventType = line.slice(7).trim();
            else if (line.startsWith('data: ')) dataPayload += (dataPayload ? '\n' : '') + line.slice(6);
          }
          if (eventType === 'error') {
            askError = dataPayload || 'LLM connection failed';
          } else if (eventType === 'partial') {
            askExplanation += dataPayload;
          } else if (eventType === 'complete' || eventType === 'message') {
            try {
              const parsed = JSON.parse(dataPayload);
              if (parsed.explanation) askExplanation = parsed.explanation;
            } catch {
              if (dataPayload) askExplanation = dataPayload;
            }
          }
        }
      }
    } catch (e) {
      if (e.name === 'AbortError') return;
      askError = 'LLM connection failed';
    } finally {
      if (!controller.signal.aborted) {
        askLoading = false;
        askAbortController = null;
      }
    }
  }

  // Concept search state
  let conceptQuery = $state('');
  let conceptLoading = $state(false);
  let conceptNodes = $state(null); // null = no active search
  let conceptEdges = null;
  let debounceTimer = null;

  // Load repos when in workspace/repo scope (graph dropdown)
  $effect(() => {
    if (scopeType !== 'tenant') {
      loadRepos();
    }
    if (scopeType === 'workspace') {
      loadWsRepos();
    }
  });

  // Auto-select repo when scope.repoId is set
  $effect(() => {
    if (scopeType === 'repo' && scope.repoId && scope.repoId !== selectedRepoId) {
      selectedRepoId = scope.repoId;
      clearConceptSearch();
      loadGraph(scope.repoId);
    }
  });

  async function loadWsRepos() {
    wsReposLoading = true;
    wsReposError = null;
    try {
      wsRepos = scope.workspaceId
        ? await api.repos({ workspaceId: scope.workspaceId })
        : await api.allRepos();
    } catch (e) {
      wsReposError = e.message ?? 'Failed to load repositories';
      wsRepos = [];
    } finally {
      wsReposLoading = false;
    }
  }

  function selectRepo(repo) {
    // In workspace-scope mode, selecting a repo loads its graph in this view
    selectedRepoId = repo.id;
    clearConceptSearch();
    loadGraph(repo.id);
  }

  async function loadRepos() {
    reposLoading = true;
    try {
      repos = await api.allRepos();
    } catch (e) {
      showToast('Failed to load repositories: ' + e.message, { type: 'error' });
    } finally {
      reposLoading = false;
    }
  }

  async function loadGraph(repoId) {
    if (!repoId) { graph = null; return; }
    loading = true;
    graph = null;
    graphError = null;
    selectedNode = null;
    try {
      graph = await api.repoGraph(repoId);
    } catch (e) {
      showToast('Failed to load graph: ' + e.message, { type: 'error' });
      graphError = e.message;
      graph = { nodes: [], edges: [] };
    } finally {
      loading = false;
    }
  }

  function onRepoChange(e) {
    selectedRepoId = e.target.value;
    clearConceptSearch();
    loadGraph(selectedRepoId);
  }

  function onSelectNode(node) {
    selectedNode = node;
  }

  function onSearchInput(e) {
    conceptQuery = e.target.value;
    clearTimeout(debounceTimer);
    if (!conceptQuery.trim()) {
      conceptNodes = null;
      conceptEdges = null;
      return;
    }
    debounceTimer = setTimeout(() => doConceptSearch(conceptQuery.trim()), 300);
  }

  function onSearchKeydown(e) {
    if (e.key === 'Enter') {
      clearTimeout(debounceTimer);
      const q = conceptQuery.trim();
      if (q) doConceptSearch(q);
    }
  }

  async function doConceptSearch(q) {
    if (!selectedRepoId) return;
    conceptLoading = true;
    try {
      const result = await api.getGraphConcept(selectedRepoId, q);
      conceptNodes = result.nodes ?? [];
      conceptEdges = result.edges ?? [];
    } catch (e) {
      showToast('Concept search failed: ' + e.message, { type: 'error' });
      conceptNodes = [];
      conceptEdges = [];
    } finally {
      conceptLoading = false;
    }
  }

  function clearConceptSearch() {
    conceptQuery = '';
    conceptNodes = null;
    conceptEdges = null;
    clearTimeout(debounceTimer);
  }

  let searchInputEl = $state(null);

  function onWindowKeydown(e) {
    if (e.key === '/' && e.target.tagName !== 'INPUT' && e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'SELECT' && !e.target.isContentEditable) {
      e.preventDefault();
      searchInputEl?.focus();
    }
  }

  onDestroy(() => {
    clearTimeout(debounceTimer);
    askAbortController?.abort();
  });

  let selectedRepo = $derived.by(() => repos.find(r => r.id === selectedRepoId) ?? null);
  let conceptFilterIds = $derived.by(() =>
    conceptNodes ? new Set(conceptNodes.map(n => n.id)) : null
  );
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#if scopeType === 'tenant'}
  <!-- Tenant scope: workspace cards grid (S4.4a) -->
  <WorkspaceCards {onSelectWorkspace} />

{:else if scopeType === 'workspace'}
  <!-- Workspace scope: repo list for graph exploration — S4.4b -->
  <div class="ws-repo-list" aria-busy={wsReposLoading}>
    <div class="ws-repo-header">
      <h1 class="page-title">Workspace Architecture</h1>
      <p class="ws-repo-desc">Select a workspace repository to explore its knowledge graph</p>
    </div>
    {#if wsReposLoading}
      <div class="ws-repo-grid">
        <Skeleton height="80px" />
        <Skeleton height="80px" />
        <Skeleton height="80px" />
      </div>
    {:else if wsReposError}
      <div class="error-banner" role="alert">
        <span>{wsReposError}</span>
        <button onclick={() => { wsReposError = null; loadWsRepos(); }} class="retry-btn">Retry</button>
      </div>
    {:else if wsRepos.length === 0}
      <EmptyState title="No repositories" description="Create a repository in this workspace to explore its architecture." />
    {:else}
      <div class="ws-repo-grid">
        {#each wsRepos as repo (repo.id)}
          <button class="ws-repo-card" onclick={() => selectRepo(repo)} aria-label="Explore {repo.name} repository">
            <div class="ws-repo-card-left">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" class="ws-repo-icon" aria-hidden="true">
                <path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/>
              </svg>
              <div class="ws-repo-info">
                <span class="ws-repo-name">{repo.name}</span>
                {#if repo.description}
                  <span class="ws-repo-description">{repo.description}</span>
                {/if}
              </div>
            </div>
            <span class="ws-repo-explore">Explore &rarr;</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

{:else}
  <!-- Repo scope: architecture + code — existing graph view (S4.4b/c) -->
  <div class="explorer-view">
    <!-- Header -->
    <div class="explorer-header">
      <div class="header-left">
        <h1 class="page-title">System Explorer</h1>
        <p class="subtitle">Realized architecture — navigate the living knowledge graph</p>
      </div>
      <div class="header-right">
        {#if reposLoading}
          <div class="repo-selector-skeleton">
            <Skeleton lines={1} />
          </div>
        {:else}
          <div class="repo-select-wrap">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" class="repo-icon" aria-hidden="true">
              <path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/>
            </svg>
            <select
              class="repo-select"
              value={selectedRepoId}
              onchange={onRepoChange}
              aria-label="Select repository to explore"
            >
              <option value="">Select a repository…</option>
              {#each repos as repo}
                <option value={repo.id}>{repo.name}</option>
              {/each}
            </select>
          </div>
        {/if}

        {#if graph && explorerTab === 'architecture'}
          <div class="graph-stats">
            <span class="stat">
              <span class="stat-val">{graph.nodes?.length ?? 0}</span>
              <span class="stat-label">nodes</span>
            </span>
            <span class="stat-sep">·</span>
            <span class="stat">
              <span class="stat-val">{graph.edges?.length ?? 0}</span>
              <span class="stat-label">edges</span>
            </span>
          </div>
        {/if}
      </div>
    </div>

    <!-- Architecture / Code tab switcher — only shown when a repo is selected -->
    {#if selectedRepoId}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="explorer-tabs" role="tablist" aria-label="Explorer view"
        onkeydown={(e) => {
          const tabs = ['architecture', 'briefing', 'code'];
          const idx = tabs.indexOf(explorerTab);
          if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
            e.preventDefault();
            const ni = (idx + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length;
            explorerTab = tabs[ni];
            document.getElementById('explorer-tab-' + tabs[ni])?.focus();
          }
        }}
      >
        <button
          class="explorer-tab-btn {explorerTab === 'architecture' ? 'active' : ''}"
          role="tab"
          id="explorer-tab-architecture"
          aria-selected={explorerTab === 'architecture'}
          tabindex={explorerTab === 'architecture' ? 0 : -1}
          onclick={() => { explorerTab = 'architecture'; }}
          type="button"
        >Architecture</button>
        <button
          class="explorer-tab-btn {explorerTab === 'briefing' ? 'active' : ''}"
          role="tab"
          id="explorer-tab-briefing"
          aria-selected={explorerTab === 'briefing'}
          tabindex={explorerTab === 'briefing' ? 0 : -1}
          onclick={() => { explorerTab = 'briefing'; }}
          type="button"
        >Briefing</button>
        <button
          class="explorer-tab-btn {explorerTab === 'code' ? 'active' : ''}"
          role="tab"
          id="explorer-tab-code"
          aria-selected={explorerTab === 'code'}
          tabindex={explorerTab === 'code' ? 0 : -1}
          onclick={() => { explorerTab = 'code'; }}
          type="button"
        >Code</button>
      </div>
    {/if}

    <!-- Control bar — shown when architecture tab is active + repo loaded -->
    {#if selectedRepoId && explorerTab === 'architecture'}
      <div class="concept-search-bar">
        <!-- Filter toggle -->
        <button
          class="ctrl-btn icon-btn"
          class:active={filterVisible}
          onclick={() => { filterVisible = !filterVisible; }}
          title="Toggle filter panel"
          aria-label="Toggle filter panel"
          aria-pressed={filterVisible}
          type="button"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>
          </svg>
        </button>

        <!-- Concept search -->
        <div class="search-input-wrap">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" class="search-icon" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <input
            type="search"
            class="concept-input"
            placeholder="Search concepts… (/)"
            disabled={loading}
            value={conceptQuery}
            oninput={onSearchInput}
            onkeydown={onSearchKeydown}
            aria-label="Search concepts by name or qualified name"
            bind:this={searchInputEl}
          />
        </div>

        <span aria-live="polite" class="sr-only">
          {#if conceptLoading}
            Searching…
          {:else if conceptNodes !== null && conceptQuery.trim()}
            {#if conceptNodes.length > 0}
              {conceptNodes.length} concept{conceptNodes.length === 1 ? '' : 's'} found
            {:else}
              No concepts found
            {/if}
          {/if}
        </span>
        {#if conceptLoading}
          <span class="search-loading">
            <span class="spinner" aria-hidden="true"></span>
            Searching…
          </span>
        {:else if conceptNodes !== null && conceptQuery.trim()}
          {#if conceptNodes.length > 0}
            <span class="concept-chip">
              {conceptNodes.length} {conceptNodes.length === 1 ? 'node' : 'nodes'} matching '{conceptQuery.trim()}'
              <button class="chip-clear" onclick={clearConceptSearch} aria-label="Clear search">✕</button>
            </span>
          {:else}
            <span class="concept-chip no-results">
              No nodes matching '{conceptQuery.trim()}'
              <button class="chip-clear" onclick={clearConceptSearch} aria-label="Clear search">✕</button>
            </span>
          {/if}
        {/if}

        <span class="control-spacer"></span>

        <!-- Ask input (conversational exploration, §8 of system-explorer.md) -->
        <div class="ask-wrap">
          <input
            type="text"
            class="ask-input"
            placeholder="Ask: How does auth work?"
            bind:value={askQuery}
            onkeydown={(e) => { if (e.key === 'Enter') submitAsk(); }}
            disabled={askLoading}
            aria-label="Ask about architecture"
            aria-busy={askLoading}
          />
          <button
            class="ctrl-btn ask-btn"
            onclick={submitAsk}
            disabled={askLoading || !askQuery.trim()}
            type="button"
            aria-label="Submit question"
          >
            {#if askLoading}
              <span class="spinner" aria-hidden="true"></span>
            {:else}
              Ask
            {/if}
          </button>
        </div>
      </div>

      <!-- Ask explanation/error feedback -->
      {#if askExplanation || askError}
        <div class="ask-feedback" role="status" aria-live="polite">
          {#if askError}
            <span class="ask-error">{askError}</span>
          {:else}
            <span class="ask-explanation">{askExplanation}</span>
          {/if}
        </div>
      {/if}
    {/if}

    <!-- Main content -->
    <div class="explorer-body">
      <!-- Filter panel (collapsible left panel, per ui-navigation.md §3) -->
      {#if explorerTab === 'architecture'}
        <ExplorerFilterPanel visible={filterVisible} onfilterchange={onFilterChange} />
      {/if}

      <div class="explorer-body-main">
        {#if !selectedRepoId}
          <div class="empty-state-wrap">
            <EmptyState
              title="Select a repository"
              description="Choose a repository above to explore its realized architecture — types, traits, functions, endpoints, and their relationships."
            />
            {#if repos.length === 0 && !reposLoading}
              <p class="hint">No repositories found. Create a project and repository to get started.</p>
              <button class="go-admin-btn" onclick={() => goToWorkspaceSettings?.()}>Go to Settings</button>
            {/if}
          </div>

        {:else if explorerTab === 'briefing'}
          <div class="briefing-wrap">
            <Briefing workspaceId={scope.workspaceId} repoId={selectedRepoId} scope={scopeType === 'repo' ? 'repo' : 'workspace'} workspaceName={workspaceName} />
          </div>

        {:else if explorerTab === 'code'}
          <ExplorerCodeTab repoId={selectedRepoId} repo={selectedRepo} />

        {:else if loading}
          <div class="loading-wrap">
            <Skeleton lines={8} />
            <p class="loading-msg">Fetching knowledge graph…</p>
          </div>

        {:else if graphError}
          <div class="graph-error" role="alert">
            <p>Failed to load graph: {graphError}</p>
            <button onclick={() => loadGraph(selectedRepoId)} aria-label="Retry loading graph">Retry</button>
          </div>

        {:else if graph}
          <MoldableView
            nodes={graph.nodes ?? []}
            edges={graph.edges ?? []}
            repoId={selectedRepoId}
            onSelectNode={onSelectNode}
            conceptFilterIds={conceptFilterIds}
            conceptQuery={conceptQuery.trim()}
          />
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  /* ── Workspace scope repo list ──────────────────────────────────────── */
  .ws-repo-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-6);
    overflow-y: auto;
    height: 100%;
  }

  .ws-repo-header .page-title {
    margin: 0 0 var(--space-1);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .ws-repo-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .ws-repo-grid {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .ws-repo-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), background var(--transition-fast);
    width: 100%;
  }

  .ws-repo-card:hover {
    border-color: var(--color-focus);
    background: var(--color-surface-elevated);
  }

  .ws-repo-card:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ws-repo-card-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .ws-repo-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .ws-repo-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .ws-repo-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .ws-repo-description {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-repo-explore {
    font-size: var(--text-sm);
    color: var(--color-link);
    flex-shrink: 0;
    font-weight: 500;
  }

  /* ── Repo scope: graph view styles ───────────────────────────────────── */
  .explorer-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .explorer-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .header-left .page-title {
    margin: 0 0 var(--space-1);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .subtitle {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .repo-selector-skeleton {
    width: 200px;
  }

  .repo-select-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
  }

  .repo-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .repo-select {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    min-width: 180px;
    max-width: 280px;
  }

  .repo-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .repo-select option {
    background: var(--color-surface);
    color: var(--color-text);
  }

  .graph-stats {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .stat {
    display: flex;
    align-items: baseline;
    gap: 3px;
  }

  .stat-val {
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .stat-sep {
    color: var(--color-text-muted);
  }

  .explorer-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: row;
  }

  .explorer-body-main {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .briefing-wrap {
    flex: 1;
    overflow-y: auto;
  }

  .empty-state-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
  }

  .hint {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-align: center;
    margin: 0;
  }

  .go-admin-btn {
    background: var(--color-link);
    color: var(--color-text-inverse);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }
  .go-admin-btn:hover { background: var(--color-link-hover); }
  .go-admin-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .loading-wrap {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .loading-msg {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-align: center;
    margin: 0;
    font-style: italic;
  }

  /* Architecture / Code tab switcher */
  .explorer-tabs {
    display: flex;
    gap: 0;
    padding: 0 var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
  }

  .explorer-tab-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .explorer-tab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .explorer-tab-btn:not(.active):hover {
    color: var(--color-text);
  }

  .explorer-tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* Concept search bar */
  .concept-search-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .search-input-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    min-width: 220px;
  }

  .search-input-wrap:focus-within {
    border-color: var(--color-focus);
    box-shadow: 0 0 0 2px var(--color-focus);
  }

  .search-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .concept-input {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    outline: none;
    width: 100%;
    min-width: 160px;
  }

  .concept-input::placeholder {
    color: var(--color-text-muted);
  }

  .concept-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  /* Remove browser default search cancel button */
  .concept-input::-webkit-search-cancel-button { display: none; }

  .search-loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid var(--color-border-strong);
    border-top-color: var(--color-focus);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner { animation: none; }
    .ws-repo-card,
    .go-admin-btn,
    .chip-clear,
    .graph-error button { transition: none; }
  }

  .concept-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    background: color-mix(in srgb, var(--color-focus) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-focus) 30%, transparent);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    color: var(--color-focus);
    font-family: var(--font-mono);
  }

  .concept-chip.no-results {
    background: color-mix(in srgb, var(--color-text-muted) 10%, transparent);
    border-color: var(--color-border-strong);
    color: var(--color-text-muted);
  }

  .chip-clear {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: var(--text-xs);
    line-height: 1;
    padding: 0 var(--space-1);
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }

  .chip-clear:hover { opacity: 1; }
  .chip-clear:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* ── Workspace repo error ─────────────────────────────────────────────── */
  .error-banner {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-4);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-link) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-link) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-link) 25%, transparent);
    border-color: var(--color-link);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Graph error state ────────────────────────────────────────────────── */
  .graph-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-6);
    text-align: center;
    flex: 1;
  }

  .graph-error p {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .graph-error button {
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .graph-error button:hover {
    background: var(--color-surface);
    border-color: var(--color-focus);
  }

  .graph-error button:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Filter toggle button ──────────────────────────────────────────── */
  .ctrl-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    flex-shrink: 0;
  }

  .ctrl-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .ctrl-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ctrl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .icon-btn.active {
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .control-spacer {
    flex: 1;
  }

  /* ── Ask input ─────────────────────────────────────────────────────── */
  .ask-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .ask-input {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    min-width: 180px;
    max-width: 260px;
    outline: none;
    transition: border-color var(--transition-fast);
  }

  .ask-input::placeholder { color: var(--color-text-muted); }

  .ask-input:focus {
    border-color: var(--color-focus);
    box-shadow: 0 0 0 2px var(--color-focus);
  }

  .ask-btn {
    padding: var(--space-1) var(--space-3);
  }

  /* ── Ask feedback ──────────────────────────────────────────────────── */
  .ask-feedback {
    padding: var(--space-2) var(--space-6);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-sm);
    flex-shrink: 0;
    max-height: 100px;
    overflow-y: auto;
  }

  .ask-explanation { color: var(--color-text-secondary); }
  .ask-error { color: var(--color-danger); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
