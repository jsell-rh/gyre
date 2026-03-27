<script>
  import { onMount, getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import InlineChat from '../lib/InlineChat.svelte';
  import { toastInfo } from '../lib/toast.svelte.js';

  /**
   * Briefing View — S4.3
   *
   * Spec refs: ui-layout.md §8 (Briefing layout), HSI §9 (Briefing interaction)
   *
   * Props:
   *   workspaceId — workspace UUID (workspace scope)
   *   repoId      — repo UUID (repo scope)
   *   scope       — 'workspace' | 'tenant' | 'repo'
   */
  let { workspaceId = null, repoId = null, scope = 'workspace', workspaceName = null, trustLevel = null } = $props();

  // Shell context API (S4.1 App Shell) — falls back gracefully when not mounted in shell
  const openDetailPanel = getContext('openDetailPanel') ?? ((entity) => {});

  // --- Time range ---
  const TIME_RANGES = [
    { label: 'Since last visit', value: 'last_visit' },
    { label: 'Last 24h',         value: '24h' },
    { label: 'Last 7d',          value: '7d' },
    { label: 'Last 30d',         value: '30d' },
    { label: 'Custom range',     value: 'custom' },
  ];

  let selectedRange = $state('last_visit');
  let customSince = $state(''); // ISO date string for custom range

  // --- Data ---
  let loading = $state(true);
  let error = $state(null);
  let briefing = $state(null);
  let sinceLabel = $state('');
  let workspaceMap = $state({});

  function sinceEpochForRange(range) {
    const now = Math.floor(Date.now() / 1000);
    switch (range) {
      case '24h':  return now - 86400;
      case '7d':   return now - 7 * 86400;
      case '30d':  return now - 30 * 86400;
      case 'custom': {
        if (!customSince) return null;
        return Math.floor(new Date(customSince).getTime() / 1000);
      }
      default: return null; // server uses stored last_seen_at
    }
  }

  function labelForRange(range) {
    switch (range) {
      case '24h':    return '24 hours';
      case '7d':     return '7 days';
      case '30d':    return '30 days';
      case 'custom': return 'custom range';
      default:       return 'last visit';
    }
  }

  function isEmpty(data) {
    if (!data) return true;
    return (
      !data.completed?.length &&
      !data.in_progress?.length &&
      !data.cross_workspace?.length &&
      !data.exceptions?.length &&
      !data.metrics
    );
  }

  async function load() {
    loading = true;
    error = null;

    const since = sinceEpochForRange(selectedRange);
    sinceLabel = labelForRange(selectedRange);

    try {
      if (scope === 'workspace' && workspaceId) {
        const raw = await api.getWorkspaceBriefing(workspaceId, since);
        briefing = isEmpty(raw) ? { completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null } : raw;
      } else if (scope === 'tenant') {
        const workspaces = await api.workspaces();
        const wsList = workspaces || [];
        workspaceMap = Object.fromEntries(wsList.map(w => [w.id, w.name ?? w.id]));
        const results = await Promise.allSettled(
          wsList.map(w => api.getWorkspaceBriefing(w.id, since).then(b => ({ ...b, _wsId: w.id })))
        );
        const merged = {
          completed: [],
          in_progress: [],
          cross_workspace: [],
          exceptions: [],
          metrics: null,
        };
        let mrsCount = 0, runsCount = 0, costUsd = 0, budgetPct = 0, budgetN = 0;
        for (const r of results) {
          if (r.status !== 'fulfilled' || !r.value) continue;
          const b = r.value;
          const wsId = b._wsId;
          merged.completed.push(...(b.completed ?? []).map(item => ({ ...item, workspace_id: item.workspace_id ?? wsId })));
          merged.in_progress.push(...(b.in_progress ?? []).map(item => ({ ...item, workspace_id: item.workspace_id ?? wsId })));
          merged.cross_workspace.push(...(b.cross_workspace ?? []).map(item => ({ ...item, workspace_id: item.workspace_id ?? wsId })));
          merged.exceptions.push(...(b.exceptions ?? []).map(item => ({ ...item, workspace_id: item.workspace_id ?? wsId })));
          if (b.metrics) {
            mrsCount  += b.metrics.mrs_count ?? 0;
            runsCount += b.metrics.runs_count ?? 0;
            costUsd   += b.metrics.cost_usd ?? 0;
            if (b.metrics.budget_pct != null) {
              budgetPct += b.metrics.budget_pct;
              budgetN++;
            }
          }
        }
        merged.metrics = {
          mrs_count:  mrsCount,
          runs_count: runsCount,
          cost_usd:   costUsd,
          budget_pct: budgetN ? Math.round(budgetPct / budgetN) : null,
        };
        briefing = isEmpty(merged) ? { completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null } : merged;
      } else {
        // Repo scope — no briefing endpoint yet; show empty state
        briefing = { completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null };
      }
    } catch (e) {
      if (e.message && e.message.includes('404')) {
        // 404: no briefing data yet — show empty state
        briefing = { completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null };
        error = 'Briefing data not yet available';
      } else {
        // Real error — set briefing to null to prevent "All caught up" showing alongside error
        briefing = null;
        if (e.message) error = e.message;
      }
    } finally {
      loading = false;
    }
  }

  async function onRangeChange(val) {
    selectedRange = val;
    if (val !== 'custom') await load();
  }

  async function onCustomApply() {
    if (customSince) await load();
  }

  function openEntity(type, id, data = {}) {
    openDetailPanel({ type, id, data });
  }

  function handleViewSpec(specRef) {
    openEntity('spec', specRef, { path: specRef });
  }

  function handleReviewChanges(item) {
    openEntity('spec', item.spec_ref, { path: item.spec_ref, source_workspace: item.source_workspace });
  }

  function handleViewDiff(item) {
    openEntity('mr', item.mr_id, { repo: item.repo, mr_id: item.mr_id });
  }

  function handleViewOutput(item) {
    openEntity('mr', item.mr_id, { repo: item.repo, tab: 'gates' });
  }

  async function handleDismiss(item) {
    if (briefing) {
      briefing = {
        ...briefing,
        cross_workspace: briefing.cross_workspace.filter(x => x.id !== item.id),
      };
      toastInfo('Item dismissed');
    }
  }

  function briefingAskHandler(question) {
    if (!workspaceId) {
      throw new Error('Briefing Q&A is only available within a workspace.');
    }
    return api.briefingAsk(workspaceId, { question, history: [] });
  }

  onMount(load);
</script>

<div class="briefing" data-testid="briefing-view" aria-busy={loading}>
    <span class="sr-only" aria-live="polite">{loading ? '' : 'Briefing loaded'}</span>
    <!-- Header -->
    <div class="briefing-header">
      <div class="header-left">
        <h1 class="briefing-title">Briefing</h1>
        {#if !loading}
          <span class="briefing-since" data-testid="since-label">Since {sinceLabel}</span>
        {/if}
        {#if scope === 'workspace' && workspaceName}
          <span class="briefing-workspace">Workspace: {workspaceName}</span>
        {/if}
        {#if trustLevel}
          <span class="briefing-trust">Trust: {trustLevel}</span>
        {/if}
      </div>
      <div class="header-right">
        <div class="time-range-selector" data-testid="time-range-selector">
          <select
            class="range-select"
            value={selectedRange}
            onchange={(e) => onRangeChange(e.target.value)}
            aria-label="Briefing time range"
          >
            {#each TIME_RANGES as opt}
              <option value={opt.value}>{opt.label}</option>
            {/each}
          </select>
          {#if selectedRange === 'custom'}
            <input
              class="date-input"
              type="date"
              bind:value={customSince}
              aria-label="Custom start date"
              data-testid="custom-date-input"
            />
            <button class="apply-btn" onclick={onCustomApply} disabled={!customSince}>Apply</button>
          {/if}
        </div>
      </div>
    </div>

    {#if error}
      <div class="error-banner" role="alert" data-testid="error-banner">
        {#if !isEmpty(briefing)}
          Could not load live briefing: {error}. Showing cached data.
        {:else}
          Unable to load briefing data: {error}. Check your connection.
        {/if}
        <button class="action-btn" onclick={load}>Retry</button>
      </div>
    {/if}

    {#if loading}
      <div class="skeleton-stack">
        <Skeleton height="80px" />
        <Skeleton height="120px" />
        <Skeleton height="80px" />
        <Skeleton height="60px" />
      </div>
    {:else if briefing}
      <!-- COMPLETED -->
      {#if briefing.completed?.length}
        <section class="briefing-section" data-testid="section-completed" aria-labelledby="briefing-completed">
          <h2 class="section-heading" id="briefing-completed">
            <span class="section-icon completed-icon" aria-hidden="true">✓</span>
            COMPLETED
          </h2>
          {#each briefing.completed as item (item.id ?? item.title)}
            <div class="section-item" data-testid="completed-item">
              <div class="item-title">
                <span class="item-icon completed-icon" aria-hidden="true">✓</span>
                <span class="item-name">{item.title}</span>
                {#if scope === 'tenant' && item.workspace_id}
                  <Badge value={workspaceMap[item.workspace_id] ?? item.workspace_id} variant="default" />
                {/if}
                {#if item.spec_ref}
                  <button
                    class="entity-ref"
                    onclick={() => handleViewSpec(item.spec_ref)}
                    data-testid="spec-ref-link"
                    aria-label="View spec {item.spec_ref}"
                  >
                    spec: {item.spec_ref}
                  </button>
                {/if}
              </div>
              <div class="item-detail">
                {#if item.mrs_merged != null}
                  <span>{item.mrs_merged} MR{item.mrs_merged !== 1 ? 's' : ''} merged. All gates passed.</span>
                {/if}
                {#if item.decision}
                  <span class="item-decision">
                    Decision: {item.decision}
                    {#if item.confidence}
                      <span class="confidence-badge confidence-{item.confidence}">(confidence: {item.confidence})</span>
                    {/if}
                  </span>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {/if}

      <!-- IN PROGRESS -->
      {#if briefing.in_progress?.length}
        <section class="briefing-section" data-testid="section-in-progress" aria-labelledby="briefing-inprogress">
          <h2 class="section-heading" id="briefing-inprogress">
            <span class="section-icon inprogress-icon" aria-hidden="true">◐</span>
            IN PROGRESS
          </h2>
          {#each briefing.in_progress as item (item.id ?? item.title)}
            <div class="section-item" data-testid="in-progress-item">
              <div class="item-title">
                <span class="item-icon inprogress-icon" aria-hidden="true">◐</span>
                <span class="item-name">{item.title}</span>
                {#if scope === 'tenant' && item.workspace_id}
                  <Badge value={workspaceMap[item.workspace_id] ?? item.workspace_id} variant="default" />
                {/if}
                {#if item.spec_ref}
                  <button
                    class="entity-ref"
                    onclick={() => handleViewSpec(item.spec_ref)}
                    data-testid="spec-ref-link"
                    aria-label="View spec {item.spec_ref}"
                  >
                    spec: {item.spec_ref}
                  </button>
                {/if}
              </div>
              <div class="item-detail">
                {#if item.sub_specs_total}
                  <span>{item.sub_specs_done ?? 0}/{item.sub_specs_total} sub-specs complete.</span>
                {/if}
                {#if item.active_agents}
                  <span>{item.active_agents} agent{item.active_agents !== 1 ? 's' : ''} active.</span>
                {/if}
              </div>
              {#if item.uncertainties?.length}
                <div class="uncertainties">
                  {#each item.uncertainties as u}
                    <div class="uncertainty-row">
                      <span class="uncertainty-icon" aria-hidden="true">⚠</span>
                      <button
                        class="entity-ref agent-ref"
                        onclick={() => openEntity('agent', u.agent_id, { name: u.agent_id })}
                        data-testid="agent-ref-link"
                        aria-label="View agent {u.agent_id}"
                      >
                        {u.agent_id}
                      </button>
                      <span class="uncertainty-text">uncertain: "{u.text}"</span>
                    </div>
                  {/each}
                </div>
              {/if}
              <div class="item-actions">
                {#if item.uncertainties?.length}
                  {#each item.uncertainties as u}
                    <button
                      class="action-btn"
                      onclick={() => openEntity('agent', u.agent_id, { name: u.agent_id })}
                      data-testid="respond-to-agent-btn"
                    >
                      Respond to {u.agent_id}
                    </button>
                  {/each}
                {/if}
                {#if item.spec_ref}
                  <button
                    class="action-btn secondary"
                    onclick={() => handleViewSpec(item.spec_ref)}
                    data-testid="view-spec-btn"
                  >
                    View spec
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {/if}

      <!-- CROSS-WORKSPACE -->
      {#if briefing.cross_workspace?.length}
        <section class="briefing-section" data-testid="section-cross-workspace" aria-labelledby="briefing-crossworkspace">
          <h2 class="section-heading" id="briefing-crossworkspace">
            <span class="section-icon cross-icon" aria-hidden="true">↔</span>
            CROSS-WORKSPACE
          </h2>
          {#each briefing.cross_workspace as item (item.id ?? item.spec_ref)}
            <div class="section-item" data-testid="cross-workspace-item">
              <div class="item-title">
                <span class="item-icon cross-icon" aria-hidden="true">↔</span>
                <span class="item-name">{item.description ?? item.spec_ref}</span>
                {#if scope === 'tenant' && item.workspace_id}
                  <Badge value={workspaceMap[item.workspace_id] ?? item.workspace_id} variant="default" />
                {/if}
                {#if item.spec_ref}
                  <button
                    class="entity-ref"
                    onclick={() => handleReviewChanges(item)}
                    data-testid="spec-ref-link"
                    aria-label="Review changes to {item.spec_ref}"
                  >
                    {item.spec_ref}
                  </button>
                {/if}
              </div>
              <div class="item-actions">
                <button
                  class="action-btn"
                  onclick={() => handleReviewChanges(item)}
                  data-testid="review-changes-btn"
                >
                  Review changes
                </button>
                <button
                  class="action-btn secondary"
                  onclick={() => handleDismiss(item)}
                  data-testid="dismiss-btn"
                  aria-label="Dismiss: {item.description ?? item.spec_ref ?? 'item'}"
                >
                  Dismiss
                </button>
              </div>
            </div>
          {/each}
        </section>
      {/if}

      <!-- EXCEPTIONS -->
      {#if briefing.exceptions?.length}
        <section class="briefing-section exceptions-section" data-testid="section-exceptions" aria-labelledby="briefing-exceptions">
          <h2 class="section-heading" id="briefing-exceptions">
            <span class="section-icon exception-icon" aria-hidden="true">✗</span>
            EXCEPTIONS
          </h2>
          {#each briefing.exceptions as item (item.id ?? item.mr_id)}
            <div class="section-item exception-item" data-testid="exception-item">
              <div class="item-title">
                <span class="item-icon exception-icon" aria-hidden="true">✗</span>
                {#if scope === 'tenant' && item.workspace_id}
                  <Badge value={workspaceMap[item.workspace_id] ?? item.workspace_id} variant="default" />
                {/if}
                <span class="item-name">
                  Gate failure:
                  {#if item.repo && item.mr_id}
                    <button
                      class="entity-ref mr-ref"
                      onclick={() => handleViewDiff(item)}
                      data-testid="mr-ref-link"
                      aria-label="View MR #{item.mr_id} in {item.repo}"
                    >
                      {item.repo} MR #{item.mr_id}
                    </button>
                  {:else}
                    {item.description}
                  {/if}
                </span>
              </div>
              {#if item.description}
                <div class="item-detail exception-detail">{item.description}</div>
              {/if}
              <div class="item-actions">
                {#if item.mr_id}
                  <button
                    class="action-btn danger"
                    onclick={() => handleViewDiff(item)}
                    data-testid="view-diff-btn"
                  >
                    View Diff
                  </button>
                  <button
                    class="action-btn secondary"
                    onclick={() => handleViewOutput(item)}
                    data-testid="view-output-btn"
                  >
                    View Output
                  </button>
                  <button
                    class="action-btn secondary"
                    onclick={() => openEntity('mr', item.mr_id, { action: 'override' })}
                    data-testid="override-btn"
                  >
                    Override
                  </button>
                  <button
                    class="action-btn secondary"
                    onclick={() => openEntity('mr', item.mr_id, { action: 'close' })}
                    data-testid="close-mr-btn"
                  >
                    Close MR
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {/if}

      <!-- METRICS -->
      {#if briefing.metrics}
        <section class="metrics-row" data-testid="section-metrics">
          <h2 class="section-heading metrics-heading">METRICS</h2>
          <div class="metrics-grid">
            {#if briefing.metrics.mrs_count != null}
              <div class="metric-cell" data-testid="metric-mrs">
                <span class="metric-val">{briefing.metrics.mrs_count}</span>
                <span class="metric-label">MRs</span>
              </div>
            {/if}
            {#if briefing.metrics.runs_count != null}
              <div class="metric-cell" data-testid="metric-runs">
                <span class="metric-val">{briefing.metrics.runs_count}</span>
                <span class="metric-label">runs</span>
              </div>
            {/if}
            {#if briefing.metrics.cost_usd != null}
              <div class="metric-cell" data-testid="metric-cost">
                <span class="metric-val">${briefing.metrics.cost_usd.toFixed(2)}</span>
                <span class="metric-label">cost</span>
              </div>
            {/if}
            {#if briefing.metrics.budget_pct != null}
              <div class="metric-cell" data-testid="metric-budget">
                <span class="metric-val">{briefing.metrics.budget_pct}%</span>
                <span class="metric-label">budget</span>
              </div>
            {/if}
          </div>
        </section>
      {/if}

      {#if !briefing.completed?.length && !briefing.in_progress?.length && !briefing.cross_workspace?.length && !briefing.exceptions?.length && !briefing.metrics}
        <EmptyState
          title="All caught up"
          description="No activity since your last visit."
        />
      {/if}

      <!-- Q&A Chat (bottom) -->
      <div class="chat-section" data-testid="briefing-chat">
        <InlineChat
          recipient="this briefing"
          recipientType="llm-qa"
          onmessage={briefingAskHandler}
        />
      </div>
    {/if}
  </div>

<style>
  .briefing {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 1000px;
    min-height: 100%;
  }

  .briefing-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .header-left {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .briefing-title {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .briefing-since {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .briefing-workspace {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .briefing-trust {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .time-range-selector {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .range-select {
    appearance: none;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-6) var(--space-2) var(--space-3);
    cursor: pointer;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 6px center;
    background-size: 10px;
  }

  .range-select:focus:not(:focus-visible) {
    outline: none;
  }

  .range-select:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .date-input {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
  }

  .apply-btn {
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse, #fff);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    transition: background var(--transition-fast);
  }

  .apply-btn:hover { background: var(--color-primary-hover); }
  .apply-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .error-banner {
    background: color-mix(in srgb, var(--color-warning, #f59e0b) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-warning, #d97706);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
  }

  .skeleton-stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .briefing-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    background: var(--color-surface);
  }

  .section-heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    margin: 0 0 var(--space-2) 0;
  }

  .completed-icon { color: var(--color-success, #22c55e); }
  .inprogress-icon { color: var(--color-warning, #f59e0b); }
  .cross-icon { color: var(--color-text-secondary); }
  .exception-icon { color: var(--color-danger); }

  .section-item {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .section-item:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .item-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .item-icon { flex-shrink: 0; font-size: var(--text-sm); }

  .item-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .entity-ref {
    background: none;
    border: none;
    padding: 0 var(--space-1);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link, var(--color-primary));
    text-decoration: underline;
    text-underline-offset: 2px;
    transition: color var(--transition-fast);
  }

  .entity-ref:hover { color: var(--color-primary-hover); }
  .agent-ref { color: var(--color-text-secondary); }
  .agent-ref:hover { color: var(--color-text); }
  .mr-ref { color: var(--color-danger); }

  .item-detail {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    padding-left: calc(var(--space-2) + var(--text-sm));
  }

  .item-decision {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .confidence-badge {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
  }

  .confidence-badge.confidence-high   { color: var(--color-success, #22c55e); }
  .confidence-badge.confidence-medium { color: var(--color-warning, #f59e0b); }
  .confidence-badge.confidence-low    { color: var(--color-danger); }

  .uncertainties {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-warning, #f59e0b) 8%, transparent);
    border-radius: var(--radius);
    border-left: 3px solid var(--color-warning, #f59e0b);
    margin-left: calc(var(--space-2) + var(--text-sm));
  }

  .uncertainty-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .uncertainty-icon { color: var(--color-warning, #f59e0b); flex-shrink: 0; }
  .uncertainty-text { color: var(--color-text-secondary); font-style: italic; }

  .item-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding-left: calc(var(--space-2) + var(--text-sm));
  }

  .action-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-2) var(--space-3);
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }

  .action-btn.secondary {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
    color: var(--color-text-secondary);
  }

  .action-btn.secondary:hover {
    border-color: var(--color-border-strong);
    color: var(--color-text);
  }

  .action-btn.danger {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
    color: var(--color-danger);
  }

  .action-btn.danger:hover {
    background: color-mix(in srgb, var(--color-danger) 25%, transparent);
    border-color: var(--color-danger);
  }

  .action-btn:focus-visible,
  .apply-btn:focus-visible,
  .entity-ref:focus-visible,
  .date-input:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .exceptions-section {
    border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);
  }

  .exception-detail {
    color: var(--color-danger);
  }

  .metrics-row {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    background: var(--color-surface);
  }

  .metrics-heading {
    margin: 0 0 var(--space-3) 0;
  }

  .metrics-grid {
    display: flex;
    gap: var(--space-6);
    flex-wrap: wrap;
  }

  .metric-cell {
    display: flex;
    align-items: baseline;
    gap: var(--space-1);
  }

  .metric-val {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .metric-label {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .chat-section {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    background: var(--color-surface);
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .apply-btn,
    .entity-ref,
    .action-btn { transition: none; }
  }
</style>
