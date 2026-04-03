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
  import Badge from '../lib/Badge.svelte';
  import Icon from '../lib/Icon.svelte';
  import { entityName, shortId } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import { specStatusTooltip, taskStatusTooltip, mrStatusTooltip, agentStatusTooltip } from '../lib/statusTooltips.js';

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
  } = $props();

  let expandedStage = $state(null);

  function toggleStage(stageId) {
    expandedStage = expandedStage === stageId ? null : stageId;
    onStageClick?.(stageId);
  }

  function nav(type, id, data) {
    goToEntityDetail?.(type, id, data ?? {});
  }

  const stages = $derived([
    {
      id: 'specs',
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
      label: 'Tasks',
      count: tasks.total,
      detail: tasks.blocked > 0 ? `${tasks.blocked} blocked` : tasks.in_progress > 0 ? `${tasks.in_progress} active` : tasks.done > 0 ? `${tasks.done} done` : '',
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

  // Filtered entity lists for each expanded stage
  let expandedSpecs = $derived(specsList.filter(s => {
    const st = s.approval_status ?? s.status;
    return st === 'pending' || st === 'approved' || st === 'draft';
  }).slice(0, 8));

  let expandedTasks = $derived(tasksList.filter(t =>
    t.status === 'in_progress' || t.status === 'blocked' || t.status === 'backlog'
  ).slice(0, 8));

  let expandedAgents = $derived(agentsList.filter(a =>
    a.status === 'active'
  ).slice(0, 8));

  let expandedMrs = $derived(mrsList.filter(m =>
    m.status === 'open'
  ).slice(0, 8));

  let expandedMerged = $derived(mrsList.filter(m =>
    m.status === 'merged'
  ).sort((a, b) => (b.merged_at ?? b.updated_at ?? 0) - (a.merged_at ?? a.updated_at ?? 0)).slice(0, 5));
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
        class:is-expanded={expandedStage === stage.id}
        onclick={() => toggleStage(stage.id)}
        title={stage.tooltip}
        aria-expanded={expandedStage === stage.id}
      >
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
    {#if budget?.config?.monthly_limit_usd}
      {@const pct = budget.usage?.total_cost_usd ? Math.round((budget.usage.total_cost_usd / budget.config.monthly_limit_usd) * 100) : 0}
      <span class="pipeline-budget" title="Budget: ${budget.usage?.total_cost_usd?.toFixed(2) ?? '0'} / ${budget.config.monthly_limit_usd} ({pct}% used)">
        <span class="budget-bar-mini"><span class="budget-bar-fill" class:budget-warn={pct > 75} class:budget-danger={pct > 90} style="width: {Math.min(pct, 100)}%"></span></span>
        <span class="budget-pct" class:budget-warn={pct > 75} class:budget-danger={pct > 90}>{pct}%</span>
      </span>
    {/if}
  </div>

  <!-- Expandable entity list -->
  {#if expandedStage}
    <div class="pipeline-expansion">
      {#if expandedStage === 'specs'}
        {#if expandedSpecs.length === 0}
          <p class="expansion-empty">No specs yet. Push a spec manifest to get started.</p>
        {:else}
          <div class="expansion-list">
            {#each expandedSpecs as spec}
              {@const status = spec.approval_status ?? spec.status}
              <div class="expansion-row">
                <button class="expansion-name" onclick={() => onNavigateSpec?.(spec)} title={specStatusTooltip(status)}>
                  <span class="expansion-dot expansion-dot-{status === 'pending' ? 'warn' : status === 'approved' ? 'ok' : 'muted'}"></span>
                  <span class="expansion-label">{spec.path?.split('/').pop()?.replace(/\.md$/, '') ?? spec.path}</span>
                  <Badge value={status} variant={status === 'approved' ? 'success' : status === 'pending' ? 'warning' : status === 'rejected' ? 'danger' : 'muted'} />
                </button>
                {#if status === 'pending' && onApproveSpec}
                  <span class="expansion-actions">
                    <button class="expansion-action-btn expansion-approve" onclick={(e) => onApproveSpec(spec, e)}>Approve</button>
                    {#if onRejectSpec}
                      <button class="expansion-action-btn expansion-reject" onclick={(e) => onRejectSpec(spec, e)}>Reject</button>
                    {/if}
                  </span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

      {:else if expandedStage === 'tasks'}
        {#if expandedTasks.length === 0}
          <p class="expansion-empty">No active tasks. Approve specs to generate tasks.</p>
        {:else}
          <div class="expansion-list">
            {#each expandedTasks as task}
              <button class="expansion-row expansion-clickable" onclick={() => nav('task', task.id, task)} title={taskStatusTooltip(task)}>
                <span class="expansion-dot expansion-dot-{task.status === 'in_progress' ? 'ok' : task.status === 'blocked' ? 'danger' : 'muted'}"></span>
                <span class="expansion-label">{task.title ?? entityName('task', task.id)}</span>
                <Badge value={task.status} variant={task.status === 'in_progress' ? 'success' : task.status === 'blocked' ? 'danger' : 'muted'} />
                {#if task.spec_path}
                  <span class="expansion-meta">{task.spec_path.split('/').pop()?.replace(/\.md$/, '')}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

      {:else if expandedStage === 'agents'}
        {#if expandedAgents.length === 0}
          <p class="expansion-empty">No agents currently running.</p>
        {:else}
          <div class="expansion-list">
            {#each expandedAgents as agent}
              {@const spawnedAt = agent.created_at ?? agent.spawned_at}
              {@const elapsed = spawnedAt ? Math.round((Date.now() / 1000 - spawnedAt) / 60) : 0}
              <button class="expansion-row expansion-clickable" onclick={() => nav('agent', agent.id, agent)} title={agentStatusTooltip(agent.status)}>
                <span class="expansion-dot expansion-dot-ok"></span>
                <span class="expansion-label">{agent.name ?? entityName('agent', agent.id)}</span>
                {#if agent.spec_path}
                  <span class="expansion-meta">{agent.spec_path.split('/').pop()?.replace(/\.md$/, '')}</span>
                {/if}
                <span class="expansion-time">{elapsed < 60 ? `${elapsed}m` : `${Math.floor(elapsed/60)}h`}</span>
              </button>
            {/each}
          </div>
        {/if}

      {:else if expandedStage === 'mrs'}
        {#if expandedMrs.length === 0}
          <p class="expansion-empty">No open merge requests.</p>
        {:else}
          <div class="expansion-list">
            {#each expandedMrs as mr}
              <div class="expansion-row">
                <button class="expansion-name" onclick={() => nav('mr', mr.id, mr)} title={mrStatusTooltip(mr)}>
                  {#if mr._gates?.failed > 0}
                    <span class="expansion-dot expansion-dot-danger"></span>
                  {:else if mr.queue_position != null}
                    <span class="expansion-dot expansion-dot-ok"></span>
                  {:else}
                    <span class="expansion-dot expansion-dot-info"></span>
                  {/if}
                  <span class="expansion-label">{mr.title ?? 'Untitled'}</span>
                  {#if mr._gates?.total > 0}
                    <span class="expansion-gates">
                      {#if mr._gates.failed > 0}<span class="gate-fail-inline">✗{mr._gates.failed}</span>{/if}
                      {#if mr._gates.passed > 0}<span class="gate-pass-inline">✓{mr._gates.passed}</span>{/if}
                    </span>
                  {/if}
                </button>
                {#if mr.status === 'open' && mr.queue_position == null && onEnqueueMr}
                  <button class="expansion-action-btn expansion-approve" onclick={(e) => onEnqueueMr(mr, e)}>Enqueue</button>
                {:else if mr.queue_position != null}
                  <span class="expansion-meta">#{mr.queue_position + 1}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

      {:else if expandedStage === 'merged'}
        {#if expandedMerged.length === 0}
          <p class="expansion-empty">No merged MRs yet.</p>
        {:else}
          <div class="expansion-list">
            {#each expandedMerged as mr}
              <button class="expansion-row expansion-clickable" onclick={() => nav('mr', mr.id, mr)} title={mrStatusTooltip(mr)}>
                <span class="expansion-dot expansion-dot-ok"></span>
                <span class="expansion-label">{mr.title ?? 'Untitled'}</span>
                {#if mr.merge_commit_sha}
                  <code class="expansion-sha">{mr.merge_commit_sha.slice(0, 7)}</code>
                {/if}
                {#if mr.merged_at ?? mr.updated_at}
                  <span class="expansion-time">{relativeTime(mr.merged_at ?? mr.updated_at)}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
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

  .pipeline-stage.is-expanded {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 6%, transparent);
  }

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

  /* ── Expansion panel ─────────────────────────────────────────── */
  .pipeline-expansion {
    border-top: 1px solid var(--color-border);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    max-height: 320px;
    overflow-y: auto;
  }

  .expansion-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: var(--space-1) 0;
    text-align: center;
  }

  .expansion-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .expansion-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-2);
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .expansion-row:hover {
    background: var(--color-surface);
  }

  .expansion-clickable {
    cursor: pointer;
    background: transparent;
    border: none;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    color: var(--color-text);
  }

  .expansion-name {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    color: var(--color-text);
    padding: 0;
  }

  .expansion-name:hover .expansion-label {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .expansion-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .expansion-dot-ok { background: var(--color-success); }
  .expansion-dot-warn { background: var(--color-warning); }
  .expansion-dot-danger { background: var(--color-danger); }
  .expansion-dot-info { background: var(--color-info, #1e90ff); }
  .expansion-dot-muted { background: var(--color-text-muted); }

  .expansion-label {
    font-size: var(--text-xs);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .expansion-meta {
    font-size: 10px;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .expansion-time {
    font-size: 10px;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .expansion-sha {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .expansion-gates {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
    font-size: 10px;
  }

  .expansion-actions {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  .expansion-action-btn {
    font-family: var(--font-body);
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    border: none;
    transition: background var(--transition-fast);
  }

  .expansion-approve {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }
  .expansion-approve:hover { background: color-mix(in srgb, var(--color-success) 25%, transparent); }

  .expansion-reject {
    color: var(--color-text-muted);
    background: transparent;
  }
  .expansion-reject:hover { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 12%, transparent); }

  .gate-fail-inline {
    color: var(--color-danger);
    font-weight: 600;
  }

  .gate-pass-inline {
    color: var(--color-success);
  }

  @media (max-width: 640px) {
    .pipeline-stage {
      padding: var(--space-1) var(--space-2);
      min-width: 48px;
    }
    .stage-count { font-size: var(--text-base); }
    .stage-detail { display: none; }
  }
</style>
