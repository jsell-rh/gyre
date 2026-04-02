<script>
  /**
   * PipelineOverview — compact horizontal flow showing the autonomous dev pipeline.
   *
   * Specs (3) → Tasks (7) → Agents (2 active) → MRs (4) → Merged (12)
   *
   * Replaces both "Provenance Pipeline Summary" and "Development Flow" sections
   * from WorkspaceHome with a single, compact, clickable visualization.
   */
  let {
    specs = { total: 0, pending: 0, approved: 0 },
    tasks = { total: 0, in_progress: 0, blocked: 0, done: 0 },
    agents = { total: 0, active: 0 },
    mrs = { total: 0, open: 0, merged: 0, failed_gates: 0 },
    onStageClick = undefined,
  } = $props();

  const stages = $derived([
    {
      id: 'specs',
      label: 'Specs',
      count: specs.total,
      detail: specs.pending > 0 ? `${specs.pending} pending` : specs.approved > 0 ? `${specs.approved} approved` : '',
      alert: specs.pending > 0,
      alertColor: 'var(--color-warning)',
    },
    {
      id: 'tasks',
      label: 'Tasks',
      count: tasks.total,
      detail: tasks.blocked > 0 ? `${tasks.blocked} blocked` : tasks.in_progress > 0 ? `${tasks.in_progress} active` : '',
      alert: tasks.blocked > 0,
      alertColor: 'var(--color-danger)',
    },
    {
      id: 'agents',
      label: 'Agents',
      count: agents.total,
      detail: agents.active > 0 ? `${agents.active} active` : '',
      alert: false,
      alertColor: '',
      highlight: agents.active > 0,
    },
    {
      id: 'mrs',
      label: 'MRs',
      count: mrs.total,
      detail: mrs.open > 0 ? `${mrs.open} open` : '',
      alert: mrs.failed_gates > 0,
      alertColor: 'var(--color-danger)',
    },
    {
      id: 'merged',
      label: 'Merged',
      count: mrs.merged,
      detail: '',
      alert: false,
      alertColor: '',
    },
  ]);
</script>

<div class="pipeline-overview" data-testid="pipeline-overview">
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
      onclick={() => onStageClick?.(stage.id)}
      title="{stage.label}: {stage.count}{stage.detail ? ` (${stage.detail})` : ''}"
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
</div>

<style>
  .pipeline-overview {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
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
    gap: 1px;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    min-width: 64px;
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

  @media (max-width: 640px) {
    .pipeline-stage {
      padding: var(--space-1) var(--space-2);
      min-width: 48px;
    }
    .stage-count { font-size: var(--text-base); }
    .stage-detail { display: none; }
  }
</style>
