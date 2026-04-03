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
  import Badge from '../lib/Badge.svelte';
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
    onclick = undefined,
  } = $props();

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
  <button class="repo-card" class:repo-card-active={health === 'healthy'} class:repo-card-alert={health === 'gate' || health === 'gate_failure'} onclick={onclick} data-testid="repo-card">
    <div class="repo-card-header">
      <span class="repo-card-health" style="color: {h.color}" title={h.label} aria-label={h.label}>{h.dot}</span>
      <span class="repo-card-name">{repo.name}</span>
      {#if stats.last_activity}
        <span class="repo-card-time">{relativeTime(stats.last_activity)}</span>
      {/if}
    </div>

    <!-- Status summary — the ONE most important thing about this repo right now -->
    {#if statusSummary}
      <div class="repo-card-status repo-card-status-{statusSummary.variant}">
        <span class="status-text">{statusSummary.text}</span>
        <span class="status-why">{statusSummary.why}</span>
      </div>
    {:else if repo.description}
      <p class="repo-card-desc">{repo.description}</p>
    {/if}

    <!-- Compact stats bar -->
    <div class="repo-card-stats">
      {#if stats.specs > 0}<span class="repo-stat"><Icon name="spec" size={10} /> {stats.specs}</span>{/if}
      {#if stats.tasks > 0}<span class="repo-stat"><Icon name="task" size={10} /> {stats.tasks}</span>{/if}
      {#if stats.agents > 0}<span class="repo-stat repo-stat-active"><Icon name="agent" size={10} /> {stats.agents} active</span>{/if}
      {#if stats.openMrs > 0}<span class="repo-stat"><Icon name="git-merge" size={10} /> {stats.openMrs} open</span>{/if}
      {#if stats.failedGates > 0}<span class="repo-stat repo-stat-danger">{stats.failedGates} gate fail</span>{/if}
    </div>

    <!-- Latest MR (if any) — shows recent output -->
    {#if latestMr}
      <div class="repo-card-latest">
        <Badge value={latestMr.status ?? 'open'} variant={latestMr.status === 'merged' ? 'success' : latestMr.status === 'closed' ? 'muted' : 'info'} />
        {#if goToEntityDetail}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <span class="latest-title latest-title-link" role="link" tabindex="0" onclick={(e) => { e.stopPropagation(); goToEntityDetail('mr', latestMr.id, latestMr); }}>{latestMr.title ?? 'Untitled MR'}</span>
        {:else}
          <span class="latest-title">{latestMr.title ?? 'Untitled MR'}</span>
        {/if}
        {#if latestMr._gates}
          <span class="latest-gates-summary" onclick={(e) => { e.stopPropagation(); if (goToEntityDetail) goToEntityDetail('mr', latestMr.id, { ...latestMr, _openTab: 'gates' }); }}>
            {#if latestMr._gates.details?.length > 0}
              {#each latestMr._gates.details.slice(0, 2) as g}
                <span class="gate-chip-mini gate-chip-mini-{g.status}" title="{g.name}: {g.status}">{g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'} {g.name}</span>
              {/each}
              {#if latestMr._gates.details.length > 2}
                <span class="gate-chip-mini-more">+{latestMr._gates.details.length - 2}</span>
              {/if}
            {:else}
              {#if latestMr._gates.failed > 0}<span class="gate-fail-count">&#10007;{latestMr._gates.failed}</span>{/if}
              {#if latestMr._gates.passed > 0}<span class="gate-pass-count">&#10003;{latestMr._gates.passed}</span>{/if}
            {/if}
          </span>
        {:else if latestMr.gate_status}
          <span class="latest-gate latest-gate-{latestMr.gate_status}">{latestMr.gate_status}</span>
        {/if}
      </div>
    {/if}
  </button>
{/if}

<style>
  .repo-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
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
    font-size: var(--text-sm);
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
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Compact stats bar */
  .repo-card-stats {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .repo-stat {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-weight: 500;
  }

  .repo-stat-active { color: var(--color-success); }
  .repo-stat-danger { color: var(--color-danger); font-weight: 600; }

  /* Status summary — prominent one-liner with WHY sub-text */
  .repo-card-status {
    font-size: var(--text-xs);
    font-weight: 500;
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .status-why {
    font-weight: 400;
    font-size: 10px;
    opacity: 0.8;
  }

  .repo-card-status-danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
  }

  .repo-card-status-warning {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
  }

  .repo-card-status-success {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
  }

  .repo-card-status-info {
    color: var(--color-info, #1e90ff);
    background: color-mix(in srgb, var(--color-info, #1e90ff) 8%, transparent);
  }

  /* Latest MR preview */
  .repo-card-latest {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    border-top: 1px solid var(--color-border);
  }

  .latest-title {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .latest-title-link {
    cursor: pointer;
  }

  .latest-title-link:hover {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .latest-gate {
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .latest-gate-passed {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
  }

  .latest-gate-failed {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }

  .latest-gate-pending {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
  }

  /* Latest MR gate summary (clickable to gates tab) */
  .latest-gates-summary {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
  }

  .latest-gates-summary:hover {
    text-decoration: underline;
  }

  .gate-pass-count {
    color: var(--color-success);
  }

  .gate-fail-count {
    color: var(--color-danger);
  }

  .gate-chip-mini {
    font-size: 9px;
    font-weight: 500;
    padding: 0 3px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .gate-chip-mini-passed { color: var(--color-success); }
  .gate-chip-mini-failed { color: var(--color-danger); }
  .gate-chip-mini-pending { color: var(--color-text-muted); }
  .gate-chip-mini-more { font-size: 9px; color: var(--color-text-muted); }
</style>
