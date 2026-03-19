<script>
  import { api } from '../lib/api.js';
  import StatusBadge from './StatusBadge.svelte';
  import AgentCardPanel from './AgentCardPanel.svelte';

  let agents = $state([]);
  let repos = $state([]);
  let tasks = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filter = $state('');
  let selected = $state(null);
  let showSpawnModal = $state(false);
  let spawnResult = $state(null);
  let spawnError = $state(null);
  let spawnLoading = $state(false);

  // Spawn form fields
  let spawnName = $state('');
  let spawnRepoId = $state('');
  let spawnTaskId = $state('');
  let spawnBranch = $state('');

  const statuses = ['Idle', 'Active', 'Blocked', 'Error', 'Dead'];
  const filtered = $derived(filter ? agents.filter((a) => a.status === filter) : agents);

  function formatTime(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  $effect(() => {
    api.agents()
      .then((data) => { agents = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
    api.allRepos().then((data) => { repos = data; }).catch(() => {});
    api.tasks().then((data) => { tasks = data; }).catch(() => {});
  });

  function selectAgent(a) {
    selected = selected?.id === a.id ? null : a;
  }

  function openSpawnModal() {
    spawnName = ''; spawnRepoId = ''; spawnTaskId = ''; spawnBranch = '';
    spawnResult = null; spawnError = null;
    showSpawnModal = true;
  }

  function closeSpawnModal() { showSpawnModal = false; }

  async function doSpawn() {
    if (!spawnName || !spawnRepoId || !spawnTaskId || !spawnBranch) {
      spawnError = 'All fields are required.';
      return;
    }
    spawnLoading = true; spawnError = null; spawnResult = null;
    try {
      spawnResult = await api.spawnAgent({ name: spawnName, repo_id: spawnRepoId, task_id: spawnTaskId, branch: spawnBranch });
      agents = await api.agents();
    } catch (e) {
      spawnError = e.message;
    } finally {
      spawnLoading = false;
    }
  }
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Agents</h2>
    <div class="controls">
      <select bind:value={filter}>
        <option value="">All statuses</option>
        {#each statuses as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
      <button class="spawn-btn" onclick={openSpawnModal}>+ Spawn Agent</button>
    </div>
  </div>

  {#if showSpawnModal}
    <div class="modal-backdrop" onclick={closeSpawnModal}>
      <div class="modal" onclick={(e) => e.stopPropagation()}>
        <h3>Spawn Agent</h3>

        {#if spawnResult}
          <div class="spawn-success">
            <p class="success-msg">Agent spawned successfully.</p>
            <dl>
              <dt>Agent ID</dt><dd>{spawnResult.agent.id}</dd>
              <dt>Token</dt><dd class="token">{spawnResult.token}</dd>
              <dt>Clone URL</dt><dd class="clone-url">{spawnResult.clone_url}</dd>
              <dt>Worktree</dt><dd>{spawnResult.worktree_path}</dd>
              <dt>Branch</dt><dd>{spawnResult.branch}</dd>
            </dl>
            <button class="modal-btn" onclick={closeSpawnModal}>Close</button>
          </div>
        {:else}
          <div class="form">
            <label>
              Name
              <input bind:value={spawnName} placeholder="worker-1" />
            </label>
            <label>
              Repository
              <select bind:value={spawnRepoId}>
                <option value="">Select repo...</option>
                {#each repos as r}
                  <option value={r.id}>{r.name}</option>
                {/each}
              </select>
            </label>
            <label>
              Task
              <select bind:value={spawnTaskId}>
                <option value="">Select task...</option>
                {#each tasks as t}
                  <option value={t.id}>{t.title}</option>
                {/each}
              </select>
            </label>
            <label>
              Branch
              <input bind:value={spawnBranch} placeholder="feat/my-feature" />
            </label>
            {#if spawnError}
              <p class="form-error">{spawnError}</p>
            {/if}
            <div class="form-actions">
              <button class="modal-btn secondary" onclick={closeSpawnModal}>Cancel</button>
              <button class="modal-btn primary" onclick={doSpawn} disabled={spawnLoading}>
                {spawnLoading ? 'Spawning...' : 'Spawn'}
              </button>
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if filtered.length === 0}
    <p class="state-msg muted">No agents found.</p>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Status</th>
            <th>Task</th>
            <th>Last Heartbeat</th>
            <th>Spawned</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as a}
            <tr class:selected={selected?.id === a.id} onclick={() => selectAgent(a)}>
              <td class="name">{a.name}</td>
              <td><StatusBadge value={a.status} /></td>
              <td class="dim">{a.current_task_id ?? '—'}</td>
              <td class="dim">{formatTime(a.last_heartbeat)}</td>
              <td class="dim">{formatTime(a.spawned_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>

      {#if selected}
        <div class="detail">
          <h3>Agent Detail: {selected.name}</h3>
          <dl>
            <dt>ID</dt><dd>{selected.id}</dd>
            <dt>Status</dt><dd><StatusBadge value={selected.status} /></dd>
            <dt>Parent</dt><dd>{selected.parent_id ?? '—'}</dd>
            <dt>Current Task</dt><dd>{selected.current_task_id ?? '—'}</dd>
            <dt>Budget (s)</dt><dd>{selected.lifetime_budget_secs ?? '—'}</dd>
            <dt>Spawned</dt><dd>{formatTime(selected.spawned_at)}</dd>
            <dt>Last Heartbeat</dt><dd>{formatTime(selected.last_heartbeat)}</dd>
          </dl>
          <AgentCardPanel agentId={selected.id} />
        </div>
      {/if}
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
  h3 { margin: 0 0 0.75rem; font-size: 0.9rem; color: var(--text); }

  select {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.3rem 0.6rem; font-size: 0.82rem; cursor: pointer;
  }

  .spawn-btn {
    background: var(--accent); color: #fff; border: none; border-radius: 4px;
    padding: 0.3rem 0.75rem; font-size: 0.82rem; cursor: pointer; font-weight: 600;
  }
  .spawn-btn:hover { opacity: 0.88; }

  .modal-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,0.55); z-index: 100;
    display: flex; align-items: center; justify-content: center;
  }

  .modal {
    background: var(--surface); border: 1px solid var(--border); border-radius: 8px;
    padding: 1.5rem; min-width: 360px; max-width: 480px; width: 100%;
  }

  .form { display: flex; flex-direction: column; gap: 0.75rem; }

  .form label {
    display: flex; flex-direction: column; gap: 0.25rem;
    font-size: 0.82rem; color: var(--text-dim);
  }

  .form input, .form select {
    background: var(--bg); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.4rem 0.6rem; font-size: 0.85rem;
  }

  .form-error { color: #f87171; font-size: 0.82rem; margin: 0; }

  .form-actions { display: flex; gap: 0.5rem; justify-content: flex-end; margin-top: 0.25rem; }

  .modal-btn {
    border: 1px solid var(--border); border-radius: 4px; padding: 0.35rem 0.9rem;
    font-size: 0.82rem; cursor: pointer; background: var(--surface); color: var(--text);
  }
  .modal-btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .modal-btn.primary:hover { opacity: 0.88; }
  .modal-btn.secondary:hover { background: var(--surface-hover); }
  .modal-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .spawn-success dl {
    display: grid; grid-template-columns: 7rem 1fr; gap: 0.4rem 0.5rem;
    font-size: 0.82rem; margin: 0.75rem 0 1rem;
  }
  .spawn-success dt { color: var(--text-dim); }
  .spawn-success dd { margin: 0; color: var(--text-muted); word-break: break-all; }
  .spawn-success .token { font-family: monospace; font-size: 0.75rem; }
  .spawn-success .clone-url { font-family: monospace; font-size: 0.75rem; }
  .success-msg { color: #4ade80; font-size: 0.85rem; margin: 0 0 0.5rem; }

  .scroll { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; }

  table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }

  th {
    text-align: left; padding: 0.4rem 0.6rem;
    color: var(--text-dim); font-weight: 500; font-size: 0.78rem;
    border-bottom: 1px solid var(--border); text-transform: uppercase; letter-spacing: 0.04em;
  }

  td { padding: 0.45rem 0.6rem; border-bottom: 1px solid var(--border-subtle); vertical-align: middle; }

  tr { cursor: pointer; transition: background 0.1s; }
  tr:hover { background: var(--surface-hover); }
  tr.selected { background: var(--accent-muted); }

  .name { color: var(--text); font-weight: 500; }
  .dim { color: var(--text-muted); font-size: 0.82rem; }

  .detail {
    margin-top: 1.5rem; padding: 1rem; background: var(--surface);
    border: 1px solid var(--border); border-radius: 6px;
  }

  dl { display: grid; grid-template-columns: 8rem 1fr; gap: 0.35rem 0.75rem; font-size: 0.85rem; }
  dt { color: var(--text-dim); }
  dd { margin: 0; color: var(--text-muted); }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
