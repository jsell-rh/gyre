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
        <button class="repo-activity-link repo-activity-{latestMr.status === 'merged' ? 'success' : 'info'}" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', latestMr.id, { repo_id: repo.id, title: latestMr.title }); }}>{latestMr.status === 'merged' ? 'Last merged' : 'Latest MR'}: {latestMr.title ?? 'Untitled'}</button>
      </div>
    {/if}

    <!-- Mini pipeline flow — shows where this repo is in the lifecycle -->
    {#if stats.specs > 0 || stats.tasks > 0 || stats.agents > 0 || stats.mrs > 0}
      <div class="repo-card-pipeline">
        <button class="repo-pipe-stage" class:repo-pipe-active={specBreakdown?.pending > 0} class:repo-pipe-done={stats.specs > 0 && !specBreakdown?.pending} onclick={(e) => handleStatClick('specs', e)} title="{stats.specs} spec{stats.specs !== 1 ? 's' : ''}{specBreakdown?.pending ? ` (${specBreakdown.pending} pending)` : ''}">
          <span class="repo-pipe-count">{stats.specs}</span>
          <span class="repo-pipe-label">Specs</span>
        </button>
        <span class="repo-pipe-arrow">→</span>
        <button class="repo-pipe-stage" class:repo-pipe-active={stats.tasks > 0} onclick={(e) => handleStatClick('tasks', e)} title="{stats.tasks} task{stats.tasks !== 1 ? 's' : ''}">
          <span class="repo-pipe-count">{stats.tasks}</span>
          <span class="repo-pipe-label">Tasks</span>
        </button>
        <span class="repo-pipe-arrow">→</span>
        <button class="repo-pipe-stage" class:repo-pipe-active={stats.agents > 0} onclick={(e) => handleStatClick('agents', e)} title="{stats.agents} active agent{stats.agents !== 1 ? 's' : ''}">
          <span class="repo-pipe-count">{stats.agents}</span>
          <span class="repo-pipe-label">Agents</span>
        </button>
        <span class="repo-pipe-arrow">→</span>
        <button class="repo-pipe-stage" class:repo-pipe-active={stats.openMrs > 0} class:repo-pipe-warn={stats.failedGates > 0} onclick={(e) => handleStatClick('mrs', e)} title="{stats.mrs} MR{stats.mrs !== 1 ? 's' : ''}{stats.openMrs > 0 ? ` (${stats.openMrs} open)` : ''}{stats.failedGates > 0 ? ` — ${stats.failedGates} gate failures` : ''}">
          <span class="repo-pipe-count">{stats.mrs}</span>
          <span class="repo-pipe-label">MRs</span>
        </button>
        {#if stats.mrs > 0 || stats.specs > 0}
          <button class="repo-pipe-code" onclick={(e) => handleStatClick('code', e)} title="Browse code with agent attribution">
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" width="10" height="10"><path d="M5 4L1 8l4 4M11 4l4 4-4 4"/></svg>
          </button>
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
    gap: 1px;
    font-size: 10px;
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

  /* Mini pipeline flow in repo card */
  .repo-card-pipeline {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
  }

  .repo-pipe-stage {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0;
    padding: 1px 4px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    min-width: 32px;
    transition: all var(--transition-fast);
  }

  .repo-pipe-stage:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }

  .repo-pipe-count {
    font-size: 11px;
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    line-height: 1;
  }

  .repo-pipe-active .repo-pipe-count { color: var(--color-primary); }
  .repo-pipe-done .repo-pipe-count { color: var(--color-success); }
  .repo-pipe-warn .repo-pipe-count { color: var(--color-danger); }

  .repo-pipe-label {
    font-size: 8px;
    font-weight: 500;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .repo-pipe-arrow {
    color: var(--color-text-muted);
    font-size: 8px;
    opacity: 0.5;
    flex-shrink: 0;
  }

  .repo-pipe-code {
    display: flex;
    align-items: center;
    padding: 2px 4px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--color-text-muted);
    margin-left: auto;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .repo-pipe-code:hover {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
  }
</style>
