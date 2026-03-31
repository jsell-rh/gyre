<script>
  /**
   * RepoMode — repo view with horizontal tab bar (§3 of ui-navigation.md)
   *
   * Slice 3 adds:
   *   - Repo header: name, active agent count (clickable → panel), budget %, clone URL (copyable)
   *   - Agent slide-in panel: lists active agents for this repo
   *   - Fixed Decisions tab: passes repoId so Inbox filters to this repo only
   *   - Verified tab prop wiring for all tabs
   */
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import ExplorerView from './ExplorerView.svelte';
  import SpecDashboard from './SpecDashboard.svelte';
  import Inbox from './Inbox.svelte';
  import ExplorerCodeTab from './ExplorerCodeTab.svelte';
  import RepoSettings from './RepoSettings.svelte';
  import AgentCardPanel from './AgentCardPanel.svelte';

  const openDetailPanel = getContext('openDetailPanel') ?? null;

  let {
    workspace = null,
    repo = null,
    activeTab = 'specs',
    onTabChange = undefined,
    workspaceBudget = null,
  } = $props();

  const TABS = [
    { id: 'specs',        labelKey: 'repo_mode.tabs.specs' },
    { id: 'tasks',        labelKey: 'repo_mode.tabs.tasks' },
    { id: 'mrs',          labelKey: 'repo_mode.tabs.mrs' },
    { id: 'architecture', labelKey: 'repo_mode.tabs.architecture' },
    { id: 'decisions',    labelKey: 'repo_mode.tabs.decisions' },
    { id: 'code',         labelKey: 'repo_mode.tabs.code' },
    { id: 'settings',     labelKey: 'repo_mode.tabs.settings', titleKey: 'repo_mode.settings_title' },
  ];

  // ── Active agents for this repo ────────────────────────────────────────
  let activeAgents = $state([]);
  let agentsLoading = $state(false);
  let agentPanelOpen = $state(false);
  let agentPanelEl = $state(null);
  let selectedAgentId = $state(null);

  $effect(() => {
    const repoId = repo?.id;
    if (!repoId) { activeAgents = []; return; }
    let aborted = false;
    agentsLoading = true;
    api.agents({ repoId, status: 'active' })
      .then(list => { if (!aborted) activeAgents = Array.isArray(list) ? list : []; })
      .catch(() => { if (!aborted) activeAgents = []; })
      .finally(() => { if (!aborted) agentsLoading = false; });
    return () => { aborted = true; };
  });

  // ── Tasks for this repo ──────────────────────────────────────────────
  let repoTasks = $state([]);
  let tasksLoading = $state(false);
  let tasksLoaded = $state(false);

  $effect(() => {
    if (activeTab !== 'tasks') return;
    const repoId = repo?.id;
    if (!repoId || tasksLoaded) return;
    let aborted = false;
    tasksLoading = true;
    api.tasks({ repoId })
      .then(list => { if (!aborted) { repoTasks = Array.isArray(list) ? list : []; tasksLoading = false; tasksLoaded = true; } })
      .catch(() => { if (!aborted) { repoTasks = []; tasksLoading = false; tasksLoaded = true; } });
    return () => { aborted = true; };
  });

  // Reset when repo changes
  $effect(() => {
    if (repo?.id) { tasksLoaded = false; mrsLoaded = false; }
  });

  // ── MRs for this repo ────────────────────────────────────────────────
  let repoMrs = $state([]);
  let mrsLoading = $state(false);
  let mrsLoaded = $state(false);

  $effect(() => {
    if (activeTab !== 'mrs') return;
    const repoId = repo?.id;
    if (!repoId || mrsLoaded) return;
    let aborted = false;
    mrsLoading = true;
    api.mergeRequests({ repository_id: repoId })
      .then(async (list) => {
        if (aborted) return;
        const mrList = Array.isArray(list) ? list : [];
        // Enrich MRs with gate results summary (best-effort, parallel)
        const gatePromises = mrList.map(mr =>
          api.mrGates(mr.id).then(gates => {
            const arr = Array.isArray(gates) ? gates : (gates?.gates ?? []);
            const passed = arr.filter(g => g.status === 'Passed' || g.status === 'passed').length;
            const failed = arr.filter(g => g.status === 'Failed' || g.status === 'failed').length;
            const details = arr.map(g => ({
              name: g.name ?? g.gate_name ?? 'Gate',
              status: (g.status === 'Passed' || g.status === 'passed') ? 'passed' : (g.status === 'Failed' || g.status === 'failed') ? 'failed' : 'pending',
              gate_type: g.gate_type,
              required: g.required,
            }));
            return { id: mr.id, passed, failed, total: arr.length, details };
          }).catch(() => ({ id: mr.id, passed: 0, failed: 0, total: 0, details: [] }))
        );
        const gateResults = await Promise.all(gatePromises);
        if (aborted) return;
        const gateMap = Object.fromEntries(gateResults.map(g => [g.id, g]));
        repoMrs = mrList.map(mr => ({ ...mr, _gates: gateMap[mr.id] }));
        // Enrich MRs missing diff_stats (best-effort, don't block render)
        const needsDiffStats = repoMrs.filter(mr => !mr.diff_stats).slice(0, 20);
        if (needsDiffStats.length > 0) {
          Promise.all(needsDiffStats.map(mr =>
            api.mrDiff(mr.id).then(diff => ({ id: mr.id, diff_stats: { files_changed: diff?.files_changed ?? 0, insertions: diff?.insertions ?? 0, deletions: diff?.deletions ?? 0 } }))
              .catch(() => null)
          )).then(results => {
            if (aborted) return;
            const statsMap = Object.fromEntries((results.filter(Boolean)).map(r => [r.id, r.diff_stats]));
            repoMrs = repoMrs.map(mr => statsMap[mr.id] ? { ...mr, diff_stats: statsMap[mr.id] } : mr);
          });
        }
        mrsLoading = false;
        mrsLoaded = true;
      })
      .catch(() => { if (!aborted) { repoMrs = []; mrsLoading = false; mrsLoaded = true; } });
    return () => { aborted = true; };
  });

  // ── Human-friendly entity name cache ──────────────────────────────────
  let entityNameCache = $state({});

  function queueNameResolution(type, id) {
    if (!id) return;
    const key = `${type}:${id}`;
    if (entityNameCache[key] !== undefined) return;
    queueMicrotask(() => {
      if (entityNameCache[key] !== undefined) return;
      entityNameCache = { ...entityNameCache, [key]: null };
      const fetcher = type === 'agent' ? api.agent(id).then(a => a?.name) :
                      type === 'task' ? api.task(id).then(t => t?.title) :
                      type === 'mr' ? api.mergeRequest(id).then(m => m?.title) :
                      Promise.resolve(null);
      fetcher.then(name => {
        if (name) entityNameCache = { ...entityNameCache, [key]: name };
      }).catch(() => {});
    });
  }

  function entityName(type, id) {
    if (!id) return '';
    const cached = entityNameCache[`${type}:${id}`];
    if (cached) return cached;
    queueNameResolution(type, id);
    return shortId(id);
  }

  function shortId(id) {
    if (!id) return '';
    return id.length > 12 ? id.slice(0, 8) + '...' : id;
  }

  function fmtRelTime(ts) {
    if (!ts) return '';
    const now = Date.now() / 1000;
    const diff = now - ts;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function taskStatusVariant(s) {
    if (s === 'done') return 'success';
    if (s === 'in_progress') return 'warning';
    if (s === 'blocked') return 'danger';
    return 'muted';
  }

  function mrStatusVariant(s) {
    if (s === 'merged') return 'success';
    if (s === 'open') return 'info';
    if (s === 'closed') return 'danger';
    return 'muted';
  }

  // Move focus to panel when it opens
  $effect(() => {
    if (agentPanelOpen && agentPanelEl) {
      agentPanelEl.focus();
    }
  });

  // ── Clone URL ─────────────────────────────────────────────────────────
  let cloneCopied = $state(false);
  let cloneCopyTimer = null;

  const cloneUrl = $derived(
    repo?.clone_url
    ?? (repo?.name ? `${window.location.origin}/git/${repo.name}.git` : null)
  );

  async function copyCloneUrl() {
    if (!cloneUrl) return;
    try {
      await navigator.clipboard.writeText(cloneUrl);
      cloneCopied = true;
      clearTimeout(cloneCopyTimer);
      cloneCopyTimer = setTimeout(() => { cloneCopied = false; }, 2000);
    } catch { /* clipboard unavailable */ }
  }

  // ── Budget % ──────────────────────────────────────────────────────────
  const budgetPct = $derived.by(() => {
    if (!workspaceBudget) return null;
    const used = workspaceBudget.used_credits ?? 0;
    const total = workspaceBudget.total_credits ?? 0;
    if (!total) return null;
    return Math.round((used / total) * 100);
  });

  // ── Keyboard navigation for tab bar ───────────────────────────────────
  function handleTabKeydown(e) {
    const idx = TABS.findIndex(t => t.id === activeTab);
    if (idx < 0) return;
    let next = -1;
    if (e.key === 'ArrowRight') { next = (idx + 1) % TABS.length; }
    else if (e.key === 'ArrowLeft') { next = (idx - 1 + TABS.length) % TABS.length; }
    else if (e.key === 'Home') { next = 0; }
    else if (e.key === 'End') { next = TABS.length - 1; }
    if (next >= 0) {
      e.preventDefault();
      onTabChange?.(TABS[next].id);
      const btn = e.currentTarget?.querySelector(`#tab-${TABS[next].id}`);
      btn?.focus();
    }
  }
</script>

<div class="repo-mode" data-testid="repo-mode">

  <!-- ── Repo header ─────────────────────────────────────────────────── -->
  <div class="repo-header" data-testid="repo-header">
    <span class="repo-name" data-testid="repo-name">{repo?.name ?? ''}</span>

    <div class="repo-meta">
      <!-- Agent count (clickable → slide-in panel) -->
      <button
        class="agent-count-btn"
        onclick={() => { agentPanelOpen = true; }}
        aria-label={$t('repo_mode.agent_count_click', { values: { label: agentsLoading ? $t('repo_mode.loading_agents') : $t('repo_mode.agents_active', { values: { count: activeAgents.length } }) } })}
        data-testid="agent-count-btn"
      >
        {#if agentsLoading}
          <span class="meta-value">{$t('repo_mode.loading_agents')}</span>
        {:else}
          <span class="meta-value">{$t('repo_mode.agents_active', { values: { count: activeAgents.length } })}</span>
        {/if}
      </button>

      <!-- Budget % -->
      {#if budgetPct !== null}
        <span class="meta-sep" aria-hidden="true">·</span>
        <span class="budget-display" data-testid="budget-display">{$t('repo_mode.budget_label', { values: { pct: budgetPct } })}</span>
      {/if}

      <!-- Clone URL -->
      {#if cloneUrl}
        <span class="meta-sep" aria-hidden="true">·</span>
        <button
          class="clone-btn"
          onclick={copyCloneUrl}
          aria-label={cloneCopied ? $t('repo_mode.clone_url_copied') : $t('repo_mode.copy_clone_url')}
          title={cloneUrl}
          data-testid="clone-url-btn"
        >
          <span class="clone-url-text">{cloneUrl}</span>
          <span class="clone-icon" aria-hidden="true">{cloneCopied ? '✓' : '📋'}</span>
        </button>
      {/if}
    </div>
  </div>

  <!-- ── Tab bar ─────────────────────────────────────────────────────── -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="tab-bar" role="tablist" aria-label={$t('repo_mode.repo_navigation')} data-testid="repo-tab-bar" onkeydown={handleTabKeydown}>
    {#each TABS as tab}
      <button
        class="tab-btn"
        class:active={activeTab === tab.id}
        role="tab"
        id="tab-{tab.id}"
        aria-selected={activeTab === tab.id}
        aria-controls="tabpanel-{tab.id}"
        tabindex={activeTab === tab.id ? 0 : -1}
        onclick={() => onTabChange?.(tab.id)}
        title={tab.titleKey ? $t(tab.titleKey) : $t(tab.labelKey)}
      >
        {$t(tab.labelKey)}
      </button>
    {/each}
  </div>

  <!-- ── Tab content ─────────────────────────────────────────────────── -->
  <div class="tab-content" role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" tabindex="0">
    {#if activeTab === 'specs'}
      <SpecDashboard
        workspaceId={workspace?.id ?? null}
        repoId={repo?.id ?? null}
        scope="repo"
      />
    {:else if activeTab === 'tasks'}
      <div class="list-tab">
        {#if tasksLoading}
          <p class="list-loading">Loading tasks...</p>
        {:else if repoTasks.length === 0}
          <div class="list-empty">
            <p>No tasks for this repository yet.</p>
            <p class="list-empty-hint">Tasks are created when specs are approved and work is decomposed.</p>
          </div>
        {:else}
          <table class="entity-table">
            <thead>
              <tr>
                <th>Status</th>
                <th>Title</th>
                <th>Priority</th>
                <th>Type</th>
                <th>Spec</th>
                <th>Agent</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {#each repoTasks as task}
                <tr class="entity-row" onclick={() => openDetailPanel?.({ type: 'task', id: task.id, data: task })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'task', id: task.id, data: task }); }}>
                  <td title={task.status === 'blocked' ? `Blocked${task.depends_on?.length ? ` by ${task.depends_on.length} task(s)` : ''}` : task.status === 'in_progress' && task.assigned_to ? `In progress — assigned to agent` : task.status === 'done' ? 'Completed' : task.status === 'backlog' ? 'Awaiting assignment' : ''}><Badge value={task.status ?? 'backlog'} variant={taskStatusVariant(task.status)} /></td>
                  <td class="cell-title">{task.title ?? 'Untitled task'}</td>
                  <td>{#if task.priority}<Badge value={task.priority} variant={task.priority === 'high' || task.priority === 'critical' ? 'danger' : task.priority === 'low' ? 'muted' : 'warning'} />{/if}</td>
                  <td class="cell-type">{task.task_type ?? ''}</td>
                  <td class="cell-mono">{#if task.spec_path}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'spec', id: task.spec_path, data: { path: task.spec_path, repo_id: task.repo_id ?? repo?.id } }); }} title={task.spec_path}>{task.spec_path.split('/').pop()}</button>{/if}</td>
                  <td class="cell-mono">{#if task.assigned_to}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'agent', id: task.assigned_to, data: {} }); }} title={task.assigned_to}>{entityName('agent', task.assigned_to)}</button>{/if}</td>
                  <td class="cell-time">{fmtRelTime(task.updated_at ?? task.created_at)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else if activeTab === 'mrs'}
      <div class="list-tab">
        {#if mrsLoading}
          <p class="list-loading">Loading merge requests...</p>
        {:else if repoMrs.length === 0}
          <div class="list-empty">
            <p>No merge requests for this repository yet.</p>
            <p class="list-empty-hint">MRs are created when agents complete their work.</p>
          </div>
        {:else}
          <table class="entity-table">
            <thead>
              <tr>
                <th>Status</th>
                <th>Title</th>
                <th>Branch</th>
                <th>Agent</th>
                <th>Spec</th>
                <th>Gates</th>
                <th>Changes</th>
                <th>Updated</th>
              </tr>
            </thead>
            <tbody>
              {#each repoMrs as mr}
                <tr class="entity-row" onclick={() => openDetailPanel?.({ type: 'mr', id: mr.id, data: mr })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'mr', id: mr.id, data: mr }); }}>
                  <td title={mr.queue_position != null ? `Position ${mr.queue_position + 1} in merge queue — gates will run before merge` : mr.status === 'merged' ? `Merged${mr.merge_commit_sha ? ' at ' + mr.merge_commit_sha.slice(0, 7) : ''}` : mr.status === 'open' ? 'Open — ready to enqueue for merge' : mr.status === 'closed' ? 'Closed without merging' : ''}><Badge value={mr.queue_position != null ? `queued #${mr.queue_position + 1}` : (mr.status ?? 'open')} variant={mr.queue_position != null ? 'warning' : mrStatusVariant(mr.status)} /></td>
                  <td class="cell-title">{mr.title ?? 'Untitled MR'}</td>
                  <td class="cell-mono"><span class="branch-ref">{mr.source_branch ?? ''}</span>{#if mr.target_branch}<span class="branch-arrow">→</span><span class="branch-ref">{mr.target_branch}</span>{/if}</td>
                  <td class="cell-mono">{#if mr.author_agent_id}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'agent', id: mr.author_agent_id, data: {} }); }} title={mr.author_agent_id}>{entityName('agent', mr.author_agent_id)}</button>{:else}{''}{/if}</td>
                  <td class="cell-mono">{#if mr.spec_ref}{@const specPath = mr.spec_ref.split('@')[0]}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'spec', id: specPath, data: { path: specPath, repo_id: mr.repository_id ?? repo?.id } }); }} title={mr.spec_ref}>{specPath.split('/').pop()}</button>{/if}</td>
                  <td>
                    {#if mr._gates?.total > 0}
                      <button class="gate-cell-repo gate-cell-clickable" title="View gate details: {mr._gates.details?.map(g => `${g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'} ${g.name}${g.required === false ? ' (advisory)' : ''}`).join(', ') ?? ''}" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'mr', id: mr.id, data: { ...mr, _openTab: 'gates' } }); }}>
                        <span class="gate-summary-compact">
                          {#if mr._gates.failed > 0}
                            <span class="gate-fail-compact">✗{mr._gates.failed}</span>
                          {/if}
                          {#if mr._gates.passed > 0}
                            <span class="gate-pass-compact">✓{mr._gates.passed}</span>
                          {/if}
                          {#if mr._gates.total - mr._gates.passed - mr._gates.failed > 0}
                            <span class="gate-pending-compact">○{mr._gates.total - mr._gates.passed - mr._gates.failed}</span>
                          {/if}
                        </span>
                        {#if mr._gates.details?.length > 0}
                          <span class="gate-names-repo">
                            {#each mr._gates.details as g}
                              <span class="gate-tag gate-tag-{g.status}">{g.name}{#if g.gate_type} · {g.gate_type.replace(/_/g, ' ')}{/if}</span>
                            {/each}
                          </span>
                        {/if}
                      </button>
                    {/if}
                  </td>
                  <td>
                    {#if mr.diff_stats}
                      <span class="diff-stat-compact">
                        <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      </span>
                    {/if}
                  </td>
                  <td class="cell-time">{fmtRelTime(mr.merged_at ?? mr.updated_at ?? mr.created_at)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else if activeTab === 'architecture'}
      <ExplorerView
        scope={{ type: 'repo', workspaceId: workspace?.id, repoId: repo?.id }}
        workspaceName={workspace?.name ?? null}
      />
    {:else if activeTab === 'decisions'}
      <!-- repoId scopes Inbox to this repo's notifications only (§3 Decisions tab) -->
      <Inbox workspaceId={workspace?.id} repoId={repo?.id} scope="repo" />
    {:else if activeTab === 'code'}
      {#if repo?.id}
        <ExplorerCodeTab repoId={repo.id} {repo} />
      {:else}
        <div class="tab-placeholder">
          <p>{$t('repo_mode.no_repo_selected')}</p>
        </div>
      {/if}
    {:else if activeTab === 'settings'}
      <RepoSettings {workspace} {repo} />
    {/if}
  </div>
</div>

<!-- ── Agent slide-in panel ──────────────────────────────────────────── -->
{#if agentPanelOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="panel-overlay"
    role="presentation"
    onclick={() => { agentPanelOpen = false; }}
    data-testid="agent-panel-overlay"
  >
    <div
      class="agent-panel"
      role="dialog"
      aria-modal="true"
      aria-label={$t('repo_mode.active_agents')}
      tabindex="-1"
      bind:this={agentPanelEl}
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => { if (e.key === 'Escape') agentPanelOpen = false; }}
      data-testid="agent-panel"
    >
      <div class="agent-panel-header">
        <h2 class="agent-panel-title">{$t('repo_mode.active_agents')}</h2>
        <button
          class="panel-close-btn"
          onclick={() => { agentPanelOpen = false; }}
          aria-label={$t('common.close')}
          data-testid="agent-panel-close"
        >✕</button>
      </div>

      <div class="agent-panel-body">
        {#if agentsLoading}
          <p class="agent-panel-loading">{$t('repo_mode.loading_agents_panel')}</p>
        {:else if activeAgents.length === 0}
          <p class="agent-panel-empty">{$t('repo_mode.no_active_agents')}</p>
        {:else}
          {#each activeAgents as agent}
            <button
              class="agent-row"
              class:agent-row-selected={selectedAgentId === agent.id}
              data-testid="agent-row"
              onclick={() => { selectedAgentId = selectedAgentId === agent.id ? null : agent.id; }}
              aria-expanded={selectedAgentId === agent.id}
              aria-label={$t('repo_mode.agent_label', { values: { name: agent.name ?? agent.id } })}
            >
              <div class="agent-row-info">
                <span class="agent-row-name">{agent.name ?? agent.id}</span>
                <span class="agent-row-status agent-status-{agent.status ?? 'active'}">{agent.status ?? 'active'}</span>
              </div>
              {#if agent.task_id ?? agent.current_task_id}
                {@const tId = agent.task_id ?? agent.current_task_id}
                <span class="agent-row-task" title={tId}>{$t('repo_mode.task_label', { values: { id: entityName('task', tId) } })}</span>
              {/if}
              {#if agent.branch}
                <span class="agent-row-branch">{agent.branch}</span>
              {/if}
            </button>
            {#if selectedAgentId === agent.id}
              <AgentCardPanel agentId={agent.id} />
            {/if}
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .repo-mode {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  /* ── Repo header ────────────────────────────────────────────────────── */
  .repo-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .repo-name {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .repo-meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .meta-sep {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  /* Agent count button */
  .agent-count-btn {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-body);
    color: var(--color-link);
    font-size: var(--text-sm);
    transition: color var(--transition-fast);
  }

  .agent-count-btn:hover {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .agent-count-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .meta-value {
    font-size: var(--text-sm);
    color: inherit;
  }

  /* Budget display */
  .budget-display {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  /* Clone URL button */
  .clone-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    transition: color var(--transition-fast);
    max-width: 280px;
    overflow: hidden;
  }

  .clone-btn:hover {
    color: var(--color-text-secondary);
  }

  .clone-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .clone-url-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 240px;
  }

  .clone-icon {
    flex-shrink: 0;
    font-style: normal;
  }

  /* ── Tab bar ────────────────────────────────────────────────────────── */
  .tab-bar {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .tab-btn {
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .tab-btn:hover {
    color: var(--color-text);
  }

  .tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Tab content ────────────────────────────────────────────────────── */
  .tab-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .tab-content:focus { outline: none; }
  .tab-content:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .tab-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .tab-placeholder p {
    margin: 0;
  }

  /* ── Agent slide-in panel ────────────────────────────────────────────── */
  .panel-overlay {
    position: fixed;
    inset: 0;
    z-index: 300;
    background: color-mix(in srgb, var(--color-bg) 40%, transparent);
    display: flex;
    justify-content: flex-end;
  }

  .agent-panel {
    width: 360px;
    max-width: 90vw;
    height: 100%;
    background: var(--color-surface-elevated);
    border-left: 1px solid var(--color-border-strong);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .agent-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-5);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .agent-panel-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .panel-close-btn {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .panel-close-btn:hover {
    color: var(--color-text);
    background: var(--color-border);
  }

  .panel-close-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .agent-panel-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .agent-panel-loading,
  .agent-panel-empty {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-align: center;
    margin: var(--space-6) 0;
    font-style: italic;
  }

  .agent-row {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 100%;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    color: var(--color-text);
    transition: border-color var(--transition-fast);
  }

  .agent-row:hover {
    border-color: var(--color-border-strong);
  }

  .agent-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .agent-row-selected {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 5%, var(--color-surface));
  }

  .agent-row-info {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .agent-row-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-row-status {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
    border: 1px solid color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .agent-row-status.agent-status-running {
    background: color-mix(in srgb, var(--color-info) 15%, transparent);
    color: var(--color-info);
    border-color: color-mix(in srgb, var(--color-info) 30%, transparent);
  }

  .agent-row-task {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .agent-row-branch {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  /* ── Entity list tabs (Tasks, MRs) ──────────────────────────────────── */
  .list-tab {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4) var(--space-6);
  }

  .list-loading {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
    padding: var(--space-8) 0;
    text-align: center;
  }

  .list-empty {
    text-align: center;
    padding: var(--space-8) var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .list-empty-hint {
    font-size: var(--text-xs);
    margin-top: var(--space-2);
    opacity: 0.7;
  }

  .entity-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .entity-table thead th {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .entity-table tbody .entity-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .entity-table tbody .entity-row:hover {
    background: var(--color-surface-elevated);
  }

  .entity-table tbody .entity-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .entity-table td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .cell-title {
    font-weight: 500;
    color: var(--color-text);
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cell-mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .cell-type {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .cell-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .diff-stat-compact {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    display: inline-flex;
    gap: var(--space-1);
  }

  .diff-ins { color: var(--color-success); font-weight: 600; }
  .diff-del { color: var(--color-danger); font-weight: 600; }

  /* Gate summary in MR table */
  .gate-summary-compact {
    display: inline-flex;
    gap: var(--space-1);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .gate-pass-compact { color: var(--color-success); font-weight: 600; }
  .gate-fail-compact { color: var(--color-danger); font-weight: 600; }
  .gate-pending-compact { color: var(--color-text-muted); }

  .gate-cell-repo { display: flex; flex-direction: column; gap: 2px; }
  .gate-cell-clickable { background: none; border: 1px solid transparent; padding: var(--space-1); border-radius: var(--radius-sm); cursor: pointer; text-align: left; font: inherit; color: inherit; transition: border-color var(--transition-fast), background var(--transition-fast); }
  .gate-cell-clickable:hover { border-color: var(--color-border); background: var(--color-surface-hover, rgba(0,0,0,0.03)); }
  .gate-cell-clickable:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 1px; }
  .gate-names-repo { display: flex; flex-wrap: wrap; gap: 2px; }
  .gate-tag {
    font-size: 10px;
    padding: 0 3px;
    border-radius: var(--radius);
    white-space: nowrap;
    line-height: 1.4;
  }
  .gate-tag-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-tag-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-tag-pending { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  /* ── Entity link buttons in tables ──────────────────────────────────── */
  .entity-link-btn {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link, var(--color-primary));
    text-decoration: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 130px;
    display: inline-block;
    vertical-align: middle;
    text-align: left;
  }

  .entity-link-btn:hover {
    text-decoration: underline;
    color: var(--color-primary);
  }

  .entity-link-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
    border-radius: var(--radius-sm);
  }

  .branch-ref {
    max-width: 100px;
    display: inline-block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    vertical-align: middle;
  }

  .branch-arrow {
    color: var(--color-text-muted);
    margin: 0 2px;
    font-size: var(--text-xs);
  }

  /* ── Responsive ─────────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .repo-header {
      padding: var(--space-2) var(--space-3);
      gap: var(--space-2);
    }

    .repo-name {
      font-size: var(--text-base);
    }

    .clone-url-text {
      max-width: 140px;
    }

    .tab-bar {
      padding: 0 var(--space-2);
    }

    .tab-btn {
      padding: var(--space-3) var(--space-3);
      font-size: var(--text-xs);
    }

    .agent-panel {
      width: 100vw;
      max-width: 100vw;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tab-btn,
    .agent-count-btn,
    .clone-btn,
    .panel-close-btn {
      transition: none;
    }
  }
</style>
