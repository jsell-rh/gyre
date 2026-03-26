<script>
  import { onMount, onDestroy, getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Use shell context API for detail panel — S4.1 app shell manages the split layout
  const openDetailPanel = getContext('openDetailPanel');
  const navigate = getContext('navigate');

  let notifications = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let expandedId = $state(null);
  let showDismissed = $state(false);
  let actionStates = $state({});
  let refreshInterval;
  let workspaceMap = $state({});

  // Badge variant per notification type
  const TYPE_VARIANTS = {
    agent_clarification: 'danger',
    spec_approval: 'warning',
    gate_failure: 'danger',
    cross_workspace_change: 'info',
    conflicting_interpretations: 'warning',
    meta_spec_drift: 'info',
    budget_warning: 'warning',
    trust_suggestion: 'default',
    spec_assertion_failure: 'danger',
    suggested_link: 'default',
  };

  // Human-readable type labels
  const TYPE_LABELS = {
    agent_clarification: 'Clarification',
    spec_approval: 'Spec Approval',
    gate_failure: 'Gate Failure',
    cross_workspace_change: 'Cross-WS Change',
    conflicting_interpretations: 'Conflict',
    meta_spec_drift: 'Meta Drift',
    budget_warning: 'Budget',
    trust_suggestion: 'Trust',
    spec_assertion_failure: 'Assertion Fail',
    suggested_link: 'Suggested Link',
  };

  async function loadWorkspaceNames() {
    if (scope !== 'tenant') return;
    try {
      const wsList = await api.workspaces();
      workspaceMap = Object.fromEntries(
        (Array.isArray(wsList) ? wsList : []).map(w => [w.id, w.name ?? w.id])
      );
    } catch {
      // best-effort — fall back to raw IDs
    }
  }

  async function loadNotifications() {
    try {
      loading = true;
      error = null;
      let data = await api.myNotifications();

      if (!Array.isArray(data)) data = [];

      // Client-side scope filtering
      if (repoId) {
        data = data.filter(n => n.repo_id === repoId);
      } else if (workspaceId) {
        data = data.filter(n => n.workspace_id === workspaceId);
      }

      // Sort by priority ascending (1 = highest)
      notifications = data.sort((a, b) => a.priority - b.priority);
    } catch (e) {
      error = e.message || 'Failed to load notifications';
      notifications = [];
    } finally {
      loading = false;
    }
  }

  function getBody(n) {
    try {
      return JSON.parse(n.body || '{}');
    } catch {
      return {};
    }
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return 'just now';
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  function toggleExpand(id) {
    expandedId = expandedId === id ? null : id;
  }

  function openDetail(entity) {
    openDetailPanel?.(entity);
  }

  let visibleNotifications = $derived(
    notifications.filter(n => showDismissed || !n.dismissed_at)
  );

  let unresolvedCount = $derived(
    notifications.filter(n => !n.resolved_at && !n.dismissed_at).length
  );

  async function handleDismiss(n) {
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.markNotificationRead(n.id);
    } catch {
      // dismiss optimistically even on failure
    }
    notifications = notifications.map(item =>
      item.id === n.id ? { ...item, dismissed_at: new Date().toISOString() } : item
    );
    actionStates = { ...actionStates, [n.id]: { loading: false } };
    if (expandedId === n.id) expandedId = null;
  }

  // Spec paths in notification bodies may include a leading "specs/" prefix.
  // The /specs/{path} API endpoint already provides the /specs/ segment, so strip it.
  function normalizeSpecPath(path) {
    return path ? path.replace(/^specs\//, '') : path;
  }

  async function handleApproveSpec(n) {
    const body = getBody(n);
    if (!body.spec_path || !body.spec_sha) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'approve' } };
    try {
      await api.approveSpec(normalizeSpecPath(body.spec_path), body.spec_sha);
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: 'Approved' },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || 'Approval failed' },
      };
    }
  }

  async function handleRejectSpec(n) {
    const body = getBody(n);
    if (!body.spec_path) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'reject' } };
    try {
      await api.revokeSpec(normalizeSpecPath(body.spec_path), 'Rejected from inbox');
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: 'Rejected' },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || 'Rejection failed' },
      };
    }
  }

  async function handleRetry(n) {
    const body = getBody(n);
    if (!body.mr_id) return;
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.enqueue(body.mr_id);
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: 'Re-queued' },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || 'Retry failed' },
      };
    }
  }

  function handleRespondToAgent(n) {
    const body = getBody(n);
    openDetail({ type: 'agent', id: body.agent_id || n.entity_ref, data: n, defaultTab: 'chat' });
  }

  function handleViewSpec(n) {
    const body = getBody(n);
    const specPath = body.spec_path || n.entity_ref;
    if (specPath) {
      openDetail({ type: 'spec', id: specPath, data: n });
    }
  }

  function handleViewMr(n) {
    const body = getBody(n);
    if (body.mr_id) {
      openDetail({ type: 'mr', id: body.mr_id, data: n });
    }
  }

  async function handleIncreaseTrust(n) {
    navigate?.('admin');
    await handleDismiss(n);
  }

  function handleAdjustMetaSpec(n) {
    navigate?.('meta-specs');
  }

  onMount(() => {
    loadWorkspaceNames();
    loadNotifications();
    refreshInterval = setInterval(loadNotifications, 60000);
  });

  onDestroy(() => {
    if (refreshInterval) clearInterval(refreshInterval);
  });
