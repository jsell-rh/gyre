<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { onSelectRepo, workspaceId = '' } = $props();

  let projects = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let selected = $state(null);
  let repos = $state([]);
  let reposLoading = $state(false);

  // New project modal
  let showNewProject = $state(false);
  let projName = $state('');
  let projDesc = $state('');
  let projCreating = $state(false);

  // Add repo modal
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

  async function loadProjects(wsId = '') {
    try {
      projects = await api.projects({ workspaceId: wsId });
    } catch (err) {
      error = err.message;
    }
    loading = false;
  }

  $effect(() => { loadProjects(workspaceId); });

  async function selectProject(p) {
    if (selected?.id === p.id) { selected = null; repos = []; return; }
    selected = p;
    repos = [];
    reposLoading = true;
    try {
      repos = await api.repos(p.id);
    } catch {
      repos = [];
    }
    reposLoading = false;
  }

  async function createProject() {
    if (!projName.trim()) return;
    projCreating = true;
    try {
      await api.createProject({ name: projName.trim(), description: projDesc.trim() || undefined });
      toastSuccess('Project created');
      showNewProject = false;
      projName = ''; projDesc = '';
      loading = true;
      await loadProjects(workspaceId);
    } catch (e) {
      toastError(e.message);
    }
    projCreating = false;
  }

  async function addRepo() {
    if (!repoName.trim() || !selected) return;
    repoCreating = true;
    try {
      await api.createRepo({ name: repoName.trim(), project_id: selected.id, default_branch: repoBranch.trim() || 'main' });
      toastSuccess('Repository created');
      showAddRepo = false;
      repoName = ''; repoBranch = 'main';
      reposLoading = true;
      repos = await api.repos(selected.id);
      reposLoading = false;
    } catch (e) {
      toastError(e.message);
    }
    repoCreating = false;
  }

  async function addMirrorRepo() {
    if (!mirrorName.trim() || !mirrorUrl.trim() || !selected) return;
    mirrorCreating = true;
    try {
      await api.createMirrorRepo({ name: mirrorName.trim(), project_id: selected.id, url: mirrorUrl.trim(), interval_secs: mirrorInterval || 300 });
      toastSuccess('Mirror repository created');
      showMirrorRepo = false;
      mirrorName = ''; mirrorUrl = ''; mirrorInterval = 300;
      reposLoading = true;
      repos = await api.repos(selected.id);
      reposLoading = false;
    } catch (e) {
      toastError(e.message);
    }
    mirrorCreating = false;
  }
</script>

<Modal bind:open={showNewProject} title="New Project" onsubmit={createProject}>
  <div class="form">
    <label class="form-label">Name
      <input class="form-input" bind:value={projName} placeholder="my-project" />
    </label>
    <label class="form-label">Description
      <input class="form-input" bind:value={projDesc} placeholder="Optional description" />
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewProject = false)}>Cancel</Button>
    <Button variant="primary" onclick={createProject} disabled={projCreating || !projName.trim()}>
      {projCreating ? 'Creating…' : 'Create Project'}
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={showAddRepo} title="Add Repository" onsubmit={addRepo}>
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
      {repoCreating ? 'Creating…' : 'Add Repository'}
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

