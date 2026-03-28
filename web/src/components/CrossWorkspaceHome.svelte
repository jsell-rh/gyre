<script>
  /**
   * CrossWorkspaceHome — tenant-scope cross-workspace dashboard (§10 of ui-navigation.md)
   *
   * Sections: Decisions, Workspaces, Specs, Briefing, Agent Rules.
   * Shown when user navigates to /all.
   *
   * Spec refs:
   *   ui-navigation.md §10 (Cross-Workspace View)
   */
  import { api } from '../lib/api.js';

  let {
    onSelectWorkspace = undefined,
  } = $props();

  // ── Notification type icons (HSI §8) ────────────────────────────────────
  const TYPE_ICONS = {
    agent_clarification: '?',
    spec_approval: '✋',
    gate_failure: '⚠',
    cross_workspace_change: '↔',
    conflicting_interpretations: '⚡',
    meta_spec_drift: '~',
    budget_warning: '💰',
    trust_suggestion: '🔒',
    spec_assertion_failure: '✗',
    suggested_link: '🔗',
  };

  const SPEC_STATUS_ICONS = {
    draft: '📝',
    pending: '⏳',
    approved: '✅',
    implemented: '✅',
    merged: '✅',
  };

  const KIND_LABELS = {
    Persona: 'Persona',
    Principle: 'Principle',
    Standard: 'Standard',
    Process: 'Process',
  };

  // ── Decisions state ─────────────────────────────────────────────────────
  let decisionsLoading = $state(true);
  let decisionsError = $state(null);
  let notifications = $state([]);

  // ── Workspaces state ────────────────────────────────────────────────────
  let workspacesLoading = $state(true);
  let workspacesError = $state(null);
  let workspaces = $state([]);

  // ── Specs state ─────────────────────────────────────────────────────────
  let specsLoading = $state(true);
  let specsError = $state(null);
  let specs = $state([]);

  // ── Briefing state ───────────────────────────────────────────────────────
  // Cross-workspace briefing: aggregate per-workspace briefings (§10)
  let briefingLoading = $state(true);
  let briefingError = $state(null);
  let briefingSummaries = $state([]); // [{ workspaceName, summary }]

  // ── Agent Rules state ────────────────────────────────────────────────────
  let rulesLoading = $state(true);
  let rulesError = $state(null);
  let globalMetaSpecs = $state([]);

  // ── Load all sections ────────────────────────────────────────────────────
  $effect(() => {
    loadDecisions();
    loadWorkspaces().then(() => loadBriefings());
    loadSpecs();
    loadAgentRules();
  });

  async function loadDecisions() {
    decisionsLoading = true;
    decisionsError = null;
    try {
      const data = await api.myNotifications({ limit: 20, unread: true });
      notifications = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      decisionsError = e?.message ?? 'Failed to load decisions';
    } finally {
      decisionsLoading = false;
    }
  }

  async function loadWorkspaces() {
    workspacesLoading = true;
    workspacesError = null;
    try {
      const data = await api.workspaces();
      workspaces = Array.isArray(data) ? data : [];
    } catch (e) {
      workspacesError = e?.message ?? 'Failed to load workspaces';
    } finally {
      workspacesLoading = false;
    }
  }

  async function loadSpecs() {
    specsLoading = true;
    specsError = null;
    try {
      // All specs across all workspaces (no workspace_id filter)
      const data = await api.specsForWorkspace(null);
      specs = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      specsError = e?.message ?? 'Failed to load specs';
    } finally {
      specsLoading = false;
    }
  }

  // loadBriefings is called after loadWorkspaces completes so we have the workspace list
  async function loadBriefings() {
    briefingLoading = true;
    briefingError = null;
    try {
      // Spec §10: client-side aggregation — call briefing per workspace, merge sections
      const results = await Promise.allSettled(
        workspaces.map(async (ws) => {
          const data = await api.getWorkspaceBriefing(ws.id);
          const summary = data?.summary ?? data?.content ?? '';
          return { workspaceName: ws.name, summary };
        })
      );
      briefingSummaries = results
        .filter((r) => r.status === 'fulfilled' && r.value.summary)
        .map((r) => r.value);
    } catch (e) {
      briefingError = e?.message ?? 'Failed to load briefing';
    } finally {
      briefingLoading = false;
    }
  }

  async function loadAgentRules() {
    rulesLoading = true;
    rulesError = null;
    try {
      const data = await api.metaSpecs({ scope: 'Global' });
      globalMetaSpecs = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      rulesError = e?.message ?? 'Failed to load agent rules';
    } finally {
      rulesLoading = false;
    }
  }

  // ── Derived ──────────────────────────────────────────────────────────────
  let specsByKind = $derived.by(() => {
    const groups = {};
    for (const ms of globalMetaSpecs) {
      const k = ms.kind ?? 'Other';
      if (!groups[k]) groups[k] = [];
      groups[k].push(ms);
    }
    return groups;
  });
