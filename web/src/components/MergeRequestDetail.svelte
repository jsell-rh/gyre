<script>
  import { api } from '../lib/api.js';

  let { mr: initialMr, repo, onBack } = $props();

  let mr = $state(initialMr);
  let reviews = $state([]);
  let comments = $state([]);
  let loading = $state(true);
  let actionMsg = $state(null);
  let actionError = $state(null);
  let enqueueing = $state(false);

  $effect(() => {
    loadDetails();
  });

  async function loadDetails() {
    loading = true;
    try {
      [reviews, comments] = await Promise.all([
        api.mrReviews(mr.id),
        api.mrComments(mr.id),
      ]);
    } catch { /* ignore */ }
    loading = false;
  }

  async function submitReview(decision) {
    actionMsg = null; actionError = null;
    try {
      const review = await api.submitReview(mr.id, {
        reviewer_agent_id: 'dashboard',
        decision,
      });
      reviews = [...reviews, review];
      actionMsg = decision === 'approved' ? 'Approved.' : 'Changes requested.';
      mr = await api.mergeRequest(mr.id);
    } catch (e) {
      actionError = e.message;
    }
  }

  async function addToQueue() {
    enqueueing = true; actionMsg = null; actionError = null;
    try {
      await api.enqueue(mr.id);
      actionMsg = 'Added to merge queue.';
    } catch (e) {
      actionError = e.message;
    } finally {
      enqueueing = false;
    }
  }

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }

  const statusColors = {
    open: '#60a5fa',
    approved: '#4ade80',
    merged: '#a78bfa',
    closed: '#94a3b8',
  };

  const decisionColors = {
    approved: '#4ade80',
    changes_requested: '#f97316',
  };

  const decisionLabels = {
    approved: 'Approved',
    changes_requested: 'Changes Requested',
  };
</script>

