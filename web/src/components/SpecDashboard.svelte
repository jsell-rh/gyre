<script>
  /**
   * SpecDashboard — S4.5 Specs View (list pane)
   *
   * Spec ref: ui-layout.md §6 (Specs View Layout — Full-Width list, split on row click)
   *           human-system-interface.md §1 (nav scope: Specs)
   *
   * Props:
   *   workspaceId — string | null
   *   repoId      — string | null
   *   scope       — 'tenant' | 'workspace' | 'repo'
   *
   * Shell context (from S4.1 App Shell, PR #394):
   *   openDetailPanel({type, id, data}) — opens detail panel at 40%, compresses list to 60%
   */

  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Button from '../lib/Button.svelte';
  import Modal from '../lib/Modal.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Shell context — openDetailPanel may not exist (e.g. in tests or older shell)
  const openDetailPanel = getContext('openDetailPanel') ?? null;

  // ── List state ──────────────────────────────────────────────────────────────
  let specs = $state([]);
  let loading = $state(true);
  let error = $state(null);

  // Filters
  let filterStatus = $state('all');
  let filterKind = $state('all');

  // Sort (workspace/tenant scope)
  let sortCol = $state('path');
  let sortDir = $state('asc');

  // Repo-scope progress bars (preloaded for all specs in this repo)
  let progressMap = $state({});

  // Selected path (for row highlight when detail panel open)
  let selectedPath = $state(null);

  // New spec modal
  let showNewSpec = $state(false);
  let newSpecPath = $state('');
  let newSpecContent = $state('# New Spec\n\n## Overview\n\n');
  let newSpecSaving = $state(false);
  let pathTouched = $state(false);

  // ── Constants ───────────────────────────────────────────────────────────────
  const STATUS_FILTERS = ['all', 'approved', 'pending', 'deprecated'];
  const TABLE_COLS = [
    ['path',            'Path'],
    ['approval_status', 'Status'],
    ['kind',            'Kind'],
    ['owner',           'Owner'],
    ['updated_at',      'Updated'],
  ];

  // ── Load specs ──────────────────────────────────────────────────────────────
  async function load() {
    loading = true;
    error = null;
    try {
      specs = await api.specsForWorkspace(workspaceId);
      if (scope === 'repo' && repoId) {
        loadProgressMap().catch(e => console.error('Progress load failed:', e));
      }
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  async function loadProgressMap() {
    if (!repoId) return;
    const paths = specs.map((s) => s.path);
    const results = await Promise.allSettled(
      paths.map((p) => api.specProgress(p, repoId))
    );
    const map = {};
    results.forEach((r, i) => {
      if (r.status === 'fulfilled' && r.value) {
        map[paths[i]] = r.value;
      }
    });
    progressMap = map;
  }

  $effect(() => {
    void scope; void workspaceId; void repoId;
    // Clear stale selection from previous scope
    selectedPath = null;
    load();
  });

  // ── Derived: filtered + sorted ──────────────────────────────────────────────
  const allKinds = $derived.by(() => {
    const set = new Set(specs.map((s) => s.kind || 'feature'));
    return ['all', ...Array.from(set).sort()];
  });

  const filtered = $derived.by(() => {
    let result = specs;
    if (filterStatus !== 'all') {
      result = result.filter((s) => s.approval_status === filterStatus);
    }
    if (filterKind !== 'all') {
      result = result.filter((s) => (s.kind || 'feature') === filterKind);
    }
    if (scope !== 'repo') {
      result = sortList(result, sortCol, sortDir);
    }
    return result;
  });

  function sortList(list, col, dir) {
    return [...list].sort((a, b) => {
      const av = String(a[col] ?? '');
      const bv = String(b[col] ?? '');
      const cmp = av.localeCompare(bv);
      return dir === 'asc' ? cmp : -cmp;
    });
  }

  function toggleSort(col) {
    if (sortCol === col) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortCol = col;
      sortDir = 'asc';
    }
  }

  function sortArrow(col) {
    if (sortCol !== col) return '↕';
    return sortDir === 'asc' ? '↑' : '↓';
  }

  // ── Row click → open detail panel ──────────────────────────────────────────
  function handleRowClick(spec) {
    selectedPath = spec.path;
    if (openDetailPanel) {
      openDetailPanel({
        type: 'spec',
        id: spec.path,
        data: { ...spec, repo_id: repoId },
      });
    }
  }

  // ── Progress bar helpers ────────────────────────────────────────────────────
  function progressFraction(path) {
    const p = progressMap[path];
    if (!p || !p.total_tasks) return 0;
    return p.completed_tasks / p.total_tasks;
  }

  function progressLabel(path) {
    const p = progressMap[path];
    if (!p) return null;
    return `${p.completed_tasks}/${p.total_tasks} tasks`;
  }

  // ── New spec ────────────────────────────────────────────────────────────────
  async function saveNewSpec() {
    if (!repoId || !newSpecPath.trim() || newSpecSaving) return;
    newSpecSaving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: newSpecPath.trim(),
        content: newSpecContent,
        message: `Create ${newSpecPath.trim()} via UI`,
      });
      toastSuccess(`Spec created — MR #${result.mr_id} created`);
      showNewSpec = false;
      newSpecPath = '';
      newSpecContent = '# New Spec\n\n## Overview\n\n';
      await load();
    } catch (e) {
      toastError(`Create failed: ${e.message}`);
    } finally {
      newSpecSaving = false;
    }
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────
  function statusColor(s) {
    if (s === 'approved')   return 'success';
    if (s === 'pending')    return 'warning';
    if (s === 'deprecated') return 'neutral';
    return 'neutral';
  }

  function statusIcon(s) {
    if (s === 'approved')   return '✓';
    if (s === 'pending')    return '◐';
    if (s === 'deprecated') return '✗';
    return '?';
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
</script>

<div class="spec-view">
  <span class="sr-only" aria-live="polite">{loading ? "" : "specs loaded"}</span>
  <!-- ── Header ─────────────────────────────────────────────────────────────── -->
  <div class="view-header">
    <div>
      <h1 class="page-title">Specs</h1>
      {#if scope === 'tenant'}
        <p class="page-desc">All specs across workspaces</p>
      {:else if scope === 'workspace'}
        <p class="page-desc">Specs in this workspace</p>
      {/if}
    </div>
    <div class="header-actions">
      {#if scope === 'repo' && repoId}
        <Button variant="primary" onclick={() => (showNewSpec = true)}>+ New Spec</Button>
      {/if}
      <Button variant="secondary" onclick={load}>Refresh</Button>
    </div>
  </div>

  <!-- ── Filter bar ─────────────────────────────────────────────────────────── -->
  <div class="filter-bar">
    <div class="filter-group" role="group" aria-label="Filter by status">
      {#each STATUS_FILTERS as f}
        <button
          class="pill"
          class:active={filterStatus === f}
          onclick={() => (filterStatus = f)}
          aria-pressed={filterStatus === f}
        >
          {f.charAt(0).toUpperCase() + f.slice(1)}
        </button>
      {/each}
    </div>

    {#if allKinds.length > 2}
      <div class="filter-group" role="group" aria-label="Filter by kind">
        <span class="filter-label">Kind:</span>
        {#each allKinds as k}
          <button
            class="pill"
            class:active={filterKind === k}
            onclick={() => (filterKind = k)}
            aria-pressed={filterKind === k}
          >
            {k.charAt(0).toUpperCase() + k.slice(1)}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- ── Content area ───────────────────────────────────────────────────────── -->
  <div class="content-area" aria-busy={loading}>
    {#if loading}
      <div class="skeleton-rows">
        {#each Array(7) as _}
          <Skeleton width="100%" height="2.5rem" />
        {/each}
      </div>

    {:else if error}
      <div class="error-banner" role="alert">
        <span>{error}</span>
        <button onclick={load} class="retry-btn">Retry</button>
      </div>

    {:else if filtered.length === 0}
      <EmptyState
        title="No specs found"
        description={filterStatus === 'all' && filterKind === 'all'
          ? 'No specs are registered.'
          : 'No specs match the current filters.'}
      />
      {#if filterStatus !== 'all' || filterKind !== 'all'}
        <div class="clear-filters-wrap">
          <button class="clear-filters-btn" onclick={() => { filterStatus = 'all'; filterKind = 'all'; }}>Clear filters</button>
        </div>
      {/if}

    {:else if scope === 'repo'}
      <!-- Repo scope: progress bar list -->
      <ul class="spec-list" role="list" aria-label="Specs">
        {#each filtered as spec (spec.path)}
          {@const pct = Math.round(progressFraction(spec.path) * 100)}
          {@const label = progressLabel(spec.path)}
          <li
            class="spec-row"
            class:selected={selectedPath === spec.path}
            tabindex="0"
            aria-current={selectedPath === spec.path ? 'true' : undefined}
            onclick={() => handleRowClick(spec)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleRowClick(spec); }
              if (e.key === 'ArrowDown') { e.preventDefault(); const next = e.currentTarget.nextElementSibling; if (next) next.focus(); }
              if (e.key === 'ArrowUp') { e.preventDefault(); const prev = e.currentTarget.previousElementSibling; if (prev) prev.focus(); }
            }}
          >
            <span class="spec-path" title={spec.path}>{spec.path}</span>
            <span class="spec-status-inline {statusColor(spec.approval_status)}">
              <span aria-hidden="true">{statusIcon(spec.approval_status)}</span> {spec.approval_status ?? 'unknown'}
            </span>
            {#if label}
              <span class="progress-label-text">{label}</span>
              <div
                class="progress-bar-wrap"
                title="{pct}% complete"
                role="progressbar"
                aria-valuenow={pct}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-label="{spec.path} progress: {pct}%"
              >
                <div class="progress-bar">
                  <div class="progress-fill" style="width: {pct}%"></div>
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>

    {:else}
      <!-- Workspace / tenant scope: sortable table -->
      <table class="specs-table" role="grid" aria-label="Specs registry">
        <thead>
          <tr>
            {#each TABLE_COLS as [col, label]}
              <th scope="col" aria-sort={sortCol === col ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                <button class="sort-btn" onclick={() => toggleSort(col)}>
                  {label}
                  <span class="sort-arrow" aria-hidden="true">{sortArrow(col)}</span>
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each filtered as spec (spec.path)}
            <tr
              class:selected={selectedPath === spec.path}
              onclick={() => handleRowClick(spec)}
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleRowClick(spec); }
                if (e.key === 'ArrowDown') { e.preventDefault(); const next = e.currentTarget.nextElementSibling; if (next) next.focus(); }
                if (e.key === 'ArrowUp') { e.preventDefault(); const prev = e.currentTarget.previousElementSibling; if (prev) prev.focus(); }
              }}
              aria-selected={selectedPath === spec.path}
              aria-label="Spec: {spec.path}"
            >
              <td class="col-path">
                <span class="spec-path" title={spec.path}>{spec.path}</span>
              </td>
              <td>
                <Badge
                  value="{spec.approval_status ?? 'unknown'}"
                  color={statusColor(spec.approval_status)}
                />
              </td>
              <td class="col-kind">{spec.kind || '—'}</td>
              <td class="col-owner">{spec.owner || '—'}</td>
              <td class="col-time">{relTime(spec.updated_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<!-- ── New Spec modal (Editor Split layout per ui-layout.md §2) ────────────── -->
<Modal bind:open={showNewSpec} title="New Spec" size="lg">
  <div class="new-spec-body">
    <!-- Left: editor -->
    <div class="editor-pane">
      <label class="field-label" for="new-spec-path">Spec Path</label>
      <input
        id="new-spec-path"
        class="field-input mono"
        type="text"
        bind:value={newSpecPath}
        placeholder="system/my-feature.md"
        aria-required="true"
        aria-invalid={pathTouched && !newSpecPath.trim() ? 'true' : 'false'}
        aria-describedby={pathTouched && !newSpecPath.trim() ? 'path-error' : undefined}
        onblur={() => { pathTouched = true; }}
      />
      {#if pathTouched && !newSpecPath.trim()}
        <span id="path-error" role="alert" style="color: var(--color-danger); font-size: var(--text-xs);">Path is required</span>
      {/if}
      <label class="field-label" for="new-spec-content">Content</label>
      <textarea
        id="new-spec-content"
        class="spec-editor"
        bind:value={newSpecContent}
        spellcheck="false"
      ></textarea>
    </div>
    <!-- Right: preview -->
    <div class="preview-pane">
      <span class="preview-label">Markdown source</span>
      <pre class="preview-pre">{newSpecContent}</pre>
    </div>
  </div>
  <div class="modal-footer">
    <Button variant="secondary" onclick={() => { showNewSpec = false; }}>Cancel</Button>
    <Button
      variant="primary"
      onclick={saveNewSpec}
      disabled={newSpecSaving || !newSpecPath.trim()}
      aria-busy={newSpecSaving}
    >
      {newSpecSaving ? 'Saving…' : 'Save & Create MR'}
    </Button>
  </div>
</Modal>

<style>
  /* ── Page ────────────────────────────────────────────────────────────────── */
  .spec-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  /* ── Header ──────────────────────────────────────────────────────────────── */
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
    text-transform: capitalize;
  }

  .header-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  /* ── Filter bar ──────────────────────────────────────────────────────────── */
  .filter-bar {
    display: flex;
    gap: var(--space-4);
    flex-shrink: 0;
    flex-wrap: wrap;
    align-items: center;
  }

  .filter-group {
    display: flex;
    gap: var(--space-1);
    align-items: center;
  }

  .filter-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .pill {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .pill:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .pill:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .pill.active {
    background: color-mix(in srgb, var(--color-link) 12%, transparent);
    border-color: var(--color-link);
    color: var(--color-link);
    font-weight: 500;
  }

  /* ── Content area ────────────────────────────────────────────────────────── */
  .content-area {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* ── Sortable table (workspace/tenant scope) ─────────────────────────────── */
  .specs-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .specs-table th {
    text-align: left;
    padding: 0;
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .sort-arrow {
    font-size: var(--text-xs);
    opacity: 0.6;
  }

  .specs-table td {
    padding: var(--space-3);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    vertical-align: middle;
  }

  .specs-table tr {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .specs-table tbody tr:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .specs-table tr:hover td {
    background: var(--color-surface-elevated);
  }

  .specs-table tr.selected td {
    background: color-mix(in srgb, var(--color-link) 6%, transparent);
  }

  .col-path { max-width: 240px; }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
  }

  .col-kind,
  .col-owner {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  /* ── Repo progress list ──────────────────────────────────────────────────── */
  .spec-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }

  .spec-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    transition: background var(--transition-fast);
    flex-wrap: wrap;
  }

  .spec-row:hover { background: var(--color-surface-elevated); }

  .spec-row.selected { background: color-mix(in srgb, var(--color-link) 6%, transparent); }

  .spec-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .spec-row .spec-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spec-status-inline {
    font-size: var(--text-xs);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .spec-status-inline.success { color: var(--color-success); }
  .spec-status-inline.warning { color: var(--color-warning); }
  .spec-status-inline.neutral { color: var(--color-text-muted); }

  .progress-label-text {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
    min-width: 70px;
    text-align: right;
  }

  .progress-bar-wrap {
    flex-shrink: 0;
    width: 80px;
  }

  .progress-bar {
    height: 6px;
    background: var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: var(--radius-sm);
    transition: width var(--transition-slow);
  }

  /* ── New spec modal body ─────────────────────────────────────────────────── */
  .new-spec-body {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    min-height: 360px;
    overflow: hidden;
  }

  .editor-pane,
  .preview-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow: hidden;
  }

  .preview-pane {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
  }

  .preview-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .preview-pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--color-text);
    overflow-y: auto;
    flex: 1;
  }

  .spec-editor {
    flex: 1;
    min-height: 280px;
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    resize: none;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .spec-editor:focus:not(:focus-visible) {
    outline: none;
  }

  .spec-editor:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  /* ── Shared ──────────────────────────────────────────────────────────────── */
  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .field-input.mono { font-family: var(--font-mono); }

  .field-input:focus:not(:focus-visible) {
    outline: none;
  }

  .field-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  /* ── Clear filters ───────────────────────────────────────────────────── */
  .clear-filters-wrap {
    display: flex;
    justify-content: center;
    margin-top: var(--space-3);
  }

  .clear-filters-btn {
    padding: var(--space-1) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .clear-filters-btn:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text);
  }

  .clear-filters-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Error banner ─────────────────────────────────────────────────────── */
  .error-banner {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-4);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-link) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-link) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-link) 25%, transparent);
    border-color: var(--color-link);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .pill,
    .sort-btn,
    .specs-table tr,
    .spec-row,
    .progress-fill,
    .spec-editor,
    .field-input,
    .clear-filters-btn,
    .retry-btn {
      transition: none;
      animation: none;
    }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
