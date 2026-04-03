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
  <button class="repo-card" class:repo-card-active={health === 'healthy'} class:repo-card-alert={health === 'gate' || health === 'gate_failure'} onclick={onclick} data-testid="repo-card" title={statusSummary ? `${statusSummary.text}\n${statusSummary.why}` : h.label}>
    <div class="repo-card-header">
      <span class="repo-card-health" style="color: {h.color}" aria-label={h.label}>{h.dot}</span>
      <span class="repo-card-name">{repo.name}</span>
      {#if stats.last_activity}
        <span class="repo-card-time">{relativeTime(stats.last_activity)}</span>
      {/if}
    </div>

    <!-- What's happening now — one line of context -->
    {#if statusSummary}
      <div class="repo-card-activity">
        <span class="repo-activity-text repo-activity-{statusSummary.variant}">{statusSummary.text}</span>
        <span class="repo-activity-why">{statusSummary.why}</span>
      </div>
    {:else if latestMr}
      <div class="repo-card-activity">
        <span class="repo-activity-text repo-activity-{latestMr.status === 'merged' ? 'success' : 'info'}">{latestMr.status === 'merged' ? 'Last merged' : 'Latest MR'}: {latestMr.title ?? 'Untitled'}</span>
      </div>
    {/if}

    <!-- Compact stats — clickable to navigate to repo tab -->
    <div class="repo-card-stats">
      {#if stats.specs > 0}<button class="repo-stat repo-stat-btn" onclick={(e) => handleStatClick('specs', e)} title="View specs">{stats.specs} spec{stats.specs !== 1 ? 's' : ''}</button>{/if}
      {#if stats.tasks > 0}<button class="repo-stat repo-stat-btn" onclick={(e) => handleStatClick('tasks', e)} title="View tasks">{stats.tasks} task{stats.tasks !== 1 ? 's' : ''}</button>{/if}
      {#if stats.agents > 0}<button class="repo-stat repo-stat-btn repo-stat-active" onclick={(e) => handleStatClick('agents', e)} title="View agents">{stats.agents} agent{stats.agents !== 1 ? 's' : ''}</button>{/if}
      {#if stats.mrs > 0}<button class="repo-stat repo-stat-btn" onclick={(e) => handleStatClick('mrs', e)} title="View merge requests">{stats.mrs} MR{stats.mrs !== 1 ? 's' : ''}{#if stats.openMrs > 0} ({stats.openMrs} open){/if}</button>{/if}
      {#if stats.failedGates > 0}<button class="repo-stat repo-stat-btn repo-stat-danger" onclick={(e) => handleStatClick('mrs', e)} title="View failed gates">{stats.failedGates} failed gate{stats.failedGates !== 1 ? 's' : ''}</button>{/if}
    </div>
  </button>
{/if}

<style>
  .repo-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
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

  /* Activity summary */
  .repo-card-activity {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-size: 10px;
  }

  .repo-activity-text {
    font-weight: 600;
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

  .repo-stat-btn {
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    color: inherit;
    padding: 0 2px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .repo-stat-btn:hover {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    text-decoration: underline;
  }

  .repo-stat-active { color: var(--color-success); }
  .repo-stat-danger { color: var(--color-danger); font-weight: 600; }
  .repo-stat-warning { color: var(--color-warning); }
  .repo-stat-info { color: var(--color-info, #1e90ff); }
  .repo-stat-success { color: var(--color-success); }

  .repo-stat-summary {
    font-size: 10px;
    font-weight: 500;
    margin-left: auto;
  }
</style>
