<script>
  import { api } from '../lib/api.js';
  import Tabs from '../lib/Tabs.svelte';
  import Table from '../lib/Table.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';

  let { repo, onBack, onSelectMr } = $props();

  let activeTab = $state('branches');
  let branches = $state([]);
  let commits = $state([]);
  let mrs = $state([]);
  let selectedBranch = $state(repo.default_branch || 'main');
  let loading = $state(false);
  let error = $state(null);
  let cloneCopied = $state(false);

  let jjChanges = $state([]);
  let jjLoading = $state(false);
  let jjError = $state(null);
  let jjInitLoading = $state(false);
  let jjInitMsg = $state(null);

  const cloneUrl = `${window.location.origin}/git/${repo.project_id}/${repo.name}.git`;

  const tabs = $derived([
    { id: 'branches', label: 'Branches', count: branches.length || undefined },
    { id: 'commits',  label: 'Commits' },
    { id: 'mrs',      label: 'Merge Requests', count: mrs.length || undefined },
    { id: 'jj',       label: 'jj' },
  ]);

  async function copyCloneUrl() {
    try {
      await navigator.clipboard.writeText(cloneUrl);
      cloneCopied = true;
      setTimeout(() => { cloneCopied = false; }, 2000);
    } catch { /* clipboard not available */ }
  }

  $effect(() => {
    loadBranches();
    loadMrs();
  });

  $effect(() => {
    if (activeTab === 'commits') loadCommits(selectedBranch);
  });

  $effect(() => {
    if (activeTab === 'jj') loadJjLog();
  });

  async function loadJjLog() {
    jjLoading = true; jjError = null;
    try {
      jjChanges = await api.jjLog(repo.id);
    } catch (e) {
      jjError = e.message;
    } finally {
      jjLoading = false;
    }
  }

  async function initJj() {
    jjInitLoading = true; jjInitMsg = null; jjError = null;
    try {
      await api.jjInit(repo.id);
      jjInitMsg = 'jj initialized successfully.';
      await loadJjLog();
    } catch (e) {
      jjError = e.message;
    } finally {
      jjInitLoading = false;
    }
  }

  async function loadBranches() {
    loading = true; error = null;
    try {
      branches = await api.repoBranches(repo.id);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadCommits(branch) {
    loading = true; error = null;
    try {
      commits = await api.repoCommits(repo.id, branch);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadMrs() {
    try {
      mrs = await api.mergeRequests({ repository_id: repo.id });
    } catch { mrs = []; }
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

  function shortSha(sha) {
    return sha ? sha.slice(0, 8) : '—';
  }
</script>

<div class="page">
  <div class="page-hdr">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack}>← Projects</button>
      <span class="sep">/</span>
      <h1 class="repo-name">{repo.name}</h1>
    </div>
    <span class="default-badge">default: {repo.default_branch}</span>
  </div>

  <div class="clone-bar">
    <span class="clone-label">Clone</span>
    <code class="clone-url-text">{cloneUrl}</code>
    <button class="copy-btn" onclick={copyCloneUrl}>{cloneCopied ? 'Copied!' : 'Copy'}</button>
  </div>

  <div class="tabs-wrap">
    <Tabs {tabs} bind:active={activeTab} />
  </div>

  <div class="tab-content">
    {#if error}
      <div class="error-msg">Error: {error}</div>
    {:else if loading && (activeTab === 'branches' || activeTab === 'commits')}
      <Skeleton lines={8} height="2.5rem" />
    {:else if activeTab === 'branches'}
      {#if branches.length === 0}
        <EmptyState title="No branches" description="No branches found in this repository." />
      {:else}
        <Table
          columns={[
            { key: 'name', label: 'Branch', sortable: true },
            { key: 'sha', label: 'Head SHA' },
            { key: 'default', label: '' },
          ]}
        >
          {#snippet children()}
            {#each branches as b (b.name)}
              <tr>
                <td class="branch-name-cell">{b.name}</td>
                <td><code class="sha">{shortSha(b.sha)}</code></td>
                <td>
                  {#if b.name === repo.default_branch}
                    <Badge value="default" variant="info" />
                  {/if}
                </td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {:else if activeTab === 'commits'}
      <div class="commits-toolbar">
        <label class="branch-label">
          Branch:
          <select class="branch-select" bind:value={selectedBranch} onchange={() => loadCommits(selectedBranch)}>
            {#each branches as b (b.name)}
              <option value={b.name}>{b.name}</option>
            {/each}
            {#if branches.length === 0}
              <option value={selectedBranch}>{selectedBranch}</option>
            {/if}
          </select>
        </label>
      </div>
      {#if commits.length === 0}
        <EmptyState title="No commits" description="No commits found on branch {selectedBranch}." />
      {:else}
        <Table
          columns={[
            { key: 'sha', label: 'SHA' },
            { key: 'message', label: 'Message' },
            { key: 'author', label: 'Author' },
            { key: 'time', label: 'Time' },
          ]}
        >
          {#snippet children()}
            {#each commits as c (c.sha)}
              <tr>
                <td><code class="sha">{shortSha(c.sha)}</code></td>
                <td class="commit-msg-cell">{c.message}</td>
                <td class="secondary-cell">{c.author}</td>
                <td class="secondary-cell">{relativeTime(c.timestamp)}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {:else if activeTab === 'mrs'}
      {#if mrs.length === 0}
        <EmptyState title="No merge requests" description="No merge requests for this repository." />
      {:else}
        <Table
          columns={[
            { key: 'status', label: 'Status' },
            { key: 'title', label: 'Title' },
            { key: 'author', label: 'Author' },
            { key: 'branches', label: 'Branches' },
          ]}
        >
          {#snippet children()}
            {#each mrs as mr (mr.id)}
              <tr class="clickable" onclick={() => onSelectMr(mr)}>
                <td><Badge value={mr.status} /></td>
                <td class="mr-title-cell">{mr.title}</td>
                <td class="secondary-cell">{mr.author ?? '—'}</td>
                <td class="secondary-cell mono">{mr.source_branch} → {mr.target_branch}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {:else if activeTab === 'jj'}
      <div class="jj-toolbar">
        <button class="jj-btn primary" onclick={initJj} disabled={jjInitLoading}>
          {jjInitLoading ? 'Initializing…' : 'Init jj'}
        </button>
        <button class="jj-btn" onclick={loadJjLog} disabled={jjLoading}>Refresh</button>
        {#if jjInitMsg}
          <span class="jj-success">{jjInitMsg}</span>
        {/if}
      </div>
      {#if jjError}
        <div class="error-msg">{jjError}</div>
      {:else if jjLoading}
        <Skeleton lines={6} height="2.5rem" />
      {:else if jjChanges.length === 0}
        <EmptyState title="No jj changes" description="No jj changes found. Initialize jj first." />
      {:else}
        <Table
          columns={[
            { key: 'change_id', label: 'Change ID' },
            { key: 'description', label: 'Description' },
            { key: 'author', label: 'Author' },
            { key: 'bookmarks', label: 'Bookmarks' },
          ]}
        >
          {#snippet children()}
            {#each jjChanges as c (c.change_id)}
              <tr>
                <td><code class="sha">{c.change_id.slice(0, 8)}</code></td>
                <td class="commit-msg-cell">{c.description || '(no description)'}</td>
                <td class="secondary-cell">{c.author}</td>
                <td class="secondary-cell">{c.bookmarks.join(', ') || '—'}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {/if}
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .page-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .breadcrumb { display: flex; align-items: center; gap: var(--space-2); }

  .back-btn {
    background: none;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: 0;
    transition: color var(--transition-fast);
  }

  .back-btn:hover { color: var(--color-link-hover); }

  .sep { color: var(--color-text-muted); }

  .repo-name {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .default-badge {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 0.15rem 0.5rem;
  }

  .clone-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .clone-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .clone-url-text {
    flex: 1;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 0.2rem var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .copy-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-link);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: 0.2rem var(--space-3);
    cursor: pointer;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .copy-btn:hover { background: var(--color-surface-elevated); }

  .tabs-wrap { flex-shrink: 0; }

  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .commits-toolbar { flex-shrink: 0; }

  .branch-label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .branch-select {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
  }

  .sha {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .branch-name-cell {
    font-weight: 500;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .commit-msg-cell {
    color: var(--color-text);
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mr-title-cell { color: var(--color-text); font-weight: 500; }

  .secondary-cell { color: var(--color-text-secondary); font-size: var(--text-xs); }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  .clickable { cursor: pointer; }

  .jj-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .jj-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .jj-btn:hover { background: var(--color-surface-elevated); }
  .jj-btn.primary { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
  .jj-btn.primary:hover { background: var(--color-primary-hover); }
  .jj-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .jj-success { font-size: var(--text-sm); color: var(--color-success); }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }
</style>
