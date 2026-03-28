<script>
  /**
   * WorkspaceHome — workspace dashboard (§2 of ui-navigation.md)
   *
   * Sections: Decisions, Repos, Briefing, Specs, Agent Rules.
   * Implements real data loading for all five sections.
   *
   * Spec refs:
   *   ui-navigation.md §2 (Workspace Home sections)
   *   HSI §8 (notification types + priority table)
   *   HSI §2 (trust-level filtering)
   */
  import { api } from '../lib/api.js';
  import Briefing from './Briefing.svelte';

  let {
    workspace = null,
    onSelectRepo = undefined,
    decisionsCount = 0,
  } = $props();

  // ── Notification type icons + labels (HSI §8) ─────────────────────────
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

  const SPEC_STATUS_ICONS = {
    draft: '📝',
    pending: '⏳',
    approved: '✅',
    implemented: '✅',
    merged: '✅',
  };

  // ── Decisions state ────────────────────────────────────────────────────
  let decisionsLoading = $state(true);
  let decisionsError = $state(null);
  let notifications = $state([]);
  let actionStates = $state({});

  // ── Repos state ────────────────────────────────────────────────────────
  let reposLoading = $state(true);
  let reposError = $state(null);
  let repos = $state([]);
  let showNewRepoModal = $state(false);

  // ── Specs state ────────────────────────────────────────────────────────
  let specsLoading = $state(true);
  let specsError = $state(null);
  let specs = $state([]);
  let specsStatusFilter = $state('');
  let specsOwnerMe = $state(false);

  // ── Agent Rules state ──────────────────────────────────────────────────
  let rulesLoading = $state(true);
  let rulesError = $state(null);
  let workspaceMetaSpecs = $state([]);
  let globalMetaSpecs = $state([]);

  // ── Repo lookup map (id → repo) ────────────────────────────────────────
  let repoMap = $state({});

  // ── Trust-level filtering ──────────────────────────────────────────────
  // At Guided/Autonomous trust, exclude priority-10 items (suggested links)
  function shouldExcludeByTrust(n) {
    const trust = workspace?.trust_level;
    if (trust === 'Guided' || trust === 'Autonomous') {
      return (n.priority ?? 0) >= 10;
    }
    return false;
  }

  // ── Health computation ─────────────────────────────────────────────────
  // Derived from gate_failure notifications + active_agents count on repo
  function repoHealth(repo) {
    const hasGateFailure = notifications.some(
      n => n.notification_type === 'gate_failure' &&
           n.repo_id === repo.id &&
           !n.resolved_at
    );
    if (hasGateFailure) return 'gate';
    if ((repo.active_agents ?? 0) > 0) return 'healthy';
    return 'idle';
  }

  // ── Notification body parsing ──────────────────────────────────────────
  function getBody(n) {
    try {
      return JSON.parse(n.body || '{}');
    } catch {
      return {};
    }
  }

  function normalizeSpecPath(path) {
    return path ? path.replace(/^specs\//, '') : path;
  }

  // ── Decisions: load ────────────────────────────────────────────────────
  async function loadDecisions() {
    if (!workspace?.id) return;
    decisionsLoading = true;
    decisionsError = null;
    try {
      let data = await api.myNotifications();
      if (!Array.isArray(data)) data = [];
      data = data.filter(n => n.workspace_id === workspace.id);
      data = data.filter(n => !n.dismissed_at && !n.resolved_at);
      data = data.filter(n => !shouldExcludeByTrust(n));
      data.sort((a, b) => (a.priority ?? 999) - (b.priority ?? 999));
      notifications = data;
    } catch (e) {
      decisionsError = e.message || 'Failed to load decisions';
      notifications = [];
    } finally {
      decisionsLoading = false;
    }
  }

  // ── Repos: load ────────────────────────────────────────────────────────
  async function loadRepos() {
    if (!workspace?.id) return;
    reposLoading = true;
    reposError = null;
    try {
      const data = await api.workspaceRepos(workspace.id);
      repos = Array.isArray(data) ? data : [];
      repoMap = Object.fromEntries(repos.map(r => [r.id, r]));
    } catch (e) {
      reposError = e.message || 'Failed to load repos';
      repos = [];
    } finally {
      reposLoading = false;
    }
  }

  // ── Specs: load ────────────────────────────────────────────────────────
  async function loadSpecs() {
    if (!workspace?.id) return;
    specsLoading = true;
    specsError = null;
    try {
      const data = await api.specsForWorkspace(workspace.id);
      specs = Array.isArray(data) ? data : [];
    } catch (e) {
      specsError = e.message || 'Failed to load specs';
      specs = [];
    } finally {
      specsLoading = false;
    }
  }

  // ── Agent Rules: load ──────────────────────────────────────────────────
  async function loadRules() {
    if (!workspace?.id) return;
    rulesLoading = true;
    rulesError = null;
    try {
      const [wsData, globalData] = await Promise.all([
        api.getMetaSpecs({ scope: 'Workspace', scope_id: workspace.id }).catch(() => []),
        api.getMetaSpecs({ scope: 'Global' }).catch(() => []),
      ]);
      workspaceMetaSpecs = Array.isArray(wsData) ? wsData : [];
      globalMetaSpecs = Array.isArray(globalData) ? globalData : [];
    } catch (e) {
      rulesError = e.message || 'Failed to load agent rules';
    } finally {
      rulesLoading = false;
    }
  }

  // ── Notification inline actions ────────────────────────────────────────
  async function handleApproveSpec(n) {
    const body = getBody(n);
    if (!body.spec_path || !body.spec_sha) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'approve' } };
    try {
      await api.approveSpec(normalizeSpecPath(body.spec_path), body.spec_sha);
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: 'Approved' } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || 'Failed' } };
    }
  }

  async function handleRejectSpec(n) {
    const body = getBody(n);
    if (!body.spec_path) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'reject' } };
    try {
      await api.revokeSpec(normalizeSpecPath(body.spec_path), 'Rejected');
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: new Date().toISOString() } : item
      );
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: 'Rejected' } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || 'Failed' } };
    }
  }

  async function handleRetry(n) {
    const body = getBody(n);
    if (!body.mr_id) return;
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.enqueue(body.mr_id);
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: 'Re-queued' } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || 'Failed' } };
    }
  }

  async function handleDismiss(n) {
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.markNotificationRead(n.id);
    } catch {
      // best-effort dismiss
    }
    notifications = notifications.filter(item => item.id !== n.id);
    actionStates = { ...actionStates, [n.id]: { loading: false } };
  }

  // ── Spec navigation ────────────────────────────────────────────────────
  function navigateToSpec(spec) {
    const repo = repoMap[spec.repo_id];
    if (repo && onSelectRepo) {
      onSelectRepo(repo, 'specs', spec.path);
    }
  }

  // ── Derived: filtered specs ────────────────────────────────────────────
  let filteredSpecs = $derived(
    specs.filter(s => {
      if (specsStatusFilter && s.status !== specsStatusFilter) return false;
      return true;
    })
  );

  // ── Derived: meta-spec aggregates ─────────────────────────────────────
  let allMetaSpecs = $derived([...globalMetaSpecs, ...workspaceMetaSpecs]);
  let requiredMetaSpecs = $derived(allMetaSpecs.filter(m => m.required));
  let recentlyUpdated = $derived(
    allMetaSpecs.filter(m => {
      if (!m.updated_at) return false;
      const age = Date.now() - new Date(m.updated_at).getTime();
      return age < 7 * 24 * 3600 * 1000; // within last 7 days
    })
  );

  // ── Relative time helper ───────────────────────────────────────────────
  function relTime(ts) {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return 'just now';
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  // ── Load all data when workspace changes ───────────────────────────────
  $effect(() => {
    void workspace?.id;
    loadDecisions();
    loadRepos();
    loadSpecs();
    loadRules();
  });
</script>

<div class="workspace-home" data-testid="workspace-home">
  {#if !workspace}
    <!-- No workspace selected — prompt user to select one -->
    <div class="no-workspace">
      <div class="no-workspace-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="48" height="48">
          <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
          <polyline points="9 22 9 12 15 12 15 22"/>
        </svg>
      </div>
      <h2 class="no-workspace-title">Select a workspace</h2>
      <p class="no-workspace-desc">Choose a workspace from the selector above to get started.</p>
    </div>
  {:else}
    <div class="sections">

      <!-- ── Decisions ─────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-decisions" data-testid="section-decisions">
        <div class="section-header">
          <h2 class="section-title" id="section-decisions">
            Decisions
            {#if notifications.length > 0}
              <span class="section-badge" aria-label="{notifications.length} decisions">{notifications.length}</span>
            {/if}
          </h2>
          {#if notifications.length > 0}
            <a class="section-action" href="/workspaces/{workspace.slug ?? workspace.id}/decisions">View all</a>
          {/if}
        </div>
        <div class="section-body">
          {#if decisionsLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if decisionsError}
            <p class="error-text" role="alert">{decisionsError}</p>
          {:else if notifications.length === 0}
            <p class="empty-text" data-testid="decisions-empty">No decisions needed — system is running autonomously.</p>
          {:else}
            <ul class="decision-list" role="list">
              {#each notifications.slice(0, 5) as n (n.id)}
                {@const body = getBody(n)}
                {@const state = actionStates[n.id] ?? {}}
                <li class="decision-item" data-testid="decision-item">
                  <span class="decision-icon" aria-hidden="true">{TYPE_ICONS[n.notification_type] ?? '•'}</span>
                  <div class="decision-content">
                    <span class="decision-type">{TYPE_LABELS[n.notification_type] ?? n.notification_type}</span>
                    <span class="decision-desc">{n.message ?? n.description ?? body.description ?? ''}</span>
                    {#if n.repo_id && repoMap[n.repo_id]}
                      <span class="decision-repo">{repoMap[n.repo_id].name}</span>
                    {/if}
                  </div>
                  <div class="decision-actions">
                    {#if state.success}
                      <span class="action-feedback success">{state.message}</span>
                    {:else if state.loading}
                      <span class="action-feedback">…</span>
                    {:else}
                      {#if n.notification_type === 'spec_approval' && body.spec_path && body.spec_sha}
                        <button
                          class="inline-btn approve"
                          onclick={() => handleApproveSpec(n)}
                          data-testid="btn-approve"
                          aria-label="Approve spec"
                        >Approve</button>
                        <button
                          class="inline-btn reject"
                          onclick={() => handleRejectSpec(n)}
                          data-testid="btn-reject"
                          aria-label="Reject spec"
                        >Reject</button>
                      {:else if n.notification_type === 'gate_failure' && body.mr_id}
                        <button
                          class="inline-btn"
                          onclick={() => handleRetry(n)}
                          data-testid="btn-retry"
                          aria-label="Retry gate"
                        >Retry</button>
                      {/if}
                      <button
                        class="inline-btn secondary"
                        onclick={() => handleDismiss(n)}
                        data-testid="btn-dismiss"
                        aria-label="Dismiss"
                      >Dismiss</button>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

      <!-- ── Repos ─────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-repos" data-testid="section-repos">
        <div class="section-header">
          <h2 class="section-title" id="section-repos">Repos</h2>
        </div>
        <div class="section-body">
          {#if reposLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if reposError}
            <p class="error-text" role="alert">{reposError}</p>
          {:else if repos.length === 0}
            <p class="empty-text" data-testid="repos-empty">No repositories yet.</p>
          {:else}
            <ul class="repo-list" role="list">
              {#each repos as repo (repo.id)}
                {@const health = repoHealth(repo)}
                <li class="repo-row" data-testid="repo-row">
                  <button
                    class="repo-btn"
                    onclick={() => onSelectRepo?.(repo)}
                    aria-label="Open repository {repo.name}"
                    data-testid="repo-link"
                  >
                    <span class="repo-name">{repo.name}</span>
                    <span class="repo-meta">
                      {#if (repo.active_spec_count ?? 0) > 0}
                        <span class="repo-stat">{repo.active_spec_count} spec{repo.active_spec_count !== 1 ? 's' : ''} active</span>
                      {/if}
                      {#if (repo.active_agents ?? 0) > 0}
                        <span class="repo-stat">{repo.active_agents} agent{repo.active_agents !== 1 ? 's' : ''}</span>
                      {/if}
                    </span>
                    <span class="repo-health health-{health}" aria-label="Status: {health}" data-testid="repo-health">
                      {#if health === 'healthy'}● healthy
                      {:else if health === 'gate'}⚠ gate
                      {:else}○ idle
                      {/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="repo-actions">
            <button class="section-btn" onclick={() => { showNewRepoModal = true; }} data-testid="btn-new-repo">+ New Repo</button>
            <button class="section-btn" onclick={() => { showNewRepoModal = true; }} data-testid="btn-import-repo">Import</button>
          </div>
        </div>
      </section>

      <!-- ── Briefing ──────────────────────────────────────────────────── -->
      <section class="home-section home-section-briefing" aria-labelledby="section-briefing" data-testid="section-briefing">
        <div class="section-header">
          <h2 class="section-title" id="section-briefing">Briefing</h2>
        </div>
        <div class="section-body section-body-briefing">
          <Briefing workspaceId={workspace.id} scope="workspace" workspaceName={workspace.name} />
        </div>
      </section>

      <!-- ── Specs ─────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-specs" data-testid="section-specs">
        <div class="section-header">
          <h2 class="section-title" id="section-specs">Specs</h2>
          <div class="header-controls">
            <select
              class="filter-select"
              value={specsStatusFilter}
              onchange={(e) => { specsStatusFilter = e.target.value; }}
              aria-label="Filter specs by status"
              data-testid="specs-status-filter"
            >
              <option value="">All statuses</option>
              <option value="draft">Draft</option>
              <option value="pending">Pending</option>
              <option value="approved">Approved</option>
              <option value="implemented">Implemented</option>
            </select>
          </div>
        </div>
        <div class="section-body">
          {#if specsLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if specsError}
            <p class="error-text" role="alert">{specsError}</p>
          {:else if filteredSpecs.length === 0}
            <p class="empty-text" data-testid="specs-empty">
              {specsStatusFilter ? 'No specs with that status.' : 'No specs yet.'}
            </p>
          {:else}
            <table class="specs-table" data-testid="specs-table">
              <thead>
                <tr>
                  <th>Repo</th>
                  <th>Path</th>
                  <th>Status</th>
                  <th>Progress</th>
                  <th>Last activity</th>
                </tr>
              </thead>
              <tbody>
                {#each filteredSpecs as spec (spec.id ?? spec.path)}
                  <tr
                    class="spec-row"
                    onclick={() => navigateToSpec(spec)}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') navigateToSpec(spec); }}
                    data-testid="spec-row"
                    aria-label="Open spec {spec.path}"
                  >
                    <td class="spec-repo">{repoMap[spec.repo_id]?.name ?? spec.repo_id ?? '—'}</td>
                    <td class="spec-path">{spec.path}</td>
                    <td class="spec-status">
                      <span class="status-icon" aria-hidden="true">{SPEC_STATUS_ICONS[spec.status] ?? '•'}</span>
                      {spec.status ?? '—'}
                    </td>
                    <td class="spec-progress">
                      {#if spec.tasks_total != null}
                        {spec.tasks_done ?? 0}/{spec.tasks_total}
                      {:else}
                        —
                      {/if}
                    </td>
                    <td class="spec-activity">{relTime(spec.updated_at)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </section>

      <!-- ── Agent Rules ───────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-agent-rules" data-testid="section-agent-rules">
        <div class="section-header">
          <h2 class="section-title" id="section-agent-rules">Agent Rules</h2>
          <a class="section-action" href="/workspaces/{workspace.slug ?? workspace.id}/agent-rules"
             data-testid="manage-rules-link"
             onclick={(e) => { e.preventDefault(); window.history.pushState({ mode: 'workspace_home', wsId: workspace.id, repoName: null, repoTab: 'specs' }, '', `/workspaces/${encodeURIComponent(workspace.slug ?? workspace.id)}/agent-rules`); }}
          >Manage rules</a>
        </div>
        <div class="section-body">
          {#if rulesLoading}
            <div class="skeleton-row"></div>
          {:else if rulesError}
            <p class="error-text" role="alert">{rulesError}</p>
          {:else}
            <p class="rules-summary" data-testid="rules-summary">
              {allMetaSpecs.length} meta-spec{allMetaSpecs.length !== 1 ? 's' : ''} active
              {#if requiredMetaSpecs.length > 0}
                ({requiredMetaSpecs.length} required)
              {/if}
            </p>

            {#if recentlyUpdated.length > 0}
              <div class="reconcile-status" role="status" data-testid="reconcile-status">
                Reconciling: {recentlyUpdated.length} meta-spec{recentlyUpdated.length !== 1 ? 's' : ''} recently updated
              </div>
            {/if}

            {#if requiredMetaSpecs.length > 0}
              <ul class="rules-list" role="list" data-testid="rules-list">
                {#each requiredMetaSpecs as ms (ms.id)}
                  <li class="rule-item" data-testid="rule-item">
                    <span class="rule-lock" aria-label="Required" aria-hidden="true">🔒</span>
                    <span class="rule-name">{ms.name}</span>
                    {#if ms.kind}
                      <span class="rule-kind">{ms.kind.replace('meta:', '')}</span>
                    {/if}
                    {#if ms.version}
                      <span class="rule-version">v{ms.version}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else if allMetaSpecs.length === 0}
              <p class="empty-text">No meta-specs configured.</p>
            {/if}
          {/if}
        </div>
      </section>

    </div>
  {/if}
</div>

<style>
  .workspace-home {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6) var(--space-8);
    max-width: 860px;
    margin: 0 auto;
    width: 100%;
  }

  /* ── No workspace selected ──────────────────────────────────────────── */
  .no-workspace {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-16) var(--space-8);
    text-align: center;
    color: var(--color-text-muted);
  }

  .no-workspace-icon {
    opacity: 0.3;
  }

  .no-workspace-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .no-workspace-desc {
    font-size: var(--text-sm);
    margin: 0;
  }

  /* ── Sections layout ────────────────────────────────────────────────── */
  .sections {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .home-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-sm);
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
    min-width: 18px;
    height: 18px;
    padding: 0 var(--space-1);
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .section-action {
    font-size: var(--text-xs);
    color: var(--color-primary);
    text-decoration: none;
  }

  .section-action:hover {
    text-decoration: underline;
  }

  .section-body {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* Briefing section — let Briefing.svelte manage its own padding */
  .section-body-briefing {
    padding: 0;
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  /* ── Skeleton ───────────────────────────────────────────────────────── */
  .skeleton-row {
    height: 32px;
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  /* ── Error / empty ──────────────────────────────────────────────────── */
  .error-text {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .empty-text {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  /* ── Decisions ──────────────────────────────────────────────────────── */
  .decision-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .decision-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .decision-item:last-child {
    border-bottom: none;
  }

  .decision-icon {
    flex-shrink: 0;
    font-size: var(--text-sm);
    width: 20px;
    text-align: center;
    padding-top: 2px;
  }

  .decision-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .decision-type {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .decision-desc {
    font-size: var(--text-sm);
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .decision-repo {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .decision-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .action-feedback {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .action-feedback.success {
    color: var(--color-success);
  }

  /* ── Repos ──────────────────────────────────────────────────────────── */
  .repo-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .repo-row {
    display: block;
  }

  .repo-btn {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-2);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    text-align: left;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .repo-btn:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }

  .repo-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .repo-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    font-family: var(--font-mono);
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-meta {
    display: flex;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .repo-stat {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .repo-health {
    font-size: var(--text-xs);
    font-weight: 500;
    flex-shrink: 0;
  }

  .health-healthy { color: var(--color-success); }
  .health-gate { color: var(--color-warning); }
  .health-idle { color: var(--color-text-muted); }

  .repo-actions {
    display: flex;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  /* ── Specs table ────────────────────────────────────────────────────── */
  .specs-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .specs-table th {
    text-align: left;
    padding: var(--space-2) var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .spec-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .spec-row:hover {
    background: var(--color-surface-elevated);
  }

  .spec-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .spec-row td {
    padding: var(--space-2) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .spec-row:last-child td {
    border-bottom: none;
  }

  .spec-repo {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
    max-width: 200px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .spec-status {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    white-space: nowrap;
    color: var(--color-text-secondary);
    text-transform: capitalize;
  }

  .status-icon {
    font-size: var(--text-xs);
  }

  .spec-progress {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .spec-activity {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  /* ── Filters ────────────────────────────────────────────────────────── */
  .filter-select {
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

  .filter-select:hover {
    border-color: var(--color-primary);
  }

  .filter-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Agent Rules ────────────────────────────────────────────────────── */
  .rules-summary {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .reconcile-status {
    font-size: var(--text-xs);
    color: var(--color-warning);
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border-radius: var(--radius);
    border-left: 3px solid var(--color-warning);
  }

  .rules-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .rule-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    padding: var(--space-1) 0;
  }

  .rule-lock {
    flex-shrink: 0;
    font-size: var(--text-sm);
  }

  .rule-name {
    font-weight: 500;
    color: var(--color-text);
    flex: 1;
    min-width: 0;
  }

  .rule-kind {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    text-transform: capitalize;
  }

  .rule-version {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
  }

  /* ── Buttons ────────────────────────────────────────────────────────── */
  .inline-btn {
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .inline-btn:hover {
    border-color: var(--color-border-strong);
    color: var(--color-text);
  }

  .inline-btn.approve {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
    color: var(--color-success);
  }

  .inline-btn.approve:hover {
    background: color-mix(in srgb, var(--color-success) 20%, transparent);
    border-color: var(--color-success);
  }

  .inline-btn.reject {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
    color: var(--color-danger);
  }

  .inline-btn.reject:hover {
    background: color-mix(in srgb, var(--color-danger) 20%, transparent);
    border-color: var(--color-danger);
  }

  .inline-btn.secondary {
    color: var(--color-text-muted);
  }

  .inline-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .section-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .section-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .section-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (max-width: 768px) {
    .workspace-home {
      padding: var(--space-4);
    }

    .spec-meta,
    .spec-activity {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .skeleton-row { animation: none; }
    .inline-btn, .section-btn, .repo-btn, .filter-select { transition: none; }
  }
</style>
