<script>
  import { api } from '../lib/api.js';
  import StatusBadge from './StatusBadge.svelte';

  let tasks = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filterAgent = $state('');
  let filterPriority = $state('');

  const columns = [
    { key: 'Backlog',    label: 'Backlog' },
    { key: 'InProgress', label: 'In Progress' },
    { key: 'Review',     label: 'Review' },
    { key: 'Done',       label: 'Done' },
    { key: 'Blocked',    label: 'Blocked' },
  ];

  const priorities = ['Low', 'Medium', 'High', 'Critical'];

  const agents = $derived([...new Set(tasks.map((t) => t.assigned_to).filter(Boolean))].sort());

  const filtered = $derived(
    tasks.filter((t) => {
      if (filterAgent && t.assigned_to !== filterAgent) return false;
      if (filterPriority && t.priority !== filterPriority) return false;
      return true;
    })
  );

  function columnTasks(key) {
    return filtered.filter((t) => t.status === key);
  }

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  $effect(() => {
    api.tasks()
      .then((data) => { tasks = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
  });
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Task Board</h2>
    <div class="controls">
      <select bind:value={filterPriority}>
        <option value="">All priorities</option>
        {#each priorities as p}<option value={p}>{p}</option>{/each}
      </select>
      <select bind:value={filterAgent}>
        <option value="">All agents</option>
        {#each agents as a}<option value={a}>{a}</option>{/each}
      </select>
    </div>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else}
    <div class="board">
      {#each columns as col}
        <div class="column">
          <div class="col-header">
            <span class="col-title">{col.label}</span>
            <span class="col-count">{columnTasks(col.key).length}</span>
          </div>
          <div class="cards">
            {#each columnTasks(col.key) as task (task.id)}
              <div class="card">
                <div class="card-title">{task.title}</div>
                <div class="card-meta">
                  <StatusBadge value={task.priority} type="priority" />
                  {#if task.assigned_to}
                    <span class="assignee">{task.assigned_to}</span>
                  {/if}
                </div>
                {#if task.labels?.length}
                  <div class="labels">
                    {#each task.labels as label}
                      <span class="label-tag">{label}</span>
                    {/each}
                  </div>
                {/if}
                {#if task.pr_link}
                  <div class="pr-link">
                    <a href={task.pr_link} target="_blank" rel="noreferrer">PR ↗</a>
                  </div>
                {/if}
              </div>
            {:else}
              <p class="empty-col">—</p>
            {/each}
          </div>
        </div>
      {/each}
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

  .controls { display: flex; gap: 0.5rem; }

  select {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.3rem 0.6rem; font-size: 0.82rem; cursor: pointer;
  }

  .board {
    flex: 1; overflow-x: auto; overflow-y: hidden;
    display: flex; gap: 1rem; padding: 1rem 1.25rem;
  }

  .column {
    min-width: 220px; flex: 1;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 6px; display: flex; flex-direction: column; max-height: 100%;
  }

  .col-header {
    display: flex; justify-content: space-between; align-items: center;
    padding: 0.6rem 0.75rem; border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .col-title { font-size: 0.82rem; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .col-count { font-size: 0.78rem; color: var(--text-dim); background: var(--surface-hover); padding: 0.1rem 0.4rem; border-radius: 3px; }

  .cards { flex: 1; overflow-y: auto; padding: 0.5rem; display: flex; flex-direction: column; gap: 0.5rem; }

  .card {
    background: var(--bg); border: 1px solid var(--border);
    border-radius: 5px; padding: 0.6rem 0.75rem;
    display: flex; flex-direction: column; gap: 0.4rem;
    transition: border-color 0.1s;
  }

  .card:hover { border-color: var(--accent-muted); }

  .card-title { font-size: 0.85rem; color: var(--text); line-height: 1.3; }

  .card-meta { display: flex; align-items: center; gap: 0.4rem; flex-wrap: wrap; }

  .assignee { font-size: 0.75rem; color: var(--text-dim); }

  .labels { display: flex; gap: 0.3rem; flex-wrap: wrap; }

  .label-tag {
    font-size: 0.7rem; color: var(--text-dim);
    background: var(--surface-hover); border: 1px solid var(--border);
    padding: 0.05rem 0.35rem; border-radius: 3px;
  }

  .pr-link a { font-size: 0.75rem; color: var(--accent); text-decoration: none; }
  .pr-link a:hover { text-decoration: underline; }

  .empty-col { font-size: 0.82rem; color: var(--text-dim); text-align: center; padding: 1rem 0; font-style: italic; }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
</style>
