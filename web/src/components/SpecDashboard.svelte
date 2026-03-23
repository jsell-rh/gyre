<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Card from '../lib/Card.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { repoId = '' } = $props();

  let specs = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filterStatus = $state('all');
  let filterRepo = $state('');
  let repos = $state([]);
  let selected = $state(null);
  let detailTab = $state('info');
  let historyLoading = $state(false);
  let history = $state([]);

  // Approve modal
  let showApprove = $state(false);
  let approveSha = $state('');
  let approveWorking = $state(false);

  // Revoke modal
  let showRevoke = $state(false);
  let revokeReason = $state('');
  let revokeWorking = $state(false);

  const filters = ['all', 'pending', 'approved', 'drifted'];

  async function load() {
    loading = true;
    error = null;
    try {
      specs = await api.getSpecs();
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  $effect(() => { load(); });

  // Sync repoId prop into filterRepo
  $effect(() => { filterRepo = repoId || ''; });

  // Fetch repos for the filter dropdown
  $effect(() => {
    api.allRepos().then((data) => { repos = Array.isArray(data) ? data : []; }).catch(() => {});
  });

  const filtered = $derived(() => {
    let result = specs;
    // Apply repo filter (match by repo name or owner field)
    if (filterRepo) {
      const repo = repos.find(r => r.id === filterRepo);
      if (repo) {
        result = result.filter((s) =>
          (s.owner ?? '').toLowerCase().includes(repo.name.toLowerCase()) ||
          (s.path ?? '').toLowerCase().includes(repo.name.toLowerCase())
        );
      }
    }
    if (filterStatus === 'approved') return result.filter((s) => s.approval_status === 'approved');
    if (filterStatus === 'pending') return result.filter((s) => s.approval_status === 'pending');
    if (filterStatus === 'drifted') return result.filter((s) => s.drift_status === 'drifted');
    return result;
  });

  const stats = $derived(() => ({
    total: specs.length,
    approved: specs.filter((s) => s.approval_status === 'approved').length,
    pending: specs.filter((s) => s.approval_status === 'pending').length,
    drifted: specs.filter((s) => s.drift_status === 'drifted').length,
  }));

  function selectSpec(s) {
    if (selected?.path === s.path) { selected = null; return; }
    selected = s;
    detailTab = 'info';
    history = [];
  }

  function switchTab(tab) {
    detailTab = tab;
    if (tab === 'history' && selected && history.length === 0) {
      historyLoading = true;
      api.getSpecHistory(selected.path)
        .then((h) => { history = Array.isArray(h) ? h : []; })
        .catch(() => { history = []; })
        .finally(() => { historyLoading = false; });
    }
  }

  function openApprove(spec) {
    selected = spec;
    approveSha = spec.current_sha || '';
    showApprove = true;
  }

  async function doApprove() {
    if (!approveSha.trim() || approveSha.trim().length !== 40) {
      toastError('A valid 40-character SHA is required');
      return;
    }
    approveWorking = true;
    try {
      await api.approveSpec(selected.path, approveSha.trim());
      toastSuccess('Spec approved');
      showApprove = false;
      approveSha = '';
      await load();
      if (selected) {
        selected = specs.find((s) => s.path === selected.path) ?? null;
      }
    } catch (e) {
      toastError(e.message);
    }
    approveWorking = false;
  }

  function openRevoke(spec) {
    selected = spec;
    revokeReason = '';
    showRevoke = true;
  }

  async function doRevoke() {
    if (!revokeReason.trim()) {
      toastError('Revocation reason is required');
      return;
    }
    revokeWorking = true;
    try {
      await api.revokeSpec(selected.path, revokeReason.trim());
      toastSuccess('Approval revoked');
      showRevoke = false;
      await load();
      if (selected) {
        selected = specs.find((s) => s.path === selected.path) ?? null;
      }
    } catch (e) {
      toastError(e.message);
    }
    revokeWorking = false;
  }

  function shortSha(sha) {
    return sha ? sha.substring(0, 7) : '—';
  }

  function relTime(ts) {
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

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  function statusColor(s) {
    if (s === 'approved') return 'success';
    if (s === 'pending') return 'warning';
    if (s === 'drifted') return 'danger';
    return 'neutral';
  }
</script>

<div class="page" class:has-detail={!!selected}>
  <!-- Left: list -->
  <div class="list-pane">
    <div class="view-header">
      <div>
        <h1 class="page-title">Spec Registry</h1>
        <p class="page-desc">Specification manifest and approval ledger (M21)</p>
      </div>
      <Button variant="secondary" onclick={load}>Refresh</Button>
    </div>

    <!-- Stats row -->
    <div class="stats-row">
      <div class="stat-card">
        <span class="stat-val">{stats().total}</span>
        <span class="stat-lbl">Total</span>
      </div>
      <div class="stat-card approved">
        <span class="stat-val">{stats().approved}</span>
        <span class="stat-lbl">Approved</span>
      </div>
      <div class="stat-card pending">
        <span class="stat-val">{stats().pending}</span>
        <span class="stat-lbl">Pending</span>
      </div>
      <div class="stat-card drifted">
        <span class="stat-val">{stats().drifted}</span>
        <span class="stat-lbl">Drifted</span>
      </div>
    </div>

    <!-- Filter bar: status pills + repo dropdown -->
    <div class="filter-bar">
      {#each filters as f}
        <button
          class="pill"
          class:active={filterStatus === f}
          onclick={() => (filterStatus = f)}
        >
          {f.charAt(0).toUpperCase() + f.slice(1)}
        </button>
      {/each}
      {#if repos.length > 0}
        <div class="repo-filter-wrap">
          <select
            class="repo-filter-select"
            bind:value={filterRepo}
            aria-label="Filter specs by repo"
          >
            <option value="">All Repos</option>
            {#each repos as r}
              <option value={r.id}>{r.name}</option>
            {/each}
          </select>
        </div>
      {/if}
    </div>

    <!-- Table -->
    <div class="table-wrap">
      {#if loading}
        <div class="skeleton-rows">
          {#each Array(6) as _}
            <Skeleton width="100%" height="2.5rem" />
          {/each}
        </div>
      {:else if error}
        <EmptyState title="Failed to load specs" description={error} />
      {:else if filtered().length === 0}
        <EmptyState
          title="No specs found"
          description={filterStatus === 'all'
            ? 'No specs are registered in the manifest.'
            : `No specs match the "${filterStatus}" filter.`}
        />
      {:else}
        <table class="specs-table">
          <thead>
            <tr>
              <th>Path</th>
              <th>Title</th>
              <th>Owner</th>
              <th>Status</th>
              <th>SHA</th>
              <th>Updated</th>
            </tr>
          </thead>
          <tbody>
            {#each filtered() as s (s.path)}
              <tr
                class:selected={selected?.path === s.path}
                onclick={() => selectSpec(s)}
                tabindex="0"
                onkeydown={(e) => e.key === 'Enter' && selectSpec(s)}
                role="button"
                aria-pressed={selected?.path === s.path}
              >
                <td class="path-cell">
                  <span class="spec-path">{s.path}</span>
                </td>
                <td class="title-cell">{s.title || '—'}</td>
                <td class="owner-cell">{s.owner || '—'}</td>
                <td>
                  <Badge value={s.approval_status || 'unknown'} color={statusColor(s.approval_status)} />
                </td>
                <td><code class="sha">{shortSha(s.current_sha)}</code></td>
                <td class="time-cell">{relTime(s.updated_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>

  <!-- Right: detail panel -->
  {#if selected}
    <div class="detail-pane">
      <div class="detail-header">
        <div class="detail-title-row">
          <span class="detail-title">{selected.title || selected.path}</span>
          <button class="close-btn" onclick={() => (selected = null)} aria-label="Close detail">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
              <path d="M18 6L6 18M6 6l12 12"/>
            </svg>
          </button>
        </div>
        <div class="detail-badges">
          <Badge value={selected.approval_status || 'unknown'} color={statusColor(selected.approval_status)} />
          {#if selected.drift_status && selected.drift_status !== 'clean'}
            <Badge value={`drift: ${selected.drift_status}`} color="warning" />
          {/if}
        </div>
        <div class="detail-actions">
          {#if selected.approval_status !== 'approved'}
            <Button variant="primary" onclick={() => openApprove(selected)}>Approve</Button>
          {:else}
            <Button variant="danger" onclick={() => openRevoke(selected)}>Revoke</Button>
          {/if}
        </div>
      </div>

      <div class="detail-tabs">
        {#each ['info', 'history', 'links'] as tab}
          <button
            class="tab-btn"
            class:active={detailTab === tab}
            onclick={() => switchTab(tab)}
          >
            {tab.charAt(0).toUpperCase() + tab.slice(1)}
          </button>
        {/each}
      </div>

      <div class="detail-body">
        {#if detailTab === 'info'}
          <dl class="meta-list">
            <dt>Path</dt>
            <dd class="mono">{selected.path}</dd>

            <dt>Title</dt>
            <dd>{selected.title || '—'}</dd>

            <dt>Owner</dt>
            <dd class="mono">{selected.owner || '—'}</dd>

            <dt>Current SHA</dt>
            <dd class="mono">{selected.current_sha || '—'}</dd>

            <dt>Approval Mode</dt>
            <dd>{selected.approval_mode || '—'}</dd>

            <dt>Approval Status</dt>
            <dd><Badge value={selected.approval_status || 'unknown'} color={statusColor(selected.approval_status)} /></dd>

            <dt>Drift Status</dt>
            <dd>{selected.drift_status || '—'}</dd>

            <dt>Created</dt>
            <dd>{fmtDate(selected.created_at)}</dd>

            <dt>Updated</dt>
            <dd>{fmtDate(selected.updated_at)}</dd>
          </dl>

        {:else if detailTab === 'history'}
          {#if historyLoading}
            <div class="skeleton-rows">
              {#each Array(4) as _}
                <Skeleton width="100%" height="2rem" />
              {/each}
            </div>
          {:else if history.length === 0}
            <EmptyState title="No approval history" description="No approval events recorded for this spec." />
          {:else}
            <div class="history-list">
              {#each history as ev (ev.id)}
                <div class="history-item" class:revoked={!ev.is_active}>
                  <div class="history-meta">
                    <Badge value={ev.is_active ? 'Active' : 'Revoked'} color={ev.is_active ? 'success' : 'danger'} />
                    <span class="history-approver mono">{ev.approver_id}</span>
                    <span class="history-time">{fmtDate(ev.approved_at)}</span>
                  </div>
                  <div class="history-sha mono">{shortSha(ev.spec_sha)}</div>
                  {#if ev.revocation_reason}
                    <div class="history-reason">Revoked: {ev.revocation_reason}</div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

        {:else if detailTab === 'links'}
          <div class="links-section">
            <div class="links-group">
              <h4>Linked Tasks</h4>
              {#if selected.linked_tasks?.length}
                <ul class="links-list">
                  {#each selected.linked_tasks as tid}
                    <li class="link-item mono">{tid}</li>
                  {/each}
                </ul>
              {:else}
                <p class="no-links">No linked tasks</p>
              {/if}
            </div>
            <div class="links-group">
              <h4>Linked Merge Requests</h4>
              {#if selected.linked_mrs?.length}
                <ul class="links-list">
                  {#each selected.linked_mrs as mrid}
                    <li class="link-item mono">{mrid}</li>
                  {/each}
                </ul>
              {:else}
                <p class="no-links">No linked merge requests</p>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<!-- Approve Modal -->
<Modal bind:open={showApprove} title="Approve Spec" size="sm">
  <div class="modal-form">
    {#if selected}
      <div class="modal-path-info">
        <span class="field-label">Spec Path</span>
        <code class="modal-path mono">{selected.path}</code>
      </div>
    {/if}
    <label class="field-label" for="approve-sha">SHA (40 hex characters)</label>
    <input
      id="approve-sha"
      class="field-input mono"
      type="text"
      bind:value={approveSha}
      placeholder="40-character commit SHA"
      maxlength="40"
    />
    <p class="field-hint">The SHA of the spec file version you are approving.</p>
    <div class="modal-actions">
      <Button variant="secondary" onclick={() => (showApprove = false)}>Cancel</Button>
      <Button variant="primary" onclick={doApprove} disabled={approveWorking}>
        {approveWorking ? 'Approving…' : 'Approve'}
      </Button>
    </div>
  </div>
</Modal>

<!-- Revoke Modal -->
<Modal bind:open={showRevoke} title="Revoke Approval" size="sm">
  <div class="modal-form">
    {#if selected}
      <div class="modal-path-info">
        <span class="field-label">Spec Path</span>
        <code class="modal-path mono">{selected.path}</code>
      </div>
    {/if}
    <label class="field-label" for="revoke-reason">Reason for revocation</label>
    <textarea
      id="revoke-reason"
      class="field-textarea"
      bind:value={revokeReason}
      placeholder="Describe why you are revoking this approval…"
      rows="3"
    ></textarea>
    <div class="modal-actions">
      <Button variant="secondary" onclick={() => (showRevoke = false)}>Cancel</Button>
      <Button variant="danger" onclick={doRevoke} disabled={revokeWorking}>
        {revokeWorking ? 'Revoking…' : 'Revoke'}
      </Button>
    </div>
  </div>
</Modal>

<style>
  .page {
    display: flex;
    height: 100%;
    overflow: hidden;
  }

  .list-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
    min-width: 0;
  }

  .page.has-detail .list-pane {
    max-width: 60%;
  }

  .view-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .page-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* Stats row */
  .stats-row {
    display: flex;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .stat-card {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    gap: var(--space-1);
  }

  .stat-val {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
  }

  .stat-lbl {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .stat-card.approved .stat-val { color: var(--color-success); }
  .stat-card.pending .stat-val  { color: var(--color-warning); }
  .stat-card.drifted .stat-val  { color: var(--color-danger); }

  /* Filter pills */
  .filter-bar {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .pill {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast);
  }

  .pill:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .pill.active {
    background: rgba(238, 0, 0, 0.12);
    border-color: var(--color-primary);
    color: var(--color-primary);
    font-weight: 500;
  }

  .repo-filter-wrap {
    display: flex;
    align-items: center;
    margin-left: auto;
  }

  .repo-filter-select {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    outline: none;
  }

  .repo-filter-select:focus {
    border-color: var(--color-primary);
  }

  /* Table */
  .table-wrap {
    flex: 1;
    overflow-y: auto;
  }

  .skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .specs-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .specs-table th {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
  }

  .specs-table td {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    vertical-align: middle;
  }

  .specs-table tr {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .specs-table tr:hover td {
    background: var(--color-surface-elevated);
  }

  .specs-table tr.selected td {
    background: rgba(238, 0, 0, 0.06);
  }

  .path-cell { max-width: 220px; }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    word-break: break-all;
  }

  .title-cell {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .owner-cell {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sha {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    padding: 2px 4px;
    border-radius: var(--radius-sm);
  }

  .time-cell {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  /* Detail pane */
  .detail-pane {
    width: 380px;
    min-width: 320px;
    border-left: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-surface);
  }

  .detail-header {
    padding: var(--space-5) var(--space-5) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .detail-title-row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .detail-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    word-break: break-word;
    flex: 1;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    border-radius: var(--radius);
    padding: var(--space-1);
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .close-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .detail-badges {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .detail-actions {
    display: flex;
    gap: var(--space-2);
  }

  /* Tabs */
  .detail-tabs {
    display: flex;
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .tab-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover { color: var(--color-text); }

  .tab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .detail-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-5);
  }

  /* Meta list */
  .meta-list {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    font-size: var(--text-sm);
  }

  .meta-list dt {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    padding-top: 2px;
  }

  .meta-list dd {
    margin: 0;
    color: var(--color-text);
    word-break: break-all;
  }

  .meta-list .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  /* History */
  .history-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .history-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .history-item.revoked {
    opacity: 0.6;
  }

  .history-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .history-approver {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .history-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .history-sha {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .history-reason {
    font-size: var(--text-xs);
    color: var(--color-danger);
    font-style: italic;
  }

  /* Links */
  .links-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .links-group h4 {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-2);
  }

  .links-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .link-item {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
  }

  .no-links {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
    font-style: italic;
  }

  /* Modal form */
  .modal-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .modal-path-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .modal-path {
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    word-break: break-all;
  }

  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .field-input,
  .field-textarea {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    box-sizing: border-box;
    resize: vertical;
  }

  .field-input.mono { font-family: var(--font-mono); }

  .field-input:focus,
  .field-textarea:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .mono { font-family: var(--font-mono); }
</style>
