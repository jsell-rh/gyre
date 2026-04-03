<script>
  import { getContext, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import ExplorerTreemap from '../lib/ExplorerTreemap.svelte';
  import ExplorerChat from '../lib/ExplorerChat.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import WorkspaceCards from './WorkspaceCards.svelte';
  import ExplorerCodeTab from './ExplorerCodeTab.svelte';
  import ExplorerFilterPanel from './ExplorerFilterPanel.svelte';
  import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';
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

  // New explorer state
  let explorerCanvasState = $state({ selectedNode: null, zoom: 1, visibleGroups: [], breadcrumb: [] });
  let activeViewQuery = $state(null);
  let explorerFilter = $state('all');
  let explorerLens = $state('structural');
  let explorerSavedViews = $state([]);
  let detailNode = $state(null);

  // Spec editor state (inline editing with progressive preview, §3)
  let specEditorOpen = $state(false);
  let specEditorPath = $state('');
  let specEditorContent = $state('');
  let specEditorOriginal = $state('');
  let specEditorLoading = $state(false);
  let specEditorError = $state('');
  let predictLoading = $state(false);
  let predictError = $state('');
  let ghostOverlays = $state([]);

  async function openSpecEditor(specPath) {
    if (!specPath || !selectedRepoId) return;
    specEditorOpen = true;
    specEditorPath = specPath;
    specEditorContent = '';
    specEditorOriginal = '';
    specEditorError = '';
    specEditorLoading = true;
    predictError = '';
    ghostOverlays = [];
    try {
      const spec = await api.specContent(specPath, selectedRepoId);
      const content = spec?.content ?? spec?.body ?? spec?.text ?? '';
      specEditorContent = content;
      specEditorOriginal = content;
    } catch (e) {
      specEditorError = e.message ?? 'Failed to load spec';
    } finally {
      specEditorLoading = false;
    }
  }

  function closeSpecEditor() {
    specEditorOpen = false;
    specEditorPath = '';
    specEditorContent = '';
    specEditorOriginal = '';
    specEditorError = '';
    predictError = '';
    ghostOverlays = [];
  }

  async function runPrediction() {
    if (!selectedRepoId || !specEditorPath || predictLoading) return;
    predictLoading = true;
    predictError = '';
    ghostOverlays = [];
    try {
      const result = await api.graphPredict(selectedRepoId, {
        spec_path: specEditorPath,
        draft_content: specEditorContent,
      });
      const overlays = [];
      for (const item of (result?.added ?? [])) {
        overlays.push({ id: item.id ?? `ghost-add-${overlays.length}`, name: item.name ?? item.qualified_name ?? 'new node', type: item.node_type ?? item.type ?? 'unknown', action: 'add' });
      }
      for (const item of (result?.changed ?? [])) {
        overlays.push({ id: item.id ?? `ghost-change-${overlays.length}`, name: item.name ?? item.qualified_name ?? 'changed node', type: item.node_type ?? item.type ?? 'unknown', action: 'change' });
      }
      for (const item of (result?.removed ?? [])) {
        overlays.push({ id: item.id ?? `ghost-remove-${overlays.length}`, name: item.name ?? item.qualified_name ?? 'removed node', type: item.node_type ?? item.type ?? 'unknown', action: 'remove' });
      }
      ghostOverlays = overlays;
    } catch (e) {
      predictError = e.message ?? 'Prediction failed';
    } finally {
      predictLoading = false;
    }
  }

  let specEditorDirty = $derived(specEditorContent !== specEditorOriginal);

  // Filter panel state
  let filterVisible = $state(false);
  let insightsCollapsed = $state(true);
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
        askError = $t('explorer_view.ask_request_failed', { values: { status: res.status } });
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
            askError = dataPayload || $t('explorer_view.ask_llm_failed');
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
      askError = $t('explorer_view.ask_llm_failed');
    } finally {
      if (!controller.signal.aborted) {
        askLoading = false;
        askAbortController = null;
        if (!askError) askQuery = '';
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
      wsReposError = e.message ?? $t('explorer_view.repos_load_failed', { values: { error: '' } });
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
      showToast($t('explorer_view.repos_load_failed', { values: { error: e.message } }), { type: 'error' });
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
      showToast($t('explorer_view.graph_error', { values: { error: e.message } }), { type: 'error' });
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
      showToast($t('explorer_view.concept_search_failed', { values: { error: e.message } }), { type: 'error' });
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
    ghostOverlays = [];
  });

  let selectedRepo = $derived.by(() => repos.find(r => r.id === selectedRepoId) ?? null);
  let conceptFilterIds = $derived.by(() =>
    conceptNodes ? new Set(conceptNodes.map(n => n.id)) : null
  );

  // ── Repo dependencies & risk metrics ────────────────────────────────
  let repoDeps = $state(null);
  let repoDepsLoading = $state(false);
  let repoRisks = $state(null);
  let repoRisksLoading = $state(false);
  let graphTypes = $state(null);
  let graphModules = $state(null);
  let graphTimeline = $state(null);

  $effect(() => {
    if (!selectedRepoId || explorerTab !== 'architecture') return;
    if (repoDeps !== null) return; // already loaded
    repoDepsLoading = true;
    repoRisksLoading = true;
    Promise.all([
      api.repoDependencies(selectedRepoId).catch(() => []),
      api.repoDependents(selectedRepoId).catch(() => []),
      api.repoGraphRisks(selectedRepoId).catch(() => []),
      api.repoGraphTypes(selectedRepoId).catch(() => ({ nodes: [] })),
      api.repoGraphModules(selectedRepoId).catch(() => ({ nodes: [] })),
      api.repoGraphTimeline(selectedRepoId).catch(() => []),
    ]).then(([deps, depts, risks, types, modules, timeline]) => {
      repoDeps = { dependencies: Array.isArray(deps) ? deps : [], dependents: Array.isArray(depts) ? depts : [] };
      repoRisks = Array.isArray(risks) ? risks : [];
      graphTypes = types?.nodes ?? (Array.isArray(types) ? types : []);
      graphModules = modules?.nodes ?? (Array.isArray(modules) ? modules : []);
      graphTimeline = Array.isArray(timeline) ? timeline : [];
    }).finally(() => { repoDepsLoading = false; repoRisksLoading = false; });
  });

  // Reset when repo changes
  $effect(() => {
    if (selectedRepoId) {
      repoDeps = null;
      repoRisks = null;
      graphTypes = null;
      graphModules = null;
      graphTimeline = null;
    }
  });
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#if scopeType === 'tenant'}
  <!-- Tenant scope: workspace cards grid (S4.4a) -->
  <WorkspaceCards {onSelectWorkspace} />

