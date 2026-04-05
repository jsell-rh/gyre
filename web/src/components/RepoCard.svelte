<script>
  /**
   * RepoCard — GitHub-style repository card for workspace home.
   *
   * Shows: name, health indicator, key stats, last activity time.
   * Enhanced: shows latest activity summary and active work context.
   * Click navigates to repo mode.
   */
  import { getContext } from 'svelte';
  import { relativeTime } from '../lib/timeFormat.js';
  import { entityName, formatId } from '../lib/entityNames.svelte.js';
  import Icon from '../lib/Icon.svelte';

  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  let {
    repo = null,
    health = 'idle',
    stats = {},
    activeAgentNames = [],
    specBreakdown = null,
    latestMr = null,
    latestAgent = null,
    /** @type {Array} All agents for this repo (used for single-agent navigation) */
    activeAgents = [],
    /** @type {Array} All MRs for this repo (used for single-MR navigation) */
    failedMrs = [],
    /** @type {Array} Merge queue items for this repo */
    queueItems = [],
    /** @type {Array} Pending specs for this repo (for inline approve/reject) */
    pendingSpecs = [],
    /** @type {Function} Quick approve spec callback */
    onApproveSpec = undefined,
    /** @type {Function} Quick reject spec callback */
    onRejectSpec = undefined,
    onclick = undefined,
    onStatClick = undefined,
  } = $props();

  function handleStatClick(tab, e) {
    e.stopPropagation();
    if (onStatClick) {
      onStatClick(repo, tab);
    } else if (onclick) {
      onclick();
    }
  }

  const HEALTH = {
    healthy: { color: 'var(--color-success)', dot: '\u25CF', label: 'Active — agents are implementing code' },
    gate: { color: 'var(--color-danger)', dot: '\u26A0', label: 'Gate failure — needs attention' },
    gate_failure: { color: 'var(--color-danger)', dot: '\u26A0', label: 'Gate failure — needs attention' },
    idle: { color: 'var(--color-text-muted)', dot: '\u25CB', label: 'Idle — no active agents' },
  };

  let h = $derived(HEALTH[health] ?? HEALTH.idle);

  // Derive the most relevant "what's happening" summary
  let statusSummary = $derived.by(() => {
    if (stats.failedGates > 0) return { text: `${stats.failedGates} gate failure${stats.failedGates !== 1 ? 's' : ''}`, why: 'MR merge blocked until gates pass', variant: 'danger' };
    if (stats.agents > 0) return { text: `${stats.agents} agent${stats.agents !== 1 ? 's' : ''} working`, why: 'Implementing code from approved specs', variant: 'success' };
    if (specBreakdown?.pending > 0) return { text: `${specBreakdown.pending} spec${specBreakdown.pending !== 1 ? 's' : ''} awaiting review`, why: 'Agents cannot start until specs are approved', variant: 'warning' };
    if (stats.openMrs > 0) return { text: `${stats.openMrs} open MR${stats.openMrs !== 1 ? 's' : ''}`, why: 'Ready to enqueue for merge', variant: 'info' };
    return null;
  });
</script>