<div class="page">
  <div class="page-hdr">
    <div>
      <h1 class="page-title">Projects</h1>
      {#if !loading}
        <p class="page-desc">{projects.length} project{projects.length !== 1 ? 's' : ''}</p>
      {/if}
    </div>
    <Button variant="primary" onclick={() => (showNewProject = true)}>+ New Project</Button>
  </div>

  {#if loading}
    <div class="project-grid">
      {#each Array(6) as _}
        <div class="project-card skeleton-card">
          <div class="card-hdr">
            <Skeleton width="60%" height="1.2rem" />
            <Skeleton width="80px" height="0.875rem" />
          </div>
          <Skeleton lines={2} height="0.875rem" />
        </div>
      {/each}
    </div>
  {:else if error}
    <div class="error-msg">Error: {error}</div>
  {:else if projects.length === 0}
    <EmptyState
      title="No projects yet"
      description="Create your first project to get started with Gyre."
    />
  {:else}
    <div class="scroll">
      <div class="project-grid">
        {#each projects as p (p.id)}
          <div
            class="project-card"
            class:selected={selected?.id === p.id}
            onclick={() => selectProject(p)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectProject(p); } }}
            role="button"
            tabindex="0"
            aria-label="Select project {p.name}"
            aria-pressed={selected?.id === p.id}
          >
            <div class="card-hdr">
              <h2 class="project-name">{p.name}</h2>
              <span class="project-date">{formatDate(p.created_at)}</span>
            </div>
            {#if p.description}
              <p class="project-desc">{p.description}</p>
            {:else}
              <p class="project-desc muted">No description</p>
            {/if}

            {#if selected?.id === p.id}
              <div class="repos-section">
                <div class="repos-hdr">
                  <h4 class="repos-title">Repositories</h4>
                  <div class="repo-actions">
                    <!-- converted to button for a11y -->
                    <button class="add-repo-btn" onclick={(e) => { e.stopPropagation(); showAddRepo = true; }}>+ Add Repo</button>
                    <!-- converted to button for a11y -->
                    <button class="add-repo-btn" onclick={(e) => { e.stopPropagation(); showMirrorRepo = true; }}>⟳ Mirror</button>
                  </div>
                </div>
                {#if reposLoading}
                  <Skeleton lines={3} height="1.5rem" />
                {:else if repos.length === 0}
                  <p class="no-repos">No repositories in this project.</p>
                {:else}
                  <ul class="repo-list">
                    {#each repos as r (r.id)}
                      <li class="repo-item">
                        <!-- converted to button for a11y -->
                        <button
                          class="repo-link"
                          onclick={(e) => { e.stopPropagation(); onSelectRepo && onSelectRepo(r); }}
                        >
                          {r.name}
                        </button>
                        {#if r.is_mirror}
                          <span class="mirror-badge" title={r.mirror_url}>mirror</span>
                        {/if}
                        {#if r.url}
                          <!-- svelte-ignore a11y_click_events_have_key_events -->
                          <a
                            class="repo-url"
                            href={r.url}
                            target="_blank"
                            rel="noreferrer"
                            onclick={(e) => e.stopPropagation()}
                          >
                            {r.url}
                          </a>
                        {/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          </div>
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

  .scroll { flex: 1; overflow-y: auto; }

  .project-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-6);
  }

  .project-card {
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

  .project-card:hover { border-color: var(--color-border-strong); background: var(--color-surface-elevated); }
  .project-card.selected { border-color: var(--color-primary); }
  .skeleton-card { cursor: default; }
  .skeleton-card:hover { border-color: var(--color-border); background: var(--color-surface); }

  .card-hdr {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .project-name {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    line-height: 1.3;
  }

  .project-date {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .project-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.4;
  }

  .project-desc.muted { color: var(--color-text-muted); font-style: italic; }

  .repos-section {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .repos-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .repo-actions {
    display: flex;
    gap: var(--space-3);
  }

  .repos-title {
    font-family: var(--font-display);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
  }

  .add-repo-btn {
    background: none;
    border: none;
    padding: 0;
    font-size: var(--text-xs);
    color: var(--color-link);
    cursor: pointer;
    transition: color var(--transition-fast);
    font-family: var(--font-body);
  }

  .add-repo-btn:hover { color: var(--color-link-hover); }

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

  .form-input:focus { outline: none; border-color: var(--color-primary); }

  .repo-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .repo-item {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .repo-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: var(--color-link);
    font-weight: 500;
    cursor: pointer;
    transition: color var(--transition-fast);
    font-family: var(--font-body);
    font-size: var(--text-sm);
  }

  .repo-link:hover { color: var(--color-link-hover); text-decoration: underline; }

  .repo-url {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }

  .repo-url:hover { text-decoration: underline; }

  .mirror-badge {
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: 3px;
    padding: 0 var(--space-1);
    font-weight: 500;
  }

  .no-repos {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }
</style>
