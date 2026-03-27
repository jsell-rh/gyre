<script>
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Button from '../lib/Button.svelte';

  const navigate = getContext('navigate');

  let { task, onBack = undefined } = $props();

  let detail = $state(null);
  let loading = $state(true);
  let error = $state(null);
  let activeTab = $state('info');

  const tabs = [
    { id: 'info',      label: 'Info' },
    { id: 'artifacts', label: 'Artifacts' },
  ];

  // Ralph refs from task description/labels
  let ralphRefs = $derived.by(() => {
    if (!detail) return [];
    const refs = [];
    if (detail.labels) {
      for (const lbl of detail.labels) {
        if (lbl.startsWith('spec-') || lbl.startsWith('ralph-')) refs.push(lbl);
      }
    }
    return refs;
  });

  async function load() {
    loading = true;
    error = null;
    try {
      detail = await api.task(task.id);
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  $effect(() => {
    const _id = task.id; // explicit dependency — prevents re-fire on unrelated reactive updates
    load();
  });

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(ts).toLocaleString();
  }

  const STATUS_COLORS = {
    Backlog: 'neutral',
    InProgress: 'info',
    Review: 'warning',
    Done: 'success',
    Blocked: 'danger',
  };
</script>

<div class="task-detail" aria-busy={loading}>
  <div class="detail-header">
    <Button variant="secondary" onclick={onBack} disabled={!onBack} aria-label="Go back to task list">← Back</Button>
    <div class="header-meta">
      {#if detail}
        <Badge value={detail.status} color={STATUS_COLORS[detail.status]} />
        <Badge value={detail.priority} />
      {:else}
        <Skeleton width="80px" height="1.4rem" />
      {/if}
    </div>
  </div>

  <div class="detail-body">
    {#if loading}
      <div class="loading-area">
        <Skeleton width="60%" height="2rem" />
        <Skeleton width="100%" height="1rem" />
        <Skeleton width="80%" height="1rem" />
      </div>
    {:else if error}
      <div role="alert">
        <EmptyState title="Failed to load task" description={error} />
      </div>
      <div class="retry-area">
        <Button variant="secondary" onclick={load}>Retry</Button>
      </div>
    {:else if detail}
      <h1 class="task-title">{detail.title}</h1>

      <Tabs {tabs} bind:active={activeTab} />

      <div role="tabpanel" id="tabpanel-info" aria-labelledby="tab-info" hidden={activeTab !== 'info'}>
        <div class="info-grid">
          <div class="info-row">
            <span class="info-label">ID</span>
            <span class="info-val mono">{detail.id}</span>
          </div>
          <div class="info-row">
            <span class="info-label">Status</span>
            <span class="info-val"><Badge value={detail.status} color={STATUS_COLORS[detail.status]} /></span>
          </div>
          <div class="info-row">
            <span class="info-label">Priority</span>
            <span class="info-val"><Badge value={detail.priority} /></span>
          </div>
          {#if detail.assigned_to}
            <div class="info-row">
              <span class="info-label">Assigned To</span>
              <span class="info-val mono">
                {#if navigate}
                  <button class="link-btn" onclick={() => navigate('agents')}>{detail.assigned_to}</button>
                {:else}
                  {detail.assigned_to}
                {/if}
              </span>
            </div>
          {/if}
          {#if detail.parent_task_id}
            <div class="info-row">
              <span class="info-label">Parent Task</span>
              <span class="info-val mono">
                {#if navigate}
                  <button class="link-btn" onclick={() => navigate('task-detail', { task: { id: detail.parent_task_id } })}>{detail.parent_task_id}</button>
                {:else}
                  {detail.parent_task_id}
                {/if}
              </span>
            </div>
          {/if}
          <div class="info-row">
            <span class="info-label">Created</span>
            <span class="info-val">{fmtDate(detail.created_at)}</span>
          </div>
          <div class="info-row">
            <span class="info-label">Updated</span>
            <span class="info-val">{fmtDate(detail.updated_at)}</span>
          </div>
          {#if detail.labels?.length}
            <div class="info-row">
              <span class="info-label">Labels</span>
              <div class="info-val label-list">
                {#each detail.labels as lbl}
                  <span class="label-pill">{lbl}</span>
                {/each}
              </div>
            </div>
          {/if}
          {#if detail.description}
            <div class="info-row description-row">
              <span class="info-label">Description</span>
              <p class="info-val description">{detail.description}</p>
            </div>
          {/if}
        </div>
      </div>

      <div role="tabpanel" id="tabpanel-artifacts" aria-labelledby="tab-artifacts" hidden={activeTab !== 'artifacts'}>
        <div class="artifacts-section">
          {#if detail.pr_link}
            <div class="artifact-card">
              <span class="artifact-icon" aria-hidden="true">↗</span>
              <div class="artifact-body">
                <span class="artifact-label">Pull Request</span>
                <a class="artifact-link" href={detail.pr_link} target="_blank" rel="noreferrer">
                  {detail.pr_link}
                  <span class="sr-only">(opens in new tab)</span>
                </a>
              </div>
            </div>
          {/if}
          {#if ralphRefs.length}
            <div class="artifact-section-title">Ralph Loop Refs</div>
            {#each ralphRefs as ref}
              <div class="artifact-card">
                <span class="artifact-icon" aria-hidden="true">⚡</span>
                <div class="artifact-body">
                  <span class="artifact-label mono">{ref}</span>
                </div>
              </div>
            {/each}
          {/if}
          {#if !detail.pr_link && !ralphRefs.length}
            <EmptyState title="No artifacts" description="Artifacts appear here when an agent completes this task and opens a merge request." />
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .task-detail {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-surface);
  }

  .header-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .detail-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .loading-area {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .task-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
    line-height: 1.3;
  }

  .info-grid {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .info-row {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
  }

  .info-label {
    width: 110px;
    flex-shrink: 0;
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
    padding-top: 3px;
  }

  .info-val {
    font-size: var(--text-sm);
    color: var(--color-text);
    flex: 1;
  }

  .info-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .description-row { align-items: flex-start; }
  .description {
    margin: 0;
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .label-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .label-pill {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
  }

  /* Artifacts */
  .artifacts-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .artifact-section-title {
    font-size: var(--text-xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
    margin-top: var(--space-2);
  }

  .artifact-card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  .artifact-icon {
    font-size: var(--text-lg);
    flex-shrink: 0;
  }

  .artifact-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .artifact-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
  }

  .artifact-label.mono { font-family: var(--font-mono); }

  .artifact-link {
    font-size: var(--text-sm);
    color: var(--color-link);
    text-decoration: none;
    word-break: break-all;
    transition: color var(--transition-fast);
  }

  .artifact-link:hover { text-decoration: underline; color: var(--color-link-hover); }
  .artifact-link:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .link-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    padding: 0;
    text-decoration: underline;
    text-underline-offset: var(--space-1);
    transition: opacity var(--transition-fast);
  }

  .retry-area {
    display: flex;
    justify-content: center;
    padding: var(--space-4);
  }

  .link-btn:hover { opacity: 0.8; }
  .link-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .artifact-link,
    .link-btn { transition: none; }
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
