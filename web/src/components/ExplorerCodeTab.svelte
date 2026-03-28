<script>
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { repoId = null, repo = null } = $props();

  const openDetailPanel = getContext('openDetailPanel');

  let subTab = $state('branches');
  const SUB_TABS = [
    { id: 'branches', label: 'Branches' },
    { id: 'commits', label: 'Commits' },
    { id: 'merge-requests', label: 'Merge Requests' },
    { id: 'merge-queue', label: 'Merge Queue' },
  ];

  // Clone URL copy state
  let cloneCopied = $state(false);
  let cloneUrl = $derived(repo?.clone_url ?? '');
  let copyTimer = null;

  async function copyCloneUrl() {
    if (!cloneUrl) return;
    try {
      await navigator.clipboard.writeText(cloneUrl);
      cloneCopied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => { cloneCopied = false; copyTimer = null; }, 2000);
    } catch {
      // clipboard not available — silently fail
    }
  }

  $effect(() => {
    return () => { if (copyTimer) clearTimeout(copyTimer); };
  });

  // Per-tab data
  let branches = $state([]);
  let commits = $state([]);
  let mrs = $state([]);
  let queue = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filterQuery = $state('');

  // Sort state
  let sortField = $state('name');
  let sortDir = $state('asc');

  $effect(() => {
    if (repoId) loadTab(subTab);
  });

  async function loadTab(tab) {
    if (!repoId) return;
    loading = true;
    error = null;
    filterQuery = '';
    try {
      if (tab === 'branches') {
        branches = await api.repoBranches(repoId);
      } else if (tab === 'commits') {
        const branch = repo?.default_branch ?? 'main';
        commits = await api.repoCommits(repoId, branch, 50);
      } else if (tab === 'merge-requests') {
        mrs = await api.mergeRequests({ repository_id: repoId });
      } else if (tab === 'merge-queue') {
        const all = await api.mergeQueue();
        queue = Array.isArray(all) ? all.filter(e => e.repository_id === repoId || e.repo_id === repoId) : [];
      }
    } catch (e) {
      error = 'Failed to load ' + tab + ': ' + e.message;
    } finally {
      loading = false;
    }
  }

  function switchSubTab(id) {
    subTab = id;
    loadTab(id);
  }

  function onRowClick(row, type) {
    openDetailPanel?.({ type, id: row.id, data: row });
  }

  function toggleSort(field) {
    if (sortField === field) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortField = field;
      sortDir = 'asc';
    }
  }

  function sortIcon(field) {
    if (sortField !== field) return '↕';
    return sortDir === 'asc' ? '↑' : '↓';
  }

  function matchesFilter(row) {
    if (!filterQuery.trim()) return true;
    const q = filterQuery.toLowerCase();
    const str = Object.values(row).filter(v => typeof v === 'string').join(' ').toLowerCase();
    return str.includes(q);
  }

  let filteredBranches = $derived.by(() => {
    let rows = branches.filter(matchesFilter);
    rows.sort((a, b) => {
      const av = a[sortField] ?? '';
      const bv = b[sortField] ?? '';
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredMrs = $derived.by(() => {
    let rows = mrs.filter(matchesFilter);
    rows.sort((a, b) => {
      const av = a[sortField] ?? '';
      const bv = b[sortField] ?? '';
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredCommits = $derived.by(() => commits.filter(matchesFilter));

  let filteredQueue = $derived.by(() => queue.filter(matchesFilter));

  function relativeTime(ts) {
    if (!ts) return '';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }
</script>

<div class="code-tab">
  <span class="sr-only" aria-live="polite">{loading ? "" : "code view loaded"}</span>

  <!-- Clone URL header -->
  {#if cloneUrl}
    <div class="clone-url-bar">
      <span class="clone-label">Clone</span>
      <code class="clone-url-text">{cloneUrl}</code>
      <button class="clone-copy-btn" onclick={copyCloneUrl} aria-label="Copy clone URL" title="Copy clone URL">
        {#if cloneCopied}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>
          Copied!
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
          Copy
        {/if}
      </button>
    </div>
  {/if}

  <!-- Sub-tab bar -->
  <div class="subtab-bar" role="tablist" aria-label="Code sub-tabs">
    {#each SUB_TABS as st}
      <button
        class="subtab-btn {subTab === st.id ? 'active' : ''}"
        role="tab"
        aria-selected={subTab === st.id}
        onclick={() => switchSubTab(st.id)}
        type="button"
      >{st.label}</button>
    {/each}
  </div>

  <!-- Filter input -->
  <div class="filter-bar">
    <input
      type="search"
      class="filter-input"
      placeholder="Filter {SUB_TABS.find(t => t.id === subTab)?.label ?? ''}…"
      bind:value={filterQuery}
      aria-label="Filter list"
    />
  </div>

  <!-- Content -->
  <div class="table-wrap" role="tabpanel" aria-busy={loading}>
    {#if error}
      <div class="error-banner" role="alert">
        <span>{error}</span>
        <button class="retry-btn" onclick={() => { error = null; loadTab(subTab); }}>Retry</button>
      </div>
    {:else if loading}
      <Skeleton lines={6} />
    {:else if subTab === 'branches'}
      {#if filteredBranches.length === 0}
        <EmptyState title="No branches" message={filterQuery ? 'No branches match your filter.' : 'No branches found for this repository.'} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('name')}>Name {sortIcon('name')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('last_commit')}>Last Commit {sortIcon('last_commit')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author')}>Author {sortIcon('author')}</button></th>
              <th scope="col">Status</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredBranches as branch}
              <tr class="table-row" onclick={() => onRowClick(branch, 'branch')} tabindex="0" role="button" aria-label="View branch {branch.name}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(branch, 'branch'); } }}>
                <td class="mono">{branch.name}</td>
                <td class="secondary">{branch.last_commit ? branch.last_commit.slice(0, 7) : '—'}</td>
                <td class="secondary">{branch.author ?? '—'}</td>
                <td><span class="status-badge">{branch.status ?? 'active'}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'commits'}
      {#if filteredCommits.length === 0}
        <EmptyState title="No commits" message={filterQuery ? 'No commits match your filter.' : 'No commits found for this branch.'} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col">SHA</th>
              <th scope="col">Message</th>
              <th scope="col">Author</th>
              <th scope="col">Date</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredCommits as commit}
              <tr class="table-row" tabindex="0" role="button" aria-label="Commit {commit.sha ?? commit.id ?? ''}">
                <td class="mono">{(commit.sha ?? commit.id ?? '').slice(0, 7)}</td>
                <td class="commit-msg">{commit.message ?? commit.summary ?? '—'}</td>
                <td class="secondary">{commit.author ?? commit.author_name ?? '—'}</td>
                <td class="secondary">{relativeTime(commit.timestamp ?? commit.authored_at ?? commit.date)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'merge-requests'}
      {#if filteredMrs.length === 0}
        <EmptyState title="No merge requests" message={filterQuery ? 'No MRs match your filter.' : 'No open merge requests for this repository.'} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('title')}>Title {sortIcon('title')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('status')}>Status {sortIcon('status')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author_id')}>Author {sortIcon('author_id')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('updated_at')}>Updated {sortIcon('updated_at')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredMrs as mr}
              <tr class="table-row" onclick={() => onRowClick(mr, 'mr')} tabindex="0" role="button" aria-label="View MR {mr.title}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(mr, 'mr'); } }}>
                <td>{mr.title}</td>
                <td><span class="status-badge status-{mr.status}">{mr.status}</span></td>
                <td class="secondary">{mr.author_id ?? '—'}</td>
                <td class="secondary">{relativeTime(mr.updated_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'merge-queue'}
      {#if filteredQueue.length === 0}
        <EmptyState title="Merge queue empty" message={filterQuery ? 'No entries match your filter.' : 'No entries in the merge queue for this repository.'} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col">MR</th>
              <th scope="col">Priority</th>
              <th scope="col">Status</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredQueue as entry}
              <tr class="table-row" onclick={() => onRowClick(entry, 'mr')} tabindex="0" role="button" aria-label="View queue entry" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(entry, 'mr'); } }}>
                <td class="mono">{entry.merge_request_id ?? entry.mr_id ?? '—'}</td>
                <td>{entry.priority ?? '—'}</td>
                <td><span class="status-badge">{entry.status ?? 'queued'}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}
  </div>
</div>

<style>
  .code-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* Clone URL bar */
  .clone-url-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow: hidden;
  }

  .clone-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }

  .clone-url-text {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .clone-copy-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .clone-copy-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .clone-copy-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .commit-msg {
    max-width: 400px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subtab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .subtab-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .subtab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .subtab-btn:not(.active):hover {
    color: var(--color-text);
  }

  .filter-bar {
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .filter-input {
    width: 100%;
    max-width: 320px;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    outline: none;
  }

  .filter-input:focus:not(:focus-visible) { outline: none; border-color: var(--color-focus); }
  .filter-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }
  .filter-input::-webkit-search-cancel-button { display: none; }

  .table-wrap {
    flex: 1;
    overflow-y: auto;
    padding: 0;
  }

  .code-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .code-table thead {
    position: sticky;
    top: 0;
    background: var(--color-surface-elevated);
    z-index: 1;
  }

  .code-table th {
    padding: var(--space-2) var(--space-4);
    text-align: left;
    font-weight: 600;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
    padding: 0;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .table-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .table-row:hover {
    background: var(--color-surface-hover);
  }

  .table-row td {
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
  }

  .mono { font-family: var(--font-mono); }
  .secondary { color: var(--color-text-secondary); }

  .status-badge {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    color: var(--color-text-secondary);
  }

  .status-badge.status-open { background: color-mix(in srgb, var(--color-success) 10%, transparent); border-color: color-mix(in srgb, var(--color-success) 40%, transparent); color: var(--color-success); }
  .status-badge.status-merged { background: color-mix(in srgb, var(--color-info) 10%, transparent); border-color: color-mix(in srgb, var(--color-info) 40%, transparent); color: var(--color-info); }
  .status-badge.status-closed { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border-color: color-mix(in srgb, var(--color-danger) 40%, transparent); color: var(--color-danger); }

  .table-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .sort-btn:focus-visible,
  .subtab-btn:focus-visible,
  .filter-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    font-family: var(--font-body);
    white-space: nowrap;
  }
  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }
  .retry-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .subtab-btn, .sort-btn, .table-row { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
