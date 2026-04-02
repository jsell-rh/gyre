<script>
  /**
   * RepoCard — GitHub-style repository card for workspace home.
   *
   * Shows: name, health indicator, key stats, last activity time.
   * Click navigates to repo mode.
   */
  import { relativeTime } from '../lib/timeFormat.js';
  import Badge from '../lib/Badge.svelte';

  let {
    repo = null,
    health = 'idle',
    stats = {},
    onclick = undefined,
  } = $props();

  const HEALTH = {
    healthy: { color: 'var(--color-success)', dot: '\u25CF', label: 'Active — agents are implementing code' },
    gate: { color: 'var(--color-danger)', dot: '\u26A0', label: 'Gate failure — needs attention' },
    gate_failure: { color: 'var(--color-danger)', dot: '\u26A0', label: 'Gate failure — needs attention' },
    idle: { color: 'var(--color-text-muted)', dot: '\u25CB', label: 'Idle — no active agents' },
  };

  let h = $derived(HEALTH[health] ?? HEALTH.idle);
</script>

{#if repo}
  <button class="repo-card" onclick={onclick} data-testid="repo-card">
    <div class="repo-card-header">
      <span class="repo-card-health" style="color: {h.color}" title={h.label} aria-label={h.label}>{h.dot}</span>
      <span class="repo-card-name">{repo.name}</span>
    </div>
    {#if repo.description}
      <p class="repo-card-desc">{repo.description}</p>
    {/if}
    <div class="repo-card-stats">
      {#if stats.specs != null}
        <span class="stat-chip" title="Specs">
          <span class="stat-icon" aria-hidden="true">S</span>
          <span>{stats.specs}</span>
        </span>
      {/if}
      {#if stats.tasks != null}
        <span class="stat-chip" title="Tasks">
          <span class="stat-icon" aria-hidden="true">T</span>
          <span>{stats.tasks}</span>
        </span>
      {/if}
      {#if stats.agents != null && stats.agents > 0}
        <span class="stat-chip stat-active" title="{stats.agents} agent{stats.agents !== 1 ? 's' : ''} actively implementing code">
          <span class="stat-icon" aria-hidden="true">A</span>
          <span>{stats.agents} running</span>
        </span>
      {/if}
      {#if stats.mrs != null}
        <span class="stat-chip" title="Merge requests">
          <span class="stat-icon" aria-hidden="true">M</span>
          <span>{stats.mrs}</span>
        </span>
      {/if}
      <span class="stat-spacer"></span>
      {#if stats.last_activity}
        <span class="stat-time" title="Last activity">{relativeTime(stats.last_activity)}</span>
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
  }

  .repo-card-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .repo-card-stats {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .stat-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .stat-active {
    color: var(--color-success);
    font-weight: 600;
  }

  .stat-icon {
    font-size: 10px;
    font-weight: 700;
    opacity: 0.6;
  }

  .stat-spacer { flex: 1; }

  .stat-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }
</style>
