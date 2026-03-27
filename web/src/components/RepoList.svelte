<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { onSelectRepo, workspaceId = '' } = $props();

  let repos = $state([]);
  let loading = $state(true);
  let error = $state(null);

  // New repo modal
  let showAddRepo = $state(false);
  let repoName = $state('');
  let repoBranch = $state('main');
  let repoCreating = $state(false);

  // Mirror repo modal
  let showMirrorRepo = $state(false);
  let mirrorName = $state('');
  let mirrorUrl = $state('');
  let mirrorInterval = $state(300);
  let mirrorCreating = $state(false);

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' });
  }

  async function loadRepos(wsId = '') {
    loading = true;
    error = null;
    try {
      repos = await api.repos({ workspaceId: wsId });
    } catch (err) {
      error = err.message;
    }
    loading = false;
  }

  $effect(() => { loadRepos(workspaceId); });

  async function addRepo() {
    if (!repoName.trim()) return;
    if (!workspaceId) { toastError('Select a workspace first'); return; }
    repoCreating = true;
    try {
      await api.createRepo({
        name: repoName.trim(),
        workspace_id: workspaceId,
        default_branch: repoBranch.trim() || 'main',
      });
      toastSuccess('Repository created');
      showAddRepo = false;
      repoName = ''; repoBranch = 'main';
      await loadRepos(workspaceId);
    } catch (e) {
      toastError(e.message);
    }
    repoCreating = false;
  }

  async function addMirrorRepo() {
    if (!mirrorName.trim() || !mirrorUrl.trim()) return;
    if (!workspaceId) { toastError('Select a workspace first'); return; }
    mirrorCreating = true;
    try {
      await api.createMirrorRepo({
        name: mirrorName.trim(),
        workspace_id: workspaceId,
        url: mirrorUrl.trim(),
        interval_secs: mirrorInterval ?? 300,
      });
      toastSuccess('Mirror repository created');
      showMirrorRepo = false;
      mirrorName = ''; mirrorUrl = ''; mirrorInterval = 300;
      await loadRepos(workspaceId);
    } catch (e) {
      toastError(e.message);
    }
    mirrorCreating = false;
  }
</script>

<Modal bind:open={showAddRepo} title="New Repository" onsubmit={addRepo}>
  <div class="form">
    <label class="form-label">Repository Name
      <input class="form-input" bind:value={repoName} placeholder="my-repo" />
    </label>
    <label class="form-label">Default Branch
      <input class="form-input" bind:value={repoBranch} placeholder="main" />
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showAddRepo = false)}>Cancel</Button>
    <Button variant="primary" onclick={addRepo} disabled={repoCreating || !repoName.trim()}>
      {repoCreating ? 'Creating…' : 'Create Repository'}
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={showMirrorRepo} title="Mirror Repository" onsubmit={addMirrorRepo}>
  <div class="form">
    <label class="form-label">Repository Name
      <input class="form-input" bind:value={mirrorName} placeholder="my-mirror" />
    </label>
    <label class="form-label">Remote URL
      <input class="form-input" bind:value={mirrorUrl} placeholder="https://github.com/org/repo.git" />
    </label>
    <label class="form-label">Sync Interval (seconds)
      <input class="form-input" type="number" bind:value={mirrorInterval} min="60" placeholder="300" />
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showMirrorRepo = false)}>Cancel</Button>
    <Button variant="primary" onclick={addMirrorRepo} disabled={mirrorCreating || !mirrorName.trim() || !mirrorUrl.trim()}>
      {mirrorCreating ? 'Mirroring…' : 'Create Mirror'}
    </Button>
  {/snippet}
</Modal>

<div class="page" aria-busy={loading}>
  <span class="sr-only" aria-live="polite">{loading ? "" : "repositories loaded"}</span>
  <div class="page-hdr">
    <div>
      <h1 class="page-title">Repositories</h1>
      {#if !loading}
        <p class="page-desc">{repos.length} repositor{repos.length !== 1 ? 'ies' : 'y'}{workspaceId ? ' in workspace' : ''}</p>
      {/if}
    </div>
    <div class="hdr-actions">
      <Button variant="secondary" onclick={() => (showMirrorRepo = true)}><span aria-hidden="true">⟳</span> Mirror</Button>
      <Button variant="primary" onclick={() => (showAddRepo = true)}>+ New Repo</Button>
    </div>
  </div>

  {#if loading}
    <div class="repo-grid">
      {#each Array(6) as _}
        <div class="repo-card skeleton-card">
          <div class="card-hdr">
            <Skeleton width="60%" height="1.2rem" />
            <Skeleton width="80px" height="0.875rem" />
          </div>
          <Skeleton lines={2} height="0.875rem" />
        </div>
      {/each}
    </div>
  {:else if error}
    <div class="error-msg" role="alert">Error: {error}</div>
  {:else if repos.length === 0}
    <EmptyState
      title="No repositories yet"
      description={workspaceId ? 'Create your first repository in this workspace.' : 'Select a workspace or create a repository to get started.'}
    />
  {:else}
    <div class="scroll">
      <div class="repo-grid">
        {#each repos as r (r.id)}
          <button
            class="repo-card"
            onclick={() => onSelectRepo && onSelectRepo(r)}
            aria-label="Open repository {r.name}"
          >
            <div class="card-hdr">
              <h2 class="repo-name">{r.name}</h2>
              <span class="repo-date">{formatDate(r.created_at)}</span>
            </div>
            <div class="repo-meta">
              {#if r.is_mirror}
                <span class="badge mirror-badge" title={r.mirror_url}>mirror</span>
              {/if}
              {#if r.default_branch}
                <span class="branch-pill">{r.default_branch}</span>
              {/if}
            </div>
            {#if r.url}
              <p class="repo-url">{r.url}</p>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  .page-hdr {
    flex-shrink: 0;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .page-desc { font-size: var(--text-sm); color: var(--color-text-secondary); }

  .hdr-actions {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .scroll { flex: 1; overflow-y: auto; }

  .repo-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-6);
  }

  .repo-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-5) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  .repo-card:hover { border-color: var(--color-border-strong); background: var(--color-surface-elevated); }
  .repo-card:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .skeleton-card { cursor: default; }
  .skeleton-card:hover { border-color: var(--color-border); background: var(--color-surface); }

  .card-hdr {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .repo-name {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    line-height: 1.3;
  }

  .repo-date {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .repo-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .badge {
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-1);
    font-weight: 500;
  }

  .branch-pill {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-1);
  }

  .repo-url {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .form { display: flex; flex-direction: column; gap: var(--space-3); }

  .form-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  .form-input {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .form-input:focus:not(:focus-visible) { outline: none; }
  .form-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  @media (prefers-reduced-motion: reduce) {
    .repo-card, .form-input { transition: none; }
  }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
