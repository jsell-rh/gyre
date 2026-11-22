<script>
  /**
   * AnalyticsView — full-page product analytics dashboard
   * (analytics.md §Analytics Dashboard (UI)).
   *
   * Five sections:
   *   1. Event counts bar chart — daily event volume for the last 30 days
   *      (SVG bars, one bar per day, height = total event count).
   *   2. Recent events — paginated table, filterable by event name,
   *      agent, and workspace; click a row to expand the full
   *      properties JSON.
   *   3. Decision panel — interactive evaluator: run any built-in
   *      decision against live data (GET /api/v1/analytics/decide).
   *   4. Funnel view — default spawn → implement → complete → merge
   *      funnel with drop-off rates, plus custom funnel definitions.
   *   5. Top agents — ranked by events generated, with task completion
   *      rate and average completion duration.
   *
   * Reached from the workspace top bar (chart icon next to the settings
   * gear) or /workspaces/:slug/analytics. Data retention
   * (analytics.md §Data Retention) is surfaced read-only: the configured
   * analytics_events policy from GET /admin/retention (admins only;
   * hidden when the caller cannot read it).
   */
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';
  import { relativeTime } from '../lib/timeFormat.js';

  let { workspace = null, onBack = undefined } = $props();

  // The built-in decisions (analytics.md §Decision API) — the dropdown
  // offers exactly what the server's decide endpoint accepts.
  const BUILTIN_DECISIONS = [
    'feature.promote',
    'feature.rollback',
    'gate.effectiveness',
    'agent.performance',
    'budget.headroom',
    'merge_queue.health',
  ];

  // Default funnel (analytics.md §Analytics Dashboard): spawn →
  // implement → complete → merge. The second step counts task status
  // changes INTO InProgress — the /count property filter matches the
  // event's `new_status` property.
  const DEFAULT_FUNNEL = [
    { label: 'agent.spawned', event_name: 'agent.spawned', property: null },
    { label: 'task InProgress', event_name: 'task.status_changed', property: 'new_status:InProgress' },
    { label: 'agent.completed', event_name: 'agent.completed', property: null },
    { label: 'mr.merged', event_name: 'mr.merged', property: null },
  ];

  const DAY_SECS = 86_400;
  const WINDOW_DAYS = 30;
  const PAGE_SIZE = 50;

  function sinceSecs(days) {
    return Math.floor(Date.now() / 1000) - days * DAY_SECS;
  }

  // ── 1. Event counts bar chart ──────────────────────────────────────────

  let daily = $state([]);
  let dailyLoading = $state(false);
  let dailyError = $state(null);

  /** All 30 calendar days (UTC), zero-filled for days with no events —
   *  the chart shows volume over time, not just active days. */
  let dailyBars = $derived.by(() => {
    const byDate = new Map(daily.map((d) => [d.date, d.count]));
    const bars = [];
    const today = new Date();
    for (let i = WINDOW_DAYS - 1; i >= 0; i--) {
      const d = new Date(
        Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate()) - i * DAY_SECS * 1000,
      );
      const date = d.toISOString().slice(0, 10);
      bars.push({ date, count: byDate.get(date) ?? 0 });
    }
    return bars;
  });
  let dailyMax = $derived(Math.max(1, ...dailyBars.map((b) => b.count)));

  async function loadDaily() {
    dailyLoading = true;
    dailyError = null;
    try {
      daily = (await api.analyticsDaily({ days: WINDOW_DAYS })) ?? [];
    } catch (e) {
      dailyError = e?.message ?? 'Failed to load daily event counts';
    } finally {
      dailyLoading = false;
    }
  }

  // ── 2. Recent events (paginated, filterable) ───────────────────────────

  let events = $state([]);
  let eventsLoading = $state(false);
  let eventsError = $state(null);
  let filterEventName = $state('');
  let filterAgentId = $state('');
  let filterWorkspaceId = $state('');
  let expandedEventId = $state(null);
  let hasMoreEvents = $state(false);
  let seenEventIds = $state(new Set());
  /** Cursor for "load more": the oldest timestamp on the current page —
   *  the next page fetches events at or before it and drops the ids
   *  already shown (events can share a timestamp). */
  let eventsCursorTs = $state(null);

  // Filter option lists, derived from the analytics data itself.
  let eventNameOptions = $state([]);
  let agentOptions = $state([]);
  let workspaceOptions = $state([]);

  async function loadFilterOptions() {
    try {
      const top = (await api.analyticsTop({ limit: 100, since: sinceSecs(WINDOW_DAYS) })) ?? [];
      eventNameOptions = top.map((e) => e.event_name).filter(Boolean);
    } catch { /* filter dropdown stays empty — filtering still works by API */ }
    try {
      const byAgent = (await api.analyticsEvents({ group_by: 'agent_id', since: sinceSecs(WINDOW_DAYS) })) ?? {};
      agentOptions = Object.keys(byAgent).filter((k) => k && k !== '(none)');
    } catch { /* see above */ }
    try {
      const byWs = (await api.analyticsEvents({ group_by: 'workspace_id', since: sinceSecs(WINDOW_DAYS) })) ?? {};
      workspaceOptions = Object.keys(byWs).filter((k) => k && k !== '(none)');
    } catch { /* see above */ }
  }

  async function loadEvents({ append = false } = {}) {
    eventsLoading = true;
    eventsError = null;
    try {
      const params = { limit: PAGE_SIZE };
      if (filterEventName) params.event_name = filterEventName;
      if (filterAgentId) params.agent_id = filterAgentId;
      if (filterWorkspaceId) params.workspace_id = filterWorkspaceId;
      if (append && eventsCursorTs != null) params.until = String(eventsCursorTs);
      const page = (await api.analyticsEvents(params)) ?? [];
      const fresh = page.filter((e) => !seenEventIds.has(e.id));
      if (append) {
        events = [...events, ...fresh];
      } else {
        events = fresh;
        seenEventIds = new Set(fresh.map((e) => e.id));
      }
      for (const e of fresh) seenEventIds.add(e.id);
      hasMoreEvents = page.length === PAGE_SIZE && fresh.length > 0;
      if (events.length > 0) {
        // The API returns newest-first (timestamp DESC); the cursor is
        // the oldest event on the accumulated page.
        eventsCursorTs = Math.min(...events.map((e) => e.timestamp));
      }
    } catch (e) {
      eventsError = e?.message ?? 'Failed to load events';
    } finally {
      eventsLoading = false;
    }
  }

  function applyEventFilters() {
    seenEventIds = new Set();
    eventsCursorTs = null;
    loadEvents();
  }

  function propertiesPreview(props) {
    if (props == null) return '—';
    const s = typeof props === 'string' ? props : JSON.stringify(props);
    return s.length > 60 ? s.slice(0, 60) + '…' : s;
  }

  // ── 3. Decision panel ──────────────────────────────────────────────────

  let decisionName = $state('feature.promote');
  let decisionContext = $state('{}');
  let decisionResult = $state(null);
  let decisionError = $state(null);
  let decisionLoading = $state(false);

  async function evaluateDecision() {
    decisionLoading = true;
    decisionError = null;
    decisionResult = null;
    let context;
    try {
      context = JSON.parse(decisionContext || '{}');
    } catch {
      decisionError = 'Context is not valid JSON';
      decisionLoading = false;
      return;
    }
    if (typeof context !== 'object' || context === null || Array.isArray(context)) {
      decisionError = 'Context must be a JSON object';
      decisionLoading = false;
      return;
    }
    try {
      decisionResult = await api.analyticsDecide(decisionName, context);
    } catch (e) {
      decisionError = e?.message ?? 'Decision evaluation failed';
    } finally {
      decisionLoading = false;
    }
  }

  /** Outcome-shaped fields rendered ahead of the raw metrics. */
  let decisionMainFields = $derived.by(() => {
    if (!decisionResult || typeof decisionResult !== 'object') return [];
    const priority = ['promote', 'rollback', 'healthy', 'outcome', 'confidence', 'reason'];
    return Object.entries(decisionResult).filter(([k]) => priority.includes(k));
  });
  let decisionMetricFields = $derived.by(() => {
    if (!decisionResult || typeof decisionResult !== 'object') return [];
    const priority = ['promote', 'rollback', 'healthy', 'outcome', 'confidence', 'reason', 'decision', 'evaluated_at'];
    return Object.entries(decisionResult).filter(([k]) => !priority.includes(k));
  });

  function formatDecisionValue(v) {
    if (v === true) return '✓ yes';
    if (v === false) return '✗ no';
    if (v === null || v === undefined) return '—';
    if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(3);
    if (typeof v === 'object') return JSON.stringify(v);
    return String(v);
  }

  // ── 4. Funnel view ─────────────────────────────────────────────────────

  let funnelSteps = $state(DEFAULT_FUNNEL.map((s) => ({ ...s })));
  let funnelCounts = $state([]);
  let funnelLoading = $state(false);
  let funnelError = $state(null);
  let customFunnelInput = $state('');
  let customFunnelError = $state(null);

  /** Per-step drop-off relative to the previous step. Null on the first
   *  step and when the previous step has zero events (no ratio). */
  let funnelRates = $derived.by(() => {
    const rates = [];
    let prev = null;
    for (const c of funnelCounts) {
      rates.push(prev > 0 ? c / prev : null);
      prev = c;
    }
    return rates;
  });

  async function loadFunnel() {
    funnelLoading = true;
    funnelError = null;
    try {
      const since = sinceSecs(WINDOW_DAYS);
      const counts = await Promise.all(
        funnelSteps.map((step) =>
          api.analyticsCount({
            event_name: step.event_name,
            since: String(since),
            ...(step.property ? { property: step.property } : {}),
          }),
        ),
      );
      funnelCounts = counts.map((c) => c?.count ?? 0);
    } catch (e) {
      funnelError = e?.message ?? 'Failed to load funnel counts';
    } finally {
      funnelLoading = false;
    }
  }

  /** Apply a custom funnel definition — one step per line:
   *  `event_name` or `event_name property_key:property_value`. */
  function applyCustomFunnel() {
    customFunnelError = null;
    const steps = [];
    for (const rawLine of customFunnelInput.split('\n')) {
      const line = rawLine.trim();
      if (!line) continue;
      const [event_name, property] = line.split(/\s+/);
      if (!event_name || (property && !property.includes(':'))) {
        customFunnelError = `Invalid step "${line}" — expected "event_name" or "event_name key:value"`;
        return;
      }
      steps.push({ label: property ? `${event_name} (${property})` : event_name, event_name, property: property ?? null });
    }
    if (steps.length < 2) {
      customFunnelError = 'A funnel needs at least 2 steps';
      return;
    }
    funnelSteps = steps;
    loadFunnel();
  }

  // ── 5. Top agents ──────────────────────────────────────────────────────

  let topAgents = $state([]);
  let topAgentsLoading = $state(false);
  let topAgentsError = $state(null);

  async function loadTopAgents() {
    topAgentsLoading = true;
    topAgentsError = null;
    try {
      const params = { dimension: 'agent', limit: 10, since: sinceSecs(WINDOW_DAYS) };
      if (workspace?.id) params.workspace_id = workspace.id;
      topAgents = (await api.analyticsTop(params)) ?? [];
    } catch (e) {
      topAgentsError = e?.message ?? 'Failed to load top agents';
    } finally {
      topAgentsLoading = false;
    }
  }

  function pct(rate) {
    return rate == null ? '—' : `${Math.round(rate * 100)}%`;
  }

  function fmtSecs(secs) {
    if (secs == null) return '—';
    if (secs < 90) return `${Math.round(secs)}s`;
    if (secs < 5400) return `${(secs / 60).toFixed(1)}m`;
    return `${(secs / 3600).toFixed(1)}h`;
  }

  // ── Retention (read-only display) ──────────────────────────────────────

  let retentionDays = $state(null);

  async function loadRetention() {
    try {
      const policies = (await api.adminRetention()) ?? [];
      const p = policies.find?.((p) => p?.data_type === 'analytics_events');
      if (p && typeof p.max_age_days === 'number' && p.max_age_days < Number.MAX_SAFE_INTEGER) {
        retentionDays = p.max_age_days;
      }
    } catch { /* non-admin callers cannot read retention — stay hidden */ }
  }

  // ── Initial load ───────────────────────────────────────────────────────

  onMount(() => {
    loadDaily();
    loadEvents();
    loadFilterOptions();
    loadFunnel();
    loadTopAgents();
    loadRetention();
  });
