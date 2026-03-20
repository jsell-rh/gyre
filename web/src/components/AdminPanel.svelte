<script>
  import { api } from '../lib/api.js';
  import Tabs from '../lib/Tabs.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError, toastInfo } from '../lib/toast.js';

  let health = $state(null);
  let jobs = $state([]);
  let auditEvents = $state([]);
  let agents = $state([]);
  let snapshots = $state([]);
  let retention = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let activeTab = $state('health');

  // Audit filter state
  let auditAgentFilter = $state('');
  let auditTypeFilter = $state('');

  // Kill/reassign modal state
  let actionModal = $state(null);
  let reassignTargetId = $state('');
  let actionError = $state(null);
  let actionLoading = $state(false);

  // Snapshot/export state
  let snapshotLoading = $state(false);
  let snapshotError = $state(null);
  let exportLoading = $state(false);

  // Job trigger state
  let triggerLoading = $state({});

  const TABS = [
    { id: 'health',    label: 'Health' },
    { id: 'jobs',      label: 'Jobs' },
    { id: 'audit',     label: 'Audit' },
    { id: 'agents',    label: 'Agents' },
    { id: 'snapshots', label: 'Snapshots' },
    { id: 'retention', label: 'Retention' },
  ];

  $effect(() => {
    loadAll();
  });

  async function loadAll() {
    loading = true;
    error = null;
    try {
      const [h, j, a, ag, snaps, ret] = await Promise.all([
        api.adminHealth(),
        api.adminJobs(),
        api.adminAudit(),
        api.agents(),
        api.adminListSnapshots().catch(() => []),
        api.adminRetention().catch(() => []),
      ]);
      health = h;
      jobs = j;
      auditEvents = a.events ?? [];
      agents = ag;
      snapshots = snaps;
      retention = ret;
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
      toastError(e.message);
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
      toastSuccess(`Agent ${actionModal.agent.name} killed.`);
      closeModal();
      agents = await api.agents();
    } catch (e) {
      actionError = e.message;
    } finally {
      actionLoading = false;
    }
  }

  async function confirmReassign() {
    if (!reassignTargetId) { actionError = 'Select a target agent.'; return; }
    actionLoading = true;
    actionError = null;
    try {
      await api.adminReassignAgent(actionModal.agent.id, reassignTargetId);
      toastSuccess('Tasks reassigned.');
      closeModal();
    } catch (e) {
      actionError = e.message;
    } finally {
      actionLoading = false;
    }
  }

  async function createSnapshot() {
    snapshotLoading = true;
    snapshotError = null;
    try {
      await api.adminCreateSnapshot();
      snapshots = await api.adminListSnapshots();
      toastSuccess('Snapshot created.');
    } catch (e) {
      snapshotError = e.message;
      toastError(e.message);
    } finally {
      snapshotLoading = false;
    }
  }

  async function deleteSnapshot(id) {
    snapshotLoading = true;
    snapshotError = null;
    try {
      await api.adminDeleteSnapshot(id);
      snapshots = await api.adminListSnapshots();
      toastSuccess('Snapshot deleted.');
    } catch (e) {
      snapshotError = e.message;
      toastError(e.message);
    } finally {
      snapshotLoading = false;
    }
  }

  async function restoreSnapshot(id) {
    if (!confirm(`Restore snapshot ${id}? The server will need a restart for full effect.`)) return;
    snapshotLoading = true;
    snapshotError = null;
    try {
      const result = await api.adminRestoreSnapshot(id);
      toastInfo(result.warning ?? 'Snapshot restored.');
    } catch (e) {
      snapshotError = e.message;
      toastError(e.message);
    } finally {
      snapshotLoading = false;
    }
  }

  async function downloadExport() {
    exportLoading = true;
    try {
      const data = await api.adminExport();
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `gyre-export-${Date.now()}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toastSuccess('Export downloaded.');
    } catch (e) {
      toastError(e.message);
    } finally {
      exportLoading = false;
    }
  }

  async function triggerJob(name) {
    triggerLoading = { ...triggerLoading, [name]: true };
    try {
      await api.adminRunJob(name);
      jobs = await api.adminJobs();
      toastSuccess(`Job "${name}" triggered.`);
    } catch (e) {
      toastError(e.message);
    } finally {
      triggerLoading = { ...triggerLoading, [name]: false };
    }
  }

  async function saveRetention() {
    try {
      await api.adminUpdateRetention(retention);
      toastSuccess('Retention policies saved.');
    } catch (e) {
      toastError(e.message);
    }
  }

  function formatTime(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit'
    });
  }

  function relativeTime(ts) {
    if (!ts) return '—';
    const diff = Math.floor((Date.now() - ts * 1000) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
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

  function formatBytes(bytes) {
    if (bytes == null) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<div class="panel">
  <div class="panel-header">
    <div class="header-left">
      <h2>Admin Panel</h2>
    </div>
    <button class="refresh-btn" onclick={loadAll} disabled={loading}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
        <path d="M23 4v6h-6M1 20v-6h6"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
      </svg>
      {loading ? 'Loading…' : 'Refresh'}
    </button>
  </div>

  <Tabs tabs={TABS} bind:active={activeTab} />

  <div class="admin-content">
    {#if error}
      <div class="error-banner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>
        {error}
      </div>
    {/if}

    <!-- HEALTH TAB -->
    {#if activeTab === 'health'}
      {#if loading}
        <div class="skeleton-grid">
          {#each Array(6) as _}
            <Skeleton height="80px" />
          {/each}
        </div>
      {:else if !health}
        <EmptyState
          title="No health data"
          description="Health data requires Admin role."
        />
      {:else}
        <div class="metric-grid">
          <div class="metric-card">
            <span class="metric-label">Status</span>
            <span class="metric-value success">{health.status}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Uptime</span>
            <span class="metric-value">{formatUptime(health.uptime_secs)}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Version</span>
            <span class="metric-value mono">{health.version}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Agents</span>
            <span class="metric-value">{health.agent_count ?? '—'}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Active Agents</span>
            <span class="metric-value success">{health.active_agents ?? '—'}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Tasks</span>
            <span class="metric-value">{health.task_count ?? '—'}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Projects</span>
            <span class="metric-value">{health.project_count ?? '—'}</span>
          </div>
        </div>
      {/if}

    <!-- JOBS TAB -->
    {:else if activeTab === 'jobs'}
      {#if loading}
        <Skeleton height="200px" />
      {:else if jobs.length === 0}
        <EmptyState title="No background jobs" description="Scheduled jobs will appear here." />
      {:else}
        <table class="data-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Status</th>
              <th>Interval</th>
              <th>Description</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each jobs as job}
              <tr>
                <td class="mono">{job.name}</td>
                <td>
                  <Badge value={job.status === 'success' ? 'done' : job.status === 'failed' ? 'error' : 'idle'} />
                </td>
                <td class="dim">{job.interval_secs}s</td>
                <td class="dim">{job.description}</td>
                <td>
                  <button
                    class="run-btn"
                    onclick={() => triggerJob(job.name)}
                    disabled={triggerLoading[job.name]}
                  >
                    {triggerLoading[job.name] ? 'Running…' : 'Run Now'}
                  </button>
                </td>
              </tr>
              {#if job.recent_runs && job.recent_runs.length > 0}
                <tr class="history-row">
                  <td colspan="5">
                    <div class="run-history">
                      <span class="history-label">Recent runs:</span>
                      {#each job.recent_runs.slice(-5).reverse() as run}
                        <span class="run-pill {run.status}">
                          {formatTime(run.started_at)} — {run.status}
                          {#if run.error}<span title={run.error}> ⚠</span>{/if}
                        </span>
                      {/each}
                    </div>
                  </td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>
      {/if}

    <!-- AUDIT TAB -->
    {:else if activeTab === 'audit'}
      <div class="filter-bar">
        <input
          class="filter-input"
          bind:value={auditAgentFilter}
          placeholder="Filter by agent ID…"
        />
        <input
          class="filter-input"
          bind:value={auditTypeFilter}
          placeholder="Filter by event type…"
        />
        <button class="filter-btn" onclick={loadAudit}>Apply</button>
      </div>

      {#if loading}
        <Skeleton height="200px" />
      {:else if auditEvents.length === 0}
        <EmptyState title="No audit events" description="Audit events will appear here." />
      {:else}
        <div class="table-scroll">
          <table class="data-table">
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
                  <td class="dim">{relativeTime(evt.timestamp)}</td>
                  <td class="mono dim">{evt.agent_id}</td>
                  <td><Badge value="info" /></td>
                  <td class="dim">{evt.description}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}

    <!-- AGENTS TAB -->
    {:else if activeTab === 'agents'}
      {#if loading}
        <Skeleton height="200px" />
      {:else if agents.length === 0}
        <EmptyState title="No agents" description="Registered agents will appear here." />
      {:else}
        <table class="data-table">
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
                <td class="agent-name">{agent.name}</td>
                <td><Badge value={agent.status} /></td>
                <td class="dim">{relativeTime(agent.last_heartbeat)}</td>
                <td>
                  <div class="action-row">
                    {#if agent.status !== 'dead'}
                      <button class="kill-btn" onclick={() => openKill(agent)}>Kill</button>
                    {/if}
                    <button class="secondary-btn" onclick={() => openReassign(agent)}>Reassign</button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    <!-- SNAPSHOTS TAB -->
    {:else if activeTab === 'snapshots'}
      <div class="section-actions">
        <button class="primary-btn" onclick={createSnapshot} disabled={snapshotLoading}>
          {snapshotLoading ? 'Working…' : '+ Create Snapshot'}
        </button>
        <button class="secondary-btn" onclick={downloadExport} disabled={exportLoading}>
          {exportLoading ? 'Exporting…' : '⬇ Export All Data'}
        </button>
      </div>

      {#if snapshotError}
        <div class="form-error">{snapshotError}</div>
      {/if}

      {#if snapshots.length === 0}
        <EmptyState title="No snapshots" description="Create a snapshot to preserve current state." />
      {:else}
        <table class="data-table">
          <thead>
            <tr>
              <th>Snapshot ID</th>
              <th>Created</th>
              <th>Size</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each snapshots as snap}
              <tr>
                <td class="mono dim">{snap.snapshot_id}</td>
                <td class="dim">{formatTime(snap.created_at)}</td>
                <td class="dim">{formatBytes(snap.size_bytes)}</td>
                <td>
                  <div class="action-row">
                    <button class="secondary-btn" onclick={() => restoreSnapshot(snap.snapshot_id)} disabled={snapshotLoading}>
                      Restore
                    </button>
                    <button class="kill-btn" onclick={() => deleteSnapshot(snap.snapshot_id)} disabled={snapshotLoading}>
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    <!-- RETENTION TAB -->
    {:else if activeTab === 'retention'}
      <div class="section-actions">
        <p class="section-desc">Configure how long data is retained before automatic cleanup.</p>
        <button class="primary-btn" onclick={saveRetention}>Save Policies</button>
      </div>

      {#if retention.length === 0}
        <EmptyState title="No policies loaded" description="Retention policies will appear here." />
      {:else}
        <table class="data-table">
          <thead>
            <tr>
              <th>Data Type</th>
              <th>Max Age (days)</th>
            </tr>
          </thead>
          <tbody>
            {#each retention as policy, i}
              <tr>
                <td class="mono">{policy.data_type}</td>
                <td>
                  <input
                    type="number"
                    class="age-input"
                    bind:value={retention[i].max_age_days}
                    min="1"
                    max="3650"
                  />
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </div>
</div>

<!-- Modal -->
{#if actionModal}
  <div class="modal-backdrop" onclick={closeModal}>
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      {#if actionModal.type === 'kill'}
        <h3 class="modal-title">Force Kill Agent</h3>
        <p class="modal-desc">
          Kill <strong>{actionModal.agent.name}</strong>? This will set the agent status to Dead,
          clean its worktrees, and block its active task.
        </p>
        {#if actionError}
          <div class="form-error">{actionError}</div>
        {/if}
        <div class="modal-actions">
          <button class="secondary-btn" onclick={closeModal}>Cancel</button>
          <button class="kill-btn modal-kill" onclick={confirmKill} disabled={actionLoading}>
            {actionLoading ? 'Killing…' : 'Kill Agent'}
          </button>
        </div>
      {:else if actionModal.type === 'reassign'}
        <h3 class="modal-title">Reassign Tasks</h3>
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
          <div class="form-error">{actionError}</div>
        {/if}
        <div class="modal-actions">
          <button class="secondary-btn" onclick={closeModal}>Cancel</button>
          <button class="primary-btn" onclick={confirmReassign} disabled={actionLoading}>
            {actionLoading ? 'Reassigning…' : 'Reassign'}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .header-left { display: flex; align-items: center; gap: var(--space-3); }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }
  .refresh-btn:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .refresh-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .admin-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 1000px;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
    background: rgba(240, 86, 29, 0.1);
    border: 1px solid rgba(240, 86, 29, 0.3);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  /* Health metrics */
  .metric-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-4);
  }

  .metric-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--transition-fast);
  }
  .metric-card:hover { border-color: var(--color-border-strong); }

  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .metric-value {
    font-size: var(--text-xl);
    font-weight: 700;
    font-family: var(--font-display);
    color: var(--color-text);
  }

  .metric-value.success { color: var(--color-success); }
  .metric-value.mono { font-family: var(--font-mono); font-size: var(--text-base); }

  .skeleton-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-4);
  }

  /* Data tables */
  .data-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .data-table thead th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .data-table tbody tr {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }
  .data-table tbody tr:last-child { border-bottom: none; }
  .data-table tbody tr:hover { background: var(--color-surface-elevated); }

  .data-table td {
    padding: var(--space-3) var(--space-4);
    vertical-align: middle;
    color: var(--color-text);
  }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .dim { color: var(--color-text-muted); font-size: var(--text-xs); }
  .agent-name { font-weight: 500; }

  .table-scroll { overflow-x: auto; }

  /* Filter bar */
  .filter-bar {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
    align-items: center;
  }

  .filter-input {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    min-width: 200px;
    transition: border-color var(--transition-fast);
  }
  .filter-input:focus {
    outline: none;
    border-color: var(--color-link);
  }

  .filter-btn {
    background: rgba(0, 102, 204, 0.1);
    border: 1px solid rgba(0, 102, 204, 0.3);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    transition: background var(--transition-fast);
  }
  .filter-btn:hover { background: rgba(0, 102, 204, 0.2); }

  /* Section actions */
  .section-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .section-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    flex: 1;
  }

  /* Buttons */
  .primary-btn {
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-weight: 500;
    transition: opacity var(--transition-fast);
    white-space: nowrap;
  }
  .primary-btn:hover { opacity: 0.88; }
  .primary-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .secondary-btn {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), color var(--transition-fast);
    white-space: nowrap;
  }
  .secondary-btn:hover { border-color: var(--color-border-strong); color: var(--color-text); }
  .secondary-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .kill-btn {
    background: rgba(240, 86, 29, 0.1);
    border: 1px solid rgba(240, 86, 29, 0.3);
    border-radius: var(--radius);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: background var(--transition-fast);
    white-space: nowrap;
  }
  .kill-btn:hover:not(:disabled) { background: rgba(240, 86, 29, 0.2); }
  .kill-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .run-btn {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }
  .run-btn:hover:not(:disabled) { border-color: var(--color-border-strong); color: var(--color-text); }
  .run-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .action-row { display: flex; gap: var(--space-2); }

  /* Run history */
  .history-row td {
    padding: 0 var(--space-4) var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .run-history {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .history-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .run-pill {
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
  }
  .run-pill.success { background: rgba(99,153,61,0.15); color: #7dc25a; }
  .run-pill.failed  { background: rgba(240,86,29,0.15); color: var(--color-danger); }
  .run-pill.running { background: rgba(0,102,204,0.15); color: var(--color-link); }

  /* Age input */
  .age-input {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    width: 80px;
  }

  /* Form error */
  .form-error {
    color: var(--color-danger);
    font-size: var(--text-sm);
    background: rgba(240, 86, 29, 0.1);
    border: 1px solid rgba(240, 86, 29, 0.2);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }

  /* Modal */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .modal {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    min-width: 360px;
    max-width: 480px;
    width: 100%;
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .modal-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .modal-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.6;
  }

  .modal-actions {
    display: flex;
    gap: var(--space-3);
    justify-content: flex-end;
  }

  .modal-kill { padding: var(--space-2) var(--space-6); }

  .target-select {
    width: 100%;
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }
</style>
