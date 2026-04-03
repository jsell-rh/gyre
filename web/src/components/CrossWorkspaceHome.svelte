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
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName, shortId, formatId } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import Icon from '../lib/Icon.svelte';
  import Modal from '../lib/Modal.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings') ?? null;
  const goToAgentRules = getContext('goToAgentRules') ?? null;

  /** Navigate to full-page entity detail view (falls back to side panel) */
  function nav(type, id, data) {
    if (goToEntityDetail) {
      goToEntityDetail(type, id, data ?? {});
    } else if (openDetailPanel) {
      openDetailPanel({ type, id, data: data ?? {} });
    }
  }

  let {
    onSelectWorkspace = undefined,
    onSettings = undefined,
    onManageAgentRules = undefined,
  } = $props();

  // ── Create Workspace form state ──────────────────────────────────────
  let createWsOpen = $state(false);
  let createWsForm = $state({ name: '', description: '' });
  let createWsSaving = $state(false);

  async function handleCreateWorkspace() {
    const name = createWsForm.name.trim();
    if (!name) return;
    createWsSaving = true;
    try {
      const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
      const newWs = await api.createWorkspace({ ...createWsForm, name, tenant_id: 'default', slug });
      toastSuccess($t('cross_workspace.ws_created', { values: { name } }));
      createWsOpen = false;
      createWsForm = { name: '', description: '' };
      await loadWorkspaces();
      if (newWs && onSelectWorkspace) onSelectWorkspace(newWs);
    } catch (e) {
      toastError($t('cross_workspace.ws_create_failed', { values: { error: e.message || e } }));
    } finally {
      createWsSaving = false;
    }
  }

  // ── Notification type icons (HSI §8) ────────────────────────────────────
  const TYPE_ICONS = {
    agent_clarification: '?',
    spec_approval: '!',
    gate_failure: '!',
    cross_workspace_change: '~',
    conflicting_interpretations: '!',
    meta_spec_drift: '~',
    budget_warning: '$',
    trust_suggestion: '*',
    spec_assertion_failure: '✗',
    suggested_link: '~',
  };

  const SPEC_STATUS_ICONS = {
    draft: '~',
    pending: '?',
    approved: '✓',
    implemented: '✓',
    merged: '✓',
  };

  function kindLabel(kind) {
    const key = `cross_workspace.kind_labels.${kind}`;
    const val = $t(key);
    return val !== key ? val : kind;
  }

  // ── Workspace name lookup map ────────────────────────────────────────────
  let workspaceNameMap = $state({});

  // ── Decisions state ─────────────────────────────────────────────────────
  let decisionsLoading = $state(true);
  let decisionsError = $state(null);
  let notifications = $state([]);
  let actionStates = $state({});
  let showAllDecisions = $state(false);

  function getBody(n) {
    try { return JSON.parse(n.body || '{}'); } catch { return {}; }
  }

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
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('decisions.approved') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('decisions.approval_failed') } };
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
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('decisions.rejected') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('decisions.rejection_failed') } };
    }
  }

  async function handleRetry(n) {
    const body = getBody(n);
    if (!body.mr_id) return;
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.enqueue(body.mr_id);
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('decisions.re_queued') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('decisions.retry_failed') } };
    }
  }

  async function handleDismiss(n) {
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.markNotificationRead(n.id);
      notifications = notifications.filter(item => item.id !== n.id);
      actionStates = { ...actionStates, [n.id]: { loading: false } };
    } catch {
      toastError($t('decisions.dismiss_failed'));
      actionStates = { ...actionStates, [n.id]: { loading: false } };
    }
  }

  function typeLabel(type) {
    const key = `cross_workspace.type_labels.${type}`;
    const val = $t(key);
    return val !== key ? val : type;
  }

  // ── Workspaces state ────────────────────────────────────────────────────
  let workspacesLoading = $state(true);
  let workspacesError = $state(null);
  let workspaces = $state([]);

  // ── Specs state ─────────────────────────────────────────────────────────
  let specsLoading = $state(true);
  let specsError = $state(null);
  let specs = $state([]);
  let specsSortCol = $state('path');
  let specsSortDir = $state('asc');
  let specsShowAll = $state(false);

  function toggleSpecsSort(col) {
    if (specsSortCol === col) {
      specsSortDir = specsSortDir === 'asc' ? 'desc' : 'asc';
    } else {
      specsSortCol = col;
      specsSortDir = 'asc';
    }
  }

  function specsSortArrow(col) {
    if (specsSortCol !== col) return '↕';
    return specsSortDir === 'asc' ? '↑' : '↓';
  }

  let sortedSpecs = $derived.by(() => {
    return [...specs].sort((a, b) => {
      if (specsSortCol === 'progress') {
        const av = a.tasks_total ? (a.tasks_done ?? 0) / a.tasks_total : -1;
        const bv = b.tasks_total ? (b.tasks_done ?? 0) / b.tasks_total : -1;
        return specsSortDir === 'asc' ? av - bv : bv - av;
      }
      const av = String(a[specsSortCol] ?? '');
      const bv = String(b[specsSortCol] ?? '');
      const cmp = av.localeCompare(bv);
      return specsSortDir === 'asc' ? cmp : -cmp;
    });
  });

  function relTime(ts) {
    return relativeTime(ts);
  }

  // Entity name resolution uses shared singleton cache
  function resolveEntityName(type, id) {
    return entityName(type, id);
  }

  // ── Briefing state ───────────────────────────────────────────────────────
  // Cross-workspace briefing: aggregate per-workspace briefings (§10)
  let briefingLoading = $state(true);
  let briefingError = $state(null);
  let briefingSummaries = $state([]); // [{ workspaceName, summary }]

  // ── Aggregate stat cards state ────────────────────────────────────────────
  let statsLoading = $state(true);
  let totalRepos = $state(0);
  let totalSpecs = $state(0);
  let pendingSpecs = $state(0);
  let activeAgents = $state(0);
  let openMrs = $state(0);
  let mergedMrs = $state(0);

  // ── Recent Activity state ───────────────────────────────────────────────
  let activityLoading = $state(true);
  let activityEvents = $state([]);
  let showAllActivity = $state(false);

  // ── Per-workspace enrichment data ───────────────────────────────────────
  let allRepos = $state([]);
  let allAgents = $state([]);
  let allMrs = $state([]);

  // ── Budget summary state ─────────────────────────────────────────────────
  let budgetSummary = $state(null);
  let budgetSummaryLoading = $state(true);

  // ── System health state ──────────────────────────────────────────────────
  let systemHealth = $state(null);
  let systemHealthLoading = $state(true);

  // ── Agent Rules state ────────────────────────────────────────────────────
  let rulesLoading = $state(true);
  let rulesError = $state(null);
  let globalMetaSpecs = $state([]);

  // ── Load all sections ────────────────────────────────────────────────────
  $effect(() => {
    loadWorkspaces().then(() => {
      loadDecisions();
      loadBriefings();
      loadActivity();
    });
    loadSpecs();
    loadAgentRules();
    loadBudgetSummary();
    loadStats();
    loadSystemHealth();
  });

  async function loadDecisions() {
    decisionsLoading = true;
    decisionsError = null;
    try {
      let data = await api.myNotifications();
      data = Array.isArray(data) ? data : (data?.items ?? []);
      // Exclude dismissed and resolved items
      data = data.filter(n => !n.dismissed_at && !n.resolved_at);
      data.sort((a, b) => (a.priority ?? 999) - (b.priority ?? 999));
      notifications = data;
    } catch (e) {
      decisionsError = e?.message ?? $t('cross_workspace.error_load_decisions');
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
      workspaceNameMap = Object.fromEntries(workspaces.map(w => [w.id, w.name ?? w.id]));
    } catch (e) {
      workspacesError = e?.message ?? $t('cross_workspace.error_load_workspaces');
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
      specsError = e?.message ?? $t('cross_workspace.error_load_specs');
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
          let rawSummary = data?.summary ?? data?.content ?? '';
          // Handle Rust SystemTime serialized as { tv_sec, tv_nsec }
          let summary;
          if (typeof rawSummary === 'object' && rawSummary !== null && rawSummary.tv_sec != null) {
            summary = new Date(rawSummary.tv_sec * 1000).toLocaleString();
          } else {
            summary = typeof rawSummary === 'string' ? rawSummary : String(rawSummary || '');
          }
          return { workspaceName: ws.name, summary };
        })
      );
      briefingSummaries = results
        .filter((r) => r.status === 'fulfilled' && r.value.summary)
        .map((r) => r.value);
    } catch (e) {
      briefingError = e?.message ?? $t('cross_workspace.error_load_briefing');
    } finally {
      briefingLoading = false;
    }
  }

  async function loadBudgetSummary() {
    budgetSummaryLoading = true;
    try {
      budgetSummary = await api.budgetSummary();
    } catch {
      budgetSummary = null;
    } finally {
      budgetSummaryLoading = false;
    }
  }

  async function loadSystemHealth() {
    systemHealthLoading = true;
    try {
      const [health, jobs, version] = await Promise.all([
        api.adminHealth().catch(() => null),
        api.adminJobs().catch(() => []),
        api.version().catch(() => null),
      ]);
      const jobList = Array.isArray(jobs) ? jobs : (jobs?.jobs ?? []);
      const failedJobs = jobList.filter(j => j.status === 'failed' || j.status === 'error').length;
      const runningJobs = jobList.filter(j => j.status === 'running' || j.status === 'active').length;
      systemHealth = {
        uptime_secs: health?.uptime_secs,
        server_version: version?.version ?? health?.version,
        milestone: version?.milestone,
        total_jobs: jobList.length,
        failed_jobs: failedJobs,
        running_jobs: runningJobs,
        status: failedJobs > 0 ? 'degraded' : 'healthy',
      };
    } catch {
      systemHealth = null;
    } finally {
      systemHealthLoading = false;
    }
  }

  async function loadAgentRules() {
    rulesLoading = true;
    rulesError = null;
    try {
      const data = await api.getMetaSpecs({ scope: 'Global' });
      globalMetaSpecs = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      rulesError = e?.message ?? $t('cross_workspace.error_load_rules');
    } finally {
      rulesLoading = false;
    }
  }

  async function loadStats() {
    statsLoading = true;
    try {
      const [reposData, specsData, agentsData, mrsData] = await Promise.allSettled([
        api.allRepos(),
        api.getSpecs(),
        api.agents({ status: 'active' }),
        api.mergeRequests({ status: 'open' }),
      ]);
      const reposList = reposData.status === 'fulfilled' ? (Array.isArray(reposData.value) ? reposData.value : []) : [];
      const specsList = specsData.status === 'fulfilled' ? (Array.isArray(specsData.value) ? specsData.value : (specsData.value?.items ?? [])) : [];
      const agentsList = agentsData.status === 'fulfilled' ? (Array.isArray(agentsData.value) ? agentsData.value : []) : [];
      const mrsList = mrsData.status === 'fulfilled' ? (Array.isArray(mrsData.value) ? mrsData.value : []) : [];
      allRepos = reposList;
      allAgents = agentsList;
      allMrs = mrsList;
      totalRepos = reposList.length;
      totalSpecs = specsList.length;
      pendingSpecs = specsList.filter(s => (s.approval_status ?? s.status) === 'pending').length;
      activeAgents = agentsList.length;
      openMrs = mrsList.filter(m => m.status === 'open').length;
      mergedMrs = mrsList.filter(m => m.status === 'merged').length;
    } catch {
      // leave at 0
    } finally {
      statsLoading = false;
    }
  }

  async function loadActivity() {
    activityLoading = true;
    try {
      const data = await api.activity(20);
      activityEvents = Array.isArray(data) ? data : [];
    } catch {
      activityEvents = [];
    } finally {
      activityLoading = false;
    }
  }

  function activityIcon(event) {
    const t = (event.event_type ?? event.event ?? event.type ?? '').toLowerCase();
    if (t.includes('spec') && t.includes('approv')) return '✓';
    if (t.includes('spec') && t.includes('reject')) return '✗';
    if (t.includes('spec')) return 'S';
    if (t.includes('task')) return 'T';
    if (t.includes('agent') && t.includes('spawn')) return '>';
    if (t.includes('agent') && t.includes('complet')) return 'A';
    if (t.includes('agent') && t.includes('fail')) return '!';
    if (t.includes('mr') && t.includes('merg')) return 'M';
    if (t.includes('mr') && t.includes('creat')) return '+';
    if (t.includes('gate')) return 'G';
    if (t.includes('push')) return '^';
    if (t.includes('graph')) return '#';
    if (t.includes('budget')) return '$';
    return '*';
  }

  const ACTIVITY_LABELS = {
    'MrCreated': 'MR created',
    'MrMerged': 'MR merged',
    'MrClosed': 'MR closed',
    'TaskCreated': 'Task created',
    'TaskCompleted': 'Task completed',
    'GatePass': 'Gate passed',
    'GateFail': 'Gate failed',
    'SpecApproved': 'Spec approved',
    'SpecRejected': 'Spec rejected',
    'GraphDelta': 'Architecture updated',
    'GitPush': 'Code pushed',
    'agent_spawned': 'Agent spawned',
    'agent_completed': 'Agent completed',
    'agent_failed': 'Agent failed',
    'spec_approved': 'Spec approved',
    'spec_rejected': 'Spec rejected',
    'spec_created': 'Spec created',
    'spec_updated': 'Spec updated',
    'task_created': 'Task created',
    'task_completed': 'Task completed',
    'task_assigned': 'Task assigned',
  };

  function activityLabel(event) {
    const t = event.event_type ?? event.event ?? event.type ?? '';
    return ACTIVITY_LABELS[t] ?? t.replace(/_/g, ' ').replace(/\./g, ' ');
  }

  function activityVariant(event) {
    const t = event.event_type ?? event.event ?? event.type ?? '';
    if (t.includes('fail') || t.includes('reject')) return 'danger';
    if (t.includes('merg') || t.includes('approv') || t.includes('complet') || t.includes('pass')) return 'success';
    if (t.includes('spawn') || t.includes('enqueue') || t.includes('running')) return 'warning';
    return 'info';
  }

  function activityWorkspaceName(event) {
    if (event.workspace_id && workspaceNameMap[event.workspace_id]) {
      return workspaceNameMap[event.workspace_id];
    }
    return null;
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

  // Sort workspaces by urgency: gate failures first, then pending decisions, then active agents, then alphabetical
  let sortedWorkspaces = $derived.by(() => {
    return [...workspaces].sort((a, b) => {
      const aGates = notifications.filter(n => n.workspace_id === a.id && n.notification_type === 'gate_failure').length;
      const bGates = notifications.filter(n => n.workspace_id === b.id && n.notification_type === 'gate_failure').length;
      if (aGates !== bGates) return bGates - aGates;
      const aDecisions = notifications.filter(n => n.workspace_id === a.id).length;
      const bDecisions = notifications.filter(n => n.workspace_id === b.id).length;
      if (aDecisions !== bDecisions) return bDecisions - aDecisions;
      const aAgents = allAgents.filter(ag => ag.workspace_id === a.id).length;
      const bAgents = allAgents.filter(ag => ag.workspace_id === b.id).length;
      if (aAgents !== bAgents) return bAgents - aAgents;
      return (a.name ?? '').localeCompare(b.name ?? '');
    });
  });

  // Compute workspace health status for color-coded borders
  function wsHealthStatus(wsId) {
    const gateFailures = notifications.filter(n => n.workspace_id === wsId && n.notification_type === 'gate_failure').length;
    if (gateFailures > 0) return 'critical';
    const pending = notifications.filter(n => n.workspace_id === wsId).length;
    if (pending > 0) return 'warning';
    const active = allAgents.filter(a => a.workspace_id === wsId).length;
    if (active > 0) return 'active';
    return 'idle';
  }

  // Count workspaces needing attention
  let workspacesNeedingAttention = $derived(
    workspaces.filter(ws => {
      const wsNotifs = notifications.filter(n => n.workspace_id === ws.id);
      return wsNotifs.length > 0;
    }).length
  );
</script>

<div class="cross-workspace-home" data-testid="cross-workspace-home">
  <div class="cwh-header">
    <div class="cwh-header-text">
      <h1 class="cwh-title">{$t('cross_workspace.title')}</h1>
      <p class="cwh-subtitle">{$t('cross_workspace.subtitle')}</p>
    </div>
    {#if onSettings}
      <button
        class="tenant-gear-btn"
        onclick={() => onSettings?.()}
        aria-label={$t('cross_workspace.tenant_admin')}
        title={$t('cross_workspace.tenant_admin_title')}
        data-testid="tenant-gear-btn"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
          <circle cx="12" cy="12" r="3"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
        </svg>
      </button>
    {/if}
  </div>

  <!-- ── Aggregate Stat Cards ───────────────────────────────────────────── -->
  <div class="cwh-stat-cards" data-testid="stat-cards">
    <div class="cwh-stat-card">
      <span class="cwh-stat-value">{workspacesLoading ? '...' : workspaces.length}</span>
      <span class="cwh-stat-label">Workspaces</span>
      {#if !workspacesLoading && workspacesNeedingAttention > 0}
        <span class="cwh-stat-sub cwh-stat-warn">{workspacesNeedingAttention} need attention</span>
      {/if}
    </div>
    <div class="cwh-stat-card">
      <span class="cwh-stat-value">{statsLoading ? '...' : totalRepos}</span>
      <span class="cwh-stat-label">Repos</span>
    </div>
    <div class="cwh-stat-card">
      <span class="cwh-stat-value">{statsLoading ? '...' : totalSpecs}</span>
      <span class="cwh-stat-label">Specs</span>
      {#if !statsLoading && pendingSpecs > 0}
        <span class="cwh-stat-sub cwh-stat-warn">{pendingSpecs} awaiting approval</span>
      {/if}
    </div>
    <div class="cwh-stat-card">
      <span class="cwh-stat-value">{statsLoading ? '...' : activeAgents}</span>
      <span class="cwh-stat-label">Active Agents</span>
    </div>
    <div class="cwh-stat-card">
      <span class="cwh-stat-value">{statsLoading ? '...' : openMrs}</span>
      <span class="cwh-stat-label">Open MRs</span>
      {#if !statsLoading && mergedMrs > 0}
        <span class="cwh-stat-sub">{mergedMrs} merged</span>
      {/if}
    </div>
    {#if !statsLoading && notifications.length > 0}
      <div class="cwh-stat-card cwh-stat-card-alert">
        <span class="cwh-stat-value">{notifications.length}</span>
        <span class="cwh-stat-label">Pending Decisions</span>
      </div>
    {/if}
    {#if !budgetSummaryLoading && budgetSummary?.total_cost_today != null}
      <div class="cwh-stat-card">
        <span class="cwh-stat-value">${budgetSummary.total_cost_today.toFixed(2)}</span>
        <span class="cwh-stat-label">Cost Today</span>
        {#if budgetSummary.total_tokens_today != null}
          <span class="cwh-stat-sub">{budgetSummary.total_tokens_today.toLocaleString()} tokens</span>
        {/if}
      </div>
    {/if}
    {#if !systemHealthLoading && systemHealth}
      <button class="cwh-stat-card cwh-stat-card-health" class:cwh-stat-card-degraded={systemHealth.status === 'degraded'} onclick={() => onSettings?.()} title="View system details in admin settings">
        <span class="cwh-stat-value cwh-health-dot" class:health-ok={systemHealth.status === 'healthy'} class:health-degraded={systemHealth.status === 'degraded'}>
          {systemHealth.status === 'healthy' ? '●' : '!'}
        </span>
        <span class="cwh-stat-label">{systemHealth.status === 'healthy' ? 'System Healthy' : 'System Degraded'}</span>
        {#if systemHealth.server_version}
          <span class="cwh-stat-sub">v{systemHealth.server_version}{systemHealth.milestone ? ` (${systemHealth.milestone})` : ''}</span>
        {/if}
        {#if systemHealth.failed_jobs > 0}
          <span class="cwh-stat-sub cwh-stat-warn">{systemHealth.failed_jobs} failed job{systemHealth.failed_jobs !== 1 ? 's' : ''}</span>
        {/if}
      </button>
    {/if}
  </div>

  <!-- ── Decisions ─────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-decisions" aria-labelledby="decisions-heading">
    <div class="section-header">
      <h2 class="section-title" id="decisions-heading">
        {$t('cross_workspace.sections.decisions')}
        {#if notifications.length > 0}
          <span class="section-badge" aria-label={$t('cross_workspace.pending_label', { values: { count: notifications.length } })}>{notifications.length}</span>
        {/if}
      </h2>
    </div>

    {#if decisionsLoading}
      <div class="section-loading" aria-live="polite">{$t('cross_workspace.loading_decisions')}</div>
    {:else if decisionsError}
      <div class="section-error" role="alert">{decisionsError}</div>
    {:else if notifications.length === 0}
      <p class="section-empty">{$t('cross_workspace.decisions_empty')}</p>
    {:else}
      <ul class="decisions-list" role="list">
        {#each (showAllDecisions ? notifications : notifications.slice(0, 5)) as notif (notif.id)}
          {@const body = getBody(notif)}
          {@const state = actionStates[notif.id] ?? {}}
          <li class="decision-item" data-testid="cwh-decision-item">
            <span class="decision-icon" aria-hidden="true">{TYPE_ICONS[notif.notification_type] ?? '•'}</span>
            <button class="decision-body decision-body-clickable" onclick={() => {
              if (body.mr_id) nav('mr', body.mr_id, { repo_id: notif.repo_id });
              else if (body.agent_id) nav('agent', body.agent_id, { repo_id: notif.repo_id });
              else if (body.task_id) nav('task', body.task_id, { repo_id: notif.repo_id });
              else if (body.spec_path) { const sp = normalizeSpecPath(body.spec_path); nav('spec', sp, { path: sp, repo_id: notif.repo_id }); }
            }}>
              <div class="decision-content">
                <span class="decision-type">{typeLabel(notif.notification_type)}</span>
                <span class="decision-desc">{notif.message ?? notif.title ?? $t('cross_workspace.decision_pending')}</span>
              </div>
              {#if notif.workspace_id && workspaceNameMap[notif.workspace_id]}
                <span class="decision-ws-badge" onclick={(e) => { e.stopPropagation(); const ws = workspaces.find(w => w.id === notif.workspace_id); if (ws) onSelectWorkspace?.(ws); }} title="Go to {workspaceNameMap[notif.workspace_id]}" role="link" tabindex="0">{workspaceNameMap[notif.workspace_id]}</span>
              {/if}
            </button>
            <div class="decision-actions">
              {#if state.success}
                <span class="action-feedback success">{state.message}</span>
              {:else if state.loading}
                <span class="action-feedback">…</span>
              {:else}
                {#if notif.notification_type === 'spec_approval' && body.spec_path && body.spec_sha}
                  <button class="inline-btn approve" onclick={() => handleApproveSpec(notif)} aria-label={$t('cross_workspace.approve_spec')}>{$t('cross_workspace.approve')}</button>
                  <button class="inline-btn reject" onclick={() => handleRejectSpec(notif)} aria-label={$t('cross_workspace.reject_spec')}>{$t('cross_workspace.reject')}</button>
                {:else if notif.notification_type === 'gate_failure' && body.mr_id}
                  <button class="inline-btn" onclick={() => handleRetry(notif)} aria-label={$t('cross_workspace.retry_gate')}>{$t('cross_workspace.retry')}</button>
                {/if}
                <button class="inline-btn secondary" onclick={() => handleDismiss(notif)} aria-label={$t('cross_workspace.dismiss')}>{$t('cross_workspace.dismiss')}</button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
      {#if notifications.length > 5}
        <div class="section-footer">
          <button class="view-all-btn" onclick={() => { showAllDecisions = !showAllDecisions; }}>
            {showAllDecisions ? $t('cross_workspace.show_fewer') : $t('cross_workspace.view_all_decisions', { values: { count: notifications.length } })}
          </button>
        </div>
      {/if}
    {/if}
  </section>

  <!-- ── Workspaces ────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-workspaces" aria-labelledby="workspaces-heading">
    <div class="section-header">
      <h2 class="section-title" id="workspaces-heading">{$t('cross_workspace.sections.workspaces')}</h2>
      <button
        class="new-ws-btn"
        onclick={() => { createWsForm = { name: '', description: '' }; createWsOpen = true; }}
        data-testid="create-workspace-btn"
      >
        {$t('cross_workspace.new_workspace')}
      </button>
    </div>

    {#if workspacesLoading}
      <div class="section-loading" aria-live="polite">{$t('cross_workspace.loading_workspaces')}</div>
    {:else if workspacesError}
      <div class="section-error" role="alert">{workspacesError}</div>
    {:else if workspaces.length === 0}
      <p class="section-empty">{$t('cross_workspace.workspaces_empty')}</p>
    {:else}
      <ul class="workspace-list" role="list">
        {#each sortedWorkspaces as ws (ws.id)}
          {@const wsSpecs = specs.filter(s => s.workspace_id === ws.id)}
          {@const wsNotifs = notifications.filter(n => n.workspace_id === ws.id)}
          {@const wsPendingSpecs = wsSpecs.filter(s => (s.approval_status ?? s.status) === 'pending').length}
          {@const wsApprovedSpecs = wsSpecs.filter(s => (s.approval_status ?? s.status) === 'approved').length}
          {@const wsRepos = allRepos.filter(r => r.workspace_id === ws.id)}
          {@const wsActiveAgents = allAgents.filter(a => a.workspace_id === ws.id)}
          {@const wsOpenMrs = allMrs.filter(m => m.workspace_id === ws.id)}
          {@const wsGateFailures = wsNotifs.filter(n => n.notification_type === 'gate_failure')}
          {@const wsPendingDecisions = wsNotifs.filter(n => n.notification_type === 'spec_approval' || n.notification_type === 'agent_clarification')}
          {@const wsRecentEvents = activityEvents.filter(e => e.workspace_id === ws.id).slice(0, 3)}
          {@const health = wsHealthStatus(ws.id)}
          <li class="workspace-row ws-health-{health}">
            <button
              class="workspace-btn"
              onclick={() => onSelectWorkspace?.(ws)}
              data-testid="workspace-row-{ws.id}"
            >
              <div class="workspace-btn-top">
                <span class="workspace-name">{ws.name}</span>
                <div class="workspace-indicators">
                  {#if wsGateFailures.length > 0}
                    <span class="ws-indicator ws-indicator-danger" title="{wsGateFailures.length} failing gates">! {wsGateFailures.length} gate failures</span>
                  {/if}
                  {#if wsPendingDecisions.length > 0}
                    <span class="ws-indicator ws-indicator-warning" title="{wsPendingDecisions.length} pending decisions">{wsPendingDecisions.length} pending</span>
                  {/if}
                  {#if ws.health}
                    <span class="health-badge" class:health-ok={ws.health === 'healthy'} class:health-warn={ws.health === 'gate_failure'}>
                      {ws.health === 'healthy' ? '●' : '!'} {ws.health}
                    </span>
                  {/if}
                </div>
              </div>
              {#if ws.description}
                <span class="workspace-description">{ws.description}</span>
              {/if}
              <!-- Mini pipeline: Repos → Specs → Agents → MRs -->
              <div class="ws-pipeline-row">
                <span class="ws-pipe-stage" class:ws-pipe-active={wsRepos.length > 0}>
                  <Icon name="code" size={10} />
                  <span class="ws-pipe-count">{wsRepos.length}</span>
                  <span class="ws-pipe-label">repos</span>
                </span>
                <span class="ws-pipe-arrow">→</span>
                <span class="ws-pipe-stage" class:ws-pipe-active={wsSpecs.length > 0} class:ws-pipe-warn={wsPendingSpecs > 0}>
                  <Icon name="spec" size={10} />
                  <span class="ws-pipe-count">{wsSpecs.length}</span>
                  <span class="ws-pipe-label">specs</span>
                </span>
                <span class="ws-pipe-arrow">→</span>
                <span class="ws-pipe-stage" class:ws-pipe-active={wsActiveAgents.length > 0} class:ws-pipe-success={wsActiveAgents.length > 0}>
                  <Icon name="agent" size={10} />
                  <span class="ws-pipe-count">{wsActiveAgents.length}</span>
                  <span class="ws-pipe-label">agents</span>
                </span>
                <span class="ws-pipe-arrow">→</span>
                <span class="ws-pipe-stage" class:ws-pipe-active={wsOpenMrs.length > 0} class:ws-pipe-danger={wsGateFailures.length > 0}>
                  <Icon name="git-merge" size={10} />
                  <span class="ws-pipe-count">{wsOpenMrs.length}</span>
                  <span class="ws-pipe-label">MRs</span>
                </span>
              </div>
              <div class="workspace-stats-row">
                {#if wsActiveAgents.length > 0}
                  <span class="ws-stat-chip ws-stat-agents" title="{wsActiveAgents.length} active agents">
                    {wsActiveAgents.length} active {wsActiveAgents.length === 1 ? 'agent' : 'agents'}
                  </span>
                {:else if ws.agent_count != null && ws.agent_count > 0}
                  <span class="ws-stat-chip">{ws.agent_count} agents</span>
                {/if}
                {#if wsOpenMrs.length > 0}
                  <span class="ws-stat-chip" title="{wsOpenMrs.length} open MRs">
                    {wsOpenMrs.length} open MRs
                  </span>
                {/if}
                {#if wsNotifs.length > 0}
                  <span class="ws-stat-chip ws-stat-decisions" title="{wsNotifs.length} decisions pending">
                    {wsNotifs.length} decisions
                  </span>
                {/if}
                {#if ws.budget_pct != null}
                  <span class="ws-stat-chip">{ws.budget_pct}% budget</span>
                {/if}
                {#if wsRepos.length === 0 && wsSpecs.length === 0 && wsNotifs.length === 0 && wsActiveAgents.length === 0 && !ws.agent_count}
                  <span class="ws-stat-chip ws-stat-empty">No activity</span>
                {/if}
              </div>
              {#if wsRecentEvents.length > 0}
                <div class="ws-recent-activity">
                  {#each wsRecentEvents as evt}
                    <span class="ws-recent-event">
                      <span class="ws-recent-icon">{activityIcon(evt)}</span>
                      <span class="ws-recent-label">{activityLabel(evt)}</span>
                      <span class="ws-recent-time">{relTime(evt.timestamp ?? evt.created_at)}</span>
                    </span>
                  {/each}
                </div>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- ── Recent Activity ───────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-activity" aria-labelledby="activity-heading">
    <div class="section-header">
      <h2 class="section-title" id="activity-heading">Recent Activity
        {#if !activityLoading && activityEvents.length > 0}
          <span class="section-badge">{activityEvents.length}</span>
        {/if}
      </h2>
    </div>

    {#if activityLoading}
      <div class="section-loading" aria-live="polite">Loading activity...</div>
    {:else if activityEvents.length === 0}
      <p class="section-empty">No recent activity across workspaces.</p>
    {:else}
      <div class="cwh-activity-timeline">
        {#each (showAllActivity ? activityEvents : activityEvents.slice(0, 10)) as event, i}
          {@const variant = activityVariant(event)}
          {@const wsName = activityWorkspaceName(event)}
          <div class="cwh-activity-item">
            <div class="cwh-activity-dot cwh-activity-dot-{variant}"></div>
            {#if i < Math.min(showAllActivity ? activityEvents.length : 10, activityEvents.length) - 1}<div class="cwh-activity-line"></div>{/if}
            <div class="cwh-activity-content">
              <span class="cwh-activity-icon">{activityIcon(event)}</span>
              <span class="cwh-activity-label">{activityLabel(event)}</span>
              {#if event.entity_name ?? event.title ?? event.description}
                <span class="cwh-activity-detail">{event.entity_name ?? event.title ?? event.description}</span>
              {/if}
              {#if event.entity_id && event.entity_type}
                <button class="ws-entity-link cwh-activity-entity-link" onclick={() => nav(event.entity_type, event.entity_id, event)} title="View {event.entity_type}: {event.entity_id}">{resolveEntityName(event.entity_type, event.entity_id)}</button>
              {:else}
                {#if event.agent_id}
                  <button class="ws-entity-link cwh-activity-entity-link" onclick={() => nav('agent', event.agent_id, { repo_id: event.repo_id })} title="View agent: {event.agent_id}">{resolveEntityName('agent', event.agent_id)}</button>
                {/if}
                {#if event.mr_id}
                  <button class="ws-entity-link cwh-activity-entity-link" onclick={() => nav('mr', event.mr_id, { repository_id: event.repo_id })} title="View MR: {event.mr_id}">{resolveEntityName('mr', event.mr_id)}</button>
                {/if}
                {#if event.task_id && !event.agent_id && !event.mr_id}
                  <button class="ws-entity-link cwh-activity-entity-link" onclick={() => nav('task', event.task_id, { repo_id: event.repo_id })} title="View task: {event.task_id}">{resolveEntityName('task', event.task_id)}</button>
                {/if}
                {#if event.spec_path && !event.agent_id && !event.mr_id}
                  <button class="ws-entity-link cwh-activity-entity-link" onclick={() => nav('spec', event.spec_path, { path: event.spec_path, repo_id: event.repo_id })} title="View spec: {event.spec_path}">{event.spec_path.split('/').pop()}</button>
                {/if}
              {/if}
              {#if wsName}
                <button class="cwh-activity-ws-badge cwh-activity-ws-link" onclick={(e) => { e.stopPropagation(); const ws = workspaces.find(w => w.id === event.workspace_id); if (ws) onSelectWorkspace?.(ws); }} title="Go to {wsName}">{wsName}</button>
              {/if}
              {#if event.timestamp ?? event.created_at}
                <span class="cwh-activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
      {#if activityEvents.length > 10}
        <div class="section-footer">
          <button class="view-all-btn" onclick={() => { showAllActivity = !showAllActivity; }}>
            {showAllActivity ? $t('cross_workspace.show_fewer') : `View all ${activityEvents.length} events`}
          </button>
        </div>
      {/if}
    {/if}
  </section>

  <!-- ── Specs ─────────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-specs" aria-labelledby="specs-heading">
    <div class="section-header">
      <h2 class="section-title" id="specs-heading">{$t('cross_workspace.sections.specs')}</h2>
    </div>

    {#if specsLoading}
      <div class="section-loading" aria-live="polite">{$t('cross_workspace.loading_specs')}</div>
    {:else if specsError}
      <div class="section-error" role="alert">{specsError}</div>
    {:else if specs.length === 0}
      <p class="section-empty">{$t('cross_workspace.specs_empty')}</p>
    {:else}
      <table class="specs-table" data-testid="specs-table">
        <thead>
          <tr>
            <th scope="col" aria-sort={specsSortCol === 'path' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSpecsSort('path')}>{$t('cross_workspace.col_path')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('path')}</span></button>
            </th>
            <th scope="col" aria-sort={specsSortCol === 'workspace_name' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSpecsSort('workspace_name')}>{$t('cross_workspace.col_workspace_repo')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('workspace_name')}</span></button>
            </th>
            <th scope="col" aria-sort={specsSortCol === 'status' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSpecsSort('status')}>{$t('cross_workspace.col_status')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('status')}</span></button>
            </th>
            <th scope="col" aria-sort={specsSortCol === 'progress' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSpecsSort('progress')}>{$t('workspace_home.col_progress')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('progress')}</span></button>
            </th>
            <th scope="col" aria-sort={specsSortCol === 'updated_at' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSpecsSort('updated_at')}>{$t('workspace_home.col_last_activity')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('updated_at')}</span></button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each (specsShowAll ? sortedSpecs : sortedSpecs.slice(0, 10)) as spec (spec.path ?? spec.id)}
            <tr class="spec-row clickable" onclick={() => {
              nav('spec', spec.path, { ...spec, path: spec.path, repo_id: spec.repo_id });
            }} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') nav('spec', spec.path, { ...spec, path: spec.path, repo_id: spec.repo_id }); }}>
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
              <td class="spec-progress">
                {#if spec.tasks_total != null && spec.tasks_total > 0}
                  {@const pct = Math.round(((spec.tasks_done ?? 0) / spec.tasks_total) * 100)}
                  <div class="progress-cell" title="{spec.tasks_done ?? 0} of {spec.tasks_total} tasks complete ({pct}%)">
                    <div class="progress-mini-bar">
                      <div class="progress-mini-fill" style="width: {pct}%" class:progress-complete={pct === 100}></div>
                    </div>
                    <span class="progress-fraction">{spec.tasks_done ?? 0}/{spec.tasks_total}</span>
                  </div>
                {:else if spec.tasks_total != null}
                  <span class="progress-fraction" title="No tasks created yet">0/0</span>
                {:else}
                  <span class="secondary">—</span>
                {/if}
              </td>
              <td class="spec-activity">{relTime(spec.updated_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if sortedSpecs.length > 10}
        <div class="section-footer">
          <button class="view-all-btn" onclick={() => { specsShowAll = !specsShowAll; }}>
            {specsShowAll ? $t('cross_workspace.show_fewer') : $t('cross_workspace.show_all_specs', { values: { count: sortedSpecs.length } })}
          </button>
        </div>
      {/if}
    {/if}
  </section>

  <!-- ── Briefing ─────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-briefing" aria-labelledby="briefing-heading">
    <div class="section-header">
      <h2 class="section-title" id="briefing-heading">{$t('cross_workspace.sections.briefing')}</h2>
      <span class="section-scope-tag">{$t('cross_workspace.scope_aggregated')}</span>
    </div>

    {#if briefingLoading}
      <div class="section-loading" aria-live="polite">{$t('cross_workspace.loading_briefing')}</div>
    {:else if briefingError}
      <div class="section-error" role="alert">{briefingError}</div>
    {:else if briefingSummaries.length === 0}
      <p class="section-empty">{$t('cross_workspace.briefing_empty')}</p>
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

  <!-- ── Budget Summary ──────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-budget" aria-labelledby="budget-heading">
    <div class="section-header">
      <h2 class="section-title" id="budget-heading">Budget Overview</h2>
    </div>
    {#if budgetSummaryLoading}
      <div class="loading-placeholder">Loading budget...</div>
    {:else if budgetSummary}
      {@const bs = budgetSummary}
      {@const wsBreakdown = Array.isArray(bs.workspaces) ? bs.workspaces : (bs.workspace_budgets ?? [])}
      <div class="budget-summary-grid">
        {#if bs.total_active_agents != null || bs.total_cost_today != null || bs.total_tokens_today != null}
          <div class="budget-stat-cards">
            {#if bs.total_active_agents != null}
              <div class="budget-stat-card">
                <span class="budget-stat-value">{bs.total_active_agents}</span>
                <span class="budget-stat-label">Active Agents</span>
              </div>
            {/if}
            {#if bs.total_tokens_today != null}
              <div class="budget-stat-card">
                <span class="budget-stat-value">{bs.total_tokens_today.toLocaleString()}</span>
                <span class="budget-stat-label">Tokens Today</span>
              </div>
            {/if}
            {#if bs.total_cost_today != null}
              <div class="budget-stat-card">
                <span class="budget-stat-value">${bs.total_cost_today.toFixed(2)}</span>
                <span class="budget-stat-label">Cost Today</span>
              </div>
            {/if}
          </div>
        {/if}
        {#if wsBreakdown.length > 0}
          <table class="cwh-specs-table">
            <thead>
              <tr>
                <th>Workspace</th>
                <th>Agents</th>
                <th>Tokens</th>
                <th>Cost</th>
                <th>Limit</th>
              </tr>
            </thead>
            <tbody>
              {#each wsBreakdown as ws}
                <tr>
                  <td>{workspaceNameMap[ws.workspace_id] ?? ws.workspace_name ?? (ws.workspace_id ? 'Workspace ' + ws.workspace_id.slice(0, 6) : '—')}</td>
                  <td>{ws.active_agents ?? ws.agents ?? '—'}</td>
                  <td>{(ws.tokens_today ?? ws.tokens ?? 0).toLocaleString()}</td>
                  <td>${(ws.cost_today ?? ws.cost ?? 0).toFixed(2)}</td>
                  <td>{ws.max_cost_per_day != null ? `$${ws.max_cost_per_day.toFixed(2)}/day` : '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else}
      <p class="empty-text">No budget data available. Configure workspace budgets in workspace settings.</p>
    {/if}
  </section>

  <!-- ── Agent Rules ────────────────────────────────────────────────────── -->
  <section class="cwh-section" data-testid="section-agent-rules" aria-labelledby="agent-rules-heading">
    <div class="section-header">
      <h2 class="section-title" id="agent-rules-heading">{$t('cross_workspace.sections.agent_rules')}</h2>
      <span class="section-scope-tag">{$t('cross_workspace.scope_tenant_level')}</span>
      {#if onManageAgentRules}
        <button
          class="manage-rules-btn"
          onclick={() => onManageAgentRules?.()}
          data-testid="manage-tenant-rules-btn"
        >{$t('cross_workspace.manage_tenant_rules')}</button>
      {/if}
    </div>

    {#if rulesLoading}
      <div class="section-loading" aria-live="polite">{$t('cross_workspace.loading_agent_rules')}</div>
    {:else if rulesError}
      <div class="section-error" role="alert">{rulesError}</div>
    {:else if globalMetaSpecs.length === 0}
      <p class="section-empty">{$t('cross_workspace.agent_rules_empty')}</p>
    {:else}
      {#each Object.entries(specsByKind) as [kind, items] (kind)}
        <div class="rules-group">
          <h3 class="rules-group-title">{kindLabel(kind)} <span class="rules-count">({items.length})</span></h3>
          <ul class="rules-list" role="list">
            {#each items as ms (ms.id)}
              <li class="rule-row rule-row-clickable" onclick={() => onManageAgentRules?.()} tabindex="0" role="button" title="Click to view and edit this rule" onkeydown={(e) => { if (e.key === 'Enter') onManageAgentRules?.(); }}>
                <span class="rule-name">{ms.name ?? ms.path ?? '—'}</span>
                {#if ms.required}
                  <span class="rule-required" aria-label={$t('cross_workspace.rule_required')} title="Required — agents must follow this rule">required</span>
                {/if}
                <span class="rule-version" title="Version {ms.version ?? 1}">v{ms.version ?? 1}</span>
                <span class="rule-status" class:status-approved={ms.status === 'Approved'} title={ms.status === 'Approved' ? 'This rule is approved and active' : ms.status === 'Pending' ? 'Awaiting approval' : ms.status ?? ''}>
                  {ms.status ?? '—'}
                </span>
                <span class="rule-arrow" aria-hidden="true">→</span>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    {/if}
  </section>
</div>

<!-- Create Workspace modal -->
<Modal bind:open={createWsOpen} title={$t('cross_workspace.new_workspace')} size="sm">
  <div class="create-ws-form">
    <label class="create-ws-label">{$t('cross_workspace.create_ws_name_label')}
      <input
        class="create-ws-input"
        bind:value={createWsForm.name}
        placeholder={$t('cross_workspace.create_ws_name_placeholder')}
        onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspace()}
      />
    </label>
    <label class="create-ws-label">{$t('cross_workspace.create_ws_desc_label')}
      <input
        class="create-ws-input"
        bind:value={createWsForm.description}
        placeholder={$t('cross_workspace.create_ws_desc_placeholder')}
        onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspace()}
      />
    </label>
    <div class="create-ws-actions">
      <button class="create-ws-cancel" onclick={() => (createWsOpen = false)}>{$t('cross_workspace.create_ws_cancel')}</button>
      <button
        class="create-ws-submit"
        onclick={handleCreateWorkspace}
        disabled={createWsSaving || !createWsForm.name?.trim()}
      >
        {createWsSaving ? $t('cross_workspace.create_ws_creating') : $t('cross_workspace.create_ws_submit')}
      </button>
    </div>
  </div>
</Modal>

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
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-2);
  }

  .cwh-header-text { flex: 1; min-width: 0; }

  .tenant-gear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .tenant-gear-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .tenant-gear-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .manage-rules-btn {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    padding: 0;
  }

  .manage-rules-btn:hover { text-decoration: underline; }

  .manage-rules-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .view-all-btn {
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    padding: 0;
  }

  .view-all-btn:hover {
    text-decoration: underline;
  }

  .view-all-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .decision-body-clickable {
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    padding: 0;
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast);
  }

  .decision-body-clickable:hover .decision-desc {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .decision-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .decision-type {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .decision-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .decision-ws-badge {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-border);
    border: none;
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
    white-space: nowrap;
    flex-shrink: 0;
    font-family: var(--font-body);
  }

  .decision-ws-link, .cwh-activity-ws-link {
    cursor: pointer;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .decision-ws-link:hover, .cwh-activity-ws-link:hover {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, var(--color-border));
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

  .workspace-row.ws-health-critical { border-left: 3px solid var(--color-danger); }
  .workspace-row.ws-health-warning { border-left: 3px solid var(--color-warning); }
  .workspace-row.ws-health-active { border-left: 3px solid var(--color-success); }
  .workspace-row.ws-health-idle { border-left: 3px solid transparent; }

  .workspace-btn {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: var(--space-3) var(--space-6);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
    gap: var(--space-2);
  }

  .workspace-btn:hover {
    background: var(--color-surface-elevated);
  }

  .workspace-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .workspace-btn-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
  }

  .workspace-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .workspace-description {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Mini pipeline in workspace rows */
  .ws-pipeline-row {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .ws-pipe-stage {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    opacity: 0.5;
  }

  .ws-pipe-active { opacity: 1; }
  .ws-pipe-warn { color: var(--color-warning); opacity: 1; }
  .ws-pipe-success { color: var(--color-success); opacity: 1; }
  .ws-pipe-danger { color: var(--color-danger); opacity: 1; }

  .ws-pipe-count { font-weight: 600; }
  .ws-pipe-label { font-weight: 400; }

  .ws-pipe-arrow {
    font-size: 9px;
    color: var(--color-text-muted);
    opacity: 0.4;
  }

  .workspace-stats-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .ws-stat-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
  }

  .ws-stat-empty {
    color: var(--color-text-muted);
    border: none;
    background: none;
    font-style: italic;
  }

  .ws-stat-decisions {
    color: var(--color-warning);
    border-color: color-mix(in srgb, var(--color-warning) 30%, transparent);
  }

  .ws-stat-alert {
    color: var(--color-warning);
    font-weight: 500;
    margin-left: 2px;
  }

  .ws-stat-active {
    color: var(--color-success);
    font-weight: 500;
    margin-left: 2px;
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
    padding: 0;
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-6);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .sort-arrow {
    font-size: var(--text-xs);
    opacity: 0.6;
  }

  .spec-row {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }

  .spec-row:last-child { border-bottom: none; }

  .spec-row:hover { background: var(--color-surface-elevated); }

  .spec-row.clickable { cursor: pointer; }

  .spec-row.clickable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

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

  .spec-progress {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .progress-cell { display: flex; align-items: center; gap: var(--space-2); }
  .progress-mini-bar { width: 48px; height: 6px; background: var(--color-border); border-radius: 3px; overflow: hidden; flex-shrink: 0; }
  .progress-mini-fill { height: 100%; background: var(--color-warning); border-radius: 3px; transition: width 0.3s ease; }
  .progress-mini-fill.progress-complete { background: var(--color-success); }
  .progress-fraction { font-size: var(--text-xs); color: var(--color-text-muted); }

  .rule-row-clickable { cursor: pointer; transition: background var(--transition-fast); }
  .rule-row-clickable:hover { background: var(--color-surface-hover, rgba(0,0,0,0.03)); }
  .rule-row-clickable:focus-visible { outline: 2px solid var(--color-focus); outline-offset: -1px; }
  .rule-arrow { color: var(--color-text-muted); font-size: var(--text-xs); margin-left: auto; }
  .rules-count { font-weight: 400; color: var(--color-text-muted); font-size: var(--text-xs); }

  .spec-activity {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

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

  /* ── New Workspace button ──────────────────────────────────────────── */
  .new-ws-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
    white-space: nowrap;
  }

  .new-ws-btn:hover { background: var(--color-primary-hover); }

  .new-ws-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Create Workspace modal form ──────────────────────────────────── */
  .create-ws-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .create-ws-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .create-ws-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .create-ws-input:focus:not(:focus-visible) { outline: none; }

  .create-ws-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .create-ws-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .create-ws-cancel {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .create-ws-cancel:hover { border-color: var(--color-text-muted); }

  .create-ws-submit {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .create-ws-submit:hover { background: var(--color-primary-hover); }
  .create-ws-submit:disabled { opacity: 0.5; cursor: not-allowed; }

  .create-ws-cancel:focus-visible,
  .create-ws-submit:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

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

  /* ── Budget Summary ────────────────────────────────────────────────── */
  .budget-summary-grid {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .budget-stat-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: var(--space-3);
  }

  .budget-stat-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated, var(--color-bg));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }

  .budget-stat-value {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .budget-stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .loading-placeholder {
    padding: var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .empty-text {
    padding: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  /* ── Aggregate Stat Cards ──────────────────────────────────────────── */
  .cwh-stat-cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: var(--space-3);
  }

  .cwh-stat-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-4) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .cwh-stat-value {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-mono);
    line-height: 1;
  }

  .cwh-stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-top: var(--space-1);
  }

  .cwh-stat-sub {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin-top: var(--space-1);
  }

  .cwh-stat-sub.cwh-stat-warn {
    color: var(--color-warning);
  }

  .cwh-stat-card-alert {
    border-color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 5%, var(--color-surface));
  }

  .cwh-stat-card-health {
    cursor: pointer;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .cwh-stat-card-health:hover {
    border-color: var(--color-primary);
    box-shadow: var(--shadow-sm);
  }

  .cwh-stat-card-degraded {
    border-color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 5%, var(--color-surface));
  }

  .cwh-health-dot {
    font-size: var(--text-lg) !important;
  }

  .cwh-health-dot.health-ok {
    color: var(--color-success);
  }

  .cwh-health-dot.health-degraded {
    color: var(--color-danger);
  }

  @media (max-width: 768px) {
    .cwh-stat-cards {
      grid-template-columns: repeat(2, 1fr);
    }
    .workspace-row.ws-health-critical,
    .workspace-row.ws-health-warning,
    .workspace-row.ws-health-active,
    .workspace-row.ws-health-idle {
      border-left-width: 3px;
    }
  }

  /* ── Workspace card enhancements ───────────────────────────────────── */
  .workspace-indicators {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .ws-indicator {
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    white-space: nowrap;
    font-weight: 500;
  }

  .ws-indicator-danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }

  .ws-indicator-warning {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
  }

  .ws-stat-agents {
    color: var(--color-success);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .ws-recent-activity {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: var(--space-1);
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .ws-recent-event {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .ws-recent-icon {
    flex-shrink: 0;
    width: 14px;
    text-align: center;
  }

  .ws-recent-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-recent-time {
    flex-shrink: 0;
    color: var(--color-text-muted);
    opacity: 0.7;
  }

  /* ── Activity Timeline ─────────────────────────────────────────────── */
  .cwh-activity-timeline {
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: var(--space-2) 0;
  }

  .cwh-activity-item {
    display: flex;
    position: relative;
    padding-left: 24px;
    min-height: 32px;
  }

  .cwh-activity-dot {
    position: absolute;
    left: 8px;
    top: 6px;
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
    background: var(--color-border-strong);
    z-index: 1;
  }

  .cwh-activity-dot-success { background: var(--color-success); }
  .cwh-activity-dot-danger { background: var(--color-danger); }
  .cwh-activity-dot-warning { background: var(--color-warning); }
  .cwh-activity-dot-info { background: var(--color-info); }

  .cwh-activity-line {
    position: absolute;
    left: 11px;
    top: 16px;
    bottom: -2px;
    width: 2px;
    background: var(--color-border);
  }

  .cwh-activity-content {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    padding: var(--space-1) var(--space-4) var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    min-height: 28px;
  }

  .cwh-activity-icon {
    flex-shrink: 0;
    font-size: var(--text-sm);
  }

  .cwh-activity-label {
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
  }

  .cwh-activity-detail {
    color: var(--color-text);
    font-weight: 500;
    font-size: var(--text-sm);
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cwh-activity-entity-link {
    font-size: var(--text-xs);
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    padding: 0;
    font-family: var(--font-mono);
  }

  .cwh-activity-entity-link:hover { text-decoration: underline; }

  .cwh-activity-ws-badge {
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border-radius: var(--radius-sm);
    padding: 0 var(--space-1);
    white-space: nowrap;
  }

  .cwh-activity-time {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    white-space: nowrap;
    margin-left: auto;
  }
</style>
