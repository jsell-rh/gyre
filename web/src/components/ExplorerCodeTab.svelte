<script>
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { repoId = null, repo = null } = $props();

  const openDetailPanel = getContext('openDetailPanel');

  let subTab = $state('branches');
  const SUB_TABS = [
    { id: 'branches', labelKey: 'code_tab.branches' },
    { id: 'commits', labelKey: 'code_tab.commits' },
    { id: 'merge-requests', labelKey: 'code_tab.merge_requests' },
    { id: 'merge-queue', labelKey: 'code_tab.merge_queue' },
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
        const [all, mrList] = await Promise.all([
          api.mergeQueue(),
          api.mergeRequests({ repository_id: repoId }),
        ]);
        const mrMap = Object.fromEntries((Array.isArray(mrList) ? mrList : []).map(m => [m.id, m]));
        queue = (Array.isArray(all) ? all : [])
          .filter(e => e.repository_id === repoId || e.repo_id === repoId)
          .map(e => {
            const mrId = e.merge_request_id ?? e.mr_id;
            const mr = mrMap[mrId];
            return { ...e, _mr_title: mr?.title, _mr_status: mr?.status, _mr_branch: mr?.source_branch };
          });
      }
    } catch (e) {
      error = $t('code_tab.load_failed', { values: { tab, error: e.message } });
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

  let filteredCommits = $derived.by(() => {
    let rows = commits.filter(matchesFilter);
    rows.sort((a, b) => {
      let av, bv;
      if (sortField === 'sha') { av = a.sha ?? a.id ?? ''; bv = b.sha ?? b.id ?? ''; }
      else if (sortField === 'message') { av = a.message ?? a.summary ?? ''; bv = b.message ?? b.summary ?? ''; }
      else if (sortField === 'author') { av = a.author ?? a.author_name ?? ''; bv = b.author ?? b.author_name ?? ''; }
      else if (sortField === 'date') { av = a.timestamp ?? a.authored_at ?? a.date ?? ''; bv = b.timestamp ?? b.authored_at ?? b.date ?? ''; }
      else { av = a[sortField] ?? ''; bv = b[sortField] ?? ''; }
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredQueue = $derived.by(() => {
    let rows = queue.filter(matchesFilter);
    rows.sort((a, b) => {
      let av, bv;
      if (sortField === 'mr') { av = a.merge_request_id ?? a.mr_id ?? ''; bv = b.merge_request_id ?? b.mr_id ?? ''; }
      else if (sortField === 'priority') { av = a.priority ?? 0; bv = b.priority ?? 0; return sortDir === 'asc' ? av - bv : bv - av; }
      else { av = a[sortField] ?? ''; bv = b[sortField] ?? ''; }
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  function relativeTime(ts) {
    if (!ts) return '';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60) return $t('code_tab.time_just_now');
    if (diff < 3600) return $t('code_tab.time_minutes_ago', { values: { count: Math.floor(diff / 60) } });
    if (diff < 86400) return $t('code_tab.time_hours_ago', { values: { count: Math.floor(diff / 3600) } });
    return $t('code_tab.time_days_ago', { values: { count: Math.floor(diff / 86400) } });
  }
</script>

<div class="code-tab">
  <span class="sr-only" aria-live="polite">{loading ? "" : $t('code_tab.loaded')}</span>

  <!-- Clone URL header -->
  {#if cloneUrl}
    <div class="clone-url-bar">
      <span class="clone-label">{$t('code_tab.clone')}</span>
      <code class="clone-url-text" title={cloneUrl}>{cloneUrl}</code>
      <button class="clone-copy-btn" onclick={copyCloneUrl} aria-label={$t('code_tab.copy_clone_url')} title={$t('code_tab.copy_clone_url')}>
        {#if cloneCopied}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>
          {$t('code_tab.copied')}
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
          {$t('code_tab.copy')}
        {/if}
      </button>
    </div>
  {/if}

  <!-- Sub-tab bar -->
  <div class="subtab-bar" role="tablist" aria-label={$t('code_tab.sub_tabs_label')}>
    {#each SUB_TABS as st}
      <button
        class="subtab-btn {subTab === st.id ? 'active' : ''}"
        role="tab"
        aria-selected={subTab === st.id}
        onclick={() => switchSubTab(st.id)}
        type="button"
      >{$t(st.labelKey)}</button>
    {/each}
  </div>

  <!-- Filter input -->
  <div class="filter-bar">
    <input
      type="search"
      class="filter-input"
      placeholder={$t('code_tab.filter_placeholder', { values: { tab: $t(SUB_TABS.find(st => st.id === subTab)?.labelKey ?? '') } })}
      bind:value={filterQuery}
      aria-label={$t('code_tab.filter_list')}
    />
  </div>

  <!-- Content -->
  <div class="table-wrap" role="tabpanel" aria-busy={loading}>
    {#if error}
      <div class="error-banner" role="alert">
        <span>{error}</span>
        <button class="retry-btn" onclick={() => { error = null; loadTab(subTab); }}>{$t('common.retry')}</button>
      </div>
    {:else if loading}
      <Skeleton lines={6} />
    {:else if subTab === 'branches'}
      {#if filteredBranches.length === 0}
        <EmptyState title={$t('code_tab.no_branches')} message={filterQuery ? $t('code_tab.no_branches_filter') : $t('code_tab.no_branches_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('name')}>{$t('code_tab.col_name')} {sortIcon('name')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('last_commit')}>{$t('code_tab.col_last_commit')} {sortIcon('last_commit')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author')}>{$t('code_tab.col_author')} {sortIcon('author')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('status')}>{$t('code_tab.col_status')} {sortIcon('status')}</button></th>
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
        <EmptyState title={$t('code_tab.no_commits')} message={filterQuery ? $t('code_tab.no_commits_filter') : $t('code_tab.no_commits_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('sha')}>{$t('code_tab.col_sha')} {sortIcon('sha')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('message')}>{$t('code_tab.col_message')} {sortIcon('message')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author')}>{$t('code_tab.col_author')} {sortIcon('author')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('date')}>{$t('code_tab.col_date')} {sortIcon('date')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredCommits as commit}
              <tr class="table-row" onclick={() => onRowClick(commit, 'commit')} tabindex="0" role="button" aria-label="Commit {commit.sha ?? commit.id ?? ''}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(commit, 'commit'); } }}>
                <td class="mono">{(commit.sha ?? commit.id ?? '').slice(0, 7)}</td>
                <td class="commit-msg" title={commit.message ?? commit.summary ?? ''}>{commit.message ?? commit.summary ?? '—'}</td>
                <td class="secondary">{commit.author ?? commit.author_name ?? '—'}</td>
                <td class="secondary">{relativeTime(commit.timestamp ?? commit.authored_at ?? commit.date)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'merge-requests'}
      {#if filteredMrs.length === 0}
        <EmptyState title={$t('code_tab.no_mrs')} message={filterQuery ? $t('code_tab.no_mrs_filter') : $t('code_tab.no_mrs_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('title')}>{$t('code_tab.col_title')} {sortIcon('title')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('source_branch')}>Branch {sortIcon('source_branch')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('status')}>{$t('code_tab.col_status')} {sortIcon('status')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('updated_at')}>{$t('code_tab.col_updated')} {sortIcon('updated_at')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredMrs as mr}
              <tr class="table-row" onclick={() => onRowClick(mr, 'mr')} tabindex="0" role="button" aria-label="View MR {mr.title}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(mr, 'mr'); } }}>
                <td>
                  <div class="mr-title-cell">
                    <span title={mr.title}>{mr.title}</span>
                    {#if mr.spec_ref}
                      <span class="mr-spec-ref" title={mr.spec_ref}>{mr.spec_ref.split('@')[0]?.split('/').pop()}</span>
                    {/if}
                  </div>
                </td>
                <td class="mono secondary">{mr.source_branch ?? '—'}</td>
                <td><span class="status-badge status-{mr.status}">{mr.status}</span></td>
                <td class="secondary">
                  <div class="mr-updated-cell">
                    <span>{relativeTime(mr.updated_at)}</span>
                    {#if mr.diff_stats}
                      <span class="mr-diff-stats">
                        <span class="mr-diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="mr-diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      </span>
                    {/if}
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'merge-queue'}
      {#if filteredQueue.length === 0}
        <EmptyState title={$t('code_tab.queue_empty')} message={filterQuery ? $t('code_tab.no_queue_filter') : $t('code_tab.no_queue_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('mr')}>Merge Request {sortIcon('mr')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('priority')}>{$t('code_tab.col_priority')} {sortIcon('priority')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('status')}>{$t('code_tab.col_status')} {sortIcon('status')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredQueue as entry}
              {@const mrId = entry.merge_request_id ?? entry.mr_id}
              <tr class="table-row" onclick={() => onRowClick({ id: mrId, ...entry }, 'mr')} tabindex="0" role="button" aria-label={entry._mr_title ? `View MR: ${entry._mr_title}` : $t('code_tab.view_queue_entry')} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick({ id: mrId, ...entry }, 'mr'); } }}>
                <td>
                  <div class="queue-mr-cell">
                    <span>{entry._mr_title ?? (mrId ? mrId.slice(0, 8) + '...' : '—')}</span>
                    {#if entry._mr_branch}
                      <span class="queue-branch mono">{entry._mr_branch}</span>
                    {/if}
                  </div>
                </td>
                <td><span class="priority-pill priority-{entry.priority <= 25 ? 'high' : entry.priority <= 75 ? 'normal' : 'low'}">P{entry.priority ?? '—'}</span></td>
                <td><span class="status-badge status-{entry._mr_status ?? ''}">{entry.status ?? 'queued'}</span></td>
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

  /* MR title cell with spec ref */
  .mr-title-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mr-spec-ref {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
    border-radius: var(--radius-sm);
    width: fit-content;
  }

  /* Queue MR cell */
  .queue-mr-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .queue-branch {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Priority pill */
  .priority-pill {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .priority-high {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
  }

  .priority-normal {
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    color: var(--color-warning);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
  }

  .priority-low {
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  /* MR updated cell with diff stats */
  .mr-updated-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mr-diff-stats {
    display: flex;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .mr-diff-ins { color: var(--color-success); }
  .mr-diff-del { color: var(--color-danger); }

  @media (prefers-reduced-motion: reduce) {
    .subtab-btn, .sort-btn, .table-row { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