</script>

<div class="inbox">
  <div class="inbox-header">
      <div class="inbox-title-row">
        <h1 class="inbox-title">Inbox</h1>
        {#if unresolvedCount > 0}
          <span class="inbox-badge" aria-label="{unresolvedCount} unresolved items"
            >{unresolvedCount}</span
          >
        {/if}
      </div>
      <div class="inbox-header-actions">
        <label class="dismissed-toggle">
          <input type="checkbox" bind:checked={showDismissed} />
          Show Dismissed
        </label>
        <Button variant="ghost" size="sm" onclick={loadNotifications}>Refresh</Button>
      </div>
    </div>

    {#if error}
      <div class="error-banner" role="alert">
        {error}
        <button class="retry-btn" onclick={loadNotifications}>Retry</button>
      </div>
    {/if}

    {#if loading}
      <div class="inbox-list">
        {#each [1, 2, 3] as _}
          <Skeleton height="80px" />
        {/each}
      </div>
    {:else if visibleNotifications.length === 0}
      <EmptyState title="All caught up!" description="No pending notifications." />
    {:else}
      <div class="inbox-list" role="list">
        {#each visibleNotifications as n (n.id)}
          {@const body = getBody(n)}
          {@const isExpanded = expandedId === n.id}
          {@const state = actionStates[n.id]}
          {@const isDismissed = !!n.dismissed_at}

          <div
            class="inbox-card"
            class:expanded={isExpanded}
            class:dismissed={isDismissed}
            role="listitem"
            data-type={n.notification_type}
          >
            <!-- Card header: always visible, click to expand/collapse -->
            <button
              class="card-header"
              onclick={() => toggleExpand(n.id)}
              aria-expanded={isExpanded}
              aria-controls="inbox-card-{n.id}"
              aria-label="{isExpanded ? 'Collapse' : 'Expand'}: {n.title}"
            >
              <div class="card-header-left">
                <span
                  class="priority-badge"
                  data-priority={n.priority}
                  title="Priority {n.priority}"
                >
                  P{n.priority}
                </span>
                <div class="card-header-text">
                  <span class="card-title">{n.title}</span>
                  {#if body.agent_id || body.mr_title}
                    <span class="card-subtitle">
                      {#if body.agent_id}{body.agent_id}{/if}
                      {#if body.mr_title} on {body.mr_title}{/if}
                      {#if body.spec_path}
                        (spec: {body.spec_path.split('/').pop()?.replace('.md', '')})
                      {/if}
                    </span>
                  {:else if body.spec_path}
                    <span class="card-subtitle">{body.spec_path}</span>
                  {:else if body.meta_spec_path}
                    <span class="card-subtitle">{body.meta_spec_path}</span>
                  {/if}
                </div>
              </div>
              <div class="card-header-right">
                {#if scope === 'tenant' && n.workspace_id}
                  <Badge value={workspaceMap[n.workspace_id] ?? n.workspace_id} variant="default" />
                {/if}
                <Badge
                  value={TYPE_LABELS[n.notification_type] || n.notification_type}
                  variant={TYPE_VARIANTS[n.notification_type] || 'default'}
                />
                <span class="card-age">{relativeTime(n.created_at)}</span>
                <span class="expand-icon" aria-hidden="true">{isExpanded ? '▲' : '▼'}</span>
              </div>
            </button>

            <!-- Expanded body (accordion — only one open at a time) -->
            {#if isExpanded}
              <div class="card-body" id="inbox-card-{n.id}">
                {#if body.message}
                  <blockquote class="card-message">"{body.message}"</blockquote>
                {/if}
                {#if body.diff_summary}
                  <p class="card-detail">{body.diff_summary}</p>
                {/if}
                {#if body.change_summary}
                  <p class="card-detail">{body.change_summary}</p>
                {/if}
                {#if body.output}
                  <pre class="card-output">{body.output}</pre>
                {/if}

                <!-- Entity reference links -->
                <div class="card-refs">
                  {#if body.spec_path}
                    <button
                      class="ref-link"
                      onclick={() => openDetail({ type: 'spec', id: body.spec_path, data: n })}
                    >
                      Related spec: {body.spec_path}
                    </button>
                  {/if}
                  {#if body.agent_id}
                    <button
                      class="ref-link"
                      onclick={() => openDetail({ type: 'agent', id: body.agent_id, data: n })}
                    >
                      Agent: {body.agent_id}
                    </button>
                  {/if}
                  {#if body.persona}
                    <span class="ref-info">Persona: {body.persona}</span>
                  {/if}
                  {#if body.mr_id}
                    <button
                      class="ref-link"
                      onclick={() => openDetail({ type: 'mr', id: body.mr_id, data: n })}
                    >
                      MR: {body.mr_title || body.mr_id}
                    </button>
                  {/if}
                </div>

                {#if state?.message}
                  <div
                    class="action-feedback"
                    class:success={state.success}
                    class:failure={!state.success && state.message}
                    role="alert"
                  >
                    {state.message}
                  </div>
                {/if}

                <!-- Action buttons per notification type -->
                {#if !state?.success && !isDismissed}
                  <div class="card-actions">
                    {#if n.notification_type === 'agent_clarification'}
                      <Button variant="primary" size="sm" onclick={() => handleRespondToAgent(n)}>
                        Respond to Agent
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        View Spec
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => { navigate?.('explorer'); handleDismiss(n); }}>
                        Open in Explorer
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Dismiss
                      </Button>
                    {:else if n.notification_type === 'spec_approval'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleApproveSpec(n)}
                      >
                        {state?.loading && state?.action === 'approve' ? 'Approving…' : 'Approve'}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleRejectSpec(n)}
                      >
                        {state?.loading && state?.action === 'reject' ? 'Rejecting…' : 'Reject'}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        Open Spec
                      </Button>
                    {:else if n.notification_type === 'gate_failure'}
                      <Button variant="ghost" size="sm" onclick={() => handleViewMr(n)}>
                        View Diff
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewMr(n)}>
                        View Output
                      </Button>
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleRetry(n)}
                      >
                        {state?.loading ? 'Retrying…' : 'Retry'}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewMr(n)}>Override</Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Close MR
                      </Button>
                    {:else if n.notification_type === 'cross_workspace_change'}
                      <Button variant="primary" size="sm" onclick={() => handleViewSpec(n)}>
                        Review Changes
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Dismiss
                      </Button>
                    {:else if n.notification_type === 'conflicting_interpretations'}
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        View Both
                      </Button>
                      <Button variant="primary" size="sm" disabled title="Coming soon">Pick A</Button>
                      <Button variant="primary" size="sm" disabled title="Coming soon">Pick B</Button>
                      <Button variant="ghost" size="sm" disabled title="Coming soon">Reconcile</Button>
                    {:else if n.notification_type === 'meta_spec_drift'}
                      <Button
                        variant="primary"
                        size="sm"
                        onclick={() =>
                          openDetail({
                            type: 'spec',
                            id: body.meta_spec_path || n.entity_ref,
                            data: n,
                          })}
                      >
                        View Results
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onclick={() => handleAdjustMetaSpec(n)}
                      >
                        Adjust Meta-spec
                      </Button>
                    {:else if n.notification_type === 'budget_warning'}
                      <Button variant="primary" size="sm" onclick={() => navigate?.('admin')}>Increase Limit</Button>
                      <Button variant="ghost" size="sm" disabled title="Coming soon">Pause Work</Button>
                    {:else if n.notification_type === 'trust_suggestion'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleIncreaseTrust(n)}
                      >
                        Increase Trust
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Dismiss
                      </Button>
                    {:else if n.notification_type === 'spec_assertion_failure'}
                      <Button
                        variant="primary"
                        size="sm"
                        onclick={() => openDetail({ type: 'repo', id: body.repo_id, data: n })}
                      >
                        View Code
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        Update Spec
                      </Button>
                    {:else if n.notification_type === 'suggested_link'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Confirm
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        Dismiss
                      </Button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>

<style>
  .inbox {
    padding: var(--space-6);
    max-width: 800px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .inbox-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .inbox-title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .inbox-title {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .inbox-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    padding: 0 var(--space-1);
    background: var(--color-primary);
    color: var(--color-text-inverse, #fff);
    border-radius: 999px;
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .inbox-header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .dismissed-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    user-select: none;
  }

  .dismissed-toggle input {
    cursor: pointer;
  }

  .inbox-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .inbox-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    transition: border-color var(--transition-fast);
    overflow: hidden;
  }

  .inbox-card:hover {
    border-color: var(--color-border-strong);
  }

  .inbox-card.expanded {
    border-color: var(--color-primary);
  }

  .inbox-card.dismissed {
    opacity: 0.45;
  }

  .card-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-4);
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: none;
    font-family: var(--font-body);
    color: var(--color-text);
    gap: var(--space-3);
  }

  .card-header:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .card-header-left {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    flex: 1;
    min-width: 0;
  }

  .priority-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 32px;
    height: 24px;
    border-radius: var(--radius);
    font-size: var(--text-xs);
    font-weight: 700;
    flex-shrink: 0;
    background: var(--color-danger, #ef4444);
    color: var(--color-text-inverse, #fff);
    font-family: var(--font-mono);
  }

  .priority-badge[data-priority='4'],
  .priority-badge[data-priority='5'],
  .priority-badge[data-priority='6'] {
    background: var(--color-warning, #f59e0b);
  }

  .priority-badge[data-priority='7'],
  .priority-badge[data-priority='8'],
  .priority-badge[data-priority='9'],
  .priority-badge[data-priority='10'] {
    background: var(--color-text-muted, #6b7280);
  }

  .card-header-text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .card-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .card-subtitle {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .card-age {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .expand-icon {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .card-body {
    padding: var(--space-4);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .card-message {
    margin: 0;
    padding: var(--space-3);
    background: var(--color-surface);
    border-left: 3px solid var(--color-primary);
    border-radius: 0 var(--radius) var(--radius) 0;
    font-size: var(--text-sm);
    color: var(--color-text);
    font-style: italic;
  }

  .card-detail {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .card-output {
    margin: 0;
    padding: var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-danger, #ef4444);
    white-space: pre-wrap;
    word-break: break-all;
  }

  .card-refs {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .ref-link {
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-mono);
    text-decoration: underline;
  }

  .ref-link:hover {
    opacity: 0.8;
  }

  .ref-link:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .ref-info {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .card-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .error-banner {
    background: color-mix(in srgb, var(--color-warning, #f59e0b) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-warning, #d97706);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .action-feedback {
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) 0;
  }

  .action-feedback.success {
    color: var(--color-success, #22c55e);
  }

  .action-feedback.failure {
    color: var(--color-danger, #ef4444);
  }
</style>
