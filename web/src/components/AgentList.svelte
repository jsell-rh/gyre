<script>
  import { getContext, tick } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Table from '../lib/Table.svelte';
  import AgentCardPanel from './AgentCardPanel.svelte';

  const navigate = getContext('navigate');

  let { workspaceId = '' } = $props();

  let agents = $state([]);
  let repos = $state([]);
  let tasks = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let statusFilter = $state('');
  let viewMode = $state('grid');
  let selected = $state(null);
  let showSpawnModal = $state(false);
  let spawnResult = $state(null);
  let spawnError = $state(null);
  let spawnLoading = $state(false);

  let spawnName = $state('');
  let spawnRepoId = $state('');
  let spawnTaskId = $state('');
  let spawnBranch = $state('');
  let spawnComputeTarget = $state('');
  let computeTargets = $state([]);
  let repoBranches = $state([]);

  let detailTab = $state('info');
  let agentLogLines = $state([]);
  let logsLoading = $state(false);
  let ttyLines = $state([]);
  let ttyWs = $state(null);
  let ttyConnecting = $state(false);
  let containerRecord = $state(null);
  let spawnModalEl = $state(null);
  let spawnTriggerEl = $state(null);

  $effect(() => {
    if (showSpawnModal) {
      tick().then(() => {
        if (spawnModalEl) {
          const first = spawnModalEl.querySelector('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
          (first ?? spawnModalEl).focus();
        }
      });
    }
  });

  const statuses = ['Active', 'Idle', 'Blocked', 'Error', 'Dead'];

  const filtered = $derived(
    statusFilter ? agents.filter((a) => (a.status ?? '').toLowerCase() === statusFilter.toLowerCase()) : agents
  );
  function taskLabel(id) {
    if (!id) return '—';
    const t = tasks.find(tk => tk.id === id);
    return t ? t.title : id.substring(0, 8) + '…';
  }

  function relativeTime(ts) {
    if (!ts) return '—';
    const diff = Date.now() - ts * 1000;
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s ago`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
  }

  function formatTime(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  function uptimeStr(ts) {
    if (!ts) return '—';
    const secs = Math.floor(Date.now() / 1000 - ts);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
    return `${Math.floor(secs / 86400)}d`;
  }

  $effect(() => {
    const wsId = workspaceId;
    loading = true;
    api.agents({ workspaceId: wsId })
      .then((data) => { agents = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
    api.allRepos().then((data) => { repos = data; }).catch(() => {});
    api.tasks({ workspaceId: wsId }).then((data) => { tasks = data; }).catch(() => {});
    api.computeList().then((data) => {
      computeTargets = Array.isArray(data) ? data : [];
      // M25: pre-select the default Claude Code runner target if it exists
      const defaultTarget = computeTargets.find(ct => ct.name === 'gyre-agent-default');
      if (defaultTarget && !spawnComputeTarget) spawnComputeTarget = defaultTarget.id;
    }).catch(() => {});
  });

  $effect(() => {
    if (spawnRepoId) {
      api.repoBranches(spawnRepoId).then((data) => {
        repoBranches = Array.isArray(data) ? data.map(b => b.name ?? b) : [];
      }).catch(() => { repoBranches = []; });
    } else {
      repoBranches = [];
    }
  });

  function closeTtyWs() {
    if (ttyWs) { ttyWs.close(); ttyWs = null; }
    ttyLines = [];
    ttyConnecting = false;
  }

  function selectAgent(a) {
    if (selected?.id === a.id) { selected = null; closeTtyWs(); containerRecord = null; return; }
    closeTtyWs();
    selected = a;
    detailTab = 'info';
    agentLogLines = [];
    containerRecord = null;
    api.agentContainer(a.id).then((r) => { containerRecord = r; }).catch(() => { containerRecord = null; });
  }

  function switchDetailTab(tab) {
    detailTab = tab;
    if (tab === 'logs' && selected) {
      logsLoading = true;
      api.agentLogs(selected.id, 200, 0)
        .then((lines) => { agentLogLines = lines; })
        .catch(() => { agentLogLines = []; })
        .finally(() => { logsLoading = false; });
    }
    if (tab === 'terminal' && selected) {
      closeTtyWs();
      ttyConnecting = true;
      const token = localStorage.getItem('gyre_auth_token') || 'test-token';
      const ws = new WebSocket(api.agentTtyUrl(selected.id));
      ttyWs = ws;
      ws.onopen = () => { ws.send(JSON.stringify({ type: 'Auth', token })); };
      ws.onmessage = (ev) => {
        ttyConnecting = false;
        try { const m = JSON.parse(ev.data); if (m.type === 'AuthResult') return; } catch (_) {}
        ttyLines = [...ttyLines, ev.data];
      };
      ws.onclose = () => { ttyConnecting = false; };
      ws.onerror = () => { ttyConnecting = false; };
    } else if (tab !== 'terminal') {
      closeTtyWs();
    }
  }

  function openSpawnModal() {
    spawnTriggerEl = document.activeElement;
    spawnName = ''; spawnRepoId = ''; spawnTaskId = ''; spawnBranch = ''; spawnComputeTarget = '';
    spawnResult = null; spawnError = null;
    showSpawnModal = true;
  }

  function closeSpawnModal() {
    showSpawnModal = false;
    spawnTriggerEl?.focus();
  }

  async function doSpawn() {
    if (!spawnName || !spawnRepoId || !spawnTaskId || !spawnBranch) {
      spawnError = 'All fields are required.';
      return;
    }
    spawnLoading = true; spawnError = null; spawnResult = null;
    try {
      const spawnBody = { name: spawnName, repo_id: spawnRepoId, task_id: spawnTaskId, branch: spawnBranch };
      if (spawnComputeTarget) spawnBody.compute_target_id = spawnComputeTarget;
      spawnResult = await api.spawnAgent(spawnBody);
      agents = await api.agents({ workspaceId });
    } catch (e) {
      spawnError = e.message;
    } finally {
      spawnLoading = false;
    }
  }

  const tableColumns = [
    { key: 'name', label: 'Name', sortable: true },
    { key: 'status', label: 'Status' },
    { key: 'current_task_id', label: 'Task' },
    { key: 'last_heartbeat', label: 'Heartbeat' },
    { key: 'spawned_at', label: 'Uptime' },
  ];
</script>

<div class="page">
  <div class="page-hdr">
    <div>
      <h1 class="page-title">Agents</h1>
      <p class="page-desc">{agents.length} agent{agents.length !== 1 ? 's' : ''} registered</p>
    </div>
    <div class="page-actions">
      <div class="view-toggle">
        <button class="toggle-btn" class:active={viewMode === 'grid'} onclick={() => (viewMode = 'grid')} title="Grid view" aria-label="Grid view" aria-pressed={viewMode === 'grid'}>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <rect x="1" y="1" width="6" height="6" rx="1"/>
            <rect x="9" y="1" width="6" height="6" rx="1"/>
            <rect x="1" y="9" width="6" height="6" rx="1"/>
            <rect x="9" y="9" width="6" height="6" rx="1"/>
          </svg>
        </button>
        <button class="toggle-btn" class:active={viewMode === 'table'} onclick={() => (viewMode = 'table')} title="Table view" aria-label="Table view" aria-pressed={viewMode === 'table'}>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <rect x="1" y="2" width="14" height="2" rx="1"/>
            <rect x="1" y="7" width="14" height="2" rx="1"/>
            <rect x="1" y="12" width="14" height="2" rx="1"/>
          </svg>
        </button>
      </div>
      <button class="spawn-btn" onclick={openSpawnModal}>+ Spawn Agent</button>
    </div>
  </div>

  <div class="filter-bar">
    <button class="pill" class:active={statusFilter === ''} aria-pressed={statusFilter === ''} onclick={() => (statusFilter = '')}>All</button>
    {#each statuses as s}
      <button
        class="pill"
        class:active={statusFilter === s}
        aria-pressed={statusFilter === s}
        onclick={() => (statusFilter = statusFilter === s ? '' : s)}
      >
        {s}
      </button>
    {/each}
  </div>

  {#if showSpawnModal}
    <div class="modal-backdrop" aria-hidden="true" onclick={closeSpawnModal}></div>
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label="Spawn Agent"
      bind:this={spawnModalEl}
      onkeydown={(e) => {
        if (e.key === 'Escape') { closeSpawnModal(); return; }
        if (e.key === 'Enter' && !spawnResult && e.target.tagName !== 'SELECT') doSpawn();
      }}
    >
        <h3>Spawn Agent</h3>
        {#if spawnResult}
          <div class="spawn-success">
            <p class="success-msg">Agent spawned successfully.</p>
            <dl>
              <dt>Agent ID</dt><dd>{spawnResult.agent.id}</dd>
              <dt>Token</dt><dd class="mono">{spawnResult.token}</dd>
              <dt>Clone URL</dt><dd class="mono">{spawnResult.clone_url}</dd>
              <dt>Worktree</dt><dd>{spawnResult.worktree_path}</dd>
              <dt>Branch</dt><dd>{spawnResult.branch}</dd>
            </dl>
            <button class="modal-btn" onclick={closeSpawnModal}>Close</button>
          </div>
        {:else}
          <div class="form">
            <label>Name<input bind:value={spawnName} placeholder="worker-1" aria-required="true" /></label>
            <label>Repository
              <select bind:value={spawnRepoId} aria-required="true">
                <option value="">Select repo...</option>
                {#each repos as r}<option value={r.id}>{r.name}</option>{/each}
              </select>
            </label>
            <label>Task
              <select bind:value={spawnTaskId} aria-required="true">
                <option value="">Select task...</option>
                {#each tasks as t}<option value={t.id}>{t.title}</option>{/each}
              </select>
            </label>
            <label>Branch
              <input bind:value={spawnBranch} list="branch-suggestions" placeholder="feat/my-feature (new branch name)" aria-required="true" />
              <datalist id="branch-suggestions">
                {#each repoBranches as b}<option value={b}></option>{/each}
              </datalist>
              <span class="field-hint">Enter a new branch name. Existing branches shown as suggestions.</span>
            </label>
            {#if computeTargets.length > 0}
            <label>Compute Target
              <select bind:value={spawnComputeTarget}>
                <option value="">Default (local)</option>
                {#each computeTargets as ct}<option value={ct.id}>{ct.name} ({ct.target_type})</option>{/each}
              </select>
            </label>
            {/if}
            {#if spawnError}<p class="form-error" role="alert">{spawnError}</p>{/if}
            <div class="form-actions">
              <button class="modal-btn secondary" onclick={closeSpawnModal}>Cancel</button>
              <button class="modal-btn primary" onclick={doSpawn} disabled={spawnLoading} aria-busy={spawnLoading}>
                {spawnLoading ? 'Spawning...' : 'Spawn'}
              </button>
            </div>
          </div>
        {/if}
      </div>
  {/if}

  <div class="content">
    {#if loading}
      {#if viewMode === 'grid'}
        <div class="agent-grid">
          {#each Array(6) as _}
            <div class="agent-card skeleton-card">
              <div class="card-top">
                <Skeleton width="60%" height="1.1rem" />
                <Skeleton width="60px" height="1.2rem" />
              </div>
              <Skeleton lines={3} height="0.875rem" />
            </div>
          {/each}
        </div>
      {:else}
        <Skeleton lines={8} height="2.5rem" />
      {/if}
    {:else if error}
      <div class="error-msg" role="alert">Error: {error}</div>
    {:else if filtered.length === 0}
      <EmptyState
        title="No agents found"
        description={statusFilter
          ? `No agents with status "${statusFilter}". Try a different filter.`
          : 'No agents have been spawned yet.'}
      />
    {:else if viewMode === 'grid'}
      <div class="agent-grid">
        {#each filtered as a}
          <button class="agent-card" class:selected={selected?.id === a.id} onclick={() => selectAgent(a)}>
            <div class="card-top">
              <span class="agent-name" title={a.name}>{a.name}</span>
              <Badge value={a.status} />
            </div>
            <div class="card-fields">
              <div class="field">
                <span class="field-label">Task</span>
                <span class="field-value mono" title={a.current_task_id ?? ''}>{taskLabel(a.current_task_id)}</span>
              </div>
              <div class="field">
                <span class="field-label">Uptime</span>
                <span class="field-value">{uptimeStr(a.spawned_at)}</span>
              </div>
              <div class="field">
                <span class="field-label">Heartbeat</span>
                <span class="field-value">{relativeTime(a.last_heartbeat)}</span>
              </div>
            </div>
          </button>
        {/each}
      </div>
    {:else}
      <Table columns={tableColumns}>
        {#snippet children()}
          {#each filtered as a}
            <tr class:row-selected={selected?.id === a.id} onclick={() => selectAgent(a)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectAgent(a); } }} tabindex="0" style="cursor:pointer">
              <td class="name-cell">{a.name}</td>
              <td><Badge value={a.status} /></td>
              <td class="muted" title={a.current_task_id ?? ''}>{taskLabel(a.current_task_id)}</td>
              <td class="muted">{relativeTime(a.last_heartbeat)}</td>
              <td class="muted">{uptimeStr(a.spawned_at)}</td>
            </tr>
          {/each}
        {/snippet}
      </Table>
    {/if}

    {#if selected}
      <div class="detail-panel">
        <div class="detail-header">
          <h3>Agent: {selected.name}</h3>
          <button class="close-btn" aria-label="Close agent detail" onclick={() => { selected = null; closeTtyWs(); }}>✕</button>
        </div>
        <div class="detail-tabs" role="tablist">
          <button class="dtab" class:active={detailTab === 'info'} onclick={() => switchDetailTab('info')} role="tab" aria-selected={detailTab === 'info'} id="dtab-info" aria-controls="dtabpanel-info">Info</button>
          <button class="dtab" class:active={detailTab === 'logs'} onclick={() => switchDetailTab('logs')} role="tab" aria-selected={detailTab === 'logs'} id="dtab-logs" aria-controls="dtabpanel-logs">Logs</button>
          <button class="dtab" class:active={detailTab === 'terminal'} onclick={() => switchDetailTab('terminal')} role="tab" aria-selected={detailTab === 'terminal'} id="dtab-terminal" aria-controls="dtabpanel-terminal">Terminal</button>
        </div>
        {#if detailTab === 'info'}
          <div class="detail-body" role="tabpanel" id="dtabpanel-info" aria-labelledby="dtab-info">
            <dl class="detail-dl">
              <dt>ID</dt><dd class="mono">{selected.id}</dd>
              <dt>Status</dt><dd><Badge value={selected.status} /></dd>
              <dt>Parent</dt><dd class="mono">{selected.parent_id ?? '—'}</dd>
              <dt>Current Task</dt><dd title={selected.current_task_id ?? ''}>
                {#if selected.current_task_id && navigate}
                  <button class="link-btn" onclick={() => navigate('task-detail', { task: { id: selected.current_task_id } })}>
                    {taskLabel(selected.current_task_id)}
                  </button>
                {:else}
                  {taskLabel(selected.current_task_id)}
                {/if}
              </dd>
              <dt>Budget (s)</dt><dd>{selected.lifetime_budget_secs ?? '—'}</dd>
              <dt>Spawned</dt><dd>{formatTime(selected.spawned_at)}</dd>
              <dt>Last Heartbeat</dt><dd>{formatTime(selected.last_heartbeat)}</dd>
            </dl>
            {#if containerRecord}
              <div class="container-section">
                <h4 class="section-label">Container</h4>
                <dl class="detail-dl">
                  <dt>Container ID</dt><dd class="mono">{containerRecord.container_id ?? '—'}</dd>
                  <dt>Image</dt><dd class="mono">{containerRecord.image ?? '—'}</dd>
                  <dt>Image Hash</dt><dd class="mono">{containerRecord.image_hash ?? '—'}</dd>
                  <dt>Runtime</dt><dd>{containerRecord.runtime ?? '—'}</dd>
                  <dt>Started</dt><dd>{formatTime(containerRecord.started_at)}</dd>
                  {#if containerRecord.stopped_at}
                    <dt>Stopped</dt><dd>{formatTime(containerRecord.stopped_at)}</dd>
                  {/if}
                  {#if containerRecord.exit_code != null}
                    <dt>Exit Code</dt><dd>{containerRecord.exit_code}</dd>
                  {/if}
                </dl>
              </div>
            {/if}
            <AgentCardPanel agentId={selected.id} />
          </div>
        {:else if detailTab === 'logs'}
          <div class="logs-panel" role="tabpanel" id="dtabpanel-logs" aria-labelledby="dtab-logs">
            {#if logsLoading}
              <p class="logs-empty">Loading logs…</p>
            {:else if agentLogLines.length === 0}
              <p class="logs-empty">No logs for this agent.</p>
            {:else}
              <div class="logs-output" aria-live="polite" aria-label="Agent logs">
                {#each agentLogLines as line}
                  <div class="log-line">{line}</div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="logs-panel tty-panel" role="tabpanel" id="dtabpanel-terminal" aria-labelledby="dtab-terminal">
            {#if ttyConnecting}
              <p class="logs-empty">Connecting…</p>
            {:else if ttyLines.length === 0}
              <p class="logs-empty">No output yet.</p>
            {:else}
              <div class="logs-output tty-output" aria-live="polite" aria-label="Terminal output">
                {#each ttyLines as line}
                  <div class="log-line">{line}</div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
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
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .page-desc { font-size: var(--text-sm); color: var(--color-text-secondary); }

  .page-actions { display: flex; align-items: center; gap: var(--space-2); flex-shrink: 0; }

  .view-toggle {
    display: flex;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .toggle-btn:hover, .toggle-btn.active {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .spawn-btn {
    background: var(--color-primary);
    color: var(--color-text-inverse, #fff);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .spawn-btn:hover { background: var(--color-primary-hover); }

  .filter-bar { display: flex; flex-wrap: wrap; gap: var(--space-2); flex-shrink: 0; }

  .pill {
    display: inline-flex;
    align-items: center;
    padding: 0.2rem 0.75rem;
    border-radius: 99px;
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .pill:hover { border-color: var(--color-border-strong); color: var(--color-text); }
  .pill.active { background: color-mix(in srgb, var(--color-primary) 12%, transparent); border-color: var(--color-primary); color: var(--color-primary); }

  .content {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .agent-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-6);
  }

  .agent-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    cursor: pointer;
    text-align: left;
    color: inherit;
    font: inherit;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  .agent-card:hover { border-color: var(--color-border-strong); background: var(--color-surface-elevated); }
  .agent-card.selected { border-color: var(--color-primary); background: color-mix(in srgb, var(--color-primary) 4%, transparent); }
  .skeleton-card { cursor: default; }
  .skeleton-card:hover { border-color: var(--color-border); background: var(--color-surface); }

  .card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .agent-name {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .card-fields { display: flex; flex-direction: column; gap: var(--space-2); }

  .field { display: flex; justify-content: space-between; align-items: center; gap: var(--space-2); }

  .field-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 500;
  }

  .field-value { font-size: var(--text-sm); color: var(--color-text-secondary); }

  .name-cell { font-weight: 600; color: var(--color-text); }
  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .muted { color: var(--color-text-secondary); font-size: var(--text-xs); }

  :global(tr.row-selected td) { background: color-mix(in srgb, var(--color-primary) 4%, transparent); }

  .detail-panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
  }

  .detail-header h3 {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .detail-tabs {
    display: flex;
    border-bottom: 1px solid var(--color-border);
    padding: 0 var(--space-4);
  }

  .dtab {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    margin-bottom: -1px;
    padding: var(--space-2) var(--space-3);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .dtab:hover { color: var(--color-text); }
  .dtab.active { border-bottom-color: var(--color-primary); color: var(--color-primary); }

  .logs-panel {
    display: flex;
    flex-direction: column;
    height: 320px;
    overflow: hidden;
  }

  .logs-empty {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    margin: var(--space-8) auto;
    text-align: center;
  }

  .logs-output {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg);
  }

  .log-line {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .tty-panel { height: 360px; }
  .tty-output { background: var(--color-bg, #0d0d0d); }
  .tty-output .log-line { color: var(--color-text-secondary, #d4d4d4); }

  .close-btn {
    background: none;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-base);
    padding: var(--space-1);
    line-height: 1;
    transition: color var(--transition-fast);
  }

  .close-btn:hover { color: var(--color-text); }

  .detail-body {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .detail-dl {
    display: grid;
    grid-template-columns: 8rem 1fr;
    gap: var(--space-2) var(--space-4);
    font-size: var(--text-sm);
  }

  .detail-dl dt { color: var(--color-text-muted); }
  .detail-dl dd { margin: 0; color: var(--color-text-secondary); }

  .container-section {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-3);
  }

  .section-label {
    font-family: var(--font-display);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3);
  }

  /* Modal */
  .modal-backdrop {
    position: fixed; inset: 0; background: color-mix(in srgb, black 60%, transparent); z-index: 1000;
    display: flex; align-items: center; justify-content: center;
  }

  .modal {
    position: fixed;
    z-index: 1001;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    min-width: 360px;
    max-width: 480px;
    width: 100%;
    max-height: 90vh;
    overflow-y: auto;
    box-shadow: var(--shadow-lg);
  }

  .modal h3 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-4);
  }

  .form { display: flex; flex-direction: column; gap: var(--space-3); }

  .form label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .form input, .form select {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .form input:focus:not(:focus-visible),
  .form select:focus:not(:focus-visible) { outline: none; }
  .form input:focus-visible,
  .form select:focus-visible { outline: 2px solid var(--color-focus, #4db0ff); outline-offset: 2px; border-color: var(--color-focus, #4db0ff); }

  .field-hint { font-size: 0.7rem; color: var(--color-text-muted); margin-top: 2px; }

  .form-error { color: var(--color-danger); font-size: var(--text-xs); margin: 0; }

  .form-actions { display: flex; gap: var(--space-2); justify-content: flex-end; margin-top: var(--space-2); }

  .modal-btn {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    background: var(--color-surface);
    color: var(--color-text);
    transition: all var(--transition-fast);
  }

  .modal-btn.primary { background: var(--color-primary); color: var(--color-text-inverse, #fff); border-color: var(--color-primary); }
  .modal-btn.primary:hover { background: var(--color-primary-hover); }
  .modal-btn.secondary:hover { background: var(--color-surface-elevated); }
  .modal-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .spawn-success dl {
    display: grid;
    grid-template-columns: 7rem 1fr;
    gap: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    margin: var(--space-3) 0 var(--space-4);
  }

  .spawn-success dt { color: var(--color-text-secondary); }
  .spawn-success dd { margin: 0; color: var(--color-text-muted); word-break: break-all; }
  .success-msg { color: var(--color-success); font-size: var(--text-sm); margin: 0 0 var(--space-2); }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .link-btn:hover { opacity: 0.8; }

  .toggle-btn:focus-visible,
  .spawn-btn:focus-visible,
  .pill:focus-visible,
  .modal-btn:focus-visible,
  .link-btn:focus-visible,
  .dtab:focus-visible,
  .agent-card:focus-visible,
  .close-btn:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  :global(tr[tabindex]:focus-visible td) {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: -2px;
  }
</style>
