<script>
  /**
   * PipelineOverview — expandable hero showing the autonomous dev pipeline.
   *
   * Specs (3) → Tasks (7) → Agents (2 active) → MRs (4) → Merged (12)
   *
   * Clicking a stage expands an inline entity list below the pipeline bar
   * with quick actions (approve specs, enqueue MRs). This replaces the
   * old sidebar panels entirely.
   */
  import { getContext } from 'svelte';
  import Icon from '../lib/Icon.svelte';

  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  let {
    specs = { total: 0, pending: 0, approved: 0 },
    tasks = { total: 0, in_progress: 0, blocked: 0, done: 0 },
    agents = { total: 0, active: 0 },
    mrs = { total: 0, open: 0, merged: 0, failed_gates: 0 },
    budget = null,
    // Entity lists for expandable detail
    specsList = [],
    tasksList = [],
    agentsList = [],
    mrsList = [],
    // Quick action callbacks
    onApproveSpec = undefined,
    onRejectSpec = undefined,
    onEnqueueMr = undefined,
    onStageClick = undefined,
    onNavigateSpec = undefined,
    // Breaking change impact
    breakingCount = 0,
    onImpactAnalysis = undefined,
  } = $props();

  function handleStageClick(stageId) {
    onStageClick?.(stageId);
  }

  function nav(type, id, data) {
    goToEntityDetail?.(type, id, data ?? {});
  }

  const stages = $derived([
    {
      id: 'specs',
      icon: 'spec',
      label: 'Specs',
      count: specs.total,
      detail: specs.pending > 0 ? `${specs.pending} pending` : specs.approved > 0 ? `${specs.approved} approved` : '',
      tooltip: specs.pending > 0
        ? `${specs.pending} spec(s) awaiting your approval before agents can begin work`
        : specs.approved > 0
        ? `${specs.approved} approved spec(s) ready for implementation`
        : 'No specs yet — push a spec manifest to get started',
      alert: specs.pending > 0,
      alertColor: 'var(--color-warning)',
    },
    {
      id: 'tasks',
      icon: 'task',
      label: 'Tasks',
      count: tasks.total,
      detail: tasks.blocked > 0 ? `${tasks.blocked} blocked` : tasks.in_progress > 0 ? `${tasks.in_progress} active` : tasks.done > 0 ? `${tasks.done}/${tasks.total} done` : '',
      tooltip: tasks.blocked > 0
        ? `${tasks.blocked} task(s) blocked — may need dependency resolution or spec changes`
        : tasks.in_progress > 0
        ? `${tasks.in_progress} task(s) being worked on by agents`
        : tasks.done > 0
        ? `${tasks.done} of ${tasks.total} tasks completed`
        : 'No tasks yet — approve specs to generate tasks',
      alert: tasks.blocked > 0,
      alertColor: 'var(--color-danger)',
    },
    {
      id: 'agents',
      icon: 'agent',
      label: 'Agents',
      count: agents.total,
      detail: agents.active > 0 ? `${agents.active} running` : '',
      tooltip: agents.active > 0
        ? `${agents.active} agent(s) actively implementing code`
        : agents.total > 0
        ? `${agents.total} agent(s) have run — none currently active`
        : 'No agents spawned yet',
      alert: false,
      alertColor: '',
      highlight: agents.active > 0,
    },
    {
      id: 'mrs',
      icon: 'git-merge',
      label: 'MRs',
      count: mrs.total,
      detail: mrs.failed_gates > 0 ? `${mrs.failed_gates} failed` : mrs.open > 0 ? `${mrs.open} open` : '',
      tooltip: mrs.failed_gates > 0
        ? `${mrs.failed_gates} MR(s) with gate failures — review gates tab for details`
        : mrs.open > 0
        ? `${mrs.open} MR(s) open and ready for merge queue`
        : mrs.total > 0
        ? `${mrs.total} merge request(s) total`
        : 'No merge requests yet',
      alert: mrs.failed_gates > 0,
      alertColor: 'var(--color-danger)',
    },
    {
      id: 'merged',
      icon: 'check',
      label: 'Merged',
      count: mrs.merged,
      detail: '',
      tooltip: mrs.merged > 0
        ? `${mrs.merged} MR(s) successfully passed gates and merged to main`
        : 'No merges yet',
      alert: false,
      alertColor: '',
      highlight: mrs.merged > 0,
    },
  ]);

  // Entity lists are passed through but not expanded inline — stages navigate to repo tabs
</script>

