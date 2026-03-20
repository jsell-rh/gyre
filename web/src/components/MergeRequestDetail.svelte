<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { mr: initialMr, repo, onBack } = $props();

  let mr = $state(initialMr);
  let reviews = $state([]);
  let comments = $state([]);
  let loading = $state(true);
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
    try {
      const review = await api.submitReview(mr.id, {
        reviewer_agent_id: 'dashboard',
        decision,
      });
      reviews = [...reviews, review];
      mr = await api.mergeRequest(mr.id);
      toastSuccess(decision === 'approved' ? 'MR approved.' : 'Changes requested.');
    } catch (e) {
      toastError(e.message);
    }
  }

  async function addToQueue() {
    enqueueing = true;
    try {
      await api.enqueue(mr.id);
      toastSuccess('Added to merge queue.');
    } catch (e) {
      toastError(e.message);
    } finally {
      enqueueing = false;
    }
  }

  function relativeTime(ts) {
    const now = Date.now();
    const diff = Math.floor((now - ts * 1000) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }

  // Status timeline steps
  const TIMELINE_STEPS = ['created', 'reviewed', 'approved', 'queued', 'merged'];

  let timelineStep = $derived(() => {
    if (mr.status === 'merged') return 4;
    if (mr.status === 'approved') return reviews.length > 0 ? 2 : 1;
    if (reviews.length > 0) return 1;
    return 0;
  });
</script>

<div class="panel">
  <!-- Header -->
  <div class="panel-header">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M19 12H5M12 5l-7 7 7 7"/></svg>
        {repo?.name ?? 'Repo'}
      </button>
      <span class="sep">/</span>
      <span class="mr-title-header">{mr.title}</span>
    </div>
    <Badge value={mr.status} />
  </div>

  <div class="content">
    <!-- Two-column layout -->
    <div class="two-col">
      <!-- Sidebar: meta info -->
      <aside class="info-sidebar">
        <div class="sidebar-card">
          <h4 class="sidebar-section-title">Details</h4>

          <div class="meta-row">
            <span class="meta-label">Status</span>
            <Badge value={mr.status} />
          </div>

          {#if mr.author_agent_id}
            <div class="meta-row">
              <span class="meta-label">Author</span>
              <span class="meta-value">{mr.author_agent_id}</span>
            </div>
          {/if}

          <div class="meta-row">
            <span class="meta-label">Branches</span>
            <span class="meta-value branch-ref">{mr.source_branch} → {mr.target_branch}</span>
          </div>

          <div class="meta-row">
            <span class="meta-label">Created</span>
            <span class="meta-value">{formatDate(mr.created_at)}</span>
          </div>

          {#if mr.has_conflicts != null}
            <div class="meta-row">
              <span class="meta-label">Conflicts</span>
              <span class="meta-value" style:color={mr.has_conflicts ? 'var(--color-danger)' : 'var(--color-success)'}>
                {mr.has_conflicts ? 'Yes' : 'None'}
              </span>
            </div>
          {/if}

          {#if mr.diff_stats}
            <div class="meta-divider"></div>
            <h4 class="sidebar-section-title">Changes</h4>
            <div class="diff-stats">
              <span class="diff-files">{mr.diff_stats.files_changed} files</span>
              <span class="diff-ins">+{mr.diff_stats.insertions}</span>
              <span class="diff-del">-{mr.diff_stats.deletions}</span>
            </div>
          {/if}
        </div>

        <!-- Actions -->
        <div class="sidebar-card">
          <h4 class="sidebar-section-title">Actions</h4>
          <div class="action-group">
            <button
              class="action-btn approve"
              onclick={() => submitReview('approved')}
              disabled={mr.status === 'merged' || mr.status === 'closed'}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M20 6L9 17l-5-5"/></svg>
              Approve
            </button>
            <button
              class="action-btn changes"
              onclick={() => submitReview('changes_requested')}
              disabled={mr.status === 'merged' || mr.status === 'closed'}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>
              Request Changes
            </button>
            {#if mr.status === 'approved'}
              <button class="action-btn enqueue" onclick={addToQueue} disabled={enqueueing}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M3 12h18M3 6h18M3 18h12"/></svg>
                {enqueueing ? 'Adding…' : 'Add to Queue'}
              </button>
            {/if}
          </div>
        </div>
      </aside>

      <!-- Main content -->
      <div class="main-content">
        <!-- Status Timeline -->
        <div class="timeline-card">
          <div class="timeline">
            {#each TIMELINE_STEPS as step, i}
              <div class="timeline-step" class:done={i <= timelineStep()} class:active={i === timelineStep()}>
                <div class="timeline-dot"></div>
                <span class="timeline-label">{step}</span>
              </div>
              {#if i < TIMELINE_STEPS.length - 1}
                <div class="timeline-line" class:done={i < timelineStep()}></div>
              {/if}
            {/each}
          </div>
        </div>

        <!-- Reviews -->
        <section class="section">
          <h3 class="section-title">
            Reviews
            {#if !loading}<span class="section-count">({reviews.length})</span>{/if}
          </h3>

          {#if loading}
            <div class="skeleton-group">
              <Skeleton height="4rem" />
              <Skeleton height="4rem" />
            </div>
          {:else if reviews.length === 0}
            <EmptyState
              title="No reviews yet"
              description="Use the Approve or Request Changes buttons to add a review."
            />
          {:else}
            <div class="review-list">
              {#each reviews as r (r.id)}
                <div class="review-card">
                  <div class="review-header">
                    <span class="reviewer-name">{r.reviewer_agent_id}</span>
                    <Badge value={r.decision} />
                    <span class="review-time">{relativeTime(r.created_at)}</span>
                  </div>
                  {#if r.body}
                    <p class="review-body">{r.body}</p>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <!-- Comments -->
        <section class="section">
          <h3 class="section-title">
            Comments
            {#if !loading}<span class="section-count">({comments.length})</span>{/if}
          </h3>

          {#if loading}
            <div class="skeleton-group">
              <Skeleton height="3.5rem" />
              <Skeleton height="3.5rem" />
            </div>
          {:else if comments.length === 0}
            <EmptyState
              title="No comments yet"
              description="Comments from agents will appear here."
            />
          {:else}
            <div class="comment-list">
              {#each comments as c (c.id)}
                <div class="comment-card">
                  <div class="comment-header">
                    <span class="commenter-name">{c.author_agent_id}</span>
                    {#if c.file_path}
                      <code class="file-ref">{c.file_path}{c.line_number != null ? `:${c.line_number}` : ''}</code>
                    {/if}
                    <span class="comment-time">{relativeTime(c.created_at)}</span>
                  </div>
                  <p class="comment-body">{c.body}</p>
                </div>
              {/each}
            </div>
          {/if}
        </section>
      </div>
    </div>
  </div>
</div>

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
    padding: var(--space-3) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    overflow: hidden;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }
  .back-btn:hover { color: var(--color-link-hover); }

  .sep { color: var(--color-text-muted); flex-shrink: 0; }

  .mr-title-header {
    font-family: var(--font-display);
    font-weight: 600;
    color: var(--color-text);
    font-size: var(--text-sm);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
  }

  /* Two-column layout */
  .two-col {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: var(--space-6);
    align-items: start;
  }

  /* Sidebar */
  .info-sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .sidebar-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .sidebar-section-title {
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0;
  }

  .meta-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .meta-label {
    color: var(--color-text-muted);
    flex-shrink: 0;
    font-size: var(--text-xs);
  }

  .meta-value {
    color: var(--color-text-secondary);
    text-align: right;
  }

  .branch-ref {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--color-text-muted);
  }

  .meta-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-1) 0;
  }

  .diff-stats {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
  }

  .diff-files { color: var(--color-text-secondary); }
  .diff-ins { color: var(--color-success); font-weight: 600; }
  .diff-del { color: var(--color-danger); font-weight: 600; }

  /* Action buttons */
  .action-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius);
    border: 1px solid;
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: 500;
    font-family: var(--font-body);
    transition: opacity var(--transition-fast), background var(--transition-fast);
    width: 100%;
  }

  .action-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .action-btn.approve {
    background: rgba(99, 153, 61, 0.12);
    border-color: rgba(99, 153, 61, 0.4);
    color: #7dc25a;
  }
  .action-btn.approve:hover:not(:disabled) {
    background: rgba(99, 153, 61, 0.22);
  }

  .action-btn.changes {
    background: rgba(245, 146, 27, 0.12);
    border-color: rgba(245, 146, 27, 0.4);
    color: var(--color-warning);
  }
  .action-btn.changes:hover:not(:disabled) {
    background: rgba(245, 146, 27, 0.22);
  }

  .action-btn.enqueue {
    background: rgba(0, 102, 204, 0.12);
    border-color: rgba(0, 102, 204, 0.4);
    color: var(--color-link);
  }
  .action-btn.enqueue:hover:not(:disabled) {
    background: rgba(0, 102, 204, 0.22);
  }

  /* Main content */
  .main-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  /* Timeline */
  .timeline-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-6);
  }

  .timeline {
    display: flex;
    align-items: center;
    gap: 0;
  }

  .timeline-step {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .timeline-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--color-border-strong);
    border: 2px solid var(--color-border-strong);
    transition: background var(--transition-fast);
  }

  .timeline-step.done .timeline-dot {
    background: var(--color-success);
    border-color: var(--color-success);
  }

  .timeline-step.active .timeline-dot {
    background: var(--color-primary);
    border-color: var(--color-primary);
    box-shadow: 0 0 8px rgba(238, 0, 0, 0.4);
  }

  .timeline-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: capitalize;
    white-space: nowrap;
  }

  .timeline-step.done .timeline-label,
  .timeline-step.active .timeline-label {
    color: var(--color-text-secondary);
  }

  .timeline-line {
    flex: 1;
    height: 2px;
    background: var(--color-border);
    margin-bottom: 1rem;
    min-width: var(--space-4);
    transition: background var(--transition-fast);
  }

  .timeline-line.done {
    background: var(--color-success);
  }

  /* Sections */
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .section-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 400;
    font-family: var(--font-body);
  }

  .skeleton-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* Reviews */
  .review-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .review-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--transition-fast);
  }

  .review-card:hover {
    border-color: var(--color-border-strong);
  }

  .review-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .reviewer-name {
    font-weight: 600;
    color: var(--color-text);
    font-size: var(--text-sm);
    flex: 1;
  }

  .review-time {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .review-body {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.6;
  }

  /* Comments */
  .comment-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .comment-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .comment-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .commenter-name {
    font-weight: 600;
    color: var(--color-text);
    font-size: var(--text-sm);
  }

  .file-ref {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link);
    background: rgba(0, 102, 204, 0.1);
    padding: 0.1rem var(--space-2);
    border-radius: var(--radius-sm);
  }

  .comment-time {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    margin-left: auto;
  }

  .comment-body {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.6;
  }
</style>
