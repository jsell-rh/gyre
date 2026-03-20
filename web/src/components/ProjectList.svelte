<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';

  let { onSelectRepo } = $props();

  let projects = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let selected = $state(null);
  let repos = $state([]);
  let reposLoading = $state(false);

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' });
  }

  $effect(() => {
    api.projects()
      .then((data) => { projects = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
  });

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
</script>

<div class="page">
  <div class="page-hdr">
    <div>
      <h1 class="page-title">Projects</h1>
      {#if !loading}
        <p class="page-desc">{projects.length} project{projects.length !== 1 ? 's' : ''}</p>
      {/if}
    </div>
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
          <button
            class="project-card"
            class:selected={selected?.id === p.id}
            onclick={() => selectProject(p)}
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
                <h4 class="repos-title">Repositories</h4>
                {#if reposLoading}
                  <Skeleton lines={3} height="1.5rem" />
                {:else if repos.length === 0}
                  <p class="no-repos">No repositories in this project.</p>
                {:else}
                  <ul class="repo-list">
                    {#each repos as r (r.id)}
                      <li class="repo-item">
                        <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
                        <span
                          class="repo-link"
                          onclick={(e) => { e.stopPropagation(); onSelectRepo && onSelectRepo(r); }}
                        >
                          {r.name}
                        </span>
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

  .page-hdr { flex-shrink: 0; }

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
    gap: var(--space-4);
  }

  .project-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
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

  .repos-title {
    font-family: var(--font-display);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
  }

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
    color: var(--color-link);
    font-weight: 500;
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .repo-link:hover { color: var(--color-link-hover); text-decoration: underline; }

  .repo-url {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }

  .repo-url:hover { text-decoration: underline; }

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
