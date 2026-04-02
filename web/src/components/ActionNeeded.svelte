<script>
  /**
   * ActionNeeded — compact attention bar for workspace home.
   *
   * Consolidates: pending decisions, blocked tasks, gate failures, budget warnings.
   * Shows what needs human attention right now.
   */
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import Badge from '../lib/Badge.svelte';

  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  let { items = [], onAction = undefined, onSelectRepo = undefined, onApproveSpec = undefined, onRejectSpec = undefined, onRetryGate = undefined, onDismiss = undefined } = $props();

  // Normalize PascalCase notification types
  const TYPE_NORM = {
    AgentCompleted: 'agent_completed', AgentFailed: 'agent_failed',
    SpecPendingApproval: 'spec_approval', SpecApproved: 'spec_approved',
    SpecRejected: 'spec_rejected', MrMerged: 'mr_merged',
    MrCreated: 'mr_created', MrNeedsReview: 'mr_needs_review',
    GateFailure: 'gate_failure', SuggestedSpecLink: 'suggested_link',
    TaskCreated: 'task_created', BudgetWarning: 'budget_warning',
  };

  function normType(t) {
    return TYPE_NORM[t] ?? t?.toLowerCase?.()?.replace(/([A-Z])/g, '_$1').replace(/^_/, '') ?? t;
  }

  const URGENCY = {
    gate_failure: { color: 'var(--color-danger)', icon: '!', label: 'Gate Failed' },
    spec_approval: { color: 'var(--color-warning)', icon: '?', label: 'Needs Approval' },
    agent_clarification: { color: 'var(--color-warning)', icon: '?', label: 'Agent Question' },
    budget_warning: { color: 'var(--color-warning)', icon: '$', label: 'Budget Alert' },
    mr_needs_review: { color: 'var(--color-info)', icon: 'R', label: 'Review Needed' },
    agent_failed: { color: 'var(--color-danger)', icon: '!', label: 'Agent Failed' },
  };

  let showAll = $state(false);
  let maxVisible = 5;

  let actionable = $derived(items.filter(n => {
    const nt = normType(n.notification_type);
    return URGENCY[nt] && !n.dismissed_at && !n.resolved_at;
  }));

  let visible = $derived(showAll ? actionable : actionable.slice(0, maxVisible));

  function getUrgency(n) {
    return URGENCY[normType(n.notification_type)] ?? { color: 'var(--color-text-muted)', icon: 'i', label: 'Info' };
  }

  function parseBody(n) {
    try { return typeof n.body === 'string' ? JSON.parse(n.body) : (n.body ?? {}); }
    catch { return {}; }
  }

  function getTitle(n) {
    const body = parseBody(n);
    const nt = normType(n.notification_type);
    // Provide context-rich titles instead of raw notification text
    if (nt === 'gate_failure' && body.gate_name) {
      return `Gate "${body.gate_name}" failed${body.mr_title ? ` on "${body.mr_title}"` : ''}`;
    }
    if (nt === 'spec_approval') {
      const specName = (body.spec_path ?? n.title ?? '').split('/').pop()?.replace(/\.md$/, '') ?? 'spec';
      return `Spec "${specName}" needs review before agents can begin`;
    }
    if (nt === 'agent_failed' && body.agent_name) {
      return `Agent "${body.agent_name}" failed${body.error ? `: ${body.error}` : ''}`;
    }
    return n.title ?? n.message ?? normType(n.notification_type).replace(/_/g, ' ');
  }

  function getEntityType(n) {
    const body = parseBody(n);
    if (body.spec_path ?? n.spec_path) return 'spec';
    if (body.mr_id ?? n.mr_id) return 'mr';
    if (body.agent_id ?? n.agent_id) return 'agent';
    if (body.task_id ?? n.task_id) return 'task';
    return null;
  }

  function getEntityId(n) {
    const body = parseBody(n);
    return body.spec_path ?? n.spec_path ?? body.mr_id ?? n.mr_id ?? body.agent_id ?? n.agent_id ?? body.task_id ?? n.task_id ?? null;
  }

  function handleClick(n) {
    const type = getEntityType(n);
    const id = getEntityId(n);
    if (type && id && goToEntityDetail) {
      goToEntityDetail(type, id, { repo_id: n.repo_id });
    }
  }
