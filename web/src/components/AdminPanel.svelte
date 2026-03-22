<script>
  import { api } from '../lib/api.js';
  import Tabs from '../lib/Tabs.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError, toastInfo } from '../lib/toast.svelte.js';

  let health = $state(null);
  let jobs = $state([]);
  let auditEvents = $state([]);
  let agents = $state([]);
  let snapshots = $state([]);
  let retention = $state([]);
  let siemTargets = $state([]);
  let computeTargets = $state([]);
  let networkPeers = $state([]);
  let derpMap = $state(null);
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

  // SIEM modal state
  let siemModal = $state(null); // null | { mode: 'create' | 'edit', target?: obj }
  let siemForm = $state({ url: '', format: 'json', enabled: true, filter: '' });
  let siemLoading = $state(false);

  // Compute modal state
  let computeModal = $state(false);
  let computeForm = $state({ name: '', target_type: 'local', host: '', port: '' });
  let computeLoading = $state(false);

  // Network peer modal state
  let peerModal = $state(false);
  let peerForm = $state({ agent_id: '', public_key: '', endpoint: '', allowed_ips: '' });
  let peerLoading = $state(false);

  // Agent spawn log drill-down
  let selectedAgentId = $state(null);
  let spawnLog = $state([]);
  let spawnLogLoading = $state(false);

  const TABS = [
    { id: 'health',    label: 'Health' },
    { id: 'jobs',      label: 'Jobs' },
    { id: 'audit',     label: 'Audit' },
    { id: 'agents',    label: 'Agents' },
    { id: 'snapshots', label: 'Snapshots' },
    { id: 'retention', label: 'Retention' },
    { id: 'siem',      label: 'SIEM' },
    { id: 'compute',   label: 'Compute' },
    { id: 'network',   label: 'Network' },
  ];

  $effect(() => {
    loadAll();
  });

  async function loadAll() {
    loading = true;
    error = null;
    try {
      const [h, j, a, ag, snaps, ret, siem, compute, peers, derp] = await Promise.all([
        api.adminHealth(),
        api.adminJobs(),
        api.adminAudit(),
        api.agents(),
        api.adminListSnapshots().catch(() => []),
        api.adminRetention().catch(() => []),
        api.siemList().catch(() => []),
        api.computeList().catch(() => []),
        api.networkPeers().catch(() => []),
        api.networkDerpMap().catch(() => null),
      ]);
      health = h;
      jobs = j;
      auditEvents = a.events ?? [];
      agents = ag;
      snapshots = snaps;
      retention = ret;
      siemTargets = Array.isArray(siem) ? siem : (siem?.targets ?? []);
      computeTargets = Array.isArray(compute) ? compute : (compute?.targets ?? []);
      networkPeers = Array.isArray(peers) ? peers : (peers?.peers ?? []);
      derpMap = derp;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // SIEM actions
  function openSiemCreate() {
    siemForm = { url: '', format: 'json', enabled: true, filter: '' };
    siemModal = { mode: 'create' };
  }

  function openSiemEdit(target) {
    siemForm = { url: target.url ?? '', format: target.format ?? 'json', enabled: target.enabled ?? true, filter: target.filter ?? '' };
    siemModal = { mode: 'edit', target };
  }

  function closeSiemModal() { siemModal = null; }

  async function saveSiem() {
    siemLoading = true;
    try {
      if (siemModal.mode === 'create') {
        await api.siemCreate(siemForm);
        toastSuccess('SIEM target created.');
      } else {
        await api.siemUpdate(siemModal.target.id, siemForm);
        toastSuccess('SIEM target updated.');
      }
      siemTargets = await api.siemList().then(r => Array.isArray(r) ? r : (r?.targets ?? []));
      closeSiemModal();
    } catch (e) {
      toastError(e.message);
    } finally {
      siemLoading = false;
    }
  }

  async function deleteSiem(id) {
    if (!confirm('Delete this SIEM target?')) return;
    try {
      await api.siemDelete(id);
      siemTargets = siemTargets.filter(t => t.id !== id);
      toastSuccess('SIEM target deleted.');
    } catch (e) {
      toastError(e.message);
    }
  }

  // Compute actions
  function openComputeCreate() {
    computeForm = { name: '', target_type: 'local', host: '', port: '' };
    computeModal = true;
  }

  function closeComputeModal() { computeModal = false; }

  async function saveCompute() {
    computeLoading = true;
    try {
      await api.computeCreate(computeForm);
      computeTargets = await api.computeList().then(r => Array.isArray(r) ? r : (r?.targets ?? []));
      toastSuccess('Compute target created.');
      closeComputeModal();
    } catch (e) {
      toastError(e.message);
    } finally {
      computeLoading = false;
    }
  }

  async function deleteCompute(id) {
    if (!confirm('Delete this compute target?')) return;
    try {
      await api.computeDelete(id);
      computeTargets = computeTargets.filter(t => t.id !== id);
      toastSuccess('Compute target deleted.');
    } catch (e) {
      toastError(e.message);
    }
  }

  // Network peer actions
  function openPeerCreate() {
    peerForm = { agent_id: '', public_key: '', endpoint: '', allowed_ips: '' };
    peerModal = true;
  }

  function closePeerModal() { peerModal = false; }

  async function savePeer() {
    peerLoading = true;
    try {
      await api.networkPeerCreate(peerForm);
      networkPeers = await api.networkPeers().then(r => Array.isArray(r) ? r : (r?.peers ?? []));
      toastSuccess('Peer registered.');
      closePeerModal();
    } catch (e) {
      toastError(e.message);
    } finally {
      peerLoading = false;
    }
  }

  async function deletePeer(id) {
    if (!confirm('Remove this peer?')) return;
    try {
      await api.networkPeerDelete(id);
      networkPeers = networkPeers.filter(p => p.id !== id);
      toastSuccess('Peer removed.');
    } catch (e) {
      toastError(e.message);
    }
  }

  // Spawn log drill-down
  async function showSpawnLog(agentId) {
    if (selectedAgentId === agentId) {
      selectedAgentId = null;
      spawnLog = [];
      return;
    }
    selectedAgentId = agentId;
    spawnLogLoading = true;
    spawnLog = [];
    try {
      const result = await api.agentSpawnLog(agentId);
      spawnLog = Array.isArray(result) ? result : (result?.steps ?? result?.log ?? []);
    } catch (e) {
      spawnLog = [];
    } finally {
      spawnLogLoading = false;
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
                    <button class="secondary-btn" onclick={() => showSpawnLog(agent.id)}>
                      {selectedAgentId === agent.id ? 'Hide Log' : 'Spawn Log'}
                    </button>
                  </div>
                </td>
              </tr>
              {#if selectedAgentId === agent.id}
                <tr class="spawn-log-row">
                  <td colspan="4">
                    <div class="spawn-log-panel">
                      <div class="spawn-log-header">Spawn Log — {agent.name}</div>
                      {#if spawnLogLoading}
                        <Skeleton height="60px" />
                      {:else if spawnLog.length === 0}
                        <p class="spawn-log-empty">No spawn log steps recorded.</p>
                      {:else}
                        <ol class="spawn-log-timeline">
                          {#each spawnLog as step, i}
                            <li class="spawn-log-step {step.status ?? ''}">
                              <span class="step-num">{i + 1}</span>
                              <span class="step-name">{step.step ?? step.name ?? step}</span>
                              {#if step.status}
                                <Badge value={step.status} />
                              {/if}
                              {#if step.timestamp || step.ts}
                                <span class="step-time dim">{formatTime(step.timestamp ?? step.ts)}</span>
                              {/if}
                              {#if step.message || step.detail}
                                <span class="step-detail dim">{step.message ?? step.detail}</span>
                              {/if}
                            </li>
                          {/each}
                        </ol>
                      {/if}
                    </div>
                  </td>
                </tr>
              {/if}
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

    <!-- SIEM TAB -->
    {:else if activeTab === 'siem'}
      <div class="section-actions">
        <p class="section-desc">Configure SIEM forwarding targets for security event streaming.</p>
        <button class="primary-btn" onclick={openSiemCreate}>+ Add Target</button>
      </div>
      {#if loading}
        <Skeleton height="150px" />
      {:else if siemTargets.length === 0}
        <EmptyState title="No SIEM targets" description="Add a SIEM target to forward security events." />
      {:else}
        <table class="data-table">
          <thead>
            <tr>
              <th>URL</th>
              <th>Format</th>
              <th>Filter</th>
              <th>Status</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each siemTargets as target}
              <tr>
                <td class="mono dim">{target.url}</td>
                <td class="dim">{target.format ?? 'json'}</td>
                <td class="dim">{target.filter || '—'}</td>
                <td>
                  <Badge value={target.enabled ? 'active' : 'idle'} />
                </td>
                <td>
                  <div class="action-row">
                    <button class="secondary-btn" onclick={() => openSiemEdit(target)}>Edit</button>
                    <button class="kill-btn" onclick={() => deleteSiem(target.id)}>Delete</button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    <!-- COMPUTE TAB -->
    {:else if activeTab === 'compute'}
      <div class="section-actions">
        <p class="section-desc">Register remote compute targets for agent workload dispatch.</p>
        <button class="primary-btn" onclick={openComputeCreate}>+ Add Target</button>
      </div>
      {#if loading}
        <Skeleton height="150px" />
      {:else if computeTargets.length === 0}
        <EmptyState title="No compute targets" description="Register local, Docker, or SSH compute targets." />
      {:else}
        <table class="data-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Type</th>
              <th>Host</th>
              <th>Status</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each computeTargets as ct}
              <tr>
                <td class="agent-name">{ct.name ?? ct.id}</td>
                <td><Badge value={ct.target_type ?? ct.type ?? 'local'} /></td>
                <td class="mono dim">{ct.host || '—'}</td>
                <td><Badge value={ct.status ?? 'active'} /></td>
                <td>
                  <button class="kill-btn" onclick={() => deleteCompute(ct.id)}>Delete</button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    <!-- NETWORK TAB -->
    {:else if activeTab === 'network'}
      <div class="section-actions">
        <p class="section-desc">WireGuard mesh peer registry and DERP relay map.</p>
        <button class="primary-btn" onclick={openPeerCreate}>+ Register Peer</button>
      </div>
      {#if loading}
        <Skeleton height="150px" />
      {:else}
        {#if networkPeers.length === 0}
          <EmptyState title="No peers" description="No WireGuard peers registered yet." />
        {:else}
          <table class="data-table">
            <thead>
              <tr>
                <th>Agent ID</th>
                <th>Public Key</th>
                <th>Endpoint</th>
                <th>Allowed IPs</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each networkPeers as peer}
                <tr>
                  <td class="mono dim">{peer.agent_id ?? peer.id}</td>
                  <td class="mono dim" title={peer.public_key}>{(peer.public_key ?? '').slice(0, 16)}{peer.public_key?.length > 16 ? '…' : ''}</td>
                  <td class="mono dim">{peer.endpoint || '—'}</td>
                  <td class="mono dim">{peer.allowed_ips || '—'}</td>
                  <td>
                    <button class="kill-btn" onclick={() => deletePeer(peer.id)}>Remove</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

        {#if derpMap}
          <div class="derp-section">
            <h4 class="derp-title">DERP Relay Map</h4>
            <div class="derp-card">
              <pre class="derp-json">{JSON.stringify(derpMap, null, 2)}</pre>
            </div>
          </div>
        {/if}
      {/if}
    {/if}
  </div>
</div>

<!-- Modal -->
{#if actionModal}
  <div class="modal-backdrop" aria-hidden="true" onclick={closeModal}></div>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label="Agent Action"
    onkeydown={(e) => {
      if (e.key === 'Escape') { closeModal(); return; }
      if (e.key === 'Enter' && e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'SELECT') {
        if (actionModal?.type === 'kill') confirmKill();
        else if (actionModal?.type === 'reassign') confirmReassign();
      }
    }}
  >
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
{/if}

<!-- SIEM Modal -->
{#if siemModal}
  <div class="modal-backdrop" aria-hidden="true" onclick={closeSiemModal}></div>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label="SIEM Target"
    onkeydown={(e) => { if (e.key === 'Escape') closeSiemModal(); }}
  >
    <h3 class="modal-title">{siemModal.mode === 'create' ? 'Add SIEM Target' : 'Edit SIEM Target'}</h3>
    <div class="form-field">
      <label class="form-label" for="siem-url">Webhook URL</label>
      <input id="siem-url" class="filter-input full-width" bind:value={siemForm.url} placeholder="https://siem.example.com/ingest" onkeydown={(e) => e.key === 'Enter' && saveSiem()} />
    </div>
    <div class="form-field">
      <label class="form-label" for="siem-format">Format</label>
      <select id="siem-format" class="target-select" bind:value={siemForm.format}>
        <option value="json">JSON</option>
        <option value="cef">CEF</option>
        <option value="leef">LEEF</option>
      </select>
    </div>
    <div class="form-field">
      <label class="form-label" for="siem-filter">Event Filter (optional)</label>
      <input id="siem-filter" class="filter-input full-width" bind:value={siemForm.filter} placeholder="e.g. agent.spawned,mr.merged" onkeydown={(e) => e.key === 'Enter' && saveSiem()} />
    </div>
    <div class="form-field inline-check">
      <input type="checkbox" id="siem-enabled" bind:checked={siemForm.enabled} />
      <label for="siem-enabled" class="form-label">Enabled</label>
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={closeSiemModal}>Cancel</button>
      <button class="primary-btn" onclick={saveSiem} disabled={siemLoading || !siemForm.url}>
        {siemLoading ? 'Saving…' : 'Save'}
      </button>
    </div>
  </div>
{/if}

<!-- Compute Modal -->
{#if computeModal}
  <div class="modal-backdrop" aria-hidden="true" onclick={closeComputeModal}></div>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label="Compute Target"
    onkeydown={(e) => { if (e.key === 'Escape') closeComputeModal(); }}
  >
    <h3 class="modal-title">Add Compute Target</h3>
    <div class="form-field">
      <label class="form-label" for="ct-name">Name</label>
      <input id="ct-name" class="filter-input full-width" bind:value={computeForm.name} placeholder="e.g. docker-host-1" onkeydown={(e) => e.key === 'Enter' && saveCompute()} />
    </div>
    <div class="form-field">
      <label class="form-label" for="ct-type">Type</label>
      <select id="ct-type" class="target-select" bind:value={computeForm.target_type}>
        <option value="local">Local</option>
        <option value="docker">Docker</option>
        <option value="ssh">SSH</option>
      </select>
    </div>
    {#if computeForm.target_type !== 'local'}
      <div class="form-field">
        <label class="form-label" for="ct-host">Host</label>
        <input id="ct-host" class="filter-input full-width" bind:value={computeForm.host} placeholder="host:port or hostname" onkeydown={(e) => e.key === 'Enter' && saveCompute()} />
      </div>
    {/if}
    <div class="modal-actions">
      <button class="secondary-btn" onclick={closeComputeModal}>Cancel</button>
      <button class="primary-btn" onclick={saveCompute} disabled={computeLoading || !computeForm.name}>
        {computeLoading ? 'Creating…' : 'Create'}
      </button>
    </div>
  </div>
{/if}

<!-- Network Peer Modal -->
{#if peerModal}
  <div class="modal-backdrop" aria-hidden="true" onclick={closePeerModal}></div>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label="Register Peer"
    onkeydown={(e) => { if (e.key === 'Escape') closePeerModal(); }}
  >
    <h3 class="modal-title">Register WireGuard Peer</h3>
    <div class="form-field">
      <label class="form-label" for="peer-agent-id">Agent ID</label>
      <input id="peer-agent-id" class="filter-input full-width" bind:value={peerForm.agent_id} placeholder="UUID of agent" />
    </div>
    <div class="form-field">
      <label class="form-label" for="peer-pubkey">WireGuard Public Key</label>
      <input id="peer-pubkey" class="filter-input full-width" bind:value={peerForm.public_key} placeholder="Base64 public key" />
    </div>
    <div class="form-field">
      <label class="form-label" for="peer-endpoint">Endpoint</label>
      <input id="peer-endpoint" class="filter-input full-width" bind:value={peerForm.endpoint} placeholder="1.2.3.4:51820" />
    </div>
    <div class="form-field">
      <label class="form-label" for="peer-ips">Allowed IPs</label>
      <input id="peer-ips" class="filter-input full-width" bind:value={peerForm.allowed_ips} placeholder="10.0.0.2/32" onkeydown={(e) => e.key === 'Enter' && savePeer()} />
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={closePeerModal}>Cancel</button>
      <button class="primary-btn" onclick={savePeer} disabled={peerLoading || !peerForm.public_key}>
        {peerLoading ? 'Registering…' : 'Register'}
      </button>
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
    padding: var(--space-4) var(--space-4);
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
    padding: var(--space-4) var(--space-4);
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
    position: fixed;
    z-index: 101;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    min-width: 360px;
    max-width: 480px;
    width: 100%;
    max-height: 90vh;
    overflow-y: auto;
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

  /* Spawn log drill-down */
  .spawn-log-row td { padding: 0 var(--space-4) var(--space-3); background: var(--color-bg); }

  .spawn-log-panel {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    background: var(--color-surface);
  }

  .spawn-log-header {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: var(--space-3);
  }

  .spawn-log-empty {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .spawn-log-timeline {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .spawn-log-step {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .step-num {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .step-name { font-weight: 500; color: var(--color-text); }
  .step-time { margin-left: auto; }
  .step-detail { font-size: var(--text-xs); color: var(--color-text-muted); }

  /* Form fields in modals */
  .form-field { display: flex; flex-direction: column; gap: var(--space-1); }
  .form-label { font-size: var(--text-xs); color: var(--color-text-muted); font-weight: 500; }
  .full-width { width: 100%; box-sizing: border-box; }
  .inline-check { flex-direction: row; align-items: center; gap: var(--space-2); }

  /* DERP map section */
  .derp-section { margin-top: var(--space-6); }
  .derp-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0 0 var(--space-3);
  }
  .derp-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    overflow-x: auto;
  }
  .derp-json {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