{#if repo}
  <!-- Use a div with role="button" to avoid nested-button a11y violations -->
  <div class="repo-card" class:repo-card-active={health === 'healthy'} class:repo-card-alert={health === 'gate' || health === 'gate_failure'} onclick={onclick} data-testid="repo-card" title={statusSummary ? `${statusSummary.text}\n${statusSummary.why}` : h.label} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onclick?.(); } }}>
    <div class="repo-card-header">
      <span class="repo-card-health" style="color: {h.color}" aria-label={h.label}>{h.dot}</span>
      <span class="repo-card-name">{repo.name}</span>
      {#if stats.last_activity}
        <span class="repo-card-time">{relativeTime(stats.last_activity)}</span>
      {/if}
    </div>
    {#if repo.description}
      <p class="repo-card-desc">{repo.description}</p>
    {/if}

    <!-- What's happening now — one line of context -->
    {#if statusSummary}
      <div class="repo-card-activity">
        {#if statusSummary.variant === 'danger' && failedMrs.length === 1}
          <button class="repo-activity-link repo-activity-{statusSummary.variant}" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', failedMrs[0].id, { repo_id: repo.id, title: failedMrs[0].title, _openTab: 'gates' }); }}>{statusSummary.text} — view gates</button>
        {:else if statusSummary.variant === 'success' && activeAgents.length === 1}
          <button class="repo-activity-link repo-activity-{statusSummary.variant}" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('agent', activeAgents[0].id, { repo_id: repo.id, name: activeAgents[0].name }); }}>{activeAgents[0].name ?? formatId('agent', activeAgents[0].id)} working</button>
        {:else}
          <span class="repo-activity-text repo-activity-{statusSummary.variant}">{statusSummary.text}</span>
        {/if}
        <span class="repo-activity-why">{statusSummary.why}</span>
      </div>
    {:else if latestMr}
      <div class="repo-card-activity">
        <button class="repo-activity-link repo-activity-{latestMr.status === 'merged' ? 'success' : 'info'}" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', latestMr.id, { repo_id: repo.id, title: latestMr.title }); }}>
          {latestMr.status === 'merged' ? 'Last merged' : 'Latest MR'}: {latestMr.title ?? 'Untitled'}
        </button>
        {#if latestMr.diff_stats}
          <span class="repo-activity-diff">
            <span class="diff-ins-tiny">+{latestMr.diff_stats.insertions ?? 0}</span>
            <span class="diff-del-tiny">-{latestMr.diff_stats.deletions ?? 0}</span>
          </span>
        {/if}
        {#if latestMr._gates?.details?.length > 0}
          <span class="repo-mr-gates">
            {#each latestMr._gates.details.slice(0, 3) as g}
              <span class="gate-mini gate-mini-{g.status}" title="{g.name}{g.gate_type ? ' (' + g.gate_type.replace(/_/g, ' ') + ')' : ''}">{g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'} {g.name}</span>
            {/each}
            {#if latestMr._gates.details.length > 3}
              <span class="gate-mini gate-mini-more">+{latestMr._gates.details.length - 3}</span>
            {/if}
          </span>
        {:else if latestMr._gates}
          <span class="repo-mr-gates">
            {#if latestMr._gates.passed > 0}<span class="gate-mini gate-mini-pass">✓{latestMr._gates.passed}</span>{/if}
            {#if latestMr._gates.failed > 0}<span class="gate-mini gate-mini-fail">✗{latestMr._gates.failed}</span>{/if}
          </span>
        {/if}
      </div>
    {/if}

    <!-- Pending specs inline — approve/reject without drilling down -->
    {#if pendingSpecs.length > 0}
      <div class="repo-pending-specs">
        {#each pendingSpecs.slice(0, 2) as spec}
          {@const specName = spec.title ?? spec.path?.split('/').pop()?.replace(/\.md$/, '') ?? 'Untitled'}
          <div class="repo-pending-spec" onclick={(e) => e.stopPropagation()}>
            <button class="repo-pending-spec-name" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('spec', spec.path, { path: spec.path, repo_id: repo.id }); }} title="View spec: {specName}">
              {specName}
            </button>
            <span class="repo-pending-spec-actions">
              <button class="repo-spec-action repo-spec-approve" onclick={(e) => { e.stopPropagation(); onApproveSpec?.(spec); }} title="Approve this spec">Approve</button>
              <button class="repo-spec-action repo-spec-reject" onclick={(e) => { e.stopPropagation(); onRejectSpec?.(spec); }} title="Reject this spec">Reject</button>
            </span>
          </div>
        {/each}
        {#if pendingSpecs.length > 2}
          <span class="repo-pending-more">{pendingSpecs.length - 2} more pending</span>
        {/if}
      </div>
    {/if}

    <!-- Merge queue items for this repo -->
    {#if queueItems.length > 0}
      <div class="repo-queue-strip">
        <span class="repo-queue-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><path d="M16 3h5v5M4 20L21 3"/></svg>
        </span>
        {#each queueItems.slice(0, 2) as item, i}
          {@const mrTitle = item._title ?? entityName('mr', item.merge_request_id ?? item.mr_id)}
          <button class="repo-queue-item" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', item.merge_request_id ?? item.mr_id, item._mr ?? {}); }}>
            <span class="repo-queue-pos">#{i + 1}</span>
            <span class="repo-queue-title">{mrTitle}</span>
          </button>
        {/each}
        {#if queueItems.length > 2}
          <span class="repo-queue-more">+{queueItems.length - 2}</span>
        {/if}
      </div>
    {/if}

    <!-- Pipeline mini-flow — shows where work is in the spec→task→agent→MR pipeline -->
    {#if stats.specs > 0 || stats.tasks > 0 || stats.agents > 0 || stats.mrs > 0}
      <div class="repo-card-pipeline">
        <span class="repo-pipeline-stage" class:repo-pipeline-active={specBreakdown?.pending > 0} onclick={(e) => handleStatClick('specs', e)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') handleStatClick('specs', e); }} title="{stats.specs} spec{stats.specs !== 1 ? 's' : ''}{specBreakdown?.pending ? ` (${specBreakdown.pending} pending)` : ''}">
          <span class="repo-pipeline-count">{stats.specs}</span>
          <span class="repo-pipeline-label">specs</span>
        </span>
        {#if stats.tasks > 0}
          <span class="repo-pipeline-arrow">→</span>
          <span class="repo-pipeline-stage" onclick={(e) => handleStatClick('tasks', e)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') handleStatClick('tasks', e); }} title="{stats.tasks} task{stats.tasks !== 1 ? 's' : ''}">
            <span class="repo-pipeline-count">{stats.tasks}</span>
            <span class="repo-pipeline-label">tasks</span>
          </span>
        {/if}
        {#if stats.agents > 0}
          <span class="repo-pipeline-arrow">→</span>
          <span class="repo-pipeline-stage" class:repo-pipeline-live={stats.agents > 0} onclick={(e) => handleStatClick('agents', e)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') handleStatClick('agents', e); }} title="{stats.agents} agent{stats.agents !== 1 ? 's' : ''}">
            <span class="repo-pipeline-count">{stats.agents}</span>
            <span class="repo-pipeline-label">agents</span>
          </span>
        {/if}
        {#if stats.mrs > 0}
          <span class="repo-pipeline-arrow">→</span>
          <span class="repo-pipeline-stage" class:repo-pipeline-danger={stats.failedGates > 0} class:repo-pipeline-done={stats.failedGates === 0} onclick={(e) => handleStatClick('mrs', e)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') handleStatClick('mrs', e); }} title="{stats.mrs} MR{stats.mrs !== 1 ? 's' : ''}{stats.failedGates > 0 ? ` — ${stats.failedGates} gate failures` : ''}">
            <span class="repo-pipeline-count">{stats.mrs}</span>
            <span class="repo-pipeline-label">MRs</span>
          </span>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .repo-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .repo-card:hover {
    border-color: var(--color-primary);
    box-shadow: var(--shadow-sm);
  }

  .repo-card:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .repo-card-active {
    border-left: 3px solid var(--color-success);
  }

  .repo-card-alert {
    border-left: 3px solid var(--color-danger);
  }

  .repo-card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .repo-card-health {
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .repo-card-name {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .repo-card-health-text {
    font-size: 10px;
    font-weight: 500;
    flex-shrink: 0;
    white-space: nowrap;
  }

  .repo-card-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
    white-space: nowrap;
  }

  .repo-card-desc {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.3;
  }

  /* Activity summary */
  .repo-card-activity {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-xs);
  }

  .repo-activity-text {
    font-weight: 600;
  }

  .repo-activity-link {
    font-weight: 600;
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    padding: 0;
    text-align: left;
    text-decoration: none;
  }

  .repo-activity-link:hover {
    text-decoration: underline;
  }

  .repo-activity-success { color: var(--color-success); }
  .repo-activity-danger { color: var(--color-danger); }
  .repo-activity-warning { color: var(--color-warning); }
  .repo-activity-info { color: var(--color-info, #1e90ff); }

  .repo-activity-why {
    color: var(--color-text-muted);
    font-weight: 400;
    font-style: italic;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .repo-activity-diff {
    display: inline-flex;
    gap: 3px;
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .diff-ins-tiny { color: var(--color-success); font-weight: 600; }
  .diff-del-tiny { color: var(--color-danger); font-weight: 600; }

  .repo-mr-gates {
    display: inline-flex;
    gap: 3px;
    font-size: 10px;
  }

  .gate-mini {
    font-weight: 600;
    padding: 0 3px;
    border-radius: 3px;
    font-family: var(--font-mono);
  }

  .gate-mini-pass, .gate-mini-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-mini-fail, .gate-mini-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-mini-pending { color: var(--color-text-muted); background: color-mix(in srgb, var(--color-text-muted) 8%, transparent); }
  .gate-mini-more { color: var(--color-text-muted); font-style: italic; }

  /* Merge queue strip */
  /* Pending specs inline */
  .repo-pending-specs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-warning) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 20%, transparent);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
  }

  .repo-pending-spec {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .repo-pending-spec-name {
    flex: 1;
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    font-weight: 500;
    color: var(--color-text);
    padding: 0;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-pending-spec-name:hover {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .repo-pending-spec-actions {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  .repo-spec-action {
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    border: none;
    cursor: pointer;
    font-family: inherit;
    transition: all var(--transition-fast);
  }

  .repo-spec-approve {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .repo-spec-approve:hover {
    background: var(--color-success);
    color: white;
  }

  .repo-spec-reject {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    color: var(--color-danger);
  }

  .repo-spec-reject:hover {
    background: var(--color-danger);
    color: white;
  }

  .repo-pending-more {
    font-size: 10px;
    color: var(--color-text-muted);
    font-style: italic;
  }

  .repo-queue-strip {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) 0;
    font-size: var(--text-xs);
  }

  .repo-queue-icon {
    color: var(--color-warning);
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .repo-queue-item {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
    border: none;
    border-radius: var(--radius-sm);
    padding: 1px 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    color: var(--color-text-secondary);
    transition: background var(--transition-fast);
    max-width: 140px;
    overflow: hidden;
  }

  .repo-queue-item:hover {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    text-decoration: underline;
  }

  .repo-queue-pos {
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--color-warning);
    flex-shrink: 0;
  }

  .repo-queue-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .repo-queue-more {
    color: var(--color-text-muted);
    font-size: 10px;
    flex-shrink: 0;
  }

  /* Pipeline mini-flow */
  .repo-card-pipeline {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-top: var(--space-1);
    padding-top: var(--space-1);
    border-top: 1px solid var(--color-border);
  }

  .repo-pipeline-stage {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px 4px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: 10px;
    color: var(--color-text-muted);
    transition: all var(--transition-fast);
  }

  .repo-pipeline-stage:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
    color: var(--color-primary);
  }

  .repo-pipeline-count {
    font-weight: 700;
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .repo-pipeline-label {
    font-weight: 500;
  }

  .repo-pipeline-arrow {
    color: var(--color-text-muted);
    opacity: 0.3;
    font-size: 9px;
    flex-shrink: 0;
  }

  .repo-pipeline-active { color: var(--color-warning); }
  .repo-pipeline-active .repo-pipeline-count { color: var(--color-warning); }
  .repo-pipeline-live { color: var(--color-success); }
  .repo-pipeline-live .repo-pipeline-count { color: var(--color-success); }
  .repo-pipeline-danger { color: var(--color-danger); }
  .repo-pipeline-danger .repo-pipeline-count { color: var(--color-danger); }
  .repo-pipeline-done { color: var(--color-success); }
  .repo-pipeline-done .repo-pipeline-count { color: var(--color-success); }
</style>
