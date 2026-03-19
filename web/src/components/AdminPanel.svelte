<script>
  import { api } from '../lib/api.js';

  let health = $state(null);
  let jobs = $state([]);
  let auditEvents = $state([]);
  let agents = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let activeSection = $state('health');

  // Audit filter state
  let auditAgentFilter = $state('');
  let auditTypeFilter = $state('');

  // Kill/reassign modal state
  let actionModal = $state(null); // { type: 'kill'|'reassign', agent }
  let reassignTargetId = $state('');
  let actionError = $state(null);
  let actionLoading = $state(false);

  $effect(() => {
    loadAll();
  });

  async function loadAll() {
    loading = true;
    error = null;
    try {
      const [h, j, a, ag] = await Promise.all([
        api.adminHealth(),
        api.adminJobs(),
        api.adminAudit(),
        api.agents(),
      ]);
      health = h;
      jobs = j;
      auditEvents = a.events ?? [];
      agents = ag;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadAudit() {
    try {
      const params = new URLSearchParams();
      if (auditAgentFilter) params.set('agent_id', auditAgentFilter);
      if (auditTypeFilter) params.set('event_type', auditTypeFilter);
      const result = await api.adminAudit(Object.fromEntries(params));
      auditEvents = result.events ?? [];
    } catch (e) {
      error = e.message;
    }
  }

  function openKill(agent) {
    actionModal = { type: 'kill', agent };
    actionError = null;
  }

  function openReassign(agent) {
    actionModal = { type: 'reassign', agent };
    reassignTargetId = '';
    actionError = null;
  }

  function closeModal() {
    actionModal = null;
    actionError = null;
  }

  async function confirmKill() {
    actionLoading = true;
    actionError = null;
    try {
      await api.adminKillAgent(actionModal.agent.id);
      closeModal();
      const ag = await api.agents();
      agents = ag;
    } catch (e) {
      actionError = e.message;
    } finally {
      actionLoading = false;
    }
  }

  async function confirmReassign() {
    if (!reassignTargetId) {
      actionError = 'Select a target agent.';
      return;
    }
    actionLoading = true;
    actionError = null;
    try {
      await api.adminReassignAgent(actionModal.agent.id, reassignTargetId);
      closeModal();
    } catch (e) {
      actionError = e.message;
    } finally {
      actionLoading = false;
    }
  }

  function formatTime(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  function formatUptime(secs) {
    if (secs == null) return '—';
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  }

  const sections = [
    { id: 'health', label: 'System Health' },
    { id: 'jobs', label: 'Background Jobs' },
    { id: 'audit', label: 'Audit Log' },
    { id: 'agents', label: 'Agent Management' },
  ];
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Admin Panel</h2>
    <button class="refresh-btn" onclick={loadAll} disabled={loading}>
      {loading ? 'Loading…' : 'Refresh'}
    </button>
  </div>

  {#if error}
    <p class="state-msg error">{error}</p>
  {:else}
    <div class="admin-layout">
      <nav class="admin-nav">
        {#each sections as sec}
          <button
            class="nav-item"
            class:active={activeSection === sec.id}
            onclick={() => (activeSection = sec.id)}
          >
            {sec.label}
          </button>
        {/each}
      </nav>

      <div class="admin-content">
        {#if activeSection === 'health'}
          <div class="section">
            <h3>System Health</h3>
            {#if health}
              <div class="health-grid">
                <div class="health-card">
                  <span class="card-label">Status</span>
                  <span class="card-value status-ok">{health.status}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Uptime</span>
                  <span class="card-value">{formatUptime(health.uptime_secs)}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Version</span>
                  <span class="card-value mono">{health.version}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Agents</span>
                  <span class="card-value">{health.agent_count}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Active Agents</span>
                  <span class="card-value">{health.active_agents}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Tasks</span>
                  <span class="card-value">{health.task_count}</span>
                </div>
                <div class="health-card">
                  <span class="card-label">Projects</span>
                  <span class="card-value">{health.project_count}</span>
                </div>
              </div>
            {:else if loading}
              <p class="state-msg">Loading…</p>
            {:else}
              <p class="state-msg muted">No health data available (requires Admin role).</p>
            {/if}
          </div>

        {:else if activeSection === 'jobs'}
          <div class="section">
            <h3>Background Jobs</h3>
            {#if jobs.length === 0}
              <p class="state-msg muted">No jobs data.</p>
            {:else}
              <table>
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Status</th>
                    <th>Interval</th>
                    <th>Description</th>
                  </tr>
                </thead>
                <tbody>
                  {#each jobs as job}
                    <tr>
                      <td class="name mono">{job.name}</td>
                      <td>
                        <span class="badge running">{job.status}</span>
                      </td>
                      <td class="dim">{job.interval_secs}s</td>
                      <td class="dim">{job.description}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>

        {:else if activeSection === 'audit'}
          <div class="section">
            <h3>Audit Log</h3>
            <div class="filters">
              <input
                bind:value={auditAgentFilter}
                placeholder="Filter by agent ID…"
                class="filter-input"
              />
              <input
                bind:value={auditTypeFilter}
                placeholder="Filter by event type…"
                class="filter-input"
              />
              <button class="filter-btn" onclick={loadAudit}>Apply</button>
            </div>
            {#if auditEvents.length === 0}
              <p class="state-msg muted">No events recorded.</p>
            {:else}
              <div class="table-scroll">
                <table>
                  <thead>
                    <tr>
                      <th>Time</th>
                      <th>Agent</th>
                      <th>Event</th>
                      <th>Description</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each auditEvents as evt}
                      <tr>
                        <td class="dim">{formatTime(evt.timestamp)}</td>
                        <td class="mono dim">{evt.agent_id}</td>
                        <td><span class="badge event">{evt.event_type}</span></td>
                        <td class="dim">{evt.description}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          </div>

        {:else if activeSection === 'agents'}
          <div class="section">
            <h3>Agent Management</h3>
            {#if agents.length === 0}
              <p class="state-msg muted">No agents.</p>
            {:else}
              <table>
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Status</th>
                    <th>Last Heartbeat</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {#each agents as agent}
                    <tr>
                      <td class="name">{agent.name}</td>
                      <td>
                        <span class="badge {agent.status}">{agent.status}</span>
                      </td>
                      <td class="dim">{formatTime(agent.last_heartbeat)}</td>
                      <td class="actions">
                        {#if agent.status !== 'dead'}
                          <button class="action-btn kill" onclick={() => openKill(agent)}>
                            Kill
                          </button>
                        {/if}
                        <button class="action-btn reassign" onclick={() => openReassign(agent)}>
                          Reassign
                        </button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

{#if actionModal}
  <div class="modal-backdrop" onclick={closeModal}>
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      {#if actionModal.type === 'kill'}
        <h3>Force Kill Agent</h3>
        <p class="modal-desc">
          Kill <strong>{actionModal.agent.name}</strong>? This will set the agent status to Dead,
          clean its worktrees, and block its active task.
        </p>
        {#if actionError}
          <p class="form-error">{actionError}</p>
        {/if}
        <div class="modal-actions">
          <button class="modal-btn secondary" onclick={closeModal}>Cancel</button>
          <button class="modal-btn danger" onclick={confirmKill} disabled={actionLoading}>
            {actionLoading ? 'Killing…' : 'Kill Agent'}
          </button>
        </div>
      {:else if actionModal.type === 'reassign'}
        <h3>Reassign Tasks</h3>
        <p class="modal-desc">
          Reassign all tasks from <strong>{actionModal.agent.name}</strong> to:
        </p>
        <select bind:value={reassignTargetId} class="target-select">
          <option value="">Select target agent…</option>
          {#each agents.filter((a) => a.id !== actionModal.agent.id) as a}
            <option value={a.id}>{a.name} ({a.status})</option>
          {/each}
        </select>
        {#if actionError}
          <p class="form-error">{actionError}</p>
        {/if}
        <div class="modal-actions">
          <button class="modal-btn secondary" onclick={closeModal}>Cancel</button>
          <button class="modal-btn primary" onclick={confirmReassign} disabled={actionLoading}>
            {actionLoading ? 'Reassigning…' : 'Reassign'}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h3 { margin: 0 0 1rem; font-size: 0.9rem; font-weight: 600; color: var(--text); }

  .refresh-btn {
    background: var(--surface-hover); color: var(--text-muted); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.3rem 0.75rem; font-size: 0.82rem; cursor: pointer;
  }
  .refresh-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .admin-layout { display: flex; flex: 1; overflow: hidden; }

  .admin-nav {
    width: 160px; min-width: 160px; border-right: 1px solid var(--border);
    display: flex; flex-direction: column; padding: 0.5rem; gap: 2px;
  }

  .nav-item {
    text-align: left; padding: 0.5rem 0.75rem; border: none; border-radius: 4px;
    background: transparent; color: var(--text-muted); font-size: 0.85rem; cursor: pointer;
  }
  .nav-item:hover { background: var(--surface-hover); color: var(--text); }
  .nav-item.active { background: var(--accent-muted); color: var(--accent); font-weight: 500; }

  .admin-content { flex: 1; overflow-y: auto; padding: 1.25rem; }

  .section { max-width: 900px; }

  .health-grid {
    display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 0.75rem;
  }

  .health-card {
    background: var(--surface); border: 1px solid var(--border); border-radius: 6px;
    padding: 0.85rem 1rem; display: flex; flex-direction: column; gap: 0.35rem;
  }

  .card-label { font-size: 0.75rem; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.04em; }
  .card-value { font-size: 1.1rem; font-weight: 600; color: var(--text); }
  .card-value.status-ok { color: #4ade80; }
  .card-value.mono { font-family: monospace; font-size: 0.9rem; }

  .filters { display: flex; gap: 0.5rem; margin-bottom: 1rem; flex-wrap: wrap; }

  .filter-input {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.35rem 0.65rem; font-size: 0.82rem; min-width: 200px;
  }

  .filter-btn {
    background: var(--accent-muted); color: var(--accent); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.35rem 0.75rem; font-size: 0.82rem; cursor: pointer;
  }

  .table-scroll { overflow-x: auto; }

  table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }

  th {
    text-align: left; padding: 0.4rem 0.6rem; color: var(--text-dim); font-weight: 500;
    font-size: 0.78rem; border-bottom: 1px solid var(--border);
    text-transform: uppercase; letter-spacing: 0.04em;
  }

  td { padding: 0.45rem 0.6rem; border-bottom: 1px solid var(--border-subtle); vertical-align: middle; }

  .name { color: var(--text); font-weight: 500; }
  .mono { font-family: monospace; font-size: 0.8rem; }
  .dim { color: var(--text-muted); font-size: 0.82rem; }

  .badge {
    display: inline-block; padding: 0.15rem 0.5rem; border-radius: 3px; font-size: 0.75rem;
    font-weight: 500; text-transform: lowercase; background: var(--surface-hover); color: var(--text-muted);
  }
  .badge.running { background: #14532d44; color: #4ade80; }
  .badge.active  { background: #14532d44; color: #4ade80; }
  .badge.idle    { background: #1e2536; color: var(--text-muted); }
  .badge.blocked { background: #7c2d1244; color: #f97316; }
  .badge.dead    { background: #3f161644; color: #f87171; }
  .badge.error   { background: #3f161644; color: #f87171; }
  .badge.event   { background: var(--accent-muted); color: var(--accent); }

  .actions { display: flex; gap: 0.4rem; }

  .action-btn {
    padding: 0.25rem 0.6rem; border-radius: 3px; font-size: 0.78rem; cursor: pointer;
    border: 1px solid var(--border);
  }
  .action-btn.kill { background: #3f161644; color: #f87171; border-color: #f8717144; }
  .action-btn.kill:hover { background: #3f1616; }
  .action-btn.reassign { background: var(--surface-hover); color: var(--text-muted); }
  .action-btn.reassign:hover { color: var(--text); }

  .modal-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,0.55); z-index: 100;
    display: flex; align-items: center; justify-content: center;
  }

  .modal {
    background: var(--surface); border: 1px solid var(--border); border-radius: 8px;
    padding: 1.5rem; min-width: 340px; max-width: 460px; width: 100%;
  }

  .modal-desc { font-size: 0.85rem; color: var(--text-muted); margin: 0.5rem 0 1rem; }

  .target-select {
    width: 100%; background: var(--bg); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.4rem 0.6rem; font-size: 0.85rem; margin-bottom: 0.75rem;
  }

  .modal-actions { display: flex; gap: 0.5rem; justify-content: flex-end; margin-top: 1rem; }

  .modal-btn {
    border: 1px solid var(--border); border-radius: 4px; padding: 0.35rem 0.9rem;
    font-size: 0.82rem; cursor: pointer; background: var(--surface); color: var(--text);
  }
  .modal-btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .modal-btn.primary:hover { opacity: 0.88; }
  .modal-btn.danger { background: #991b1b; color: #fff; border-color: #991b1b; }
  .modal-btn.danger:hover { opacity: 0.88; }
  .modal-btn.secondary:hover { background: var(--surface-hover); }
  .modal-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .form-error { color: #f87171; font-size: 0.82rem; margin: 0.5rem 0; }
  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
