<script>
  import { api } from '../lib/api.js';
  import MoldableView from '../lib/MoldableView.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let repos = $state([]);
  let selectedRepoId = $state('');
  let graph = $state(null);
  let loading = $state(false);
  let reposLoading = $state(true);
  let selectedNode = $state(null);

  // Load repos on mount
  $effect(() => {
    loadRepos();
  });

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
    selectedNode = null;
    try {
      graph = await api.repoGraph(repoId);
    } catch (e) {
      showToast('Failed to load graph: ' + e.message, { type: 'error' });
      graph = { nodes: [], edges: [] };
    } finally {
      loading = false;
    }
  }

  function onRepoChange(e) {
    selectedRepoId = e.target.value;
    loadGraph(selectedRepoId);
  }

  function onSelectNode(node) {
    selectedNode = node;
  }

  let selectedRepo = $derived(() => repos.find(r => r.id === selectedRepoId) ?? null);
</script>

<div class="explorer-view">
  <!-- Header -->
  <div class="explorer-header">
    <div class="header-left">
      <h2>System Explorer</h2>
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

      {#if graph}
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

  <!-- Main content -->
  <div class="explorer-body">
    {#if !selectedRepoId}
      <div class="empty-state-wrap">
        <EmptyState
          title="Select a repository"
          message="Choose a repository above to explore its realized architecture — types, traits, functions, endpoints, and their relationships."
        />
        {#if repos.length === 0 && !reposLoading}
          <p class="hint">No repositories found. Create a project and repository to get started.</p>
        {/if}
      </div>

    {:else if loading}
      <div class="loading-wrap">
        <Skeleton lines={8} />
        <p class="loading-msg">Fetching knowledge graph…</p>
      </div>

    {:else if graph}
      <MoldableView
        nodes={graph.nodes ?? []}
        edges={graph.edges ?? []}
        onSelectNode={onSelectNode}
      />
    {/if}
  </div>
</div>

<style>
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

  .header-left h2 {
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
    outline: none;
    min-width: 180px;
    max-width: 280px;
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
    flex-direction: column;
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
</style>
