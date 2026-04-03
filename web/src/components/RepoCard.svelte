<script>
  /**
   * RepoCard — GitHub-style repository card for workspace home.
   *
   * Shows: name, health indicator, key stats, last activity time.
   * Enhanced: shows latest activity summary and active work context.
   * Click navigates to repo mode.
   */
  import { relativeTime } from '../lib/timeFormat.js';
  import Badge from '../lib/Badge.svelte';
  import { entityName, shortId } from '../lib/entityNames.svelte.js';

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

    {#if repo.description}
      <p class="repo-card-desc">{repo.description}</p>
    {/if}

    <!-- Status summary — the ONE most important thing about this repo right now -->
    {#if statusSummary}
      <div class="repo-card-status repo-card-status-{statusSummary.variant}">
        <span class="status-text">{statusSummary.text}</span>
        {#if statusSummary.why}
          <span class="status-why">{statusSummary.why}</span>
        {/if}
      </div>
    {/if}

    <!-- Active agent names — show what agents are doing -->
    {#if activeAgentNames.length > 0}
      <div class="repo-card-agents">
        {#each activeAgentNames.slice(0, 3) as name}
          <span class="agent-chip">{name}</span>
        {/each}
        {#if activeAgentNames.length > 3}
          <span class="agent-chip agent-chip-more">+{activeAgentNames.length - 3}</span>
        {/if}
      </div>
    {/if}

    <!-- Latest MR (if any) — shows recent output -->
    {#if latestMr}
      <div class="repo-card-latest">
        <Badge value={latestMr.status ?? 'open'} variant={latestMr.status === 'merged' ? 'success' : latestMr.status === 'closed' ? 'muted' : 'info'} />
        <span class="latest-title">{latestMr.title ?? 'Untitled MR'}</span>
      </div>
    {/if}

    <!-- Compact stats row -->
    <div class="repo-card-stats">
      {#if stats.specs != null && stats.specs > 0}
        <span class="stat-chip" title="{stats.specs} specs{specBreakdown?.approved ? `, ${specBreakdown.approved} approved` : ''}">
          <span class="stat-icon" aria-hidden="true">S</span>{stats.specs}
        </span>
      {/if}
      {#if stats.tasks != null && stats.tasks > 0}
        <span class="stat-chip" title="{stats.tasks} tasks">
          <span class="stat-icon" aria-hidden="true">T</span>{stats.tasks}
        </span>
      {/if}
      {#if stats.mrs != null && stats.mrs > 0}
        <span class="stat-chip" title="{stats.mrs} merge requests">
          <span class="stat-icon" aria-hidden="true">M</span>{stats.mrs}
        </span>
      {/if}
    </div>
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

  /* Active agent chips */
  .repo-card-agents {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .agent-chip {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .agent-chip-more {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
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

  /* Compact stats */
  .repo-card-stats {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .stat-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .stat-icon {
    font-size: 10px;
    font-weight: 700;
    opacity: 0.6;
  }
</style>