<div class="pipeline-overview" data-testid="pipeline-overview">
  <div class="pipeline-bar">
    {#each stages as stage, i}
      {#if i > 0}
        <span class="pipeline-arrow" aria-hidden="true">
          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M4 8h8M9 4l4 4-4 4"/>
          </svg>
        </span>
      {/if}
      <button
        class="pipeline-stage"
        class:has-alert={stage.alert}
        class:has-highlight={stage.highlight}
        onclick={() => handleStageClick(stage.id)}
        title={stage.tooltip}
      >
        <span class="stage-icon"><Icon name={stage.icon} size={14} /></span>
        <span class="stage-count" style={stage.alert ? `color: ${stage.alertColor}` : ''}>{stage.count}</span>
        <span class="stage-label">{stage.label}</span>
        {#if stage.detail}
          <span class="stage-detail" style={stage.alert ? `color: ${stage.alertColor}` : ''}>{stage.detail}</span>
        {/if}
        {#if stage.alert}
          <span class="stage-alert-dot" style="background: {stage.alertColor}" aria-hidden="true"></span>
        {/if}
      </button>
    {/each}
    {#if breakingCount > 0}
      <button
        class="pipeline-breaking"
        onclick={() => onImpactAnalysis?.()}
        title="{breakingCount} breaking change{breakingCount !== 1 ? 's' : ''} — click for impact analysis"
        data-testid="pipeline-impact-btn"
      >
        <span class="breaking-icon-mini">⚠</span>
        <span class="breaking-count">{breakingCount}</span>
        <span class="breaking-label">Breaking</span>
      </button>
    {/if}
    {#if budget?.config?.monthly_limit_usd}
      {@const pct = budget.usage?.total_cost_usd ? Math.round((budget.usage.total_cost_usd / budget.config.monthly_limit_usd) * 100) : 0}
      <span class="pipeline-budget" title="Budget: ${budget.usage?.total_cost_usd?.toFixed(2) ?? '0'} / ${budget.config.monthly_limit_usd} ({pct}% used)">
        <span class="budget-bar-mini"><span class="budget-bar-fill" class:budget-warn={pct > 75} class:budget-danger={pct > 90} style="width: {Math.min(pct, 100)}%"></span></span>
        <span class="budget-pct" class:budget-warn={pct > 75} class:budget-danger={pct > 90}>{pct}%</span>
      </span>
    {/if}
  </div>

</div>

<style>
  .pipeline-overview {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .pipeline-bar {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    overflow-x: auto;
  }

  .pipeline-arrow {
    color: var(--color-text-muted);
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .pipeline-stage {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0;
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    min-width: 56px;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .pipeline-stage:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }

  .pipeline-stage:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .pipeline-stage.has-alert {
    background: color-mix(in srgb, var(--color-danger) 4%, transparent);
  }

  .pipeline-stage.has-highlight {
    background: color-mix(in srgb, var(--color-success) 6%, transparent);
  }

  .stage-icon {
    display: flex;
    align-items: center;
    color: var(--color-text-muted);
  }

  .has-alert .stage-icon { color: inherit; }
  .has-highlight .stage-icon { color: var(--color-success); }

  .stage-count {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    line-height: 1;
  }

  .stage-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 500;
  }

  .stage-detail {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .stage-alert-dot {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .pipeline-breaking {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .pipeline-breaking:hover {
    background: color-mix(in srgb, var(--color-danger) 14%, transparent);
  }

  .pipeline-breaking:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .breaking-icon-mini {
    font-size: var(--text-xs);
  }

  .breaking-count {
    font-size: var(--text-sm);
    font-weight: 700;
    color: var(--color-danger);
  }

  .breaking-label {
    font-size: var(--text-xs);
    color: var(--color-danger);
    font-weight: 500;
  }

  .pipeline-budget {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    margin-left: auto;
    flex-shrink: 0;
  }

  .budget-bar-mini {
    width: 40px;
    height: 4px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  .budget-bar-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .budget-bar-fill.budget-warn { background: var(--color-warning); }
  .budget-bar-fill.budget-danger { background: var(--color-danger); }

  .budget-pct {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    font-weight: 500;
  }

  .budget-pct.budget-warn { color: var(--color-warning); }
  .budget-pct.budget-danger { color: var(--color-danger); }

  @media (max-width: 640px) {
    .pipeline-stage {
      padding: var(--space-1) var(--space-2);
      min-width: 48px;
    }
    .stage-count { font-size: var(--text-base); }
    .stage-detail { display: none; }
  }
</style>