{:else if scopeType === 'workspace'}
  <!-- Workspace scope: repo list for graph exploration — S4.4b -->
  <div class="ws-repo-list" aria-busy={wsReposLoading}>
    <div class="ws-repo-header">
      <h1 class="page-title">{$t('explorer_view.workspace_title')}</h1>
      <p class="ws-repo-desc">{$t('explorer_view.workspace_desc')}</p>
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
        <button onclick={() => { wsReposError = null; loadWsRepos(); }} class="retry-btn">{$t('common.retry')}</button>
      </div>
    {:else if wsRepos.length === 0}
      <EmptyState title={$t('explorer_view.no_repos')} description={$t('explorer_view.no_repos_desc')} />
    {:else}
      <div class="ws-repo-grid">
        {#each wsRepos as repo (repo.id)}
          <button class="ws-repo-card" onclick={() => selectRepo(repo)} aria-label={$t('explorer_view.explore_repo', { values: { name: repo.name } })}>
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
            <span class="ws-repo-explore">{$t('explorer_view.explore_arrow')}</span>
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
        <h1 class="page-title">{scopeType === 'repo' ? $t('explorer_view.architecture_title') : $t('explorer_view.system_title')}</h1>
        {#if scopeType !== 'repo'}
          <p class="subtitle">{$t('explorer_view.system_subtitle')}</p>
        {/if}
      </div>
      <div class="header-right">
        <!-- Repo selector — hidden in repo scope (auto-selected from parent) -->
        {#if scopeType !== 'repo'}
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
                aria-label={$t('explorer_view.select_repo')}
              >
                <option value="">{$t('explorer_view.select_repo')}</option>
                {#each repos as repo}
                  <option value={repo.id}>{repo.name}</option>
                {/each}
              </select>
            </div>
          {/if}
        {/if}

        {#if graph && explorerTab === 'architecture'}
          <div class="graph-stats">
            <span class="stat">
              <span class="stat-val">{graph.nodes?.length ?? 0}</span>
              <span class="stat-label">{$t('explorer_canvas.nodes')}</span>
            </span>
            <span class="stat-sep">·</span>
            <span class="stat">
              <span class="stat-val">{graph.edges?.length ?? 0}</span>
              <span class="stat-label">{$t('explorer_canvas.edges')}</span>
            </span>
          </div>
        {/if}
      </div>
    </div>

    <!-- Architecture / Briefing / Code tab switcher — only shown in non-repo scope (in repo scope, these are separate top-level tabs) -->
    {#if selectedRepoId && scopeType !== 'repo'}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="explorer-tabs" role="tablist" aria-label={$t('explorer_view.system_title')}
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
        >{$t('explorer_view.sub_tabs.architecture')}</button>
        <button
          class="explorer-tab-btn {explorerTab === 'briefing' ? 'active' : ''}"
          role="tab"
          id="explorer-tab-briefing"
          aria-selected={explorerTab === 'briefing'}
          tabindex={explorerTab === 'briefing' ? 0 : -1}
          onclick={() => { explorerTab = 'briefing'; }}
          type="button"
        >{$t('explorer_view.sub_tabs.briefing')}</button>
        <button
          class="explorer-tab-btn {explorerTab === 'code' ? 'active' : ''}"
          role="tab"
          id="explorer-tab-code"
          aria-selected={explorerTab === 'code'}
          tabindex={explorerTab === 'code' ? 0 : -1}
          onclick={() => { explorerTab = 'code'; }}
          type="button"
        >{$t('explorer_view.sub_tabs.code')}</button>
      </div>
    {/if}

    <!-- Control bar — shown when architecture tab is active + repo loaded, but not when graph canvas is showing (canvas has its own toolbar) -->
    {#if selectedRepoId && explorerTab === 'architecture' && !graph}
      <div class="concept-search-bar">
        <!-- Filter toggle -->
        <button
          class="ctrl-btn icon-btn"
          class:active={filterVisible}
          onclick={() => { filterVisible = !filterVisible; }}
          title={$t('explorer_view.toggle_filters')}
          aria-label={$t('explorer_view.toggle_filters')}
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
            placeholder={$t('explorer_view.search_placeholder')}
            disabled={loading}
            value={conceptQuery}
            oninput={onSearchInput}
            onkeydown={onSearchKeydown}
            aria-label={$t('explorer_view.search_placeholder')}
            bind:this={searchInputEl}
          />
        </div>

        <span aria-live="polite" class="sr-only">
          {#if conceptLoading}
            {$t('explorer_view.searching')}
          {:else if conceptNodes !== null && conceptQuery.trim()}
            {#if conceptNodes.length > 0}
              {$t('explorer_view.concepts_found', { values: { count: conceptNodes.length } })}
            {:else}
              {$t('explorer_view.no_concepts')}
            {/if}
          {/if}
        </span>
        {#if conceptLoading}
          <span class="search-loading">
            <span class="spinner" aria-hidden="true"></span>
            {$t('explorer_view.searching')}
          </span>
        {:else if conceptNodes !== null && conceptQuery.trim()}
          {#if conceptNodes.length > 0}
            <span class="concept-chip">
              {$t('explorer_view.nodes_matching', { values: { count: conceptNodes.length, query: conceptQuery.trim() } })}
              <button class="chip-clear" onclick={clearConceptSearch} aria-label={$t('explorer_view.clear_search')}>✕</button>
            </span>
          {:else}
            <span class="concept-chip no-results">
              {$t('explorer_view.no_nodes_matching', { values: { query: conceptQuery.trim() } })}
              <button class="chip-clear" onclick={clearConceptSearch} aria-label={$t('explorer_view.clear_search')}>✕</button>
            </span>
          {/if}
        {/if}

        <span class="control-spacer"></span>

        <!-- Ask input (conversational exploration, §8 of system-explorer.md) -->
        <div class="ask-wrap">
          <input
            type="text"
            class="ask-input"
            placeholder={$t('explorer_view.ask_placeholder')}
            bind:value={askQuery}
            onkeydown={(e) => { if (e.key === 'Enter') submitAsk(); }}
            disabled={askLoading}
            aria-label={$t('explorer_view.ask_placeholder')}
            aria-busy={askLoading}
          />
          <button
            class="ctrl-btn ask-btn"
            onclick={submitAsk}
            disabled={askLoading || !askQuery.trim()}
            type="button"
            aria-label={$t('explorer_view.ask_btn')}
          >
            {#if askLoading}
              <span class="spinner" aria-hidden="true"></span>
            {:else}
              {$t('explorer_view.ask_btn')}
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
      <!-- Filter panel — hidden when graph canvas active (treemap has built-in filter toolbar) -->
      {#if explorerTab === 'architecture' && !graph}
        <ExplorerFilterPanel visible={filterVisible} onfilterchange={onFilterChange} />
      {/if}

      <div class="explorer-body-main">
        {#if !selectedRepoId}
          <div class="empty-state-wrap">
            {#if scopeType === 'repo'}
              <!-- Repo scope: repo ID will be set by the auto-select effect -->
              <Skeleton lines={6} />
            {:else}
              <EmptyState
                title={$t('explorer_view.select_repo')}
                description={$t('explorer_view.select_repo_desc')}
              />
              {#if repos.length === 0 && !reposLoading}
                <p class="hint">{$t('explorer_view.no_repos_hint')}</p>
                <button class="go-admin-btn" onclick={() => goToWorkspaceSettings?.()}>{$t('explorer_view.go_to_settings')}</button>
              {/if}
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
            <p class="loading-msg">{$t('explorer_view.fetching_graph')}</p>
          </div>

        {:else if graphError}
          <div class="graph-error" role="alert">
            <p>{$t('explorer_view.graph_error', { values: { error: graphError } })}</p>
            <button onclick={() => loadGraph(selectedRepoId)} aria-label={$t('common.retry')}>{$t('common.retry')}</button>
          </div>

        {:else if graph}
          <div class="explorer-split">
            <div class="explorer-canvas-area">
              <ExplorerTreemap
                repoId={selectedRepoId}
                nodes={graph.nodes ?? []}
                edges={graph.edges ?? []}
                activeQuery={activeViewQuery}
                filter={explorerFilter}
                lens={explorerLens}
                bind:canvasState={explorerCanvasState}
                onNodeDetail={(n) => { detailNode = n; }}
                {ghostOverlays}
              />
            </div>
            {#if detailNode}
              <div class="explorer-detail-area">
                <NodeDetailPanel
                  node={detailNode}
                  nodes={graph.nodes ?? []}
                  edges={graph.edges ?? []}
                  onClose={() => { detailNode = null; }}
                  onNavigate={(n) => { detailNode = n; }}
                />
                {#if detailNode.spec_path && !specEditorOpen}
                  <div class="edit-spec-action">
                    <button
                      class="edit-spec-btn"
                      onclick={() => openSpecEditor(detailNode.spec_path)}
                      type="button"
                    >{$t('explorer_view.edit_spec')}</button>
                  </div>
                {/if}
              </div>
            {/if}
            {#if specEditorOpen}
              <div class="spec-editor-panel" role="complementary" aria-label={$t('explorer_view.spec_editor_title')}>
                <div class="spec-editor-header">
                  <h3 class="spec-editor-title">{$t('explorer_view.spec_editor_title')}</h3>
                  <code class="spec-editor-path">{specEditorPath}</code>
                  <button class="spec-editor-close" onclick={closeSpecEditor} aria-label={$t('explorer_view.spec_editor_cancel')} type="button">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                      <path d="M18 6L6 18M6 6l12 12"/>
                    </svg>
                  </button>
                </div>
                <div class="spec-editor-body">
                  {#if specEditorLoading}
                    <div class="spec-editor-loading">
                      <span class="spinner" aria-hidden="true"></span>
                      <span>{$t('explorer_view.spec_editor_loading')}</span>
                    </div>
                  {:else if specEditorError}
                    <div class="spec-editor-error" role="alert">
                      <span>{$t('explorer_view.spec_editor_error', { values: { error: specEditorError } })}</span>
                    </div>
                  {:else}
                    <textarea
                      class="spec-editor-textarea"
                      bind:value={specEditorContent}
                      spellcheck="false"
                      aria-label="Spec content"
                    ></textarea>
                  {/if}
                </div>
                <div class="spec-editor-footer">
                  {#if predictError}
                    <div class="spec-editor-predict-error" role="alert">
                      {$t('explorer_view.spec_editor_predict_error', { values: { error: predictError } })}
                    </div>
                  {/if}
                  {#if ghostOverlays.length > 0}
                    <div class="spec-editor-predict-result" role="status">
                      {ghostOverlays.length} predicted {ghostOverlays.length === 1 ? 'change' : 'changes'}
                    </div>
                  {/if}
                  <div class="spec-editor-actions">
                    <button
                      class="spec-editor-cancel-btn"
                      onclick={closeSpecEditor}
                      type="button"
                    >{$t('explorer_view.spec_editor_cancel')}</button>
                    <button
                      class="spec-editor-preview-btn"
                      onclick={runPrediction}
                      disabled={predictLoading || !specEditorDirty}
                      type="button"
                    >
                      {#if predictLoading}
                        <span class="spinner" aria-hidden="true"></span>
                        {$t('explorer_view.spec_editor_predicting')}
                      {:else}
                        {$t('explorer_view.spec_editor_preview')}
                      {/if}
                    </button>
                  </div>
                </div>
              </div>
            {/if}
            <div class="explorer-chat-area">
              <ExplorerChat
                repoId={selectedRepoId}
                canvasState={explorerCanvasState}
                onViewQuery={(q) => { activeViewQuery = q; }}
                savedViews={explorerSavedViews}
              />
            </div>
          </div>
        {/if}

        <!-- Repo dependencies, risks, types, modules, timeline — hidden when graph canvas is active (canvas fills viewport) -->
        {#if selectedRepoId && explorerTab === 'architecture' && !loading && !graph && (repoDeps || repoRisks?.length || graphTypes?.length || graphModules?.length || graphTimeline?.length)}
          <div class="arch-insights-toggle">
            <button
              class="arch-insights-btn"
              onclick={() => insightsCollapsed = !insightsCollapsed}
              aria-expanded={!insightsCollapsed}
              aria-controls="arch-insights-panel"
            >
              <span class="arch-toggle-icon" class:open={!insightsCollapsed}>&#9654;</span>
              Architecture Insights
            </button>
          </div>
          <div class="arch-insights" id="arch-insights-panel" class:collapsed={insightsCollapsed}>
            <!-- Graph Types (structs/enums extracted from code) -->
            {#if graphTypes?.length > 0}
              <div class="arch-insight-section">
                <h3 class="arch-insight-title">Types ({graphTypes.length})</h3>
                <p class="arch-insight-desc">Structs, enums, and type definitions extracted from the codebase.</p>
                <div class="arch-type-grid">
                  {#each graphTypes.slice(0, 20) as node}
                    <div class="arch-type-card" title={node.qualified_name ?? node.name}>
                      <span class="arch-type-kind">{node.node_type ?? 'type'}</span>
                      <span class="arch-type-name">{node.name ?? node.qualified_name}</span>
                      {#if node.doc_comment}
                        <span class="arch-type-doc">{node.doc_comment.slice(0, 80)}{node.doc_comment.length > 80 ? '...' : ''}</span>
                      {/if}
                    </div>
                  {/each}
                  {#if graphTypes.length > 20}
                    <span class="arch-more">+{graphTypes.length - 20} more</span>
                  {/if}
                </div>
              </div>
            {/if}

            <!-- Graph Modules -->
            {#if graphModules?.length > 0}
              <div class="arch-insight-section">
                <h3 class="arch-insight-title">Modules ({graphModules.length})</h3>
                <p class="arch-insight-desc">Module hierarchy extracted from the codebase.</p>
                <ul class="arch-dep-list">
                  {#each graphModules.slice(0, 15) as mod}
                    <li class="arch-dep-item">
                      <span class="mono">{mod.qualified_name ?? mod.name}</span>
                      {#if mod.doc_comment}
                        <span class="arch-mod-doc">{mod.doc_comment.slice(0, 60)}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}

            {#if repoDeps && (repoDeps.dependencies.length > 0 || repoDeps.dependents.length > 0)}
              <div class="arch-insight-section">
                <h3 class="arch-insight-title">Cross-Repo Dependencies</h3>
                {#if repoDeps.dependencies.length > 0}
                  <div class="arch-dep-group">
                    <span class="arch-dep-label">Depends on ({repoDeps.dependencies.length})</span>
                    <ul class="arch-dep-list">
                      {#each repoDeps.dependencies as dep}
                        <li class="arch-dep-item">{dep.name ?? dep.repo_name ?? entityName('repo', dep.repo_id ?? dep)}</li>
                      {/each}
                    </ul>
                  </div>
                {/if}
                {#if repoDeps.dependents.length > 0}
                  <div class="arch-dep-group">
                    <span class="arch-dep-label">Depended on by ({repoDeps.dependents.length})</span>
                    <ul class="arch-dep-list">
                      {#each repoDeps.dependents as dep}
                        <li class="arch-dep-item">{dep.name ?? dep.repo_name ?? entityName('repo', dep.repo_id ?? dep)}</li>
                      {/each}
                    </ul>
                  </div>
                {/if}
              </div>
            {/if}
            {#if repoRisks?.length > 0}
              <div class="arch-insight-section">
                <h3 class="arch-insight-title">Risk Hotspots ({repoRisks.length})</h3>
                <p class="arch-insight-desc">Nodes scored for complexity, coupling, or churn that may warrant attention.</p>
                <ul class="arch-risk-list">
                  {#each repoRisks.slice(0, 10) as node}
                    <li class="arch-risk-item">
                      <span class="arch-risk-name">{node.qualified_name ?? node.name}</span>
                      <span class="arch-risk-score" title="Risk score">{node.risk_score ?? node.score ?? '—'}</span>
                      {#if node.risk_reason ?? node.reason}
                        <span class="arch-risk-reason">{node.risk_reason ?? node.reason}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}

            <!-- Architecture Timeline (deltas over time) -->
            {#if graphTimeline?.length > 0}
              <div class="arch-insight-section">
                <h3 class="arch-insight-title">Architecture Timeline ({graphTimeline.length} changes)</h3>
                <p class="arch-insight-desc">How the architecture has evolved over time.</p>
                <div class="arch-timeline">
                  {#each graphTimeline.slice(0, 10) as delta}
                    <div class="arch-timeline-entry">
                      <span class="arch-timeline-time">{delta.timestamp ? new Date(typeof delta.timestamp === 'number' ? delta.timestamp * 1000 : delta.timestamp).toLocaleDateString() : '—'}</span>
                      <span class="arch-timeline-label">{delta.change_type ?? delta.event ?? 'change'}</span>
                      {#if delta.added_count || delta.removed_count}
                        <span class="arch-timeline-stats">
                          {#if delta.added_count}<span class="diff-ins">+{delta.added_count}</span>{/if}
                          {#if delta.removed_count}<span class="diff-del">-{delta.removed_count}</span>{/if}
                        </span>
                      {/if}
                      {#if delta.commit_sha}
                        <code class="mono" style="font-size: var(--text-xs); color: var(--color-text-muted)">{delta.commit_sha.slice(0, 7)}</code>
                      {/if}
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
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

  /* ── Architecture insights (deps + risks) ────────────────────────── */
  .arch-insights-toggle {
    border-top: 1px solid var(--color-border);
    padding: var(--space-2) var(--space-6);
    flex-shrink: 0;
  }

  .arch-insights-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: none;
    border: none;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    padding: var(--space-1) 0;
    font-family: var(--font-body);
  }

  .arch-insights-btn:hover {
    color: var(--color-text);
  }

  .arch-toggle-icon {
    display: inline-block;
    font-size: 10px;
    transition: transform 0.15s ease;
  }

  .arch-toggle-icon.open {
    transform: rotate(90deg);
  }

  .arch-insights {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-6);
    flex-shrink: 0;
    overflow: hidden;
    max-height: 600px;
    transition: max-height 0.2s ease, padding 0.2s ease, opacity 0.2s ease;
  }

  .arch-insights.collapsed {
    max-height: 0;
    padding-top: 0;
    padding-bottom: 0;
    opacity: 0;
    pointer-events: none;
  }

  .arch-insight-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .arch-insight-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .arch-insight-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .arch-dep-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-dep-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .arch-dep-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .arch-dep-item {
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
  }

  .arch-risk-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-risk-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    font-size: var(--text-xs);
  }

  .arch-risk-name {
    font-family: var(--font-mono);
    color: var(--color-text);
    font-weight: 500;
  }

  .arch-risk-score {
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
    font-weight: 600;
    font-size: var(--text-xs);
  }

  .arch-risk-reason {
    color: var(--color-text-muted);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Graph types grid ──────────────────────────────────────────────── */
  .arch-type-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }

  .arch-type-card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
  }

  .arch-type-kind {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .arch-type-name {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .arch-type-doc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    line-height: 1.3;
  }

  .arch-mod-doc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-2);
  }

  .arch-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-2);
  }

  /* ── Architecture timeline ───────────────────────────────────────────── */
  .arch-timeline {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-timeline-entry {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    font-size: var(--text-xs);
  }

  .arch-timeline-time {
    color: var(--color-text-muted);
    flex-shrink: 0;
    min-width: 80px;
  }

  .arch-timeline-label {
    font-weight: 500;
    color: var(--color-text);
  }

  .arch-timeline-stats {
    display: flex;
    gap: var(--space-1);
    font-family: var(--font-mono);
  }

  .diff-ins { color: var(--color-success); }
  .diff-del { color: var(--color-danger); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  /* ── Explorer split layout (treemap + chat) ──────────────────────── */
  .explorer-split {
    display: flex;
    flex: 1;
    overflow: hidden;
    min-height: 0;
  }

  .explorer-canvas-area {
    flex: 1;
    overflow: hidden;
    min-width: 0;
    position: relative;
  }

  .explorer-detail-area {
    width: 320px;
    min-width: 280px;
    max-width: 380px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--color-border);
  }

  .explorer-chat-area {
    width: 360px;
    min-width: 280px;
    max-width: 480px;
    border-left: 1px solid var(--color-border);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* ── Edit Spec button (inside detail panel) ─────────────────────── */
  .edit-spec-action {
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .edit-spec-btn {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .edit-spec-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-color: var(--color-primary);
  }

  .edit-spec-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Spec Editor slide-out panel ───────────────────────────────── */
  .spec-editor-panel {
    width: 420px;
    min-width: 320px;
    max-width: 520px;
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .spec-editor-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-surface-elevated);
  }

  .spec-editor-title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .spec-editor-path {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .spec-editor-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .spec-editor-close:hover {
    color: var(--color-text);
    background: var(--color-surface);
  }

  .spec-editor-close:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .spec-editor-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .spec-editor-loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6);
    justify-content: center;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .spec-editor-error {
    padding: var(--space-4);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .spec-editor-textarea {
    flex: 1;
    width: 100%;
    resize: none;
    border: none;
    outline: none;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    tab-size: 2;
    min-height: 0;
  }

  .spec-editor-textarea:focus {
    background: color-mix(in srgb, var(--color-surface-elevated) 50%, var(--color-surface));
  }

  .spec-editor-footer {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3) var(--space-4);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--color-surface-elevated);
  }

  .spec-editor-predict-error {
    font-size: var(--text-xs);
    color: var(--color-danger);
    padding: var(--space-1) 0;
  }

  .spec-editor-predict-result {
    font-size: var(--text-xs);
    color: var(--color-success);
    font-weight: 500;
    padding: var(--space-1) 0;
  }

  .spec-editor-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }

  .spec-editor-cancel-btn {
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .spec-editor-cancel-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-text-muted);
  }

  .spec-editor-cancel-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .spec-editor-preview-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), opacity var(--transition-fast);
  }

  .spec-editor-preview-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 85%, black);
  }

  .spec-editor-preview-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spec-editor-preview-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (max-width: 900px) {
    .explorer-split {
      flex-direction: column;
    }
    .explorer-chat-area {
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--color-border);
      max-height: 50%;
    }
    .spec-editor-panel {
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--color-border);
      max-height: 50%;
    }
  }
</style>