</script>

<div class="cross-workspace-home" data-testid="cross-workspace-home">
  <div class="cwh-header">
    <h1 class="cwh-title">All Workspaces</h1>
    <p class="cwh-subtitle">Tenant-scope overview — decisions, workspaces, specs, briefing, and agent rules across your organization.</p>
  </div>

  <!-- ── Decisions ─────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-decisions" aria-labelledby="decisions-heading">
    <div class="section-header">
      <h2 class="section-title" id="decisions-heading">
        Decisions
        {#if notifications.length > 0}
          <span class="section-badge" aria-label="{notifications.length} pending">{notifications.length}</span>
        {/if}
      </h2>
    </div>

    {#if decisionsLoading}
      <div class="section-loading" aria-live="polite">Loading decisions…</div>
    {:else if decisionsError}
      <div class="section-error" role="alert">{decisionsError}</div>
    {:else if notifications.length === 0}
      <p class="section-empty">No decisions needed — system is running autonomously.</p>
    {:else}
      <ul class="decisions-list" role="list">
        {#each notifications.slice(0, 5) as notif (notif.id)}
          <li class="decision-item">
            <span class="decision-icon" aria-hidden="true">{TYPE_ICONS[notif.notification_type] ?? '•'}</span>
            <div class="decision-body">
              <span class="decision-desc">{notif.message ?? notif.title ?? 'Decision pending'}</span>
              {#if notif.workspace_name}
                <span class="decision-ws-badge">{notif.workspace_name}</span>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
      {#if notifications.length > 5}
        <div class="section-footer">
          <span class="view-all-hint">{notifications.length - 5} more decisions…</span>
        </div>
      {/if}
    {/if}
  </section>

  <!-- ── Workspaces ────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-workspaces" aria-labelledby="workspaces-heading">
    <div class="section-header">
      <h2 class="section-title" id="workspaces-heading">Workspaces</h2>
    </div>

    {#if workspacesLoading}
      <div class="section-loading" aria-live="polite">Loading workspaces…</div>
    {:else if workspacesError}
      <div class="section-error" role="alert">{workspacesError}</div>
    {:else if workspaces.length === 0}
      <p class="section-empty">No workspaces found.</p>
    {:else}
      <ul class="workspace-list" role="list">
        {#each workspaces as ws (ws.id)}
          <li class="workspace-row">
            <button
              class="workspace-btn"
              onclick={() => onSelectWorkspace?.(ws)}
              data-testid="workspace-row-{ws.id}"
            >
              <span class="workspace-name">{ws.name}</span>
              <span class="workspace-meta">
                {#if ws.agent_count != null}
                  <span>{ws.agent_count} agents</span>
                {/if}
                {#if ws.budget_pct != null}
                  <span>Budget: {ws.budget_pct}%</span>
                {/if}
                {#if ws.health}
                  <span class="health-badge" class:health-ok={ws.health === 'healthy'} class:health-warn={ws.health === 'gate_failure'}>
                    {ws.health === 'healthy' ? '●' : '⚠'} {ws.health}
                  </span>
                {/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- ── Specs ─────────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-specs" aria-labelledby="specs-heading">
    <div class="section-header">
      <h2 class="section-title" id="specs-heading">Specs</h2>
    </div>

    {#if specsLoading}
      <div class="section-loading" aria-live="polite">Loading specs…</div>
    {:else if specsError}
      <div class="section-error" role="alert">{specsError}</div>
    {:else if specs.length === 0}
      <p class="section-empty">No specs found across workspaces.</p>
    {:else}
      <table class="specs-table" data-testid="specs-table">
        <thead>
          <tr>
            <th scope="col">Path</th>
            <th scope="col">Workspace / Repo</th>
            <th scope="col">Status</th>
          </tr>
        </thead>
        <tbody>
          {#each specs.slice(0, 10) as spec (spec.path ?? spec.id)}
            <tr class="spec-row">
              <td class="spec-path">{spec.path ?? spec.name ?? '—'}</td>
              <td class="spec-attribution">
                {#if spec.workspace_name}
                  <span class="ws-tag">{spec.workspace_name}</span>
                {/if}
                {#if spec.repo_name}
                  <span class="repo-tag">{spec.repo_name}</span>
                {/if}
              </td>
              <td class="spec-status">
                <span>{SPEC_STATUS_ICONS[spec.status] ?? ''} {spec.status ?? '—'}</span>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if specs.length > 10}
        <div class="section-footer">
          <span class="view-all-hint">{specs.length - 10} more specs…</span>
        </div>
      {/if}
    {/if}
  </section>

  <!-- ── Briefing ─────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-briefing" aria-labelledby="briefing-heading">
    <div class="section-header">
      <h2 class="section-title" id="briefing-heading">Briefing</h2>
      <span class="section-scope-tag">Aggregated</span>
    </div>

    {#if briefingLoading}
      <div class="section-loading" aria-live="polite">Loading briefing…</div>
    {:else if briefingError}
      <div class="section-error" role="alert">{briefingError}</div>
    {:else if briefingSummaries.length === 0}
      <p class="section-empty">No briefing data available across workspaces.</p>
    {:else}
      <ul class="briefing-list" role="list">
        {#each briefingSummaries as item (item.workspaceName)}
          <li class="briefing-item">
            <span class="briefing-ws-badge">{item.workspaceName}</span>
            <p class="briefing-summary">{item.summary}</p>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- ── Agent Rules ────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-agent-rules" aria-labelledby="agent-rules-heading">
    <div class="section-header">
      <h2 class="section-title" id="agent-rules-heading">Agent Rules</h2>
      <span class="section-scope-tag">Tenant-level</span>
    </div>

    {#if rulesLoading}
      <div class="section-loading" aria-live="polite">Loading agent rules…</div>
    {:else if rulesError}
      <div class="section-error" role="alert">{rulesError}</div>
    {:else if globalMetaSpecs.length === 0}
      <p class="section-empty">No tenant-level agent rules defined.</p>
    {:else}
      {#each Object.entries(specsByKind) as [kind, items] (kind)}
        <div class="rules-group">
          <h3 class="rules-group-title">{KIND_LABELS[kind] ?? kind}</h3>
          <ul class="rules-list" role="list">
            {#each items as ms (ms.id)}
              <li class="rule-row">
                <span class="rule-name">{ms.name ?? ms.path ?? '—'}</span>
                {#if ms.required}
                  <span class="rule-required" aria-label="Required">🔒</span>
                {/if}
                <span class="rule-version">v{ms.version ?? 1}</span>
                <span class="rule-status" class:status-approved={ms.status === 'Approved'}>
                  {ms.status ?? '—'}
                </span>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    {/if}
  </section>
</div>

<style>
  .cross-workspace-home {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-8) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 900px;
    margin: 0 auto;
    width: 100%;
  }

  .cwh-header {
    margin-bottom: var(--space-2);
  }

  .cwh-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
  }

  .cwh-subtitle {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* ── Sections ─────────────────────────────────────────────────────────── */
  .cwh-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    gap: var(--space-3);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .section-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 20px;
    height: 20px;
    padding: 0 var(--space-1);
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .section-scope-tag {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
  }

  .section-loading,
  .section-empty {
    padding: var(--space-6);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .section-error {
    padding: var(--space-4) var(--space-6);
    font-size: var(--text-sm);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-left: 3px solid var(--color-danger);
    margin: var(--space-4) var(--space-6);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  .section-footer {
    padding: var(--space-3) var(--space-6);
    border-top: 1px solid var(--color-border);
  }

  .view-all-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* ── Decisions ────────────────────────────────────────────────────────── */
  .decisions-list {
    list-style: none;
    margin: 0;
    padding: var(--space-2) 0;
  }

  .decision-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-6);
    transition: background var(--transition-fast);
  }

  .decision-item:hover {
    background: var(--color-surface-elevated);
  }

  .decision-icon {
    font-size: var(--text-base);
    flex-shrink: 0;
    margin-top: 1px;
  }

  .decision-body {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: 1;
    min-width: 0;
  }

  .decision-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .decision-ws-badge {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
    white-space: nowrap;
    flex-shrink: 0;
  }

  /* ── Workspaces ───────────────────────────────────────────────────────── */
  .workspace-list {
    list-style: none;
    margin: 0;
    padding: var(--space-2) 0;
  }

  .workspace-row {
    border-bottom: 1px solid var(--color-border);
  }

  .workspace-row:last-child { border-bottom: none; }

  .workspace-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-3) var(--space-6);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
    gap: var(--space-4);
  }

  .workspace-btn:hover {
    background: var(--color-surface-elevated);
  }

  .workspace-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .workspace-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .workspace-meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .health-badge { white-space: nowrap; }
  .health-ok { color: var(--color-success); }
  .health-warn { color: var(--color-warning); }

  /* ── Specs ────────────────────────────────────────────────────────────── */
  .specs-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .specs-table th {
    padding: var(--space-2) var(--space-6);
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .spec-row {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }

  .spec-row:last-child { border-bottom: none; }

  .spec-row:hover { background: var(--color-surface-elevated); }

  .spec-row td {
    padding: var(--space-3) var(--space-6);
    color: var(--color-text-secondary);
    vertical-align: middle;
  }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spec-attribution {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .ws-tag,
  .repo-tag {
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
  }

  .ws-tag {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-primary);
  }

  .repo-tag {
    background: var(--color-border);
    color: var(--color-text-muted);
  }

  .spec-status { white-space: nowrap; }

  /* ── Briefing ─────────────────────────────────────────────────────────── */
  .briefing-list {
    list-style: none;
    margin: 0;
    padding: var(--space-2) 0;
  }

  .briefing-item {
    padding: var(--space-3) var(--space-6);
    border-bottom: 1px solid var(--color-border);
  }

  .briefing-item:last-child { border-bottom: none; }

  .briefing-ws-badge {
    display: inline-block;
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
    margin-bottom: var(--space-2);
  }

  .briefing-summary {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
    margin: 0;
  }

  /* ── Agent Rules ──────────────────────────────────────────────────────── */
  .rules-group {
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
  }

  .rules-group:last-child { border-bottom: none; }

  .rules-group-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--space-2) 0;
  }

  .rules-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .rule-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
  }

  .rule-name {
    flex: 1;
    color: var(--color-text-secondary);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rule-required { flex-shrink: 0; }

  .rule-version {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .rule-status {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .rule-status.status-approved { color: var(--color-success); }

  /* ── Responsive ───────────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .cross-workspace-home {
      padding: var(--space-4) var(--space-3);
    }

    .section-header,
    .decision-item,
    .workspace-btn,
    .rules-group {
      padding-left: var(--space-4);
      padding-right: var(--space-4);
    }

    .specs-table th,
    .spec-row td {
      padding-left: var(--space-4);
      padding-right: var(--space-4);
    }

    /* Hide workspace/repo attribution on small screens to save space */
    .spec-attribution { display: none; }
  }
</style>