<div class="panel">
  <div class="panel-header">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack}>← {repo?.name ?? 'Repo'}</button>
      <span class="sep">/</span>
      <span class="mr-title-header">{mr.title}</span>
    </div>
    <span class="status-badge" style:color={statusColors[mr.status] ?? 'var(--text-muted)'}>{mr.status}</span>
  </div>

  <div class="content">
    <!-- MR meta -->
    <div class="meta-card">
      <div class="meta-row">
        <span class="meta-label">Branches</span>
        <span class="meta-value branch-ref">{mr.source_branch} → {mr.target_branch}</span>
      </div>
      {#if mr.author_agent_id}
        <div class="meta-row">
          <span class="meta-label">Author</span>
          <span class="meta-value">{mr.author_agent_id}</span>
        </div>
      {/if}
      <div class="meta-row">
        <span class="meta-label">Created</span>
        <span class="meta-value">{formatDate(mr.created_at)}</span>
      </div>
      {#if mr.has_conflicts != null}
        <div class="meta-row">
          <span class="meta-label">Conflicts</span>
          <span class="meta-value" style:color={mr.has_conflicts ? '#f87171' : '#4ade80'}>
            {mr.has_conflicts ? 'Yes' : 'None'}
          </span>
        </div>
      {/if}
    </div>

    <!-- Diff stats -->
    {#if mr.diff_stats}
      <section class="section">
        <h3 class="section-title">Diff Stats</h3>
        <div class="diff-stats">
          <span class="diff-stat">{mr.diff_stats.files_changed} files changed</span>
          <span class="diff-stat ins">+{mr.diff_stats.insertions}</span>
          <span class="diff-stat del">-{mr.diff_stats.deletions}</span>
        </div>
      </section>
    {/if}

    <!-- Actions -->
    <section class="section">
      <h3 class="section-title">Actions</h3>
      <div class="actions">
        <button class="btn approve" onclick={() => submitReview('approved')} disabled={mr.status === 'merged' || mr.status === 'closed'}>
          Approve
        </button>
        <button class="btn changes" onclick={() => submitReview('changes_requested')} disabled={mr.status === 'merged' || mr.status === 'closed'}>
          Request Changes
        </button>
        {#if mr.status === 'approved'}
          <button class="btn enqueue" onclick={addToQueue} disabled={enqueueing}>
            {enqueueing ? 'Enqueueing…' : 'Add to Merge Queue'}
          </button>
        {/if}
      </div>
      {#if actionMsg}<p class="action-msg ok">{actionMsg}</p>{/if}
      {#if actionError}<p class="action-msg err">{actionError}</p>{/if}
    </section>

    <!-- Reviews -->
    <section class="section">
      <h3 class="section-title">Reviews {loading ? '' : `(${reviews.length})`}</h3>
      {#if loading}
        <p class="muted">Loading…</p>
      {:else if reviews.length === 0}
        <p class="muted">No reviews yet.</p>
      {:else}
        <ul class="review-list">
          {#each reviews as r (r.id)}
            <li class="review-item">
              <div class="review-header">
                <span class="reviewer">{r.reviewer_agent_id}</span>
                <span class="decision-badge" style:color={decisionColors[r.decision] ?? 'var(--text-muted)'}>
                  {decisionLabels[r.decision] ?? r.decision}
                </span>
                <span class="review-date">{formatDate(r.created_at)}</span>
              </div>
              {#if r.body}<p class="review-body">{r.body}</p>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- Comments -->
    <section class="section">
      <h3 class="section-title">Comments {loading ? '' : `(${comments.length})`}</h3>
      {#if loading}
        <p class="muted">Loading…</p>
      {:else if comments.length === 0}
        <p class="muted">No comments yet.</p>
      {:else}
        <ul class="comment-list">
          {#each comments as c (c.id)}
            <li class="comment-item">
              <div class="comment-header">
                <span class="commenter">{c.author_agent_id}</span>
                {#if c.file_path}
                  <code class="file-ref">{c.file_path}{c.line_number != null ? `:${c.line_number}` : ''}</code>
                {/if}
                <span class="comment-date">{formatDate(c.created_at)}</span>
              </div>
              <p class="comment-body">{c.body}</p>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 0.75rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  .breadcrumb { display: flex; align-items: center; gap: 0.5rem; overflow: hidden; }
  .back-btn {
    background: none; border: none; color: var(--accent); cursor: pointer;
    font-size: 0.88rem; padding: 0; white-space: nowrap;
  }
  .back-btn:hover { text-decoration: underline; }
  .sep { color: var(--text-dim); flex-shrink: 0; }
  .mr-title-header {
    font-weight: 600; color: var(--text); font-size: 0.92rem;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }

  .status-badge {
    font-weight: 600; font-size: 0.75rem; text-transform: uppercase;
    letter-spacing: 0.05em; flex-shrink: 0;
  }

  .content { flex: 1; overflow-y: auto; padding: 1rem 1.25rem; display: flex; flex-direction: column; gap: 1rem; }

  .meta-card {
    background: var(--surface); border: 1px solid var(--border); border-radius: 6px;
    padding: 0.75rem 1rem; display: flex; flex-direction: column; gap: 0.4rem;
  }
  .meta-row { display: flex; gap: 1rem; align-items: baseline; font-size: 0.85rem; }
  .meta-label { color: var(--text-dim); width: 5rem; flex-shrink: 0; }
  .meta-value { color: var(--text); }
  .branch-ref { font-family: 'Courier New', monospace; font-size: 0.82rem; color: var(--text-muted); }

  .section { display: flex; flex-direction: column; gap: 0.5rem; }
  .section-title {
    font-size: 0.8rem; font-weight: 600; color: var(--text-dim);
    text-transform: uppercase; letter-spacing: 0.05em; margin: 0;
  }

  .diff-stats { display: flex; gap: 1rem; align-items: center; font-size: 0.88rem; }
  .diff-stat { color: var(--text-muted); }
  .diff-stat.ins { color: #4ade80; font-weight: 600; }
  .diff-stat.del { color: #f87171; font-weight: 600; }

  .actions { display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .btn {
    padding: 0.45rem 1rem; border-radius: 5px; border: 1px solid; cursor: pointer;
    font-size: 0.85rem; font-weight: 500; transition: opacity 0.1s;
  }
  .btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn.approve { background: #16a34a22; border-color: #4ade80; color: #4ade80; }
  .btn.approve:hover:not(:disabled) { background: #16a34a44; }
  .btn.changes { background: #ea580c22; border-color: #f97316; color: #f97316; }
  .btn.changes:hover:not(:disabled) { background: #ea580c44; }
  .btn.enqueue { background: var(--accent-muted); border-color: var(--accent); color: var(--accent); }
  .btn.enqueue:hover:not(:disabled) { background: #60a5fa33; }

  .action-msg { font-size: 0.83rem; margin: 0; }
  .action-msg.ok { color: #4ade80; }
  .action-msg.err { color: #f87171; }

  .review-list, .comment-list {
    list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.4rem;
  }

  .review-item, .comment-item {
    background: var(--surface); border: 1px solid var(--border-subtle);
    border-radius: 5px; padding: 0.6rem 0.75rem;
  }

  .review-header, .comment-header {
    display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.25rem;
    font-size: 0.82rem; flex-wrap: wrap;
  }
  .reviewer, .commenter { color: var(--text); font-weight: 500; }
  .decision-badge { font-weight: 600; font-size: 0.75rem; }
  .review-date, .comment-date { color: var(--text-dim); margin-left: auto; font-size: 0.75rem; }
  .file-ref {
    font-family: 'Courier New', monospace; font-size: 0.75rem;
    color: var(--accent); background: var(--accent-muted);
    padding: 0.1rem 0.3rem; border-radius: 3px;
  }
  .review-body, .comment-body {
    margin: 0; font-size: 0.84rem; color: var(--text-muted);
    white-space: pre-wrap; word-break: break-word;
  }

  .muted { color: var(--text-dim); font-style: italic; font-size: 0.83rem; margin: 0; }
</style>