</script>

<div class="analytics-view" data-testid="analytics-view">
  <div class="page-header">
    <button
      class="back-btn"
      onclick={() => onBack?.()}
      aria-label="Back to workspace home"
      data-testid="analytics-back"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 5l-7 7 7 7"/>
      </svg>
    </button>
    <div class="page-title-group">
      <h1 class="page-title" data-testid="analytics-title">Analytics{workspace?.name ? ` — ${workspace.name}` : ''}</h1>
      {#if retentionDays != null}
        <span class="retention-note" data-testid="analytics-retention">
          Events retained {retentionDays} days
        </span>
      {/if}
    </div>
  </div>

  <div class="analytics-content">

    <!-- ── 1. Event counts bar chart ─────────────────────────────────── -->
    <section class="analytics-section" aria-labelledby="analytics-daily-title" data-testid="analytics-daily-section">
      <h2 class="section-title" id="analytics-daily-title">Event volume — last {WINDOW_DAYS} days</h2>
      {#if dailyLoading}
        <div class="section-loading" aria-live="polite">Loading event counts…</div>
      {:else if dailyError}
        <div class="section-error" role="alert" data-testid="analytics-daily-error">{dailyError}</div>
      {:else}
        <svg
          class="daily-chart"
          viewBox="0 0 {WINDOW_DAYS * 10} 100"
          preserveAspectRatio="none"
          role="img"
          aria-label="Daily event counts for the last {WINDOW_DAYS} days"
          data-testid="analytics-daily-chart"
        >
          {#each dailyBars as bar, i}
            <rect
              x={i * 10 + 1}
              y={100 - (bar.count / dailyMax) * 96}
              width="8"
              height={(bar.count / dailyMax) * 96}
              class="daily-bar"
            ><title>{bar.date}: {bar.count} events</title></rect>
          {/each}
        </svg>
        <div class="chart-meta">
          <span>{dailyBars[0]?.date}</span>
          <span class="chart-meta-max">peak {dailyMax}</span>
          <span>{dailyBars[dailyBars.length - 1]?.date}</span>
        </div>
      {/if}
    </section>

    <!-- ── 2. Recent events ──────────────────────────────────────────── -->
    <section class="analytics-section" aria-labelledby="analytics-events-title" data-testid="analytics-events-section">
      <h2 class="section-title" id="analytics-events-title">Recent events</h2>

      <div class="events-filters">
        <label class="filter-label">
          Event
          <select class="filter-select" bind:value={filterEventName} onchange={applyEventFilters} data-testid="analytics-filter-event">
            <option value="">All</option>
            {#each eventNameOptions as name}
              <option value={name}>{name}</option>
            {/each}
          </select>
        </label>
        <label class="filter-label">
          Agent
          <select class="filter-select" bind:value={filterAgentId} onchange={applyEventFilters} data-testid="analytics-filter-agent">
            <option value="">All</option>
            {#each agentOptions as id}
              <option value={id}>{id}</option>
            {/each}
          </select>
        </label>
        <label class="filter-label">
          Workspace
          <select class="filter-select" bind:value={filterWorkspaceId} onchange={applyEventFilters} data-testid="analytics-filter-workspace">
            <option value="">All</option>
            {#each workspaceOptions as id}
              <option value={id}>{id}</option>
            {/each}
          </select>
        </label>
      </div>

      {#if eventsError}
        <div class="section-error" role="alert" data-testid="analytics-events-error">{eventsError}</div>
      {:else if !eventsLoading && events.length === 0}
        <div class="section-empty">No events match the current filters.</div>
      {:else}
        <table class="events-table" data-testid="analytics-events-table">
          <thead>
            <tr>
              <th>Event</th>
              <th>Agent</th>
              <th>Timestamp</th>
              <th>Properties</th>
            </tr>
          </thead>
          <tbody>
            {#each events as e (e.id)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <tr
                class="event-row"
                class:expanded={expandedEventId === e.id}
                onclick={() => (expandedEventId = expandedEventId === e.id ? null : e.id)}
              >
                <td class="cell-mono">{e.event_name}</td>
                <td class="cell-mono">{e.agent_id ?? '—'}</td>
                <td class="cell-time" title={new Date(e.timestamp * 1000).toISOString()}>{relativeTime(e.timestamp)}</td>
                <td class="cell-props">
                  {#if expandedEventId === e.id}
                    <pre class="props-json" data-testid="analytics-event-props">{JSON.stringify(e.properties, null, 2)}</pre>
                  {:else}
                    <span class="props-preview">{propertiesPreview(e.properties)}</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="events-footer">
          <span class="events-count" data-testid="analytics-events-count">{events.length} shown</span>
          {#if hasMoreEvents}
            <button class="load-more-btn" onclick={() => loadEvents({ append: true })} disabled={eventsLoading} data-testid="analytics-events-more">
              {eventsLoading ? 'Loading…' : 'Load more'}
            </button>
          {/if}
        </div>
      {/if}
    </section>

    <!-- ── 3. Decision panel ─────────────────────────────────────────── -->
    <section class="analytics-section" aria-labelledby="analytics-decision-title" data-testid="analytics-decision-section">
      <h2 class="section-title" id="analytics-decision-title">Decision evaluator</h2>
      <div class="decision-form">
        <label class="filter-label">
          Decision
          <select class="filter-select" bind:value={decisionName} data-testid="analytics-decision-select">
            {#each BUILTIN_DECISIONS as name}
              <option value={name}>{name}</option>
            {/each}
          </select>
        </label>
        <label class="filter-label decision-context">
          Context (JSON)
          <textarea
            class="decision-context-input"
            bind:value={decisionContext}
            rows="4"
            spellcheck="false"
            data-testid="analytics-decision-context"
            placeholder={'\"flag\": \"new-checkout\", \"workspace_id\": \"...\"'}
          ></textarea>
        </label>
        <button
          class="evaluate-btn"
          onclick={evaluateDecision}
          disabled={decisionLoading}
          data-testid="analytics-decision-evaluate"
        >
          {decisionLoading ? 'Evaluating…' : 'Evaluate'}
        </button>
      </div>

      {#if decisionError}
        <div class="section-error" role="alert" data-testid="analytics-decision-error">{decisionError}</div>
      {/if}

      {#if decisionResult}
        <div class="decision-result" data-testid="analytics-decision-result">
          {#each decisionMainFields as [key, value]}
            <div class="decision-row decision-main" data-testid="analytics-decision-{key}">
              <span class="decision-key">{key}</span>
              <span class="decision-value" class:bool-true={value === true} class:bool-false={value === false}>
                {formatDecisionValue(value)}
              </span>
            </div>
          {/each}
          {#if decisionMetricFields.length > 0}
            <div class="decision-metrics">
              {#each decisionMetricFields as [key, value]}
                <div class="decision-row">
                  <span class="decision-key">{key}</span>
                  <span class="decision-value">{formatDecisionValue(value)}</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </section>

    <!-- ── 4. Funnel view ────────────────────────────────────────────── -->
    <section class="analytics-section" aria-labelledby="analytics-funnel-title" data-testid="analytics-funnel-section">
      <h2 class="section-title" id="analytics-funnel-title">Funnel</h2>

      {#if funnelLoading}
        <div class="section-loading" aria-live="polite">Loading funnel…</div>
      {:else if funnelError}
        <div class="section-error" role="alert" data-testid="analytics-funnel-error">{funnelError}</div>
      {:else}
        <div class="funnel" data-testid="analytics-funnel">
          {#each funnelSteps as step, i}
            {@const count = funnelCounts[i] ?? 0}
            {@const rate = funnelRates[i]}
            {@const width = funnelCounts[0] > 0 ? Math.max(6, (count / funnelCounts[0]) * 100) : 100}
            <div class="funnel-step" data-testid="analytics-funnel-step-{i}">
              <div class="funnel-bar-wrap">
                <div class="funnel-bar" style="width: {width}%"></div>
                <span class="funnel-count">{count}</span>
              </div>
              <div class="funnel-label">
                <span class="funnel-name">{step.label}</span>
                {#if i > 0}
                  <span
                    class="funnel-dropoff"
                    data-testid="analytics-funnel-dropoff-{i}"
                    class:dropoff-bad={rate != null && rate < 0.5}
                  >
                    {#if rate != null}
                      {Math.round(rate * 100)}% continue ({Math.round((1 - rate) * 100)}% drop-off)
                    {:else}
                      no prior step
                    {/if}
                  </span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}

      <details class="custom-funnel">
        <summary data-testid="analytics-funnel-custom-toggle">Custom funnel</summary>
        <p class="custom-funnel-hint">
          One step per line: <code>event_name</code> or
          <code>event_name key:value</code> (property equality, e.g.
          <code>task.status_changed new_status:InProgress</code>).
        </p>
        <textarea
          class="custom-funnel-input"
          bind:value={customFunnelInput}
          rows="4"
          spellcheck="false"
          data-testid="analytics-funnel-custom-input"
          placeholder={'agent.spawned\ntask.status_changed new_status:InProgress\nagent.completed\nmr.merged'}
        ></textarea>
        {#if customFunnelError}
          <div class="section-error" role="alert" data-testid="analytics-funnel-custom-error">{customFunnelError}</div>
        {/if}
        <button class="apply-funnel-btn" onclick={applyCustomFunnel} data-testid="analytics-funnel-custom-apply">
          Apply funnel
        </button>
      </details>
    </section>

    <!-- ── 5. Top agents ─────────────────────────────────────────────── -->
    <section class="analytics-section" aria-labelledby="analytics-top-title" data-testid="analytics-top-section">
      <h2 class="section-title" id="analytics-top-title">Top agents — last {WINDOW_DAYS} days</h2>
      {#if topAgentsLoading}
        <div class="section-loading" aria-live="polite">Loading top agents…</div>
      {:else if topAgentsError}
        <div class="section-error" role="alert" data-testid="analytics-top-error">{topAgentsError}</div>
      {:else if topAgents.length === 0}
        <div class="section-empty">No agent activity in the last {WINDOW_DAYS} days.</div>
      {:else}
        <table class="events-table" data-testid="analytics-top-table">
          <thead>
            <tr>
              <th>#</th>
              <th>Agent</th>
              <th>Events</th>
              <th>Completed</th>
              <th>Failed</th>
              <th>Completion rate</th>
              <th>Avg duration</th>
            </tr>
          </thead>
          <tbody>
            {#each topAgents as a, i}
              <tr data-testid="analytics-top-row">
                <td>{i + 1}</td>
                <td class="cell-mono" title={a.agent_id}>{a.agent_id}</td>
                <td>{a.event_count}</td>
                <td>{a.completed}</td>
                <td>{a.failed}</td>
                <td data-testid="analytics-top-rate">{pct(a.completion_rate)}</td>
                <td>{fmtSecs(a.avg_duration_secs)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </section>
  </div>
</div>

<style>
  .analytics-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    background: var(--color-bg);
  }

  .page-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: var(--radius);
    flex-shrink: 0;
  }

  .back-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .page-title-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .retention-note {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .analytics-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5) var(--space-6) var(--space-8);
    max-width: 1100px;
  }

  .analytics-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-5);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3);
  }

  .section-loading,
  .section-empty {
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    padding: var(--space-3) 0;
  }

  .section-error {
    color: var(--color-danger, #e00);
    font-size: var(--text-sm);
    padding: var(--space-2) 0;
  }

  /* ── Bar chart ── */
  .daily-chart {
    width: 100%;
    height: 140px;
    display: block;
  }

  .daily-bar {
    fill: var(--color-primary);
  }

  .daily-bar:hover {
    fill: var(--color-primary-hover, var(--color-primary));
  }

  .chart-meta {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin-top: var(--space-2);
  }

  /* ── Filters ── */
  .events-filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }

  .filter-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .filter-select {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    min-width: 160px;
  }

  /* ── Events table ── */
  .events-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .events-table th {
    text-align: left;
    font-weight: 600;
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .events-table td {
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: top;
  }

  .event-row {
    cursor: pointer;
  }

  .event-row:hover {
    background: var(--color-surface-elevated);
  }

  .event-row.expanded .props-preview {
    display: none;
  }

  .cell-mono {
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
  }

  .cell-time {
    white-space: nowrap;
    color: var(--color-text-secondary);
  }

  .cell-props {
    max-width: 380px;
    word-break: break-all;
  }

  .props-preview {
    color: var(--color-text-secondary);
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
  }

  .props-json {
    margin: 0;
    padding: var(--space-2);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 260px;
    overflow-y: auto;
  }

  .events-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-top: var(--space-2);
  }

  .events-count {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .load-more-btn,
  .evaluate-btn,
  .apply-funnel-btn {
    background: var(--color-primary);
    color: var(--color-text-inverted, #fff);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .load-more-btn:disabled,
  .evaluate-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .load-more-btn:focus-visible,
  .evaluate-btn:focus-visible,
  .apply-funnel-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Decision panel ── */
  .decision-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .decision-context-input,
  .custom-funnel-input {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2);
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
    resize: vertical;
  }

  .decision-result {
    margin-top: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--color-bg);
  }

  .decision-row {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-1) 0;
    font-size: var(--text-sm);
  }

  .decision-main .decision-key {
    font-weight: 600;
  }

  .decision-key {
    min-width: 180px;
    color: var(--color-text-secondary);
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
  }

  .decision-value {
    color: var(--color-text);
    word-break: break-all;
  }

  .bool-true {
    color: var(--color-success, #3c8);
    font-weight: 600;
  }

  .bool-false {
    color: var(--color-danger, #e00);
    font-weight: 600;
  }

  .decision-metrics {
    margin-top: var(--space-2);
    border-top: 1px dashed var(--color-border);
    padding-top: var(--space-2);
  }

  /* ── Funnel ── */
  .funnel {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .funnel-step {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .funnel-bar-wrap {
    position: relative;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    height: 28px;
  }

  .funnel-bar {
    height: 100%;
    background: var(--color-primary);
    border-radius: var(--radius) 0 0 var(--radius);
    opacity: 0.85;
  }

  .funnel-count {
    position: absolute;
    right: var(--space-2);
    top: 50%;
    transform: translateY(-50%);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  .funnel-label {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .funnel-name {
    font-family: var(--font-mono, monospace);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  .funnel-dropoff {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .dropoff-bad {
    color: var(--color-danger, #e00);
  }

  .custom-funnel {
    margin-top: var(--space-3);
  }

  .custom-funnel summary {
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .custom-funnel-hint {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: var(--space-2) 0;
  }

  .custom-funnel-input {
    width: 100%;
  }

  .apply-funnel-btn {
    margin-top: var(--space-2);
  }
</style>
