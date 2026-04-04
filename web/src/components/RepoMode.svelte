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
  import { entityName, shortId } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import { taskStatusTooltip, mrStatusTooltip, agentStatusTooltip } from '../lib/statusTooltips.js';
  import Badge from '../lib/Badge.svelte';
  import EntityLink from '../lib/EntityLink.svelte';
  import ExplorerView from './ExplorerView.svelte';
  import SpecDashboard from './SpecDashboard.svelte';
  import Inbox from './Inbox.svelte';
  import ExplorerCodeTab from './ExplorerCodeTab.svelte';
  import RepoSettings from './RepoSettings.svelte';
  import AgentCardPanel from './AgentCardPanel.svelte';

  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

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
    { id: 'agents',       labelKey: 'repo_mode.tabs.agents' },
    { id: 'architecture', labelKey: 'repo_mode.tabs.architecture' },
    { id: 'decisions',    labelKey: 'repo_mode.tabs.decisions' },
    { id: 'code',         labelKey: 'repo_mode.tabs.code' },
    { id: 'settings',     labelKey: 'repo_mode.tabs.settings', titleKey: 'repo_mode.settings_title' },
  ];

  // ── Notification count for decisions tab badge ─────────────────────────
  // Must match what Inbox actually shows: repo-scoped, non-dismissed notifications.
  // The server count endpoint doesn't support repo_id filtering, so we fetch
  // all notifications and filter client-side to match the Inbox view.
  let decisionsCount = $state(0);
  $effect(() => {
    const rId = repo?.id;
    if (rId) {
      api.myNotifications().then(raw => {
        const arr = Array.isArray(raw) ? raw : (raw?.notifications ?? []);
        decisionsCount = arr.filter(n => n.repo_id === rId && !n.dismissed_at).length;
      }).catch(() => {});
    }
  });

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

  // ── All agents for this repo (for Agents tab) ────────────────────────
  let allAgents = $state([]);
  let allAgentsLoading = $state(false);
  let allAgentsLoaded = $state(false);

  $effect(() => {
    if (activeTab !== 'agents') return;
    const repoId = repo?.id;
    if (!repoId || allAgentsLoaded) return;
    let aborted = false;
    allAgentsLoading = true;
    api.agents({ repoId })
      .then(async (list) => {
        if (aborted) return;
        let agentList = Array.isArray(list) ? list : [];
        // Enrich agents missing spec_path from their task (best-effort)
        const needsSpec = agentList.filter(a => !a.spec_path && (a.task_id ?? a.current_task_id));
        if (needsSpec.length > 0) {
          const results = await Promise.all(needsSpec.map(a => {
            const tid = a.task_id ?? a.current_task_id;
            return api.task(tid).then(t => ({ agentId: a.id, spec_path: t?.spec_path })).catch(() => null);
          }));
          const specMap = Object.fromEntries(results.filter(r => r?.spec_path).map(r => [r.agentId, r.spec_path]));
          if (Object.keys(specMap).length > 0) {
            agentList = agentList.map(a => specMap[a.id] ? { ...a, spec_path: specMap[a.id] } : a);
          }
        }
        allAgents = agentList;
        allAgentsLoading = false;
        allAgentsLoaded = true;
      })
      .catch(() => { if (!aborted) { allAgents = []; allAgentsLoading = false; allAgentsLoaded = true; } });
    return () => { aborted = true; };
  });

  // Reset when repo changes
  $effect(() => { if (repo?.id) allAgentsLoaded = false; });

  // ── Tasks for this repo ──────────────────────────────────────────────
  let repoTasks = $state([]);
  let tasksLoading = $state(false);
  let tasksLoaded = $state(false);
  let taskSortBy = $state('priority');   // 'priority' | 'status' | 'updated'
  let taskStatusFilter = $state('all');  // 'all' | specific status

  const PRIORITY_ORDER = { critical: 0, high: 1, medium: 2, low: 3 };
  const STATUS_ORDER = { blocked: 0, in_progress: 1, review: 2, backlog: 3, done: 4 };

  const sortedFilteredTasks = $derived.by(() => {
    let tasks = repoTasks;
    if (taskStatusFilter !== 'all') {
      tasks = tasks.filter(t => t.status === taskStatusFilter);
    }
    return [...tasks].sort((a, b) => {
      if (taskSortBy === 'priority') {
        return (PRIORITY_ORDER[a.priority] ?? 9) - (PRIORITY_ORDER[b.priority] ?? 9);
      }
      if (taskSortBy === 'status') {
        return (STATUS_ORDER[a.status] ?? 9) - (STATUS_ORDER[b.status] ?? 9);
      }
      // updated
      const aT = a.updated_at ?? a.created_at ?? 0;
      const bT = b.updated_at ?? b.created_at ?? 0;
      return bT - aT; // newest first
    });
  });

  function specShortName(path) {
    if (!path) return '';
    const seg = path.split('/').pop();
    return seg.endsWith('.md') ? seg.slice(0, -3) : seg;
  }

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
    if (repo?.id) { tasksLoaded = false; mrsLoaded = false; allAgentsLoaded = false; }
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
        // The API already enriches gate_name, gate_type, required, command from definitions
        const gatePromises = mrList.map(mr =>
          api.mrGates(mr.id).then(gates => {
            const arr = Array.isArray(gates) ? gates : (gates?.gates ?? []);
            const passed = arr.filter(g => g.status === 'Passed' || g.status === 'passed').length;
            const failed = arr.filter(g => g.status === 'Failed' || g.status === 'failed').length;
            const details = arr.map((g, idx) => {
              const gateType = g.gate_type ?? '';
              const gateTypeLabel = gateType ? gateType.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) : '';
              const gateCommand = g.command ?? '';
              return {
                name: g.gate_name ?? g.name ?? (gateTypeLabel
                  || (gateCommand ? gateCommand.split(' ')[0].split('/').pop() : '')
                  || `Check #${idx + 1}`),
                status: (g.status === 'Passed' || g.status === 'passed') ? 'passed' : (g.status === 'Failed' || g.status === 'failed') ? 'failed' : 'pending',
                gate_type: g.gate_type,
                required: g.required,
                command: g.command,
                output: g.output,
                error: g.error,
                duration_ms: g.duration_ms ?? ((g.started_at && g.finished_at) ? Math.round((g.finished_at - g.started_at) * 1000) : null),
              };
            });
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
        // Enrich open MRs with speculative merge conflict check (best-effort)
        const openMrs = repoMrs.filter(mr => mr.status === 'open');
        if (openMrs.length > 0 && repoId) {
          api.repoSpeculative(repoId).then(specs => {
            if (aborted) return;
            const specArr = Array.isArray(specs) ? specs : [];
            const conflictMap = {};
            for (const spec of specArr) {
              if (spec.has_conflicts || spec.conflict) {
                conflictMap[spec.branch ?? spec.source_branch] = true;
              }
            }
            if (Object.keys(conflictMap).length > 0) {
              repoMrs = repoMrs.map(mr =>
                mr.source_branch && conflictMap[mr.source_branch]
                  ? { ...mr, _has_conflicts: true }
                  : mr
              );
            }
          }).catch(() => {});
        }
      })
      .catch(() => { if (!aborted) { repoMrs = []; mrsLoading = false; mrsLoaded = true; } });
    return () => { aborted = true; };
  });

  // Entity name resolution + time formatting imported from shared modules

  /** Human-friendly duration from seconds, e.g. "2m 30s", "1h 15m" */
  function humanDuration(seconds) {
    if (seconds == null || seconds < 0) return '';
    const s = Math.round(seconds);
    if (s < 60) return `${s}s`;
    if (s < 3600) {
      const m = Math.floor(s / 60);
      const rem = s % 60;
      return rem > 0 ? `${m}m ${rem}s` : `${m}m`;
    }
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
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

  // ── Task quick status change ─────────────────────────────────────────
  let changingTaskStatus = $state(null);

  async function quickChangeTaskStatus(task, newStatus, e) {
    e?.stopPropagation();
    if (changingTaskStatus) return;
    changingTaskStatus = task.id;
    try {
      await api.updateTaskStatus(task.id, newStatus);
      toastSuccess(`Task "${task.title ?? 'Untitled'}" → ${newStatus}`);
      repoTasks = repoTasks.map(t => t.id === task.id ? { ...t, status: newStatus } : t);
    } catch (err) {
      toastError('Failed to update: ' + (err.message ?? err));
    } finally {
      changingTaskStatus = null;
    }
  }

  const TASK_STATUS_TRANSITIONS = {
    backlog: ['in_progress'],
    in_progress: ['done', 'blocked'],
    blocked: ['in_progress'],
    review: ['done', 'in_progress'],
    done: ['in_progress'],
  };

  // ── Task creation ────────────────────────────────────────────────────
  let createTaskOpen = $state(false);
  let createTaskForm = $state({ title: '', description: '', priority: 'medium', task_type: 'implementation', spec_path: '' });
  let createTaskSaving = $state(false);

  async function handleCreateTask() {
    const title = createTaskForm.title.trim();
    if (!title || !repo?.id) return;
    createTaskSaving = true;
    try {
      const data = {
        title,
        description: createTaskForm.description.trim() || undefined,
        priority: createTaskForm.priority || undefined,
        task_type: createTaskForm.task_type || undefined,
        spec_path: createTaskForm.spec_path || undefined,
        workspace_id: workspace?.id,
        repo_id: repo.id,
      };
      const task = await api.createTask(data);
      toastSuccess(`Task "${title}" created`);
      repoTasks = [...repoTasks, task];
      createTaskOpen = false;
      createTaskForm = { title: '', description: '', priority: 'medium', task_type: 'implementation', spec_path: '' };
    } catch (err) {
      toastError('Failed to create task: ' + (err.message ?? err));
    } finally {
      createTaskSaving = false;
    }
  }

  // ── Merge queue ──────────────────────────────────────────────────────
  let mergeQueue = $state([]);
  let mergeQueueLoading = $state(false);
  let mergeQueueLoaded = $state(false);

  $effect(() => {
    if (activeTab !== 'mrs') return;
    const repoId = repo?.id;
    if (!repoId || mergeQueueLoaded) return;
    let aborted = false;
    mergeQueueLoading = true;
    api.mergeQueue()
      .then(list => {
        if (aborted) return;
        const all = Array.isArray(list) ? list : [];
        mergeQueue = all.filter(e => e.repository_id === repoId || e.repo_id === repoId);
        mergeQueueLoading = false;
        mergeQueueLoaded = true;
      })
      .catch(() => { if (!aborted) { mergeQueue = []; mergeQueueLoading = false; mergeQueueLoaded = true; } });
    return () => { aborted = true; };
  });

  // Reset merge queue when repo changes
  $effect(() => { if (repo?.id) mergeQueueLoaded = false; });

  // ── MR quick actions ──────────────────────────────────────────────────
  let enqueueingMr = $state(null);
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  async function quickEnqueue(mr, e) {
    e?.stopPropagation();
    if (enqueueingMr) return;
    enqueueingMr = mr.id;
    try {
      await api.enqueue(mr.id);
      toastSuccess(`MR "${mr.title ?? 'Untitled'}" enqueued for merge`);
      // Refresh MR list
      const updated = await api.mergeRequest(mr.id).catch(() => null);
      if (updated) {
        repoMrs = repoMrs.map(m => m.id === mr.id ? { ...m, ...updated } : m);
      }
    } catch (err) {
      toastError('Failed to enqueue: ' + (err.message ?? err));
    } finally {
      enqueueingMr = null;
    }
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

  // Use repo.clone_url from the API which includes the correct server origin.
  // Fallback constructs URL using workspace slug. When running via Vite dev
  // proxy (port 5173), rewrite to the API server port (3000) since git
  // smart HTTP isn't proxied by Vite.
  function deriveCloneUrl() {
    if (repo?.clone_url) return repo.clone_url;
    if (!repo?.name) return null;
    const wsSlug = workspace?.slug ?? '';
    let origin = window.location.origin;
    // Vite dev server runs on 5173 but the git server is on 3000
    if (origin.includes(':5173')) {
      origin = origin.replace(':5173', ':3000');
    }
    return `${origin}/git/${wsSlug}/${repo.name}.git`;
  }
  const cloneUrl = $derived(deriveCloneUrl());

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

      <!-- Quick stats -->
      {#if !mrsLoading && repoMrs.length > 0}
        {@const openMrs = repoMrs.filter(m => m.status === 'open').length}
        {@const failedGates = repoMrs.filter(m => m._gates?.failed > 0).length}
        {#if openMrs > 0 || failedGates > 0}
          <span class="meta-sep" aria-hidden="true">·</span>
          {#if failedGates > 0}
            <button class="meta-stat meta-stat-danger" onclick={() => onTabChange?.('mrs')} title="{failedGates} MR{failedGates !== 1 ? 's' : ''} with failed gates">{failedGates} failed</button>
          {:else if openMrs > 0}
            <button class="meta-stat" onclick={() => onTabChange?.('mrs')} title="{openMrs} open merge request{openMrs !== 1 ? 's' : ''}">{openMrs} open MR{openMrs !== 1 ? 's' : ''}</button>
          {/if}
        {/if}
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
          <span class="clone-icon" aria-hidden="true">{cloneCopied ? '✓' : 'copy'}</span>
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
        {$t(tab.labelKey)}{#if tab.id === 'decisions' && decisionsCount > 0}<span class="tab-badge">{decisionsCount > 99 ? '99+' : decisionsCount}</span>{:else if tab.id === 'agents' && activeAgents.length > 0}<span class="tab-badge tab-badge-info">{activeAgents.length}</span>{:else if tab.id === 'tasks' && repoTasks.length > 0}<span class="tab-count">{repoTasks.length}</span>{:else if tab.id === 'mrs' && repoMrs.length > 0}<span class="tab-count">{repoMrs.length}</span>{/if}
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
        <div class="list-tab-header">
          <div class="list-tab-controls">
            <select class="form-select form-select-sm" bind:value={taskStatusFilter} aria-label="Filter by status">
              <option value="all">All statuses</option>
              <option value="backlog">Backlog</option>
              <option value="in_progress">In Progress</option>
              <option value="blocked">Blocked</option>
              <option value="review">Review</option>
              <option value="done">Done</option>
            </select>
            <select class="form-select form-select-sm" bind:value={taskSortBy} aria-label="Sort by">
              <option value="priority">Sort: Priority</option>
              <option value="status">Sort: Status</option>
              <option value="updated">Sort: Updated</option>
            </select>
          </div>
          <button class="create-entity-btn" onclick={() => { createTaskOpen = !createTaskOpen; }}>
            {createTaskOpen ? 'Cancel' : '+ New Task'}
          </button>
        </div>
        {#if createTaskOpen}
          <form class="create-entity-form" onsubmit={(e) => { e.preventDefault(); handleCreateTask(); }}>
            <input class="form-input" type="text" placeholder="Task title" bind:value={createTaskForm.title} required />
            <textarea class="form-textarea" placeholder="Description (optional)" bind:value={createTaskForm.description} rows="2"></textarea>
            <div class="form-row">
              <select class="form-select" bind:value={createTaskForm.priority}>
                <option value="low">Low priority</option>
                <option value="medium">Medium priority</option>
                <option value="high">High priority</option>
                <option value="critical">Critical</option>
              </select>
              <select class="form-select" bind:value={createTaskForm.task_type}>
                <option value="implementation">Implementation</option>
                <option value="investigation">Investigation</option>
                <option value="review">Review</option>
                <option value="fix">Fix</option>
              </select>
              <button class="form-submit-btn" type="submit" disabled={createTaskSaving || !createTaskForm.title.trim()}>
                {createTaskSaving ? 'Creating...' : 'Create Task'}
              </button>
            </div>
          </form>
        {/if}
        {#if tasksLoading}
          <p class="list-loading">Loading tasks...</p>
        {:else if sortedFilteredTasks.length === 0 && !createTaskOpen}
          <div class="list-empty">
            {#if repoTasks.length > 0}
              <p>No tasks match filter "{taskStatusFilter}"</p>
            {:else}
              <p>No tasks yet</p>
              <p class="list-empty-hint">Tasks are auto-created when you approve a spec. The flow:</p>
              <div class="pipeline-guide-mini">
                <span class="guide-step">Push spec</span>
                <span class="guide-arrow">→</span>
                <span class="guide-step guide-step-active">Approve</span>
                <span class="guide-arrow">→</span>
                <span class="guide-step guide-step-highlight">Tasks created</span>
                <span class="guide-arrow">→</span>
                <span class="guide-step">Agent implements</span>
                <span class="guide-arrow">→</span>
                <span class="guide-step">MR + Merge</span>
              </div>
            {/if}
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
                <th class="th-action"></th>
              </tr>
            </thead>
            <tbody>
              {#each sortedFilteredTasks as task}
                <tr class="entity-row" onclick={() => goToEntityDetail?.('task', task.id, task)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') goToEntityDetail?.('task', task.id, task); }}>
                  <td title={taskStatusTooltip(task)}>
                    <Badge value={task.status ?? 'backlog'} variant={taskStatusVariant(task.status)} />
                    {#if task.status === 'in_progress' && task.assigned_to}
                      <span class="status-why">{entityName('agent', task.assigned_to)}</span>
                    {:else if task.status === 'review'}
                      <span class="status-why">awaiting review</span>
                    {:else if task.status === 'blocked'}
                      <span class="status-why status-why-danger">blocked</span>
                    {:else if task.status === 'done'}
                      <span class="status-why status-why-ok">complete</span>
                    {/if}
                  </td>
                  <td class="cell-title">{task.title ?? 'Untitled task'}</td>
                  <td>{#if task.priority}<Badge value={task.priority} variant={task.priority === 'high' || task.priority === 'critical' ? 'danger' : task.priority === 'low' ? 'muted' : 'warning'} />{/if}</td>
                  <td class="cell-type">{task.task_type ?? ''}</td>
                  <td class="cell-mono">{#if task.spec_path}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('spec', task.spec_path, { path: task.spec_path, repo_id: task.repo_id ?? repo?.id }); }} title={task.spec_path}>{specShortName(task.spec_path)}</button>{/if}</td>
                  <td class="cell-mono">{#if task.assigned_to}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('agent', task.assigned_to, {}); }} title={task.assigned_to}>{entityName('agent', task.assigned_to)}</button>{/if}</td>
                  <td class="cell-time">{relativeTime(task.updated_at ?? task.created_at)}</td>
                  <td class="cell-action">
                    {#if TASK_STATUS_TRANSITIONS[task.status]?.length}
                      {#each TASK_STATUS_TRANSITIONS[task.status] as nextStatus}
                        <button
                          class="quick-action-btn quick-action-{nextStatus}"
                          onclick={(e) => quickChangeTaskStatus(task, nextStatus, e)}
                          disabled={changingTaskStatus === task.id}
                          title="Move to {nextStatus.replace(/_/g, ' ')}"
                        >
                          {changingTaskStatus === task.id ? '...' : nextStatus === 'in_progress' ? 'Start' : nextStatus === 'done' ? 'Done' : nextStatus === 'blocked' ? 'Block' : nextStatus.replace(/_/g, ' ')}
                        </button>
                      {/each}
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else if activeTab === 'mrs'}
      <div class="list-tab">
        <!-- Merge Queue section -->
        {#if mergeQueue.length > 0}
          <div class="merge-queue-section">
            <h3 class="section-title">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true"><path d="M16 3h5v5M4 20L21 3M21 16v5h-5M15 15l6 6M4 4l5 5"/></svg>
              Merge Queue ({mergeQueue.length})
            </h3>
            <div class="queue-entries">
              {#each mergeQueue as entry, i}
                {@const mrTitle = repoMrs.find(m => m.id === (entry.merge_request_id ?? entry.mr_id))?.title}
                <button class="queue-entry" onclick={() => goToEntityDetail?.('mr', entry.merge_request_id ?? entry.mr_id, {})} tabindex="0">
                  <span class="queue-position">#{i + 1}</span>
                  <span class="queue-mr-title">{mrTitle ?? entityName('mr', entry.merge_request_id ?? entry.mr_id)}</span>
                  <span class="queue-priority">{entry.priority != null ? `priority ${entry.priority}` : ''}</span>
                  <Badge value={entry.status ?? 'queued'} variant={entry.status === 'processing' ? 'warning' : 'info'} />
                </button>
              {/each}
            </div>
          </div>
        {/if}
        {#if mrsLoading}
          <p class="list-loading">Loading merge requests...</p>
        {:else if repoMrs.length === 0}
          <div class="list-empty">
            <p>No merge requests yet</p>
            <p class="list-empty-hint">MRs are created when agents complete tasks. The flow:</p>
            <div class="pipeline-guide-mini">
              <span class="guide-step">Agent completes</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step guide-step-highlight">MR created</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step">Gates run</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step">Signed attestation</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step">Merged</span>
            </div>
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
                <th class="th-action"></th>
              </tr>
            </thead>
            <tbody>
              {#each repoMrs as mr}
                <tr class="entity-row" onclick={() => goToEntityDetail?.('mr', mr.id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') goToEntityDetail?.('mr', mr.id, mr); }}>
                  <td title={mrStatusTooltip(mr)}>
                    <Badge value={mr.queue_position != null ? `queued #${mr.queue_position + 1}` : (mr.status ?? 'open')} variant={mr.queue_position != null ? 'warning' : mrStatusVariant(mr.status)} />
                    {#if mr.status === 'merged' && mr.merge_commit_sha}
                      <code class="sha-inline mono" title="Click to copy {mr.merge_commit_sha}" onclick={(e) => { e.stopPropagation(); navigator.clipboard.writeText(mr.merge_commit_sha); toastSuccess('SHA copied'); }} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); navigator.clipboard.writeText(mr.merge_commit_sha); } }}>{mr.merge_commit_sha.slice(0, 7)}</code>
                    {:else if mr.status === 'open' && mr._gates?.failed > 0}
                      {@const failedGateNames = (mr._gates.details ?? []).filter(g => g.status === 'failed').map(g => g.name).slice(0, 2)}
                      <span class="status-why status-why-danger">{failedGateNames.length > 0 ? failedGateNames.join(', ') : `${mr._gates.failed} gate${mr._gates.failed !== 1 ? 's' : ''}`} failed</span>
                    {:else if mr.status === 'open' && mr._gates?.passed === mr._gates?.total && mr._gates?.total > 0}
                      <span class="status-why status-why-ok">gates passed</span>
                    {/if}
                  </td>
                  <td class="cell-title">{mr.title ?? 'Untitled MR'}</td>
                  <td class="cell-mono"><span class="branch-ref">{mr.source_branch ?? ''}</span>{#if mr.target_branch}<span class="branch-arrow">→</span><span class="branch-ref">{mr.target_branch}</span>{/if}{#if mr._has_conflicts}<span class="conflict-badge" title="Speculative merge detected conflicts with main branch">conflicts</span>{/if}</td>
                  <td class="cell-mono">{#if mr.author_agent_id}<EntityLink type="agent" id={mr.author_agent_id} />{:else}{''}{/if}</td>
                  <td class="cell-mono">{#if mr.spec_ref}{@const specPath = mr.spec_ref.split('@')[0]}<EntityLink type="spec" id={specPath} data={{ path: specPath, repo_id: mr.repository_id ?? repo?.id }} />{/if}</td>
                  <td>
                    {#if mr._gates?.total > 0}
                      {@const sortedGates = [...(mr._gates.details ?? [])].sort((a, b) => {
                        const order = { failed: 0, pending: 1, running: 1, passed: 2 };
                        return (order[a.status] ?? 1) - (order[b.status] ?? 1);
                      })}
                      {@const visibleGates = sortedGates.slice(0, 3)}
                      {@const overflowCount = sortedGates.length - 3}
                      <span class="gate-names-repo">
                        {#each visibleGates as g}
                          <button class="gate-badge gate-badge-{g.status}" title="{g.name}{g.required === false ? ' (advisory)' : ''}{g.gate_type ? '\nType: ' + g.gate_type.replace(/_/g, ' ') : ''}{g.command ? '\n$ ' + g.command : ''}{g.duration_ms ? '\nDuration: ' + (g.duration_ms < 1000 ? g.duration_ms + 'ms' : (g.duration_ms / 1000).toFixed(1) + 's') : ''}{g.output ? '\nOutput: ' + g.output.slice(0, 200) : ''}\nClick to view full gate details" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', mr.id, { ...mr, _openTab: 'gates' }); }}>
                            <span class="gate-badge-icon">{g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'}</span>
                            <span class="gate-badge-name">{g.name ?? (g.gate_type ? g.gate_type.replace(/_/g, ' ') : 'check')}</span>
                            {#if g.required === false}<span class="gate-advisory-inline">(adv)</span>{/if}
                            {#if g.duration_ms}<span class="gate-duration-inline">{g.duration_ms < 1000 ? g.duration_ms + 'ms' : (g.duration_ms / 1000).toFixed(1) + 's'}</span>{/if}
                          </button>
                        {/each}
                        {#if overflowCount > 0}
                          <button class="gate-badge gate-badge-more" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', mr.id, { ...mr, _openTab: 'gates' }); }} title="{overflowCount} more gate{overflowCount > 1 ? 's' : ''}">+{overflowCount}</button>
                        {/if}
                      </span>
                      {#if mr._gates.failed > 0}
                        {@const failedGate = mr._gates.details?.find(g => g.status === 'failed')}
                        {#if failedGate}
                          <button class="gate-error-preview" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', mr.id, { ...mr, _openTab: 'gates' }); }} title="Click to see full gate output">
                            {#if failedGate.command}<code class="gate-error-cmd">$ {failedGate.command.split(' ')[0].split('/').pop()}</code>{/if}
                            {failedGate.output ? failedGate.output.split('\n')[0]?.slice(0, 80) : failedGate.error?.split('\n')[0]?.slice(0, 60) ?? 'failed'}
                          </button>
                        {/if}
                      {/if}
                    {/if}
                  </td>
                  <td>
                    {#if mr.diff_stats}
                      <button class="diff-stat-compact diff-stat-link" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', mr.id, { ...mr, _openTab: 'diff' }); }} title="View code diff: {mr.diff_stats.files_changed ?? 0} files, +{mr.diff_stats.insertions ?? 0} -{mr.diff_stats.deletions ?? 0}">
                        {#if mr.diff_stats.files_changed}<span class="diff-files">{mr.diff_stats.files_changed}f</span>{/if}
                        <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      </button>
                    {/if}
                  </td>
                  <td class="cell-time">{relativeTime(mr.merged_at ?? mr.updated_at ?? mr.created_at)}</td>
                  <td class="cell-action">
                    {#if mr.status === 'open' && mr.queue_position == null}
                      <button class="quick-action-btn" onclick={(e) => quickEnqueue(mr, e)} disabled={enqueueingMr === mr.id} title="Add to merge queue">
                        {enqueueingMr === mr.id ? '...' : 'Enqueue'}
                      </button>
                    {:else if mr.status === 'open' && mr.queue_position != null}
                      <span class="queue-badge" title="In merge queue at position {mr.queue_position + 1}">#{mr.queue_position + 1}</span>
                    {/if}
                    {#if mr.diff_stats}
                      <button class="quick-action-btn quick-action-view" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', mr.id, { ...mr, _openTab: 'diff' }); }} title="View code diff">
                        Diff
                      </button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else if activeTab === 'agents'}
      <div class="list-tab">
        {#if allAgentsLoading}
          <p class="list-loading">Loading agents...</p>
        {:else if allAgents.length === 0}
          <div class="list-empty">
            <p>No agents yet</p>
            <p class="list-empty-hint">Agents are spawned to implement tasks. Each works in an isolated branch, iterating until gates pass. The flow:</p>
            <div class="pipeline-guide-mini">
              <span class="guide-step">Task assigned</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step guide-step-highlight">Agent spawned</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step">Code → Test → Fix</span>
              <span class="guide-arrow">→</span>
              <span class="guide-step">MR created</span>
            </div>
          </div>
        {:else}
          <table class="entity-table">
            <thead>
              <tr>
                <th>Status</th>
                <th>Name</th>
                <th>Spec</th>
                <th>Task</th>
                <th>MR</th>
                <th>Duration</th>
                <th>Cost</th>
                <th>Spawned</th>
              </tr>
            </thead>
            <tbody>
              {#each allAgents as agent}
                {@const taskId = agent.task_id ?? agent.current_task_id}
                {@const completedDur = (agent.completed_at && agent.created_at) ? Math.round(agent.completed_at - agent.created_at) : null}
                {@const elapsedSec = agent.created_at ? Math.round(Date.now() / 1000 - agent.created_at) : null}
                {@const totalTokens = (agent.usage?.input_tokens ?? 0) + (agent.usage?.output_tokens ?? 0) + (agent.tokens_used ?? 0)}
                <tr class="entity-row" onclick={() => goToEntityDetail?.('agent', agent.id, agent)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') goToEntityDetail?.('agent', agent.id, agent); }}>
                  <td title={agentStatusTooltip(agent)}>
                    <Badge value={agent.status ?? 'active'} variant={agent.status === 'active' ? 'success' : (agent.status === 'idle' || agent.status === 'completed') ? 'info' : (agent.status === 'failed' || agent.status === 'dead') ? 'danger' : 'muted'} />
                    <span class="agent-status-explain">{#if agent.status === 'active' && elapsedSec != null}running {humanDuration(elapsedSec)}{:else if (agent.status === 'completed' || agent.status === 'idle') && completedDur != null}done in {humanDuration(completedDur)}{:else if agent.status === 'failed' && completedDur != null}failed after {humanDuration(completedDur)}{:else if agent.status === 'failed'}click for logs{/if}</span>
                  </td>
                  <td class="cell-title"><button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('agent', agent.id, agent); }}>{agent.name ?? entityName('agent', agent.id)}</button></td>
                  <td class="cell-mono">{#if agent.spec_path}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('spec', agent.spec_path, { path: agent.spec_path, repo_id: repo?.id }); }} title={agent.spec_path}>{specShortName(agent.spec_path)}</button>{/if}</td>
                  <td class="cell-mono">{#if taskId}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('task', taskId, {}); }} title={taskId}>{entityName('task', taskId)}</button>{/if}</td>
                  <td class="cell-mono">{#if agent.mr_id}<button class="entity-link-btn" onclick={(e) => { e.stopPropagation(); goToEntityDetail?.('mr', agent.mr_id, {}); }}>{entityName('mr', agent.mr_id)}</button>{/if}</td>
                  <td class="cell-time">{#if completedDur != null}{humanDuration(completedDur)}{:else if agent.status === 'active' && elapsedSec != null}{humanDuration(elapsedSec)}{/if}</td>
                  <td class="cell-mono">{#if totalTokens > 0}<span class="token-count" title="{totalTokens.toLocaleString()} tokens">{totalTokens > 999999 ? (totalTokens / 1000000).toFixed(1) + 'M' : totalTokens > 999 ? (totalTokens / 1000).toFixed(0) + 'k' : totalTokens}</span>{/if}</td>
                  <td class="cell-time">{relativeTime(agent.created_at)}</td>
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
            {@const tId = agent.task_id ?? agent.current_task_id}
            {@const spawnedAt = agent.created_at ?? agent.spawned_at}
            <div class="agent-row-wrap">
              <button
                class="agent-row"
                class:agent-row-selected={selectedAgentId === agent.id}
                data-testid="agent-row"
                onclick={() => { selectedAgentId = selectedAgentId === agent.id ? null : agent.id; }}
                aria-expanded={selectedAgentId === agent.id}
                aria-label={$t('repo_mode.agent_label', { values: { name: agent.name ?? agent.id } })}
              >
                <div class="agent-row-info">
                  <span class="agent-row-name">{agent.name ?? entityName('agent', agent.id)}</span>
                  <span class="agent-row-status agent-status-{agent.status ?? 'active'}">{agent.status ?? 'active'}</span>
                </div>
                {#if tId}
                  <span class="agent-row-task" title={tId}>{$t('repo_mode.task_label', { values: { id: entityName('task', tId) } })}</span>
                {/if}
                {#if agent.branch}
                  <span class="agent-row-branch">{agent.branch}</span>
                {/if}
                {#if spawnedAt}
                  <span class="agent-row-duration">{relativeTime(spawnedAt)}</span>
                {/if}
              </button>
              <button
                class="agent-row-detail-btn"
                onclick={() => { agentPanelOpen = false; goToEntityDetail?.('agent', agent.id, agent); }}
                title="Open full agent detail"
              >→</button>
            </div>
            {#if selectedAgentId === agent.id}
              <div class="agent-row-expanded">
                <AgentCardPanel agentId={agent.id} />
                <div class="agent-row-actions">
                  {#if tId}
                    <button class="agent-action-link" onclick={() => { agentPanelOpen = false; goToEntityDetail?.('task', tId, {}); }}>View Task</button>
                  {/if}
                  {#if agent.mr_id}
                    <button class="agent-action-link" onclick={() => { agentPanelOpen = false; goToEntityDetail?.('mr', agent.mr_id, {}); }}>View MR</button>
                  {/if}
                  <button class="agent-action-link agent-action-primary" onclick={() => { agentPanelOpen = false; goToEntityDetail?.('agent', agent.id, agent); }}>Full Detail →</button>
                </div>
              </div>
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

  .meta-stat {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    transition: color var(--transition-fast);
  }

  .meta-stat:hover { color: var(--color-primary); }

  .meta-stat-danger {
    color: var(--color-danger);
    font-weight: 600;
  }

  .meta-stat-danger:hover { color: var(--color-danger); opacity: 0.8; }

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

  .tab-badge {
    font-size: 10px;
    background: var(--color-danger);
    color: #fff;
    border-radius: 8px;
    padding: 0 5px;
    margin-left: 4px;
    min-width: 16px;
    text-align: center;
    line-height: 16px;
    display: inline-block;
    vertical-align: middle;
  }

  .tab-badge-info {
    background: var(--color-info, #1e90ff);
  }

  .tab-count {
    font-size: 10px;
    color: var(--color-text-muted);
    margin-left: 3px;
    font-family: var(--font-mono);
    vertical-align: middle;
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

  .agent-row-duration {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .agent-row-wrap {
    display: flex;
    gap: 0;
    align-items: stretch;
  }

  .agent-row-wrap .agent-row {
    flex: 1;
    border-radius: var(--radius) 0 0 var(--radius);
  }

  .agent-row-detail-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-left: none;
    border-radius: 0 var(--radius) var(--radius) 0;
    padding: 0 var(--space-3);
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .agent-row-detail-btn:hover {
    background: var(--color-surface-elevated);
    color: var(--color-primary);
  }

  .agent-row-expanded {
    border: 1px solid var(--color-border);
    border-top: none;
    border-radius: 0 0 var(--radius) var(--radius);
    margin-top: -1px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
  }

  .agent-row-actions {
    display: flex;
    gap: var(--space-2);
    padding-top: var(--space-2);
    margin-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .agent-action-link {
    font-size: var(--text-xs);
    color: var(--color-link);
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .agent-action-link:hover {
    background: var(--color-surface-elevated);
    text-decoration: underline;
  }

  .agent-action-primary {
    font-weight: 600;
    color: var(--color-primary);
    margin-left: auto;
  }

  /* ── Entity list tabs (Tasks, MRs) ──────────────────────────────────── */
  .list-tab {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4) var(--space-6);
  }

  /* ── Merge Queue section ─────────────────────────────────────────────── */
  .merge-queue-section {
    margin-bottom: var(--space-5);
    background: color-mix(in srgb, var(--color-warning) 5%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, var(--color-border));
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3) 0;
  }

  .queue-entries {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .queue-entry {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: border-color var(--transition-fast);
  }

  .queue-entry:hover {
    border-color: var(--color-primary);
  }

  .queue-position {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 700;
    color: var(--color-warning);
    min-width: 28px;
  }

  .queue-mr-title {
    flex: 1;
    font-size: var(--text-sm);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .queue-priority {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
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

  .pipeline-guide-mini {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    margin-top: var(--space-2);
    flex-wrap: wrap;
  }

  .guide-step {
    font-size: 11px;
    font-weight: 500;
    color: var(--color-text-muted);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .guide-step-active {
    color: var(--color-primary);
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }

  .guide-step-highlight {
    color: var(--color-success);
    border-color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    font-weight: 600;
  }

  .guide-arrow {
    font-size: 10px;
    color: var(--color-text-muted);
    opacity: 0.5;
  }

  .list-tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-3);
    gap: var(--space-2);
  }

  .list-tab-controls {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .form-select-sm {
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    flex: none;
    min-width: 0;
  }

  .agent-status-explain {
    display: block;
    font-size: 10px;
    color: var(--color-text-muted);
    margin-top: 2px;
    white-space: nowrap;
  }

  .status-why {
    display: block;
    font-size: 10px;
    color: var(--color-text-muted);
    margin-top: 2px;
    white-space: nowrap;
  }

  .status-why-danger { color: var(--color-danger); }
  .status-why-ok { color: var(--color-success); }

  .token-count {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .create-entity-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-link);
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    cursor: pointer;
    font-family: var(--font-body);
  }

  .create-entity-btn:hover { border-color: var(--color-link); }

  .create-entity-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    margin-bottom: var(--space-4);
  }

  .form-input, .form-textarea, .form-select {
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
  }

  .form-textarea { resize: vertical; min-height: 40px; }

  .form-row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  .form-select { flex: 1; }

  .form-submit-btn {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    color: var(--color-surface);
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    cursor: pointer;
    font-family: var(--font-body);
    white-space: nowrap;
  }

  .form-submit-btn:disabled { opacity: 0.5; cursor: not-allowed; }

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

  .diff-files { color: var(--color-text-secondary); font-weight: 500; }
  .diff-ins { color: var(--color-success); font-weight: 600; }
  .diff-del { color: var(--color-danger); font-weight: 600; }

  .diff-stat-link {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .diff-stat-link:hover {
    background: var(--color-surface-elevated);
    text-decoration: underline;
  }

  .diff-stat-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  /* Gate badges in MR table */

  .th-action { width: 80px; }
  .cell-action { text-align: right; }
  .quick-action-btn {
    background: var(--color-primary);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
    transition: opacity var(--transition-fast);
    white-space: nowrap;
  }
  .quick-action-btn:hover { opacity: 0.85; }
  .quick-action-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .quick-action-view { background: var(--color-surface-elevated); color: var(--color-text-secondary); border: 1px solid var(--color-border); }
  .quick-action-view:hover { background: var(--color-surface); border-color: var(--color-primary); color: var(--color-primary); }
  .quick-action-done { background: var(--color-success); }
  .quick-action-blocked { background: var(--color-danger); }
  .quick-action-in_progress { background: var(--color-warning); color: var(--color-text); }
  .queue-badge {
    display: inline-block;
    background: var(--color-warning);
    color: var(--color-text);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
  }
  .gate-names-repo { display: flex; flex-wrap: wrap; gap: 3px; align-items: center; }
  .gate-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    line-height: 1.5;
    border: 1px solid transparent;
    background: none;
    cursor: pointer;
    font-family: inherit;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .gate-badge:hover { border-color: var(--color-border); }
  .gate-badge:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 1px; }
  .gate-badge-icon { font-weight: 700; }
  .gate-badge-name { font-weight: 500; }
  .gate-badge-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-badge-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-badge-pending, .gate-badge-running { color: var(--color-text-muted); background: var(--color-surface-elevated); }
  .gate-badge-more {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    font-size: 10px;
    font-weight: 600;
  }

  .gate-advisory-inline {
    font-size: 0.7em;
    opacity: 0.7;
    font-style: italic;
  }

  .gate-duration-inline {
    font-size: 0.8em;
    opacity: 0.6;
    font-family: var(--font-mono);
  }

  .gate-error-preview {
    display: block;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 6%, transparent);
    border: none;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    margin-top: 2px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 250px;
    cursor: pointer;
    text-align: left;
  }

  .gate-error-preview:hover {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    text-decoration: underline;
  }

  .gate-error-cmd {
    font-weight: 600;
    margin-right: 3px;
  }

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

  .sha-inline {
    display: inline-block;
    margin-left: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
    vertical-align: middle;
  }

  .branch-arrow {
    color: var(--color-text-muted);
    margin: 0 2px;
    font-size: var(--text-xs);
  }

  .conflict-badge {
    display: inline-block;
    font-size: 9px;
    font-weight: 600;
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    margin-left: 4px;
    vertical-align: middle;
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
