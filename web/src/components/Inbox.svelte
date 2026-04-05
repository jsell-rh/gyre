<script>
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toastError } from '../lib/toast.svelte.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Use shell context API for detail panel — S4.1 app shell manages the split layout
  const openDetailPanel = getContext('openDetailPanel');
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  const navigate = getContext('navigate');
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings');
  const goToAgentRules = getContext('goToAgentRules');

  let notifications = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let expandedId = $state(null);
  let showDismissed = $state(false);
  let filterType = $state('all');
  let actionStates = $state({});
  let workspaceMap = $state({});

  // Entity name resolution uses shared singleton cache
  function resolveEntityName(type, id) {
    return entityName(type, id);
  }

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
    // PascalCase variants from the server
    AgentCompleted: 'success',
    AgentFailed: 'danger',
    SpecPendingApproval: 'warning',
    SpecApproved: 'success',
    SpecRejected: 'danger',
    MrMerged: 'success',
    MrCreated: 'info',
    GateFailure: 'danger',
    SuggestedSpecLink: 'default',
    TaskCreated: 'info',
    BudgetWarning: 'warning',
  };

  // Human-readable type labels — derived from i18n
  function typeLabel(typ) {
    const key = `decisions.type_labels.${typ}`;
    const val = $t(key);
    // If i18n returns the key itself, fall back to raw type
    return val === key ? typ : val;
  }

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

  async function loadNotifications(isBackground = false) {
    try {
      if (!isBackground) loading = true;
      error = null;
      let raw = await api.myNotifications();
      let data = Array.isArray(raw) ? raw : (raw?.notifications ?? []);
      // Normalize PascalCase notification types from the server to snake_case
      const typeNormMap = {
        AgentCompleted: 'agent_completed',
        AgentFailed: 'agent_failed',
        SpecPendingApproval: 'spec_approval',
        SpecApproved: 'spec_approved',
        SpecRejected: 'spec_rejected',
        MrMerged: 'mr_merged',
        MrCreated: 'mr_created',
        MrNeedsReview: 'mr_needs_review',
        GateFailure: 'gate_failure',
        SuggestedSpecLink: 'suggested_link',
        TaskCreated: 'task_created',
        BudgetWarning: 'budget_warning',
        SpecChanged: 'spec_changed',
        MetaSpecDrift: 'meta_spec_drift',
      };
      data = data.map(n => ({
        ...n,
        notification_type: typeNormMap[n.notification_type] ?? n.notification_type,
      }));

      // Client-side scope filtering
      if (repoId) {
        data = data.filter(n => n.repo_id === repoId);
      } else if (workspaceId) {
        data = data.filter(n => n.workspace_id === workspaceId);
      }

      // Sort by priority ascending (1 = highest)
      notifications = data.sort((a, b) => (a.priority ?? 999) - (b.priority ?? 999));
    } catch (e) {
      error = e.message || $t('decisions.load_failed');
      notifications = [];
    } finally {
      if (!isBackground) loading = false;
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
    if (m < 1) return $t('decisions.time_just_now');
    if (m < 60) return $t('decisions.time_minutes_ago', { values: { count: m } });
    const h = Math.floor(m / 60);
    if (h < 24) return $t('decisions.time_hours_ago', { values: { count: h } });
    return $t('decisions.time_days_ago', { values: { count: Math.floor(h / 24) } });
  }

  function toggleExpand(id) {
    expandedId = expandedId === id ? null : id;
  }

  function openDetail(entity) {
    if (goToEntityDetail && entity?.type) {
      goToEntityDetail(entity.type, entity.id, entity.data ?? {});
    } else {
      openDetailPanel?.(entity);
    }
  }

  // Pagination
  const PAGE_SIZE = 20;
  let displayLimit = $state(PAGE_SIZE);

  let allVisibleNotifications = $derived(
    notifications.filter(n => {
      if (!showDismissed && n.dismissed_at) return false;
      if (filterType !== 'all' && n.notification_type !== filterType) return false;
      return true;
    })
  );

  let availableTypes = $derived.by(() => {
    const types = new Set(notifications.map(n => n.notification_type).filter(Boolean));
    return ['all', ...Array.from(types).sort()];
  });

  let visibleNotifications = $derived(
    allVisibleNotifications.slice(0, displayLimit)
  );

  let hasMore = $derived(allVisibleNotifications.length > displayLimit);

  let unresolvedCount = $derived(
    notifications.filter(n => !n.resolved_at && !n.dismissed_at).length
  );

  async function handleDismiss(n) {
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.markNotificationRead(n.id);
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, dismissed_at: new Date().toISOString() } : item
      );
      if (expandedId === n.id) expandedId = null;
    } catch {
      toastError($t('decisions.dismiss_failed'));
    }
    actionStates = { ...actionStates, [n.id]: { loading: false } };
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
      api.resolveNotification(n.id).catch(() => toastError($t('decisions.dismiss_failed')));
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: $t('decisions.approved') },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || $t('decisions.approval_failed') },
      };
    }
  }

  async function handleRejectSpec(n) {
    const body = getBody(n);
    if (!body.spec_path) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'reject' } };
    try {
      await api.revokeSpec(normalizeSpecPath(body.spec_path), 'Rejected from inbox');
      api.resolveNotification(n.id).catch(() => toastError($t('decisions.dismiss_failed')));
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: $t('decisions.rejected') },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || $t('decisions.rejection_failed') },
      };
    }
  }

  async function handleRetry(n) {
    const body = getBody(n);
    if (!body.mr_id) return;
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.enqueue(body.mr_id);
      api.resolveNotification(n.id).catch(() => toastError($t('decisions.dismiss_failed')));
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: true, message: $t('decisions.re_queued') },
      };
    } catch (e) {
      actionStates = {
        ...actionStates,
        [n.id]: { loading: false, success: false, message: e.message || $t('decisions.retry_failed') },
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
    goToWorkspaceSettings?.();
    await handleDismiss(n);
  }

  function handleAdjustMetaSpec(n) {
    goToAgentRules?.();
  }

  // Reload when scope/workspaceId/repoId changes, and set up auto-refresh
  $effect(() => {
    void scope;
    void workspaceId;
    void repoId;
    loadWorkspaceNames();
    loadNotifications();
    const interval = setInterval(() => loadNotifications(true), 60000);
    return () => clearInterval(interval);
  });
</script>

<div class="inbox" aria-busy={loading}>
  <span class="sr-only" role="status">
    {#if !loading}{$t('decisions.notification_count', { values: { count: visibleNotifications.length } })}{/if}
  </span>
  <div class="inbox-header">
      <div class="inbox-title-row">
        <h1 class="inbox-title">{$t('decisions.title')}</h1>
        {#if unresolvedCount > 0}
          <span class="inbox-badge" aria-label={$t('decisions.unresolved_label', { values: { count: unresolvedCount } })}
            >{unresolvedCount}</span
          >
        {/if}
      </div>
      <div class="inbox-header-actions">
        {#if availableTypes.length > 2}
          <select
            class="type-filter"
            value={filterType}
            onchange={(e) => { filterType = e.target.value; }}
            aria-label={$t('decisions.filter_by_type')}
            data-testid="inbox-type-filter"
          >
            {#each availableTypes as typ}
              <option value={typ}>{typ === 'all' ? $t('decisions.all_types') : (typeLabel(typ))}</option>
            {/each}
          </select>
        {/if}
        <label class="dismissed-toggle">
          <input type="checkbox" bind:checked={showDismissed} />
          {$t('decisions.show_dismissed')}
        </label>
        <Button variant="ghost" size="sm" onclick={loadNotifications}>{$t('common.refresh')}</Button>
      </div>
    </div>

    {#if error}
      <div class="error-banner" role="alert">
        {error}
        <button class="retry-btn" onclick={loadNotifications}>{$t('common.retry')}</button>
      </div>
    {/if}

    {#if loading}
      <div class="inbox-list">
        {#each [1, 2, 3] as _}
          <Skeleton height="80px" />
        {/each}
      </div>
    {:else if visibleNotifications.length === 0}
      <EmptyState title={$t('decisions.all_caught_up')} description={$t('decisions.no_pending')} />
    {:else}
      <div class="inbox-list" role="list" aria-label={$t('decisions.notifications_label')}>
        {#each visibleNotifications as n (n.id)}
          {@const body = getBody(n)}
          {@const isExpanded = expandedId === n.id}
          {@const state = actionStates[n.id]}
          {@const isDismissed = !!n.dismissed_at}
          {@const isResolved = !!n.resolved_at}

          <div
            class="inbox-card"
            class:expanded={isExpanded}
            class:dismissed={isDismissed}
            class:resolved={isResolved && !isDismissed}
            role="listitem"
            data-type={n.notification_type}
          >
            <!-- Card header: always visible, click to expand/collapse -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="card-header"
              onclick={() => toggleExpand(n.id)}
              role="button"
              tabindex="0"
              aria-expanded={isExpanded}
              aria-controls="inbox-card-{n.id}"
              aria-label="{isExpanded ? $t('common.collapse') : $t('common.expand')}: {n.title}"
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleExpand(n.id); } }}
            >
              <div class="card-header-left">
                <span
                  class="priority-badge"
                  data-priority={n.priority}
                  aria-label={$t('decisions.priority_label', { values: { level: n.priority } })}
                >
                  P{n.priority}
                </span>
                {#if isDismissed}<span class="sr-only">({$t('decisions.dismissed')})</span>{/if}
                <div class="card-header-text">
                  <span class="card-title">{n.title}</span>
                  {#if body.agent_id || body.mr_title || body.agent_name}
                    <span class="card-subtitle">
                      {#if body.agent_name}{body.agent_name}
                      {:else if body.agent_id}{resolveEntityName('agent', body.agent_id)}{/if}
                      {#if body.mr_title} {$t('decisions.on_mr', { values: { title: body.mr_title } })}{/if}
                      {#if body.spec_path}
                        ({$t('decisions.spec_label_short', { values: { name: body.spec_path.split('/').pop()?.replace('.md', '') } })})
                      {/if}
                    </span>
                  {:else if body.spec_path}
                    <button class="card-subtitle card-subtitle-link" title={body.spec_path} onclick={(e) => { e.stopPropagation(); openDetail({ type: 'spec', id: normalizeSpecPath(body.spec_path), data: { path: normalizeSpecPath(body.spec_path), repo_id: n.repo_id } }); }}>{body.spec_path.split('/').pop()?.replace(/\.md$/, '') ?? body.spec_path}</button>
                  {:else if body.meta_spec_path}
                    <span class="card-subtitle" title={body.meta_spec_path}>{body.meta_spec_path.split('/').pop()?.replace(/\.md$/, '') ?? body.meta_spec_path}</span>
                  {/if}
                </div>
              </div>
              <div class="card-header-right">
                <!-- Quick entity jump buttons (visible without expanding) -->
                {#if body.spec_path}
                  <button class="card-quick-link" onclick={(e) => { e.stopPropagation(); openDetail({ type: 'spec', id: normalizeSpecPath(body.spec_path), data: { path: normalizeSpecPath(body.spec_path), repo_id: n.repo_id } }); }} title="View spec: {body.spec_path}">📋</button>
                {/if}
                {#if body.mr_id}
                  <button class="card-quick-link" onclick={(e) => { e.stopPropagation(); openDetail({ type: 'mr', id: body.mr_id, data: { repo_id: n.repo_id } }); }} title="View merge request">🔀</button>
                {/if}
                {#if body.agent_id}
                  <button class="card-quick-link" onclick={(e) => { e.stopPropagation(); openDetail({ type: 'agent', id: body.agent_id, data: { repo_id: n.repo_id } }); }} title="View agent">▶</button>
                {/if}
                {#if isResolved}
                  <Badge value={$t('decisions.status_resolved')} variant="success" />
                {/if}
                {#if scope === 'tenant' && n.workspace_id}
                  <Badge value={workspaceMap[n.workspace_id] ?? entityName('workspace', n.workspace_id)} variant="default" />
                {/if}
                <Badge
                  value={typeLabel(n.notification_type)}
                  variant={TYPE_VARIANTS[n.notification_type] || 'default'}
                />
                <span class="card-age">{relativeTime(n.created_at)}</span>
                <span class="expand-icon" aria-hidden="true">{isExpanded ? '▲' : '▼'}</span>
              </div>
            </div>

            <!-- Expanded body (accordion — only one open at a time) -->
            {#if isExpanded}
              <div class="card-body" id="inbox-card-{n.id}">
                {#if body.message}
                  <blockquote class="card-message">"{body.message}"</blockquote>
                {/if}
                {#if body.gate_name || body.command}
                  <div class="gate-detail">
                    {#if body.gate_name}<span class="gate-label">{$t('decisions.gate_label', { values: { name: body.gate_name } })}</span>{/if}
                    {#if body.command}<code class="gate-command">{body.command}</code>{/if}
                  </div>
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
                      {$t('decisions.related_spec', { values: { path: body.spec_path } })}
                    </button>
                  {/if}
                  {#if body.agent_id}
                    <button
                      class="ref-link"
                      onclick={() => openDetail({ type: 'agent', id: body.agent_id, data: n })}
                    >
                      {$t('decisions.agent_label', { values: { id: body.agent_name ?? entityName('agent', body.agent_id) } })}
                    </button>
                  {/if}
                  {#if body.persona}
                    <span class="ref-info">{$t('decisions.persona_label', { values: { name: body.persona } })}</span>
                  {/if}
                  {#if body.mr_id}
                    <button
                      class="ref-link"
                      onclick={() => openDetail({ type: 'mr', id: body.mr_id, data: n })}
                    >
                      {$t('decisions.mr_label', { values: { title: body.mr_title || body.mr_id } })}
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
                {#if !state?.success && !isDismissed && !n.resolved_at}
                  <div class="card-actions">
                    {#if n.notification_type === 'agent_clarification'}
                      <Button variant="primary" size="sm" onclick={() => handleRespondToAgent(n)}>
                        {$t('decisions.respond_to_agent')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        {$t('decisions.view_spec')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => {
                        const b = getBody(n);
                        openDetail({ type: 'spec', id: b.spec_path || n.entity_ref, data: n, defaultTab: 'architecture' });
                      }}>
                        {$t('decisions.view_in_explorer')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        {$t('common.dismiss')}
                      </Button>
                    {:else if n.notification_type === 'spec_approval'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleApproveSpec(n)}
                      >
                        {state?.loading && state?.action === 'approve' ? $t('decisions.approving') : $t('common.approve')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleRejectSpec(n)}
                      >
                        {state?.loading && state?.action === 'reject' ? $t('decisions.rejecting') : $t('common.reject')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        {$t('decisions.open_spec')}
                      </Button>
                    {:else if n.notification_type === 'gate_failure'}
                      <Button variant="ghost" size="sm" onclick={() => handleViewMr(n)}>
                        {$t('decisions.view_mr')}
                      </Button>
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleRetry(n)}
                      >
                        {state?.loading ? $t('decisions.retrying') : $t('decisions.retry_gate')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        {$t('common.dismiss')}
                      </Button>
                    {:else if n.notification_type === 'cross_workspace_change'}
                      <Button variant="primary" size="sm" onclick={() => handleViewSpec(n)}>
                        {$t('decisions.review_changes')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        {$t('common.dismiss')}
                      </Button>
                    {:else if n.notification_type === 'conflicting_interpretations'}
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>{$t('decisions.view_both_specs')}</Button>
                      <span class="coming-soon-note">{$t('decisions.reconciliation_note')}</span>
                      <Button variant="ghost" size="sm" disabled={state?.loading} onclick={() => handleDismiss(n)}>{$t('common.dismiss')}</Button>
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
                        {$t('decisions.view_results')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onclick={() => handleAdjustMetaSpec(n)}
                      >
                        {$t('decisions.adjust_metaspec')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleDismiss(n)} disabled={state?.loading}>{$t('common.dismiss')}</Button>
                    {:else if n.notification_type === 'budget_warning'}
                      <Button variant="primary" size="sm" onclick={() => goToWorkspaceSettings?.()}>{$t('decisions.increase_limit')}</Button>
                      <Button variant="ghost" size="sm" disabled={state?.loading} onclick={() => handleDismiss(n)}>{$t('common.dismiss')}</Button>
                    {:else if n.notification_type === 'trust_suggestion'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleIncreaseTrust(n)}
                      >
                        {$t('decisions.increase_trust')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        {$t('common.dismiss')}
                      </Button>
                    {:else if n.notification_type === 'spec_assertion_failure'}
                      <Button
                        variant="primary"
                        size="sm"
                        onclick={() => openDetail({ type: 'repo', id: body.repo_id, data: n })}
                      >
                        {$t('decisions.view_code')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleViewSpec(n)}>
                        {$t('decisions.update_spec')}
                      </Button>
                      <Button variant="ghost" size="sm" onclick={() => handleDismiss(n)} disabled={state?.loading}>{$t('common.dismiss')}</Button>
                    {:else if n.notification_type === 'suggested_link'}
                      <Button
                        variant="primary"
                        size="sm"
                        disabled={state?.loading}
                        onclick={async () => {
                          actionStates = { ...actionStates, [n.id]: { loading: true } };
                          try {
                            await api.resolveNotification(n.id);
                            notifications = notifications.map(item =>
                              item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
                            );
                            actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('decisions.accepted') } };
                          } catch {
                            actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: $t('decisions.accept_failed') } };
                          }
                        }}
                      >
                        {$t('decisions.accept')}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={state?.loading}
                        onclick={() => handleDismiss(n)}
                      >
                        {$t('common.dismiss')}
                      </Button>
                    {:else if n.notification_type === 'agent_completed' || n.notification_type === 'mr_needs_review'}
                      {@const b = getBody(n)}
                      {#if b.mr_id}
                        <Button variant="primary" size="sm" onclick={() => openDetail({ type: 'mr', id: b.mr_id, data: { repository_id: n.repo_id } })}>
                          Review MR
                        </Button>
                      {/if}
                      {#if b.agent_id}
                        <Button variant="ghost" size="sm" onclick={() => openDetail({ type: 'agent', id: b.agent_id, data: { repo_id: n.repo_id } })}>
                          View Agent
                        </Button>
                      {/if}
                      <Button variant="ghost" size="sm" disabled={state?.loading} onclick={() => handleDismiss(n)}>{$t('common.dismiss')}</Button>
                    {:else if n.notification_type === 'mr_merged' || n.notification_type === 'spec_approved' || n.notification_type === 'spec_rejected' || n.notification_type === 'task_created' || n.notification_type === 'spec_changed' || n.notification_type === 'agent_failed'}
                      {@const b = getBody(n)}
                      {#if b.mr_id}
                        <Button variant="ghost" size="sm" onclick={() => openDetail({ type: 'mr', id: b.mr_id, data: { repository_id: n.repo_id } })}>
                          View MR
                        </Button>
                      {/if}
                      {#if b.spec_path}
                        <Button variant="ghost" size="sm" onclick={() => openDetail({ type: 'spec', id: normalizeSpecPath(b.spec_path), data: { path: normalizeSpecPath(b.spec_path), repo_id: n.repo_id } })}>
                          View Spec
                        </Button>
                      {/if}
                      {#if b.agent_id}
                        <Button variant="ghost" size="sm" onclick={() => openDetail({ type: 'agent', id: b.agent_id, data: { repo_id: n.repo_id } })}>
                          View Agent
                        </Button>
                      {/if}
                      {#if b.task_id}
                        <Button variant="ghost" size="sm" onclick={() => openDetail({ type: 'task', id: b.task_id, data: { repo_id: n.repo_id } })}>
                          View Task
                        </Button>
                      {/if}
                      <Button variant="ghost" size="sm" disabled={state?.loading} onclick={() => handleDismiss(n)}>{$t('common.dismiss')}</Button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
      {#if hasMore}
        <div class="show-more-wrap">
          <button
            class="show-more-btn"
            onclick={() => { displayLimit += PAGE_SIZE; }}
            aria-label={$t('decisions.show_more_label')}
          >
            {$t('decisions.show_more', { values: { count: allVisibleNotifications.length - displayLimit } })}
          </button>
        </div>
      {/if}
    {/if}
  </div>

<style>
  .inbox {
    padding: var(--space-6);
    max-width: 800px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    overflow-y: auto;
    height: 100%;
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
    background: var(--color-focus);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .inbox-header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .type-filter {
    appearance: none;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-5) var(--space-1) var(--space-2);
    cursor: pointer;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right var(--space-1) center;
    background-size: var(--space-3);
  }

  .type-filter:hover { border-color: var(--color-primary); }

  .type-filter:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
    border-radius: var(--radius-lg);
    transition: border-color var(--transition-fast);
    overflow: hidden;
  }

  .inbox-card:hover {
    border-color: var(--color-border-strong);
  }

  .inbox-card.expanded {
    border-color: var(--color-focus);
  }

  .inbox-card.dismissed {
    opacity: 0.6;
  }

  .inbox-card.resolved {
    border-left: 3px solid var(--color-success);
    opacity: 0.7;
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
    transition: background var(--transition-fast);
  }

  .card-header:focus-visible {
    outline: 2px solid var(--color-focus);
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
    background: var(--color-danger);
    color: var(--color-text-inverse);
    font-family: var(--font-mono);
  }

  .priority-badge[data-priority='4'],
  .priority-badge[data-priority='5'],
  .priority-badge[data-priority='6'] {
    background: var(--color-warning);
  }

  .priority-badge[data-priority='7'],
  .priority-badge[data-priority='8'],
  .priority-badge[data-priority='9'],
  .priority-badge[data-priority='10'] {
    background: var(--color-text-muted);
  }

  .card-header-text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .card-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    line-height: 1.4;
  }

  .card-subtitle {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-subtitle-link {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    text-align: left;
    font: inherit;
    color: var(--color-link, var(--color-primary));
  }

  .card-subtitle-link:hover {
    text-decoration: underline;
  }

  .card-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .card-quick-link {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px 6px;
    cursor: pointer;
    font-size: 11px;
    line-height: 1;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .card-quick-link:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .card-quick-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
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
    border-left: 3px solid var(--color-focus);
    border-radius: 0 var(--radius) var(--radius) 0;
    font-size: var(--text-sm);
    color: var(--color-text);
    font-style: italic;
  }

  .gate-detail {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .gate-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .gate-command {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    word-break: break-all;
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
    color: var(--color-danger);
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
    color: var(--color-link);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-mono);
    text-decoration: underline;
    transition: color var(--transition-fast);
  }

  .ref-link:hover {
    color: var(--color-link-hover);
  }

  .ref-link:focus-visible {
    outline: 2px solid var(--color-focus);
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
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-link) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-link) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-link) 25%, transparent);
    border-color: var(--color-link);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .action-feedback {
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) 0;
  }

  .action-feedback.success {
    color: var(--color-success);
  }

  .action-feedback.failure {
    color: var(--color-danger);
  }

  .coming-soon-note {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .inbox-card,
    .card-header,
    .ref-link,
    .retry-btn { transition: none; }
  }

  /* ── Show more ─────────────────────────────────────────────────────── */
  .show-more-wrap {
    display: flex;
    justify-content: center;
    padding: var(--space-4) 0;
  }

  .show-more-btn {
    padding: var(--space-2) var(--space-6);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .show-more-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .show-more-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }
</style>
