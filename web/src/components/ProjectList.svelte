<script>
  import { api } from '../lib/api.js';

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

<div class="panel">
  <div class="panel-header">
    <h2>Projects</h2>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if projects.length === 0}
    <p class="state-msg muted">No projects yet.</p>
  {:else}
    <div class="scroll">
      <ul class="project-list">
        {#each projects as p (p.id)}
          <li>
          <button class="project-item" class:selected={selected?.id === p.id} onclick={() => selectProject(p)}>
            <div class="p-header">
              <span class="p-name">{p.name}</span>
              <span class="p-date">{formatDate(p.created_at)}</span>
            </div>
            {#if p.description}
              <p class="p-desc">{p.description}</p>
            {/if}

            {#if selected?.id === p.id}
              <div class="repos">
                <h4>Repositories</h4>
                {#if reposLoading}
                  <p class="muted">Loading…</p>
                {:else if repos.length === 0}
                  <p class="muted">No repositories.</p>
                {:else}
                  <ul class="repo-list">
                    {#each repos as r (r.id)}
                      <li class="repo-item">
                        <span class="r-name">{r.name}</span>
                        {#if r.url}<a class="r-url" href={r.url} target="_blank" rel="noreferrer">{r.url}</a>{/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h4 { margin: 0 0 0.5rem; font-size: 0.82rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.04em; }

  .scroll { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; }

  .project-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.5rem; }

  .project-item {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 6px; padding: 0.8rem 1rem;
    cursor: pointer; transition: border-color 0.1s;
    width: 100%; text-align: left; color: inherit; font: inherit;
  }

  .project-item:hover { border-color: var(--accent-muted); }
  .project-item.selected { border-color: var(--accent); }

  .p-header { display: flex; justify-content: space-between; align-items: center; }
  .p-name { font-weight: 600; color: var(--text); font-size: 0.9rem; }
  .p-date { font-size: 0.78rem; color: var(--text-dim); }
  .p-desc { margin: 0.35rem 0 0; font-size: 0.83rem; color: var(--text-muted); }

  .repos { margin-top: 0.75rem; padding-top: 0.75rem; border-top: 1px solid var(--border); }

  .repo-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.35rem; }

  .repo-item { display: flex; gap: 0.75rem; align-items: baseline; font-size: 0.83rem; }
  .r-name { color: var(--text-muted); font-weight: 500; }
  .r-url { color: var(--accent); text-decoration: none; font-size: 0.78rem; }
  .r-url:hover { text-decoration: underline; }

  .muted { color: var(--text-dim); font-style: italic; font-size: 0.82rem; }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
