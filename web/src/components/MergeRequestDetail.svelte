<script>
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';
  import { detectLang, highlightLine } from '../lib/syntaxHighlight.js';

  const navigate = getContext('navigate');

  let { mr: initialMr, repo, onBack } = $props();

  let mr = $state(initialMr);
  let reviews = $state([]);
  let comments = $state([]);
  let gates = $state([]);
  let deps = $state(null);
  let loading = $state(true);
  let enqueueing = $state(false);
  let addDepInput = $state('');
  let addingDep = $state(false);
  let removingDepId = $state(null);

  // Diff / Files tab state
  let activeTab = $state('overview'); // 'overview' | 'files'
  let diff = $state(null);
  let diffLoading = $state(false);
  let selectedFile = $state(null);

  $effect(() => {
    loadDetails();
  });

  async function loadDetails() {
    loading = true;
    try {
      [reviews, comments, gates] = await Promise.all([
        api.mrReviews(mr.id),
        api.mrComments(mr.id),
        api.mrGates(mr.id),
      ]);
      try { deps = await api.mrDependencies(mr.id); } catch { deps = null; }
    } catch (e) {
      toastError('Failed to load MR details: ' + (e.message ?? 'unknown error'));
    }
    loading = false;
  }

  async function loadDiff() {
    if (diff) return;
    diffLoading = true;
    try {
      diff = await api.mrDiff(mr.id);
      if (diff.files && diff.files.length > 0) {
        selectedFile = diff.files[0].path;
      }
    } catch (e) {
      toastError('Failed to load diff: ' + e.message);
    } finally {
      diffLoading = false;
    }
  }

  function switchTab(tab) {
    activeTab = tab;
    if (tab === 'files') loadDiff();
  }

  function gateStatusColor(status) {
    switch (status) {
      case 'passed': return 'var(--color-success)';
      case 'failed': return 'var(--color-danger)';
      case 'running': return 'var(--color-warning)';
      default: return 'var(--color-text-muted)';
    }
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

  async function addDep() {
    const depId = addDepInput.trim();
    if (!depId) return;
    addingDep = true;
    try {
      const current = deps?.depends_on ?? [];
      deps = await api.setMrDependencies(mr.id, { depends_on: [...current, depId] });
      addDepInput = '';
      toastSuccess('Dependency added.');
    } catch (e) {
      toastError(e.message);
    } finally {
      addingDep = false;
    }
  }

  async function removeDep(depId) {
    removingDepId = depId;
    try {
      await api.removeMrDependency(mr.id, depId);
      deps = { ...deps, depends_on: deps.depends_on.filter(id => id !== depId) };
      toastSuccess('Dependency removed.');
    } catch (e) {
      toastError(e.message);
    } finally {
      removingDepId = null;
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

  let timelineStep = $derived.by(() => {
    if (mr.status === 'merged') return 4;
    if (mr.status === 'approved') return reviews.length > 0 ? 2 : 1;
    if (reviews.length > 0) return 1;
    return 0;
  });

  // Syntax highlighting language
  let selectedLang = $derived(selectedFile ? detectLang(selectedFile) : 'text');
</script>

<div class="panel">
  <!-- Header -->
  <div class="panel-header">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack} aria-label="Go back">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M19 12H5M12 5l-7 7 7 7"/></svg>
        {repo?.name ?? 'Repo'}
      </button>
      <span class="sep">/</span>
      <span class="mr-title-header">{mr.title}</span>
    </div>
    <Badge value={mr.status} />
  </div>

  <!-- Tab bar -->
  <div class="tab-bar" role="tablist" aria-label="Merge request tabs">
    <button class="tab-btn" class:active={activeTab === 'overview'} onclick={() => switchTab('overview')} role="tab" aria-selected={activeTab === 'overview'}>Overview</button>
    <button class="tab-btn" class:active={activeTab === 'files'} onclick={() => switchTab('files')} role="tab" aria-selected={activeTab === 'files'}>
      Files
      {#if mr.diff_stats}<span class="tab-badge">{mr.diff_stats.files_changed}</span>{/if}
    </button>
  </div>

  <div class="content" class:content-files={activeTab === 'files'} role="tabpanel">
    {#if activeTab === 'overview'}
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
              {#if navigate}
                <button class="meta-link-btn" onclick={() => navigate('agents')}>{mr.author_agent_id}</button>
              {:else}
                <span class="meta-value">{mr.author_agent_id}</span>
              {/if}
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

          {#if mr.spec_ref}
            <div class="meta-divider"></div>
            <h4 class="sidebar-section-title">Spec Binding</h4>
            {#each [mr.spec_ref.split('@')] as [specPath, specSha]}
              <div class="spec-ref-row">
                <span class="spec-ref-path" title={mr.spec_ref}>{specPath}</span>
                {#if specSha}
                  <code class="spec-ref-sha">{specSha.slice(0, 8)}</code>
                {/if}
              </div>
            {/each}
          {/if}

          {#if mr.atomic_group}
            <div class="meta-row">
              <span class="meta-label">Atomic Group</span>
              <code class="meta-mono">{mr.atomic_group}</code>
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

        <!-- Quality Gates -->
        {#if gates.length > 0}
          <div class="sidebar-card">
            <h4 class="sidebar-section-title">Quality Gates</h4>
            <div class="gates-list">
              {#each gates as gate (gate.id)}
                <div class="gate-row">
                  <span class="gate-dot" style:background={gateStatusColor(gate.status)}></span>
                  <span class="gate-label">{gate.gate_id}</span>
                  <span class="gate-status" style:color={gateStatusColor(gate.status)}>{gate.status}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Dependencies -->
        {#if deps !== null}
          <div class="sidebar-card">
            <h4 class="sidebar-section-title">Dependencies</h4>
            {#if deps.depends_on && deps.depends_on.length > 0}
              <div class="dep-list">
                {#each deps.depends_on as depId (depId)}
                  <div class="dep-row">
                    <code class="dep-id" title={depId}>{depId.slice(0, 12)}…</code>
                    <button
                      class="dep-remove-btn"
                      onclick={() => removeDep(depId)}
                      disabled={removingDepId === depId}
                      title="Remove dependency"
                      aria-label="Remove dependency"
                    >
                      {removingDepId === depId ? '…' : '×'}
                    </button>
                  </div>
                {/each}
              </div>
            {:else}
              <p class="dep-empty">No dependencies</p>
            {/if}
            <div class="dep-add-row">
              <input
                class="dep-input"
                placeholder="MR UUID"
                bind:value={addDepInput}
                onkeydown={(e) => e.key === 'Enter' && addDep()}
              />
              <button class="dep-add-btn" onclick={addDep} disabled={addingDep || !addDepInput.trim()}>
                {addingDep ? '…' : 'Add'}
              </button>
            </div>
            {#if deps.dependents && deps.dependents.length > 0}
              <div class="meta-divider"></div>
              <h4 class="sidebar-section-title">Required by</h4>
              <div class="dep-list">
                {#each deps.dependents as depId (depId)}
                  <code class="dep-id dep-id-ro" title={depId}>{depId.slice(0, 12)}…</code>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

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
              <div class="timeline-step" class:done={i <= timelineStep} class:active={i === timelineStep}>
                <div class="timeline-dot"></div>
                <span class="timeline-label">{step}</span>
              </div>
              {#if i < TIMELINE_STEPS.length - 1}
                <div class="timeline-line" class:done={i < timelineStep}></div>
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

    {:else}
    <!-- Files tab: diff viewer -->
    <div class="files-layout">
      {#if diffLoading}
        <div class="diff-skeleton">
          <Skeleton height="2rem" />
          <Skeleton height="10rem" />
        </div>
      {:else if !diff || diff.files.length === 0}
        <EmptyState title="No files changed" description="This merge request has no diff to display." />
      {:else}
        <!-- File list sidebar -->
        <aside class="file-list">
          <div class="file-list-header">
            <span class="file-list-title">Changed Files</span>
            <span class="file-list-count">{diff.files.length}</span>
          </div>
          {#each diff.files as file (file.path)}
            <button
              class="file-item"
              class:selected={selectedFile === file.path}
              onclick={() => selectedFile = file.path}
              aria-label="View diff for {file.path}"
            >
              <span class="file-status-dot" class:modified={file.status === 'Modified'} class:added={file.status === 'Added'} class:deleted={file.status === 'Deleted'}></span>
              <span class="file-path-text">{file.path}</span>
            </button>
          {/each}
        </aside>

        <!-- Diff panel -->
        <div class="diff-panel">
          {#each diff.files as file (file.path)}
            {#if file.path === selectedFile}
              <div class="file-diff">
                <div class="file-diff-header">
                  <code class="file-diff-path">{file.path}</code>
                  <span class="file-diff-status">{file.status}</span>
                </div>
                {#each file.hunks as hunk, hi (hi)}
                  <div class="hunk">
                    <div class="hunk-header">{hunk.header}</div>
                    <div class="hunk-lines">
                      {#each hunk.lines as line, li (li)}
                        <div
                          class="diff-line"
                          class:line-add={line.type === 'add'}
                          class:line-delete={line.type === 'delete'}
                          class:line-context={line.type === 'context'}
                        >
                          <span class="line-gutter">
                            {line.type === 'add' ? '+' : line.type === 'delete' ? '-' : ' '}
                          </span>
                          <code class="line-content">{@html highlightLine(line.content, selectedLang)}</code>
                        </div>
                      {/each}
                    </div>
                  </div>
                {/each}
                {#if file.hunks.length === 0}
                  <div class="hunk-empty">No content to display.</div>
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
    {/if}
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
    padding: var(--space-4) var(--space-6);
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
    min-height: 0;
  }

  .content.content-files {
    padding: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
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

  .meta-link-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 0;
    text-align: right;
    text-decoration: underline;
    text-underline-offset: 2px;
    word-break: break-all;
  }

  .meta-link-btn:hover { opacity: 0.8; }

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
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
    color: var(--color-success);
  }
  .action-btn.approve:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-success) 22%, transparent);
  }

  .action-btn.changes {
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-warning) 40%, transparent);
    color: var(--color-warning);
  }
  .action-btn.changes:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-warning) 22%, transparent);
  }

  .action-btn.enqueue {
    background: color-mix(in srgb, var(--color-info) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-info) 40%, transparent);
    color: var(--color-link);
  }
  .action-btn.enqueue:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-info) 22%, transparent);
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
    box-shadow: 0 0 8px color-mix(in srgb, var(--color-primary) 40%, transparent);
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
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
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

  /* Tab bar */
  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    padding: 0 var(--space-6);
    flex-shrink: 0;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    font-family: var(--font-body);
    font-weight: 500;
    margin-bottom: -1px;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .tab-btn:hover { color: var(--color-text); }

  .tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
  }

  .tab-badge {
    background: var(--color-surface-raised);
    border: 1px solid var(--color-border);
    border-radius: 9999px;
    font-size: 0.65rem;
    padding: 0 var(--space-1);
    color: var(--color-text-muted);
  }

  /* Quality gates */
  .gates-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .gate-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .gate-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .gate-label {
    flex: 1;
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.7rem;
  }

  .gate-status {
    font-weight: 600;
    text-transform: capitalize;
    font-size: 0.65rem;
  }

  /* Files tab layout */
  .files-layout {
    display: grid;
    grid-template-columns: 220px 1fr;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  .diff-skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-6);
    grid-column: 1 / -1;
  }

  /* File list sidebar */
  .file-list {
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .file-list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .file-list-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .file-list-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-raised);
    border: 1px solid var(--color-border);
    border-radius: 9999px;
    padding: 0 var(--space-2);
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
    transition: background var(--transition-fast), color var(--transition-fast);
    overflow: hidden;
  }

  .file-item:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .file-item.selected {
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    color: var(--color-text);
  }

  .file-status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--color-text-muted);
  }

  .file-status-dot.modified { background: var(--color-warning); }
  .file-status-dot.added { background: var(--color-success); }
  .file-status-dot.deleted { background: var(--color-danger); }

  .file-path-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Diff panel */
  .diff-panel {
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .file-diff {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .file-diff-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
  }

  .file-diff-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  .file-diff-status {
    font-size: 0.65rem;
    color: var(--color-text-muted);
    text-transform: lowercase;
  }

  .hunk {
    border-top: 1px solid var(--color-border);
  }

  .hunk:first-child { border-top: none; }

  .hunk-header {
    background: color-mix(in srgb, var(--color-info) 6%, transparent);
    padding: var(--space-1) var(--space-4);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--color-link);
    border-bottom: 1px solid var(--color-border);
  }

  .hunk-lines {
    display: flex;
    flex-direction: column;
  }

  .diff-line {
    display: flex;
    align-items: flex-start;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.5;
    min-height: 1.5em;
  }

  .diff-line.line-add {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }

  .diff-line.line-delete {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
  }

  .diff-line.line-context {
    background: transparent;
  }

  .line-gutter {
    width: 1.5rem;
    flex-shrink: 0;
    text-align: center;
    color: var(--color-text-muted);
    user-select: none;
    padding: 0 var(--space-2);
  }

  .diff-line.line-add .line-gutter { color: var(--color-success); }
  .diff-line.line-delete .line-gutter { color: var(--color-danger); }

  .line-content {
    flex: 1;
    white-space: pre;
    overflow-x: auto;
    padding-right: var(--space-4);
    color: var(--color-text);
  }

  .hunk-empty {
    padding: var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    text-align: center;
  }

  /* Spec ref */
  .spec-ref-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    flex-wrap: wrap;
  }
  .spec-ref-path {
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .spec-ref-sha {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    color: var(--color-link);
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    padding: 0.1rem 0.3rem;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }
  .meta-mono {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--color-text-secondary);
  }

  /* Dependencies */
  .dep-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .dep-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .dep-id {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    color: var(--color-text-secondary);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dep-id-ro {
    display: block;
    margin-bottom: var(--space-1);
  }
  .dep-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    font-style: italic;
  }
  .dep-remove-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    cursor: pointer;
    font-size: 0.7rem;
    padding: 0 var(--space-1);
    line-height: 1.4;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }
  .dep-remove-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .dep-remove-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .dep-add-row {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }
  .dep-input {
    flex: 1;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    min-width: 0;
  }
  .dep-input:focus:not(:focus-visible) { outline: none; }
  .dep-input:focus-visible { outline: 2px solid var(--color-focus, #4db0ff); outline-offset: 2px; border-color: var(--color-focus, #4db0ff); }
  .dep-add-btn {
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    font-family: var(--font-body);
    white-space: nowrap;
    transition: background var(--transition-fast);
  }
  .dep-add-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--color-info) 18%, transparent); }
  .dep-add-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  /* Syntax highlighting token colors */
  :global(.hl-kw)  { color: #cc99ff; }
  :global(.hl-str) { color: #99cc88; }
  :global(.hl-cmt) { color: #6b7a8d; font-style: italic; }
  :global(.hl-num) { color: #f09a3e; }
</style>
