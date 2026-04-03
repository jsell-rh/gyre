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
  import { entityName, shortId } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import Badge from '../lib/Badge.svelte';
  import Icon from '../lib/Icon.svelte';
  import EntityLink from '../lib/EntityLink.svelte';

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
    gate_failure: { color: 'var(--color-danger)', iconName: 'alert-triangle', label: 'Gate Failed' },
    spec_approval: { color: 'var(--color-warning)', iconName: 'eye', label: 'Needs Review' },
    agent_clarification: { color: 'var(--color-warning)', iconName: 'circle-dot', label: 'Agent Question' },
    budget_warning: { color: 'var(--color-warning)', iconName: 'dollar', label: 'Budget Alert' },
    mr_needs_review: { color: 'var(--color-info)', iconName: 'eye', label: 'Review Needed' },
    agent_failed: { color: 'var(--color-danger)', iconName: 'alert-triangle', label: 'Agent Failed' },
  };

  let showAll = $state(false);
  let maxVisible = 5;

  let actionable = $derived(items.filter(n => {
    const nt = normType(n.notification_type);
    return URGENCY[nt] && !n.dismissed_at && !n.resolved_at;
  }));

  let visible = $derived(showAll ? actionable : actionable.slice(0, maxVisible));

  const HIGH_PRIORITY_TYPES = new Set(['spec_approval', 'gate_failure', 'agent_failed']);

  function isHighPriority(n) {
    return HIGH_PRIORITY_TYPES.has(normType(n.notification_type));
  }

  function getUrgency(n) {
    return URGENCY[normType(n.notification_type)] ?? { color: 'var(--color-text-muted)', iconName: 'circle', label: 'Info' };
  }

  function repoName(n) {
    const rid = n.repo_id ?? parseBody(n).repo_id;
    return rid ? entityName('repo', rid) : null;
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
      const body = parseBody(n);
      const data = { repo_id: n.repo_id };
      // Specs need path in data for proper navigation
      if (type === 'spec') {
        data.path = body.spec_path ?? n.spec_path ?? id;
      }
      goToEntityDetail(type, id, data);
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
        {@const rName = repoName(item)}
        {@const entityType = getEntityType(item)}
        {@const entityId = getEntityId(item)}
        <button
          class="action-item"
          class:action-item-high={isHighPriority(item)}
          style="border-left-color: {urgency.color}"
          onclick={() => handleClick(item)}
          title={getTitle(item)}
        >
          <span class="action-icon" style="color: {urgency.color}"><Icon name={urgency.iconName} size={14} /></span>
          <div class="action-body">
            <div class="action-meta">
              <span class="action-label">{urgency.label}</span>
              {#if rName}
                <span class="action-repo-tag" title="Repository">{rName}</span>
              {/if}
            </div>
            <span class="action-title">{getTitle(item)}</span>
            {#if entityType && entityId}
              <span class="action-ref">
                <EntityLink type={entityType} id={entityId} data={{ repo_id: item.repo_id }} showType={true} />
              </span>
            {/if}
          </div>
          <div class="action-buttons">
            {#if nt === 'spec_approval'}
              {#if onApproveSpec}
                <button class="action-btn action-btn-approve" onclick={(e) => { e.stopPropagation(); onApproveSpec(item); }} title="Approve spec">Approve</button>
              {/if}
              {#if onRejectSpec}
                <button class="action-btn action-btn-reject" onclick={(e) => { e.stopPropagation(); onRejectSpec(item); }} title="Reject spec">Reject</button>
              {/if}
              <button class="action-btn action-btn-review" onclick={(e) => { e.stopPropagation(); handleClick(item); }} title="Review spec before deciding">Review</button>
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
        {showAll ? 'Show less' : `Show ${actionable.length - maxVisible} more`}
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

  .action-item-high {
    background: color-mix(in srgb, var(--color-warning) 6%, var(--color-surface));
    border-color: color-mix(in srgb, var(--color-warning) 25%, var(--color-border));
  }

  .action-item-high:hover {
    background: color-mix(in srgb, var(--color-warning) 10%, var(--color-surface-elevated));
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

  .action-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .action-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
  }

  .action-repo-tag {
    font-size: 0.65rem;
    padding: 0 4px;
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, var(--color-border));
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .action-ref {
    display: inline-flex;
    margin-top: 1px;
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

  .action-btn-approve {
    background: color-mix(in srgb, var(--color-success, #22c55e) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success, #22c55e) 30%, var(--color-border));
    color: var(--color-success, #16a34a);
    font-weight: 600;
  }

  .action-btn-approve:hover {
    background: color-mix(in srgb, var(--color-success, #22c55e) 22%, transparent);
  }

  .action-btn-reject {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    color: var(--color-danger);
    font-weight: 600;
  }

  .action-btn-reject:hover {
    background: color-mix(in srgb, var(--color-danger) 20%, transparent);
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
