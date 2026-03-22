<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Table from '../lib/Table.svelte';
  import AgentCardPanel from './AgentCardPanel.svelte';

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

  let detailTab = $state('info');
  let agentLogLines = $state([]);
  let logsLoading = $state(false);
  let ttyLines = $state([]);
  let ttyWs = $state(null);
  let ttyConnecting = $state(false);

  const statuses = ['Active', 'Idle', 'Blocked', 'Error', 'Dead'];

  const filtered = $derived(
    statusFilter ? agents.filter((a) => a.status === statusFilter) : agents
  );

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
    api.agents()
      .then((data) => { agents = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
    api.allRepos().then((data) => { repos = data; }).catch(() => {});
    api.tasks().then((data) => { tasks = data; }).catch(() => {});
  });

  function closeTtyWs() {
    if (ttyWs) { ttyWs.close(); ttyWs = null; }
    ttyLines = [];
    ttyConnecting = false;
  }

  function selectAgent(a) {
    if (selected?.id === a.id) { selected = null; closeTtyWs(); return; }
    closeTtyWs();
    selected = a;
    detailTab = 'info';
    agentLogLines = [];
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
        <button class="toggle-btn" class:active={viewMode === 'grid'} onclick={() => (viewMode = 'grid')} title="Grid view">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <rect x="1" y="1" width="6" height="6" rx="1"/>
            <rect x="9" y="1" width="6" height="6" rx="1"/>
            <rect x="1" y="9" width="6" height="6" rx="1"/>
            <rect x="9" y="9" width="6" height="6" rx="1"/>
          </svg>
        </button>
        <button class="toggle-btn" class:active={viewMode === 'table'} onclick={() => (viewMode = 'table')} title="Table view">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
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
    <button class="pill" class:active={statusFilter === ''} onclick={() => (statusFilter = '')}>All</button>
    {#each statuses as s}
      <button
        class="pill"
        class:active={statusFilter === s}
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
            <label>Name<input bind:value={spawnName} placeholder="worker-1" /></label>
            <label>Repository
              <select bind:value={spawnRepoId}>
                <option value="">Select repo...</option>
                {#each repos as r}<option value={r.id}>{r.name}</option>{/each}
              </select>
            </label>
            <label>Task
              <select bind:value={spawnTaskId}>
                <option value="">Select task...</option>
                {#each tasks as t}<option value={t.id}>{t.title}</option>{/each}
              </select>
            </label>
            <label>Branch<input bind:value={spawnBranch} placeholder="feat/my-feature" /></label>
            {#if spawnError}<p class="form-error">{spawnError}</p>{/if}
            <div class="form-actions">
              <button class="modal-btn secondary" onclick={closeSpawnModal}>Cancel</button>
              <button class="modal-btn primary" onclick={doSpawn} disabled={spawnLoading}>
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
      <div class="error-msg">Error: {error}</div>
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
              <span class="agent-name">{a.name}</span>
              <Badge value={a.status} />
            </div>
            <div class="card-fields">
              <div class="field">
                <span class="field-label">Task</span>
                <span class="field-value mono">{a.current_task_id ?? '—'}</span>
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
            <tr class:row-selected={selected?.id === a.id} onclick={() => selectAgent(a)} style="cursor:pointer">
              <td class="name-cell">{a.name}</td>
              <td><Badge value={a.status} /></td>
              <td class="mono muted">{a.current_task_id ?? '—'}</td>
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
          <button class="close-btn" onclick={() => { selected = null; closeTtyWs(); }}>✕</button>
        </div>
        <div class="detail-tabs">
          <button class="dtab" class:active={detailTab === 'info'} onclick={() => switchDetailTab('info')}>Info</button>
          <button class="dtab" class:active={detailTab === 'logs'} onclick={() => switchDetailTab('logs')}>Logs</button>
          <button class="dtab" class:active={detailTab === 'terminal'} onclick={() => switchDetailTab('terminal')}>Terminal</button>
        </div>
        {#if detailTab === 'info'}
          <div class="detail-body">
            <dl class="detail-dl">
              <dt>ID</dt><dd class="mono">{selected.id}</dd>
              <dt>Status</dt><dd><Badge value={selected.status} /></dd>
              <dt>Parent</dt><dd class="mono">{selected.parent_id ?? '—'}</dd>
              <dt>Current Task</dt><dd class="mono">{selected.current_task_id ?? '—'}</dd>
              <dt>Budget (s)</dt><dd>{selected.lifetime_budget_secs ?? '—'}</dd>
              <dt>Spawned</dt><dd>{formatTime(selected.spawned_at)}</dd>
              <dt>Last Heartbeat</dt><dd>{formatTime(selected.last_heartbeat)}</dd>
            </dl>
            <AgentCardPanel agentId={selected.id} />
          </div>
        {:else if detailTab === 'logs'}
          <div class="logs-panel">
            {#if logsLoading}
              <p class="logs-empty">Loading logs…</p>
            {:else if agentLogLines.length === 0}
              <p class="logs-empty">No logs for this agent.</p>
            {:else}
              <div class="logs-output">
                {#each agentLogLines as line}
                  <div class="log-line">{line}</div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="logs-panel tty-panel">
            {#if ttyConnecting}
              <p class="logs-empty">Connecting…</p>
            {:else if ttyLines.length === 0}
              <p class="logs-empty">No output yet.</p>
            {:else}
              <div class="logs-output tty-output">
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
    color: #fff;
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
  .pill.active { background: rgba(238,0,0,0.12); border-color: var(--color-primary); color: var(--color-primary); }

  .content {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .agent-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
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
  .agent-card.selected { border-color: var(--color-primary); background: rgba(238,0,0,0.04); }
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

  :global(tr.row-selected td) { background: rgba(238,0,0,0.04); }

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
  .tty-output { background: #0d0d0d; }
  .tty-output .log-line { color: #d4d4d4; }

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

  /* Modal */
  .modal-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,0.6); z-index: 100;
    display: flex; align-items: center; justify-content: center;
  }

  .modal {
    position: fixed;
    z-index: 101;
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

  .form input:focus, .form select:focus { outline: none; border-color: var(--color-primary); }

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

  .modal-btn.primary { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
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
</style>