</script>

{#if actionable.length > 0}
  <section class="action-needed" data-testid="action-needed">
    <h3 class="action-needed-title">
      <span class="action-needed-icon" aria-hidden="true">!</span>
      Needs attention ({actionable.length})
    </h3>
    <div class="action-items">
      {#each visible as item}
        {@const urgency = getUrgency(item)}
        {@const body = parseBody(item)}
        {@const nt = normType(item.notification_type)}
        <button
          class="action-item"
          style="border-left-color: {urgency.color}"
          onclick={() => handleClick(item)}
          title={getTitle(item)}
        >
          <span class="action-icon" style="color: {urgency.color}">{urgency.icon}</span>
          <div class="action-body">
            <span class="action-label">{urgency.label}</span>
            <span class="action-title">{getTitle(item)}</span>
          </div>
          <div class="action-buttons">
            {#if nt === 'spec_approval'}
              <button class="action-btn action-btn-review" onclick={(e) => { e.stopPropagation(); handleClick(item); }} title="Review spec before approving">Review</button>
            {:else if nt === 'gate_failure' && onRetryGate && body.mr_id}
              <button class="action-btn" onclick={(e) => { e.stopPropagation(); onRetryGate(item); }} title="Re-enqueue for merge">Retry</button>
            {:else if (nt === 'agent_completed' || nt === 'mr_needs_review') && body.mr_id}
              <button class="action-btn" onclick={(e) => { e.stopPropagation(); handleClick(item); }} title="Review merge request">Review</button>
            {/if}
            {#if onDismiss}
              <button class="action-btn action-btn-dismiss" onclick={(e) => { e.stopPropagation(); onDismiss(item); }} title="Dismiss">✕</button>
            {/if}
          </div>
          <span class="action-time">{relativeTime(item.created_at)}</span>
        </button>
      {/each}
    </div>
    {#if actionable.length > maxVisible}
      <button class="action-show-all" onclick={() => showAll = !showAll}>
        {showAll ? 'Show less' : `Show all ${actionable.length}`}
      </button>
    {/if}
  </section>
{/if}

<style>
  .action-needed {
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-warning) 4%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-warning) 20%, var(--color-border));
    border-radius: var(--radius);
  }

  .action-needed-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3) 0;
  }

  .action-needed-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--color-warning);
    color: var(--color-text-inverse);
    font-size: 10px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .action-items {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .action-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-left: 3px solid var(--color-text-muted);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .action-item:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border-strong);
  }

  .action-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .action-icon {
    font-size: var(--text-sm);
    font-weight: 700;
    flex-shrink: 0;
    width: 18px;
    text-align: center;
  }

  .action-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .action-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
  }

  .action-title {
    font-size: var(--text-sm);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .action-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
    white-space: nowrap;
  }

  .action-show-all {
    display: block;
    width: 100%;
    margin-top: var(--space-2);
    padding: var(--space-1);
    background: transparent;
    border: none;
    color: var(--color-link);
    font-size: var(--text-xs);
    cursor: pointer;
    text-align: center;
  }

  .action-show-all:hover { text-decoration: underline; }

  .action-buttons {
    display: flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .action-btn {
    padding: 2px 8px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .action-btn:hover {
    background: var(--color-border);
    border-color: var(--color-border-strong);
  }

  .action-btn-review {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 30%, var(--color-border));
    color: var(--color-primary);
    font-weight: 600;
  }

  .action-btn-review:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .action-btn-dismiss {
    padding: 2px 4px;
    color: var(--color-text-muted);
    border: none;
    background: transparent;
  }

  .action-btn-dismiss:hover {
    color: var(--color-text);
    background: var(--color-border);
  }
</style>
