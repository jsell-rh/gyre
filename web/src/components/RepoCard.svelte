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
          {latestMr.status === 'merged' ? 'Merged' : 'Latest'}: {latestMr.title ?? 'Untitled'}
        </button>
        {#if latestMr.diff_stats}
          <button class="repo-activity-diff" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', latestMr.id, { repo_id: repo.id, title: latestMr.title, _openTab: 'diff' }); }} title="View diff">
            <span class="diff-ins-tiny">+{latestMr.diff_stats.insertions ?? 0}</span>
            <span class="diff-del-tiny">-{latestMr.diff_stats.deletions ?? 0}</span>
          </button>
        {/if}
        {#if latestMr.merge_commit_sha}
          <span class="repo-merge-sha" title="Merge commit: {latestMr.merge_commit_sha}">{latestMr.merge_commit_sha.slice(0, 7)}</span>
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

    <!-- Pipeline flow — shows where this repo is in the autonomous cycle -->
    {#if stats.specs > 0 || stats.tasks > 0 || stats.agents > 0 || stats.mrs > 0}
      <div class="repo-pipeline-flow">
        <button class="repo-flow-stage" class:repo-flow-active={specBreakdown?.pending > 0} class:repo-flow-done={stats.specs > 0 && !specBreakdown?.pending} onclick={(e) => handleStatClick('specs', e)} title="{stats.specs} spec{stats.specs !== 1 ? 's' : ''}{specBreakdown?.pending ? ` (${specBreakdown.pending} pending)` : ''}">
          <span class="repo-flow-count" class:repo-flow-warn={specBreakdown?.pending > 0}>{stats.specs}</span>
          <span class="repo-flow-label">specs</span>
        </button>
        <span class="repo-flow-arrow">→</span>
        <button class="repo-flow-stage" class:repo-flow-active={stats.tasks > 0} onclick={(e) => handleStatClick('tasks', e)} title="{stats.tasks} task{stats.tasks !== 1 ? 's' : ''}">
          <span class="repo-flow-count">{stats.tasks}</span>
          <span class="repo-flow-label">tasks</span>
        </button>
        <span class="repo-flow-arrow">→</span>
        <button class="repo-flow-stage" class:repo-flow-active={stats.agents > 0} onclick={(e) => handleStatClick('agents', e)} title="{stats.agents} active agent{stats.agents !== 1 ? 's' : ''}">
          <span class="repo-flow-count" class:repo-flow-live={stats.agents > 0}>{stats.agents}</span>
          <span class="repo-flow-label">agents</span>
        </button>
        <span class="repo-flow-arrow">→</span>
        <button class="repo-flow-stage" class:repo-flow-active={stats.openMrs > 0} class:repo-flow-alert={stats.failedGates > 0} onclick={(e) => handleStatClick('mrs', e)} title="{stats.mrs} MR{stats.mrs !== 1 ? 's' : ''}{stats.failedGates > 0 ? ` (${stats.failedGates} gate failures)` : ''}">
          <span class="repo-flow-count" class:repo-flow-danger={stats.failedGates > 0}>{stats.mrs}</span>
          <span class="repo-flow-label">MRs</span>
        </button>
        <button class="repo-flow-stage repo-flow-stage-code" onclick={(e) => handleStatClick('code', e)} title="Browse code">
          <span class="repo-flow-label">code</span>
        </button>
        <button class="repo-flow-stage repo-flow-stage-settings" onclick={(e) => handleStatClick('settings', e)} title="Repo settings" aria-label="Settings">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="11" height="11" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
        </button>
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
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 0 3px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .repo-activity-diff:hover { background: var(--color-surface-elevated); border-color: var(--color-border); }

  .diff-ins-tiny { color: var(--color-success); font-weight: 600; }
  .diff-del-tiny { color: var(--color-danger); font-weight: 600; }

  .repo-merge-sha {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }

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

  /* Pipeline flow — mini pipeline showing repo's autonomous cycle stage */
  .repo-pipeline-flow {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-top: var(--space-1);
    padding-top: var(--space-1);
    border-top: 1px solid var(--color-border);
  }

  .repo-flow-stage {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0;
    padding: 1px 6px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    transition: all var(--transition-fast);
    min-width: 32px;
  }

  .repo-flow-stage:hover {
    background: var(--color-surface-elevated);
  }

  .repo-flow-stage.repo-flow-active {
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  .repo-flow-stage.repo-flow-done .repo-flow-count {
    color: var(--color-success);
  }

  .repo-flow-stage.repo-flow-alert .repo-flow-count {
    color: var(--color-danger);
  }

  .repo-flow-count {
    font-size: var(--text-sm);
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
    line-height: 1;
  }

  .repo-flow-count.repo-flow-warn { color: var(--color-warning); }
  .repo-flow-count.repo-flow-live { color: var(--color-success); }
  .repo-flow-count.repo-flow-danger { color: var(--color-danger); }

  .repo-flow-label {
    font-size: 9px;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    line-height: 1;
  }

  .repo-flow-arrow {
    color: var(--color-text-muted);
    font-size: 9px;
    opacity: 0.5;
    flex-shrink: 0;
  }

  .repo-flow-stage-code {
    margin-left: auto;
  }

  .repo-flow-stage-settings {
    color: var(--color-text-muted);
  }
</style>
