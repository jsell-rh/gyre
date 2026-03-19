<script>
  import { api } from '../lib/api.js';

  let { repo, onBack, onSelectMr } = $props();

  let tab = $state('branches');
  let branches = $state([]);
  let commits = $state([]);
  let mrs = $state([]);
  let selectedBranch = $state(repo.default_branch || 'main');
  let loading = $state(false);
  let error = $state(null);
  let cloneCopied = $state(false);

  // jj state
  let jjChanges = $state([]);
  let jjLoading = $state(false);
  let jjError = $state(null);
  let jjInitLoading = $state(false);
  let jjInitMsg = $state(null);

  // Build clone URL from current window location
  const cloneUrl = `${window.location.origin}/git/${repo.project_id}/${repo.name}.git`;

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
    if (tab === 'commits') loadCommits(selectedBranch);
  });

  $effect(() => {
    if (tab === 'jj') loadJjLog();
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

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }

  function shortSha(sha) {
    return sha ? sha.slice(0, 8) : '—';
  }

  const statusColors = {
    open: '#60a5fa',
    approved: '#4ade80',
    merged: '#a78bfa',
    closed: '#94a3b8',
  };
</script>

<div class="panel">
  <div class="panel-header">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack}>← Projects</button>
      <span class="sep">/</span>
      <span class="repo-name">{repo.name}</span>
    </div>
    <span class="default-branch">default: {repo.default_branch}</span>
  </div>

  <div class="clone-bar">
    <span class="clone-label">Clone</span>
    <code class="clone-url-text">{cloneUrl}</code>
    <button class="copy-btn" onclick={copyCloneUrl}>{cloneCopied ? 'Copied!' : 'Copy'}</button>
  </div>

  <div class="tabs">
    <button class="tab" class:active={tab === 'branches'} onclick={() => (tab = 'branches')}>
      Branches {branches.length ? `(${branches.length})` : ''}
    </button>
    <button class="tab" class:active={tab === 'commits'} onclick={() => (tab = 'commits')}>
      Commits
    </button>
    <button class="tab" class:active={tab === 'mrs'} onclick={() => (tab = 'mrs')}>
      Merge Requests {mrs.length ? `(${mrs.length})` : ''}
    </button>
    <button class="tab" class:active={tab === 'jj'} onclick={() => (tab = 'jj')}>
      jj
    </button>
  </div>

  <div class="content">
    {#if error}
      <p class="state-msg error">{error}</p>
    {:else if loading}
      <p class="state-msg">Loading…</p>
    {:else if tab === 'branches'}
      {#if branches.length === 0}
        <p class="state-msg muted">No branches found.</p>
      {:else}
        <ul class="list">
          {#each branches as b (b.name)}
            <li class="list-item">
              <span class="branch-name">{b.name}</span>
              {#if b.name === repo.default_branch}
                <span class="badge default">default</span>
              {/if}
              <span class="sha">{shortSha(b.sha)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {:else if tab === 'commits'}
      <div class="commits-toolbar">
        <label class="branch-label">Branch:
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
        <p class="state-msg muted">No commits found for <code>{selectedBranch}</code>.</p>
      {:else}
        <ul class="list commits">
          {#each commits as c (c.sha)}
            <li class="list-item commit-item">
              <code class="sha">{shortSha(c.sha)}</code>
              <span class="commit-msg">{c.message}</span>
              <span class="commit-meta">{c.author} · {formatDate(c.timestamp)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {:else if tab === 'mrs'}
      {#if mrs.length === 0}
        <p class="state-msg muted">No merge requests.</p>
      {:else}
        <ul class="list">
          {#each mrs as mr (mr.id)}
            <li>
              <button class="list-item mr-item" onclick={() => onSelectMr(mr)}>
                <div class="mr-title">{mr.title}</div>
                <div class="mr-meta">
                  <span class="branch-ref">{mr.source_branch} → {mr.target_branch}</span>
                  <span class="status-badge" style:color={statusColors[mr.status] ?? 'var(--text-muted)'}>{mr.status}</span>
                </div>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {:else if tab === 'jj'}
      <div class="jj-toolbar">
        <button class="jj-init-btn" onclick={initJj} disabled={jjInitLoading}>
          {jjInitLoading ? 'Initializing…' : 'Init jj'}
        </button>
        <button class="jj-refresh-btn" onclick={loadJjLog} disabled={jjLoading}>Refresh</button>
        {#if jjInitMsg}
          <span class="jj-success">{jjInitMsg}</span>
        {/if}
      </div>
      {#if jjError}
        <p class="state-msg error">{jjError}</p>
      {:else if jjLoading}
        <p class="state-msg">Loading jj changes…</p>
      {:else if jjChanges.length === 0}
        <p class="state-msg muted">No jj changes found. Initialize jj first.</p>
      {:else}
        <ul class="list commits">
          {#each jjChanges as c (c.change_id)}
            <li class="list-item commit-item">
              <code class="sha">{c.change_id.slice(0, 8)}</code>
              <span class="commit-msg">{c.description || '(no description)'}</span>
              <span class="commit-meta">{c.author}{c.bookmarks.length ? ' · ' + c.bookmarks.join(', ') : ''}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 0.75rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  .breadcrumb { display: flex; align-items: center; gap: 0.5rem; }
  .back-btn {
    background: none; border: none; color: var(--accent); cursor: pointer;
    font-size: 0.88rem; padding: 0; line-height: 1;
  }
  .back-btn:hover { text-decoration: underline; }
  .sep { color: var(--text-dim); }
  .repo-name { font-weight: 600; color: var(--text); font-size: 0.95rem; }
  .default-branch { font-size: 0.78rem; color: var(--text-dim); }

  .clone-bar {
    display: flex; align-items: center; gap: 0.6rem;
    padding: 0.5rem 1.25rem; background: var(--surface); border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .clone-label { font-size: 0.75rem; color: var(--text-dim); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .clone-url-text {
    flex: 1; font-family: 'Courier New', monospace; font-size: 0.78rem; color: var(--text-muted);
    background: var(--bg); border: 1px solid var(--border-subtle); border-radius: 3px;
    padding: 0.2rem 0.5rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .copy-btn {
    background: none; border: 1px solid var(--border); border-radius: 4px; color: var(--accent);
    font-size: 0.75rem; padding: 0.2rem 0.6rem; cursor: pointer; white-space: nowrap;
  }
  .copy-btn:hover { background: var(--surface-hover); }

  .tabs {
    display: flex; gap: 0; border-bottom: 1px solid var(--border);
    padding: 0 1.25rem; flex-shrink: 0;
  }
  .tab {
    background: none; border: none; border-bottom: 2px solid transparent;
    color: var(--text-muted); cursor: pointer; font-size: 0.88rem;
    padding: 0.6rem 1rem; margin-bottom: -1px;
    transition: color 0.1s, border-color 0.1s;
  }
  .tab:hover { color: var(--text); }
  .tab.active { color: var(--accent); border-bottom-color: var(--accent); }

  .content { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; }

  .commits-toolbar { margin-bottom: 0.75rem; }
  .branch-label { font-size: 0.83rem; color: var(--text-muted); display: flex; align-items: center; gap: 0.5rem; }
  .branch-select {
    background: var(--surface); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-size: 0.83rem; padding: 0.25rem 0.5rem;
  }

  .list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.35rem; }

  .list-item {
    display: flex; align-items: center; gap: 0.75rem;
    padding: 0.6rem 0.75rem; border-radius: 5px;
    background: var(--surface); border: 1px solid var(--border-subtle);
    font-size: 0.85rem;
  }

  .branch-name { color: var(--text); font-weight: 500; flex: 1; }
  .badge { font-size: 0.72rem; padding: 0.1rem 0.45rem; border-radius: 3px; }
  .badge.default { background: var(--accent-muted); color: var(--accent); }
  .sha { font-family: 'Courier New', monospace; font-size: 0.78rem; color: var(--text-dim); }

  .commit-item { flex-direction: column; align-items: flex-start; gap: 0.2rem; }
  .commit-msg { color: var(--text); width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .commit-meta { font-size: 0.75rem; color: var(--text-dim); }

  .mr-item {
    flex-direction: column; align-items: flex-start; gap: 0.2rem; cursor: pointer;
    width: 100%; text-align: left; color: inherit; font: inherit;
  }
  .mr-item:hover { border-color: var(--accent); background: var(--surface-hover); }
  .mr-title { color: var(--text); font-weight: 500; }
  .mr-meta { display: flex; gap: 1rem; align-items: center; font-size: 0.78rem; }
  .branch-ref { color: var(--text-dim); }
  .status-badge { font-weight: 600; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.04em; }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }

  .jj-toolbar {
    display: flex; align-items: center; gap: 0.5rem; margin-bottom: 0.75rem; flex-wrap: wrap;
  }
  .jj-init-btn, .jj-refresh-btn {
    background: var(--surface); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-size: 0.82rem; padding: 0.3rem 0.7rem; cursor: pointer;
  }
  .jj-init-btn { background: var(--accent); color: #fff; border-color: var(--accent); }
  .jj-init-btn:hover { opacity: 0.88; }
  .jj-init-btn:disabled, .jj-refresh-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .jj-success { font-size: 0.82rem; color: #4ade80; }
</style>
