<script>
  /**
   * WorkspaceHome — workspace dashboard (§2 of ui-navigation.md)
   *
   * Sections: Decisions, Repos, Architecture, Briefing, Specs, Agent Rules.
   * Implements real data loading for all six sections.
   *
   * Spec refs:
   *   ui-navigation.md §2 (Workspace Home sections)
   *   HSI §8 (notification types + priority table)
   *   HSI §2 (trust-level filtering)
   */
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Briefing from './Briefing.svelte';
  import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
  import Modal from '../lib/Modal.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  const goToAgentRules = getContext('goToAgentRules');
  const openDetailPanel = getContext('openDetailPanel') ?? null;

  let {
    workspace = null,
    onSelectRepo = undefined,
    onWorkspaceCreated = undefined,
    decisionsCount = 0,
  } = $props();

  // ── Create Workspace form state ───────────────────────────────────────
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
      toastSuccess($t('workspace_home.ws_created', { values: { name } }));
      createWsOpen = false;
      createWsForm = { name: '', description: '' };
      onWorkspaceCreated?.(newWs);
    } catch (e) {
      toastError($t('workspace_home.ws_create_failed', { values: { error: e.message || e } }));
    } finally {
      createWsSaving = false;
    }
  }

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

  function typeLabel(type) {
    const key = `workspace_home.type_labels.${type}`;
    const val = $t(key);
    return val !== key ? val : type;
  }

  const SPEC_STATUS_ICONS = {
    draft: '📝',
    pending: '⏳',
    approved: '✅',
    rejected: '❌',
    implemented: '✅',
    merged: '✅',
  };

  function specStatusTooltip(status) {
    switch (status) {
      case 'draft': return 'Spec has been synced from the repo but not yet submitted for approval';
      case 'pending': return 'Spec is awaiting human approval before agents can implement it';
      case 'approved': return 'Spec has been approved — agents can create tasks and begin implementation';
      case 'rejected': return 'Spec was rejected — no further work should proceed on this spec';
      case 'implemented': return 'All tasks linked to this spec have been completed';
      default: return '';
    }
  }

  function taskStatusTooltip(status) {
    switch (status) {
      case 'backlog': return 'Task is waiting to be assigned to an agent';
      case 'in_progress': return 'An agent is actively working on this task';
      case 'done': return 'Task has been completed — MR created or code merged';
      case 'blocked': return 'Task is blocked by a dependency or external factor';
      case 'cancelled': return 'Task was cancelled — the linked spec may have been rejected';
      default: return '';
    }
  }

  function mrStatusTooltip(mr) {
    if (mr.queue_position != null) return `MR is position ${mr.queue_position + 1} in the merge queue — gates will run before merge`;
    switch (mr.status) {
      case 'open': return 'MR is open and ready to be enqueued for merge';
      case 'merged': return 'MR passed all required gates and was merged to the target branch';
      case 'closed': return 'MR was closed without merging — may have failed gates or been superseded';
      default: return '';
    }
  }

  function agentStatusTooltip(status) {
    switch (status) {
      case 'active': return 'Agent is currently running — implementing code, running tests, or communicating';
      case 'idle': return 'Agent has completed its work — MR should have been created';
      case 'completed': return 'Agent finished successfully';
      case 'failed': return 'Agent encountered an error during execution';
      case 'dead': return 'Agent was killed by an administrator';
      case 'stopped': return 'Agent was stopped gracefully';
      default: return '';
    }
  }

  // ── Decisions state ────────────────────────────────────────────────────
  let decisionsLoading = $state(true);
  let decisionsError = $state(null);
  let notifications = $state([]);
  let actionStates = $state({});
  let showAllDecisions = $state(false);

  // ── Repos state ────────────────────────────────────────────────────────
  let reposLoading = $state(true);
  let reposError = $state(null);
  let repos = $state([]);

  // ── Specs state ────────────────────────────────────────────────────────
  let specsLoading = $state(true);
  let specsError = $state(null);
  let specs = $state([]);
  let specsStatusFilter = $state('');

  // ── Architecture state ─────────────────────────────────────────────────
  let archExpanded = $state(false);
  let archLoading = $state(false);
  let archError = $state(null);
  let archGraph = $state(null); // { nodes: [], edges: [] }

  async function loadArchGraph() {
    if (!workspace?.id) return;
    archLoading = true;
    archError = null;
    try {
      archGraph = await api.workspaceGraph(workspace.id);
    } catch (e) {
      archError = e.message || $t('workspace_home.error_load_graph');
      archGraph = { nodes: [], edges: [] };
    } finally {
      archLoading = false;
    }
  }

  function toggleArch() {
    archExpanded = !archExpanded;
    if (archExpanded && !archGraph && !archLoading) {
      loadArchGraph();
    }
  }

  // ── Budget/Cost state ───────────────────────────────────────────────────
  let budgetLoading = $state(true);
  let budgetData = $state(null); // { config, usage }
  let costData = $state(null);   // cost summary

  // ── Agent Rules state ──────────────────────────────────────────────────
  let rulesLoading = $state(true);
  let rulesError = $state(null);
  let workspaceMetaSpecs = $state([]);
  let globalMetaSpecs = $state([]);

  // ── Repo lookup map (id → repo) ────────────────────────────────────────
  let repoMap = $state({});

  // ── Tasks state ────────────────────────────────────────────────────────
  let tasksLoading = $state(true);
  let wsTasks = $state([]);

  // ── MRs state ──────────────────────────────────────────────────────────
  let mrsLoading = $state(true);
  let wsMrs = $state([]);

  // ── Agents state ───────────────────────────────────────────────────────
  let agentsLoading = $state(true);
  let wsAgents = $state([]);

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

  // ── Tasks: load ────────────────────────────────────────────────────────
  async function loadTasks() {
    if (!workspace?.id) return;
    tasksLoading = true;
    try {
      const data = await api.tasks({ workspaceId: workspace.id });
      wsTasks = Array.isArray(data) ? data : [];
    } catch {
      wsTasks = [];
    } finally {
      tasksLoading = false;
    }
  }

  // ── MRs: load ─────────────────────────────────────────────────────────
  async function loadMrs() {
    if (!workspace?.id) return;
    mrsLoading = true;
    try {
      const data = await api.mergeRequests({ workspace_id: workspace.id });
      const mrList = Array.isArray(data) ? data : [];
      // Enrich first 10 MRs with gate results (best-effort)
      const toEnrich = mrList.slice(0, 10);
      const gatePromises = toEnrich.map(mr =>
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
      const gateMap = Object.fromEntries(gateResults.map(g => [g.id, g]));
      wsMrs = mrList.map(mr => gateMap[mr.id] ? { ...mr, _gates: gateMap[mr.id] } : mr);
    } catch {
      wsMrs = [];
    } finally {
      mrsLoading = false;
    }
  }

  // ── Agents: load ──────────────────────────────────────────────────────
  async function loadAgents() {
    if (!workspace?.id) return;
    agentsLoading = true;
    try {
      const data = await api.agents({ workspaceId: workspace.id });
      wsAgents = Array.isArray(data) ? data : [];
    } catch {
      wsAgents = [];
    } finally {
      agentsLoading = false;
    }
  }

  // ── Budget/Cost: load ──────────────────────────────────────────────────
  async function loadBudget() {
    if (!workspace?.id) return;
    budgetLoading = true;
    try {
      const [budget, costs] = await Promise.all([
        api.workspaceBudget(workspace.id).catch(() => null),
        api.costSummary().catch(() => null),
      ]);
      budgetData = budget;
      costData = costs;
    } catch {
      budgetData = null;
      costData = null;
    } finally {
      budgetLoading = false;
    }
  }

  // ── Agent Rules: load ──────────────────────────────────────────────────
  async function loadRules() {
    if (!workspace?.id) return;
    rulesLoading = true;
    rulesError = null;
    try {
      const [wsData, globalData] = await Promise.all([
        api.getMetaSpecs({ scope: 'Workspace', scope_id: workspace.id }),
        api.getMetaSpecs({ scope: 'Global' }),
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
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('workspace_home.action_approved') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('workspace_home.action_failed') } };
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
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('workspace_home.action_rejected') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('workspace_home.action_failed') } };
    }
  }

  async function handleRetry(n) {
    const body = getBody(n);
    if (!body.mr_id) return;
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.enqueue(body.mr_id);
      notifications = notifications.map(item =>
        item.id === n.id ? { ...item, resolved_at: Date.now() / 1000 } : item
      );
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('workspace_home.action_re_queued') } };
    } catch (e) {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: e.message || $t('workspace_home.action_failed') } };
    }
  }

  async function handleDismiss(n) {
    actionStates = { ...actionStates, [n.id]: { loading: true } };
    try {
      await api.markNotificationRead(n.id);
      actionStates = { ...actionStates, [n.id]: { loading: false, success: true, message: $t('decisions.dismissed') } };
      setTimeout(() => { notifications = notifications.filter(item => item.id !== n.id); }, 600);
    } catch {
      actionStates = { ...actionStates, [n.id]: { loading: false, success: false, message: $t('decisions.dismiss_failed') } };
    }
  }

  // ── Spec navigation ────────────────────────────────────────────────────
  function navigateToSpec(spec) {
    const repo = repoMap[spec.repo_id];
    if (repo && onSelectRepo) {
      onSelectRepo(repo, 'specs', spec.path);
    }
  }

  // ── New Repo form state ────────────────────────────────────────────────
  let newRepoOpen = $state(false);
  let newRepoName = $state('');
  let newRepoDescription = $state('');
  let newRepoLoading = $state(false);
  let newRepoError = $state(null);

  // ── Import Repo form state ─────────────────────────────────────────────
  let importOpen = $state(false);
  let importUrl = $state('');
  let importName = $state('');
  let importLoading = $state(false);
  let importError = $state(null);

  async function handleCreateRepo() {
    const name = newRepoName.trim();
    if (!name) return;
    newRepoLoading = true;
    newRepoError = null;
    try {
      await api.createRepo({ name, description: newRepoDescription.trim() || undefined, workspace_id: workspace.id });
      newRepoOpen = false;
      newRepoName = '';
      newRepoDescription = '';
      await loadRepos();
    } catch (e) {
      newRepoError = e.message || $t('workspace_home.error_create_repo');
    } finally {
      newRepoLoading = false;
    }
  }

  async function handleImportRepo() {
    const url = importUrl.trim();
    if (!url) return;
    // Derive name from URL if not provided (strip .git suffix, take last path segment)
    const name = importName.trim() || url.split('/').pop()?.replace(/\.git$/, '') || '';
    importLoading = true;
    importError = null;
    try {
      await api.createMirrorRepo({ url, workspace_id: workspace.id, name });
      importOpen = false;
      importUrl = '';
      importName = '';
      await loadRepos();
    } catch (e) {
      importError = e.message || 'Failed to import repository';
    } finally {
      importLoading = false;
    }
  }

  // ── Specs sort state ────────────────────────────────────────────────────
  let specsSortCol = $state('path');
  let specsSortDir = $state('asc');

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

  // ── Derived: filtered + sorted specs ──────────────────────────────────
  let filteredSpecs = $derived.by(() => {
    let result = specs.filter(s => {
      const specStatus = s.approval_status ?? s.status;
      if (specsStatusFilter && specStatus !== specsStatusFilter) return false;
      return true;
    });
    return [...result].sort((a, b) => {
      let av, bv;
      if (specsSortCol === 'repo') {
        av = repoMap[a.repo_id]?.name ?? a.repo_id ?? '';
        bv = repoMap[b.repo_id]?.name ?? b.repo_id ?? '';
      } else if (specsSortCol === 'updated_at') {
        av = a.updated_at ?? '';
        bv = b.updated_at ?? '';
      } else if (specsSortCol === 'progress') {
        av = a.tasks_total ? (a.tasks_done ?? 0) / a.tasks_total : -1;
        bv = b.tasks_total ? (b.tasks_done ?? 0) / b.tasks_total : -1;
        const cmp = av - bv;
        return specsSortDir === 'asc' ? cmp : -cmp;
      } else {
        av = String(a[specsSortCol] ?? '');
        bv = String(b[specsSortCol] ?? '');
      }
      const cmp = String(av).localeCompare(String(bv));
      return specsSortDir === 'asc' ? cmp : -cmp;
    });
  });

  // ── Derived: meta-spec aggregates ─────────────────────────────────────
  // Tag each meta-spec with its scope for badge display
  let allMetaSpecs = $derived([
    ...globalMetaSpecs.map(m => ({ ...m, _scope: 'tenant' })),
    ...workspaceMetaSpecs.map(m => ({ ...m, _scope: 'workspace' })),
  ]);
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
    const ms = typeof ts === 'number' && ts < 1e12 ? ts * 1000 : new Date(ts).getTime();
    const diff = Date.now() - ms;
    const m = Math.floor(diff / 60000);
    if (m < 1) return $t('common.time_just_now');
    if (m < 60) return $t('common.time_minutes_ago', { values: { count: m } });
    const h = Math.floor(m / 60);
    if (h < 24) return $t('common.time_hours_ago', { values: { count: h } });
    return $t('common.time_days_ago', { values: { count: Math.floor(h / 24) } });
  }

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
    // Check repo map first
    if (type === 'repo') return repoMap[id]?.name ?? shortId(id);
    const cached = entityNameCache[`${type}:${id}`];
    if (cached) return cached;
    queueNameResolution(type, id);
    return shortId(id);
  }

  function shortId(id) {
    if (!id) return '';
    return id.length > 12 ? id.slice(0, 8) + '...' : id;
  }

  function fmtDuration(startTs, endTs) {
    if (!startTs) return '';
    const end = endTs ?? Date.now() / 1000;
    const secs = Math.round(end - startTs);
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  }

  // ── Activity feed state ─────────────────────────────────────────────────
  let activityLoading = $state(true);
  let activityEvents = $state([]);

  async function loadActivity() {
    activityLoading = true;
    try {
      const data = await api.activity(30);
      activityEvents = Array.isArray(data) ? data : [];
    } catch {
      activityEvents = [];
    } finally {
      activityLoading = false;
    }
  }

  function activityIcon(event) {
    const t = event.event_type ?? event.event ?? event.type ?? '';
    if (t.includes('spec') && t.includes('approv')) return '✓';
    if (t.includes('spec') && t.includes('reject')) return '✗';
    if (t.includes('spec')) return '📋';
    if (t.includes('task')) return '☑';
    if (t.includes('agent') && t.includes('spawn')) return '▶';
    if (t.includes('agent') && t.includes('complet')) return '⬛';
    if (t.includes('mr') && t.includes('merg')) return '🔀';
    if (t.includes('mr') && t.includes('creat')) return '📝';
    if (t.includes('gate')) return '🚦';
    if (t.includes('push')) return '⬆';
    if (t.includes('graph')) return '🔗';
    return '•';
  }

  function activityLabel(event) {
    const t = event.event_type ?? event.event ?? event.type ?? '';
    return t.replace(/_/g, ' ').replace(/\./g, ' ');
  }

  function activityVariant(event) {
    const t = event.event_type ?? event.event ?? event.type ?? '';
    if (t.includes('fail') || t.includes('reject')) return 'danger';
    if (t.includes('merg') || t.includes('approv') || t.includes('complet') || t.includes('pass')) return 'success';
    if (t.includes('spawn') || t.includes('enqueue') || t.includes('running')) return 'warning';
    return 'info';
  }

  // ── Derived: provenance summary counts ──────────────────────────────
  let provenanceSummary = $derived.by(() => {
    const approved = specs.filter(s => s.approval_status === 'approved' || s.status === 'approved').length;
    const pending = specs.filter(s => s.approval_status === 'pending' || s.status === 'pending').length;
    const activeAgentCount = wsAgents.filter(a => a.status === 'active').length;
    const mergedMrs = wsMrs.filter(m => m.status === 'merged').length;
    const openMrs = wsMrs.filter(m => m.status === 'open').length;
    const inProgressTasks = wsTasks.filter(t => t.status === 'in_progress').length;
    return { approved, pending, activeAgentCount, mergedMrs, openMrs, inProgressTasks, totalTasks: wsTasks.length };
  });

  // ── Load all data when workspace changes ───────────────────────────────
  $effect(() => {
    void workspace?.id;
    loadDecisions();
    loadRepos();
    loadSpecs();
    loadRules();
    loadTasks();
    loadMrs();
    loadAgents();
    loadActivity();
    loadBudget();
  });
</script>

<div class="workspace-home" data-testid="workspace-home">
  {#if !workspace}
    <!-- No workspace selected — prompt user to select or create one -->
    <div class="no-workspace">
      <div class="no-workspace-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="48" height="48">
          <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
          <polyline points="9 22 9 12 15 12 15 22"/>
        </svg>
      </div>
      <h2 class="no-workspace-title">{$t('workspace_home.select_workspace')}</h2>
      <p class="no-workspace-desc">{$t('workspace_home.select_workspace_desc')}</p>
      <button
        class="create-ws-btn"
        onclick={() => { createWsForm = { name: '', description: '' }; createWsOpen = true; }}
        data-testid="create-workspace-btn"
      >
        {$t('workspace_home.new_workspace')}
      </button>
    </div>
  {:else}
    <div class="sections">

      <!-- ── Decisions ─────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-decisions" data-testid="section-decisions">
        <div class="section-header">
          <h2 class="section-title" id="section-decisions">
            {$t('workspace_home.sections.decisions')}
            {#if notifications.length > 0}
              <span class="section-badge" aria-label={$t('workspace_home.decisions_badge_label', { values: { count: notifications.length } })}>{notifications.length}</span>
            {/if}
          </h2>
          {#if notifications.length > 0}
            <button class="section-action-btn" onclick={() => { showAllDecisions = !showAllDecisions; }}>{showAllDecisions ? $t('workspace_home.show_less') : $t('workspace_home.view_all')}</button>
          {/if}
        </div>
        <div class="section-body">
          {#if decisionsLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if decisionsError}
            <div class="error-row" role="alert">
              <p class="error-text">{decisionsError}</p>
              <button class="retry-btn" onclick={loadDecisions} aria-label={$t('workspace_home.retry_loading_decisions')}>{$t('common.retry')}</button>
            </div>
          {:else if notifications.length === 0}
            <p class="empty-text" data-testid="decisions-empty">{$t('workspace_home.decisions_empty')}</p>
          {:else}
            <ul class="decision-list" role="list">
              {#each (showAllDecisions ? notifications : notifications.slice(0, 5)) as n (n.id)}
                {@const body = getBody(n)}
                {@const state = actionStates[n.id] ?? {}}
                <li class="decision-item" data-testid="decision-item">
                  <span class="decision-icon" aria-hidden="true">{TYPE_ICONS[n.notification_type] ?? '•'}</span>
                  <div class="decision-content">
                    <span class="decision-type">{typeLabel(n.notification_type)}</span>
                    <span class="decision-desc">{n.message ?? n.description ?? body.description ?? ''}</span>
                    {#if n.repo_id && repoMap[n.repo_id]}
                      <button
                        class="decision-repo-link"
                        onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[n.repo_id], 'decisions'); }}
                        aria-label={$t('workspace_home.go_to_repo_decisions', { values: { name: repoMap[n.repo_id].name } })}
                      >{repoMap[n.repo_id].name}</button>
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
                          aria-label={$t('common.approve')}
                        >{$t('common.approve')}</button>
                        <button
                          class="inline-btn reject"
                          onclick={() => handleRejectSpec(n)}
                          data-testid="btn-reject"
                          aria-label={$t('common.reject')}
                        >{$t('common.reject')}</button>
                      {:else if n.notification_type === 'gate_failure' && body.mr_id}
                        <button
                          class="inline-btn"
                          onclick={() => handleRetry(n)}
                          data-testid="btn-retry"
                          aria-label={$t('common.retry')}
                        >{$t('common.retry')}</button>
                      {/if}
                      <button
                        class="inline-btn secondary"
                        onclick={() => handleDismiss(n)}
                        data-testid="btn-dismiss"
                        aria-label={$t('common.dismiss')}
                      >{$t('common.dismiss')}</button>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>

      <!-- ── Provenance Overview ───────────────────────────────────────── -->
      {#if !specsLoading && !tasksLoading && !agentsLoading && !mrsLoading && (specs.length > 0 || wsTasks.length > 0 || wsAgents.length > 0 || wsMrs.length > 0)}
        <section class="home-section provenance-overview-section" aria-labelledby="section-overview" data-testid="section-overview">
          <div class="section-header">
            <h2 class="section-title" id="section-overview">Development Flow</h2>
          </div>
          <div class="section-body">
            <div class="prov-flow-bar">
              <button class="prov-flow-node prov-flow-clickable" onclick={() => document.getElementById('section-specs')?.scrollIntoView({ behavior: 'smooth' })} title="Jump to Specs section">
                <span class="prov-flow-count">{provenanceSummary.approved + provenanceSummary.pending}</span>
                <span class="prov-flow-label">Specs</span>
                {#if provenanceSummary.pending > 0}
                  <span class="prov-flow-sub prov-sub-pending">{provenanceSummary.pending} pending</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => document.getElementById('section-tasks')?.scrollIntoView({ behavior: 'smooth' })} title="Jump to Tasks section">
                <span class="prov-flow-count">{provenanceSummary.totalTasks}</span>
                <span class="prov-flow-label">Tasks</span>
                {#if provenanceSummary.inProgressTasks > 0}
                  <span class="prov-flow-sub prov-sub-active">{provenanceSummary.inProgressTasks} in progress</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => document.getElementById('section-agents')?.scrollIntoView({ behavior: 'smooth' })} title="Jump to Agents section">
                <span class="prov-flow-count">{wsAgents.length}</span>
                <span class="prov-flow-label">Agents</span>
                {#if provenanceSummary.activeAgentCount > 0}
                  <span class="prov-flow-sub prov-sub-active">{provenanceSummary.activeAgentCount} active</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => document.getElementById('section-mrs')?.scrollIntoView({ behavior: 'smooth' })} title="Jump to MRs section">
                <span class="prov-flow-count">{wsMrs.length}</span>
                <span class="prov-flow-label">MRs</span>
                {#if provenanceSummary.openMrs > 0}
                  <span class="prov-flow-sub prov-sub-pending">{provenanceSummary.openMrs} open</span>
                {/if}
                {#if provenanceSummary.mergedMrs > 0}
                  <span class="prov-flow-sub prov-sub-merged">{provenanceSummary.mergedMrs} merged</span>
                {/if}
              </button>
            </div>
          </div>
        </section>
      {/if}

      <!-- ── Recent Activity ─────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-activity" data-testid="section-activity">
        <div class="section-header">
          <h2 class="section-title" id="section-activity">Recent Activity
            {#if !activityLoading && activityEvents.length > 0}
              <span class="section-badge">{activityEvents.length}</span>
            {/if}
          </h2>
        </div>
        <div class="section-body">
          {#if activityLoading}
            <div class="skeleton-row"></div>
          {:else if activityEvents.length === 0}
            <p class="empty-text">No recent activity.</p>
          {:else}
            <div class="activity-timeline">
              {#each activityEvents.slice(0, 15) as event, i}
                {@const variant = activityVariant(event)}
                <div class="activity-item">
                  <div class="activity-dot activity-dot-{variant}"></div>
                  {#if i < Math.min(activityEvents.length, 15) - 1}<div class="activity-line"></div>{/if}
                  <div class="activity-content">
                    <span class="activity-icon">{activityIcon(event)}</span>
                    <span class="activity-label">{activityLabel(event)}</span>
                    {#if event.entity_name ?? event.title ?? event.description}
                      <span class="activity-detail">{event.entity_name ?? event.title ?? event.description}</span>
                    {/if}
                    {#if event.entity_id && event.entity_type}
                      <button class="ws-entity-link activity-entity-link" onclick={() => openDetailPanel?.({ type: event.entity_type, id: event.entity_id, data: event })} title="View {event.entity_type}">{entityName(event.entity_type, event.entity_id)}</button>
                    {:else}
                      {@const evtStr = (event.event_type ?? event.type ?? '').toLowerCase()}
                      {#if event.agent_id}
                        <button class="ws-entity-link activity-entity-link" onclick={() => openDetailPanel?.({ type: 'agent', id: event.agent_id, data: {} })} title="View agent">{entityName('agent', event.agent_id)}</button>
                      {/if}
                      {#if event.mr_id}
                        <button class="ws-entity-link activity-entity-link" onclick={() => openDetailPanel?.({ type: 'mr', id: event.mr_id, data: {} })} title="View MR">{entityName('mr', event.mr_id)}</button>
                      {/if}
                      {#if event.task_id && !event.agent_id}
                        <button class="ws-entity-link activity-entity-link" onclick={() => openDetailPanel?.({ type: 'task', id: event.task_id, data: {} })} title="View task">{entityName('task', event.task_id)}</button>
                      {/if}
                      {#if event.spec_path && !event.agent_id && !event.mr_id}
                        <button class="ws-entity-link activity-entity-link" onclick={() => openDetailPanel?.({ type: 'spec', id: event.spec_path, data: { path: event.spec_path, repo_id: event.repo_id } })} title="View spec">{event.spec_path.split('/').pop()}</button>
                      {/if}
                    {/if}
                    {#if event.timestamp ?? event.created_at}
                      <span class="activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </section>

      <!-- ── Repos ─────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-repos" data-testid="section-repos">
        <div class="section-header">
          <h2 class="section-title" id="section-repos">{$t('workspace_home.sections.repos')}</h2>
        </div>
        <div class="section-body">
          {#if reposLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if reposError}
            <div class="error-row" role="alert">
              <p class="error-text">{reposError}</p>
              <button class="retry-btn" onclick={loadRepos} aria-label={$t('workspace_home.retry_loading_repos')}>{$t('common.retry')}</button>
            </div>
          {:else if repos.length === 0}
            <p class="empty-text" data-testid="repos-empty">{$t('workspace_home.repos_empty')}</p>
          {:else}
            <ul class="repo-list" role="list">
              {#each repos as repo (repo.id)}
                {@const health = repoHealth(repo)}
                <li class="repo-row" data-testid="repo-row">
                  <button
                    class="repo-btn"
                    onclick={() => onSelectRepo?.(repo)}
                    aria-label={$t('workspace_home.open_repo', { values: { name: repo.name } })}
                    data-testid="repo-link"
                  >
                    <span class="repo-name">{repo.name}</span>
                    <span class="repo-meta">
                      {#if (repo.active_spec_count ?? 0) > 0}
                        <span class="repo-stat">{$t('workspace_home.repo_specs_active', { values: { count: repo.active_spec_count } })}</span>
                      {/if}
                      {#if (repo.active_agents ?? 0) > 0}
                        <span class="repo-stat">{$t('workspace_home.repo_agents_count', { values: { count: repo.active_agents } })}</span>
                      {/if}
                    </span>
                    <span class="repo-health health-{health}" aria-label={$t('workspace_home.repo_status', { values: { status: health } })} data-testid="repo-health">
                      {#if health === 'healthy'}● {$t('workspace_home.repo_health_healthy')}
                      {:else if health === 'gate'}⚠ {$t('workspace_home.repo_health_gate')}
                      {:else}○ {$t('workspace_home.repo_health_idle')}
                      {/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="repo-actions">
            <button
              class="section-btn"
              data-testid="btn-new-repo"
              onclick={() => { newRepoOpen = !newRepoOpen; importOpen = false; }}
              aria-expanded={newRepoOpen}
            >{$t('workspace_home.new_repo')}</button>
            <button
              class="section-btn"
              data-testid="btn-import-repo"
              onclick={() => { importOpen = !importOpen; importName = ''; newRepoOpen = false; }}
              aria-expanded={importOpen}
            >{$t('workspace_home.import')}</button>
          </div>

          {#if newRepoOpen}
            <form
              class="inline-form"
              data-testid="new-repo-form"
              onsubmit={(e) => { e.preventDefault(); handleCreateRepo(); }}
            >
              <div class="inline-form-header">
                <span class="inline-form-title">{$t('workspace_home.new_repo_title')}</span>
                <button type="button" class="inline-form-close" onclick={() => { newRepoOpen = false; newRepoError = null; }} aria-label="{$t('common.cancel')}">✕</button>
              </div>
              <label class="inline-form-label" for="new-repo-name">{$t('workspace_home.new_repo_name_label')} <span class="required" aria-hidden="true">*</span></label>
              <input
                id="new-repo-name"
                class="inline-form-input"
                data-testid="new-repo-name-input"
                type="text"
                placeholder={$t('workspace_home.new_repo_name_placeholder')}
                bind:value={newRepoName}
                required
                disabled={newRepoLoading}
              />
              <label class="inline-form-label" for="new-repo-desc">{$t('workspace_home.new_repo_desc_label')}</label>
              <input
                id="new-repo-desc"
                class="inline-form-input"
                data-testid="new-repo-description-input"
                type="text"
                placeholder={$t('workspace_home.new_repo_desc_placeholder')}
                bind:value={newRepoDescription}
                disabled={newRepoLoading}
              />
              {#if newRepoError}
                <p class="inline-form-error" role="alert" data-testid="new-repo-error">{newRepoError}</p>
              {/if}
              <div class="inline-form-actions">
                <button type="submit" class="section-btn primary" data-testid="new-repo-submit" disabled={newRepoLoading || !newRepoName.trim()}>
                  {newRepoLoading ? $t('workspace_home.new_repo_creating') : $t('workspace_home.new_repo_create')}
                </button>
                <button type="button" class="section-btn" onclick={() => { newRepoOpen = false; newRepoError = null; }}>{$t('common.cancel')}</button>
              </div>
            </form>
          {/if}

          {#if importOpen}
            <form
              class="inline-form"
              data-testid="import-repo-form"
              onsubmit={(e) => { e.preventDefault(); handleImportRepo(); }}
            >
              <div class="inline-form-header">
                <span class="inline-form-title">{$t('workspace_home.import_repo_title')}</span>
                <button type="button" class="inline-form-close" onclick={() => { importOpen = false; importError = null; importName = ''; }} aria-label="{$t('common.cancel')}">✕</button>
              </div>
              <label class="inline-form-label" for="import-url">{$t('workspace_home.import_url_label')} <span class="required" aria-hidden="true">*</span></label>
              <input
                id="import-url"
                class="inline-form-input"
                data-testid="import-url-input"
                type="url"
                placeholder={$t('workspace_home.import_url_placeholder')}
                bind:value={importUrl}
                required
                disabled={importLoading}
              />
              <label class="inline-form-label" for="import-name">{$t('workspace_home.import_name_label')}</label>
              <input
                id="import-name"
                class="inline-form-input"
                data-testid="import-name-input"
                type="text"
                placeholder={$t('workspace_home.import_name_placeholder')}
                bind:value={importName}
                disabled={importLoading}
              />
              {#if importError}
                <p class="inline-form-error" role="alert" data-testid="import-error">{importError}</p>
              {/if}
              <div class="inline-form-actions">
                <button type="submit" class="section-btn primary" data-testid="import-submit" disabled={importLoading || !importUrl.trim()}>
                  {importLoading ? $t('workspace_home.import_importing') : $t('workspace_home.import_submit')}
                </button>
                <button type="button" class="section-btn" onclick={() => { importOpen = false; importError = null; importName = ''; }}>{$t('common.cancel')}</button>
              </div>
            </form>
          {/if}
        </div>
      </section>

      <!-- ── Briefing ──────────────────────────────────────────────────── -->
      <section class="home-section home-section-briefing" aria-labelledby="section-briefing" data-testid="section-briefing">
        <div class="section-header">
          <h2 class="section-title" id="section-briefing">{$t('workspace_home.sections.briefing')}</h2>
        </div>
        <div class="section-body section-body-briefing">
          <Briefing workspaceId={workspace.id} scope="workspace" workspaceName={workspace.name} />
        </div>
      </section>

      <!-- ── Specs (§2: cross-repo spec overview) ────────────────────── -->
      <section class="home-section" aria-labelledby="section-specs" data-testid="section-specs">
        <div class="section-header">
          <h2 class="section-title" id="section-specs">{$t('workspace_home.sections.specs')}</h2>
          <div class="header-controls">
            <select
              class="filter-select"
              value={specsStatusFilter}
              onchange={(e) => { specsStatusFilter = e.target.value; }}
              aria-label={$t('workspace_home.filter_specs_by_status')}
              data-testid="specs-status-filter"
            >
              <option value="">{$t('workspace_home.all_statuses')}</option>
              <option value="draft">{$t('workspace_home.status_draft')}</option>
              <option value="pending">{$t('workspace_home.status_pending')}</option>
              <option value="approved">{$t('workspace_home.status_approved')}</option>
              <option value="rejected">Rejected</option>
              <option value="implemented">{$t('workspace_home.status_implemented')}</option>
            </select>
          </div>
        </div>
        <div class="section-body">
          {#if specsLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if specsError}
            <div class="error-row" role="alert">
              <p class="error-text">{specsError}</p>
              <button class="retry-btn" onclick={loadSpecs} aria-label={$t('workspace_home.retry_loading_specs')}>{$t('common.retry')}</button>
            </div>
          {:else if filteredSpecs.length === 0}
            <p class="empty-text" data-testid="specs-empty">
              {specsStatusFilter ? $t('workspace_home.specs_no_status') : $t('workspace_home.specs_empty')}
            </p>
          {:else}
            <table class="specs-table" data-testid="specs-table">
              <thead>
                <tr>
                  <th scope="col" aria-sort={specsSortCol === 'repo' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                    <button class="sort-btn" onclick={() => toggleSpecsSort('repo')}>{$t('workspace_home.sections.repos')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('repo')}</span></button>
                  </th>
                  <th scope="col" aria-sort={specsSortCol === 'path' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                    <button class="sort-btn" onclick={() => toggleSpecsSort('path')}>{$t('workspace_home.col_path')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('path')}</span></button>
                  </th>
                  <th scope="col" aria-sort={specsSortCol === 'status' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                    <button class="sort-btn" onclick={() => toggleSpecsSort('status')}>{$t('workspace_home.col_status')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('status')}</span></button>
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
                {#each filteredSpecs as spec (spec.id ?? spec.path)}
                  <tr
                    class="spec-row"
                    onclick={() => navigateToSpec(spec)}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') navigateToSpec(spec); }}
                    data-testid="spec-row"
                    aria-label={$t('workspace_home.open_spec', { values: { path: spec.path } })}
                  >
                    <td class="spec-repo">{repoMap[spec.repo_id]?.name ?? spec.repo_id ?? '—'}</td>
                    <td class="spec-path">{spec.path}</td>
                    <td class="spec-status" title={specStatusTooltip(spec.approval_status ?? spec.status)}>
                      <span class="status-icon" aria-hidden="true">{SPEC_STATUS_ICONS[spec.approval_status ?? spec.status] ?? '•'}</span>
                      {spec.approval_status ?? spec.status ?? '—'}
                    </td>
                    <td class="spec-progress">
                      {#if spec.tasks_total != null && spec.tasks_total > 0}
                        {@const pct = Math.round(((spec.tasks_done ?? 0) / spec.tasks_total) * 100)}
                        <div class="progress-cell" title="{spec.tasks_done ?? 0} of {spec.tasks_total} tasks done ({pct}%)">
                          <span class="progress-text">{spec.tasks_done ?? 0}/{spec.tasks_total}</span>
                          <div class="progress-mini-bar">
                            <div class="progress-mini-fill" class:progress-complete={pct === 100} style="width: {pct}%"></div>
                          </div>
                        </div>
                      {:else if spec.tasks_total != null}
                        <span class="secondary">0/0</span>
                      {:else}
                        <span class="secondary">—</span>
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

      <!-- ── Architecture (collapsible) ──────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-architecture" data-testid="section-architecture">
        <button
          class="arch-toggle-header"
          onclick={toggleArch}
          aria-expanded={archExpanded}
          aria-controls="arch-body"
          data-testid="arch-toggle"
        >
          <h2 class="section-title" id="section-architecture">{$t('workspace_home.sections.architecture')}</h2>
          <span class="arch-toggle-label" aria-hidden="true">
            {archExpanded ? '▾ ' + $t('workspace_home.hide_workspace_graph') : '▸ ' + $t('workspace_home.show_workspace_graph')}
          </span>
        </button>
        {#if archExpanded}
          <div class="section-body arch-body" id="arch-body" data-testid="arch-body">
            {#if archLoading}
              <div class="skeleton-row"></div>
              <div class="skeleton-row"></div>
            {:else if archError}
              <div class="error-row" role="alert">
                <p class="error-text">{archError}</p>
                <button class="retry-btn" onclick={loadArchGraph} aria-label={$t('workspace_home.retry_loading_graph')}>{$t('common.retry')}</button>
              </div>
            {:else if archGraph}
              <div class="arch-canvas-wrap" data-testid="arch-canvas">
                <ExplorerCanvas
                  nodes={archGraph.nodes ?? []}
                  edges={archGraph.edges ?? []}
                  workspaceId={workspace.id}
                  scope="workspace"
                />
              </div>
            {/if}
          </div>
        {/if}
      </section>

      <!-- ── Tasks ──────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-tasks" data-testid="section-tasks">
        <div class="section-header">
          <h2 class="section-title" id="section-tasks">Tasks
            {#if !tasksLoading && wsTasks.length > 0}
              <span class="section-badge">{wsTasks.length}</span>
            {/if}
          </h2>
        </div>
        <div class="section-body">
          {#if tasksLoading}
            <div class="skeleton-row"></div>
          {:else if wsTasks.length === 0}
            <p class="empty-text">No tasks in this workspace yet.</p>
          {:else}
            <table class="ws-entity-table">
              <thead>
                <tr>
                  <th>Status</th>
                  <th>Title</th>
                  <th>Priority</th>
                  <th>Type</th>
                  <th>Spec</th>
                  <th>Agent</th>
                  <th>Repo</th>
                </tr>
              </thead>
              <tbody>
                {#each wsTasks.slice(0, 10) as task}
                  <tr class="ws-entity-row" onclick={() => openDetailPanel?.({ type: 'task', id: task.id, data: task })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'task', id: task.id, data: task }); }}>
                    <td><span class="status-badge status-{task.status ?? 'backlog'}" title={taskStatusTooltip(task.status)}>{task.status ?? 'backlog'}</span></td>
                    <td class="ws-cell-title">{task.title ?? 'Untitled'}</td>
                    <td>{#if task.priority}<span class="priority-badge priority-{task.priority}">{task.priority}</span>{/if}</td>
                    <td class="ws-cell-type">{task.task_type ?? ''}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if task.spec_path}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'spec', id: task.spec_path, data: { path: task.spec_path, repo_id: task.repo_id } }); }} title={task.spec_path}>{task.spec_path.split('/').pop()}</button>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if task.assigned_to}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'agent', id: task.assigned_to, data: {} }); }} title={task.assigned_to}>{entityName('agent', task.assigned_to)}</button>{/if}</td>
                    <td class="ws-cell-mono">{repoMap[task.repo_id]?.name ?? ''}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if wsTasks.length > 10}
              <p class="show-more-hint">{wsTasks.length - 10} more tasks not shown</p>
            {/if}
          {/if}
        </div>
      </section>

      <!-- ── Merge Requests ──────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-mrs" data-testid="section-mrs">
        <div class="section-header">
          <h2 class="section-title" id="section-mrs">Merge Requests
            {#if !mrsLoading && wsMrs.length > 0}
              <span class="section-badge">{wsMrs.length}</span>
            {/if}
          </h2>
        </div>
        <div class="section-body">
          {#if mrsLoading}
            <div class="skeleton-row"></div>
          {:else if wsMrs.length === 0}
            <p class="empty-text">No merge requests in this workspace yet.</p>
          {:else}
            <table class="ws-entity-table">
              <thead>
                <tr>
                  <th>Status</th>
                  <th>Title</th>
                  <th>Branch</th>
                  <th>Agent</th>
                  <th>Gates</th>
                  <th>Changes</th>
                  <th>Spec</th>
                  <th>Repo</th>
                </tr>
              </thead>
              <tbody>
                {#each wsMrs.slice(0, 10) as mr}
                  <tr class="ws-entity-row" onclick={() => openDetailPanel?.({ type: 'mr', id: mr.id, data: mr })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'mr', id: mr.id, data: mr }); }}>
                    <td><span class="status-badge status-{mr.queue_position != null ? 'queued' : (mr.status ?? 'open')}" title={mrStatusTooltip(mr)}>{mr.queue_position != null ? `queued #${mr.queue_position + 1}` : (mr.status ?? 'open')}</span></td>
                    <td class="ws-cell-title">{mr.title ?? 'Untitled MR'}</td>
                    <td class="ws-cell-mono"><span class="branch-ref">{mr.source_branch ?? ''}</span>{#if mr.target_branch}<span class="branch-arrow">→</span><span class="branch-ref">{mr.target_branch}</span>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if mr.author_agent_id}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'agent', id: mr.author_agent_id, data: {} }); }} title={mr.author_agent_id}>{entityName('agent', mr.author_agent_id)}</button>{/if}</td>
                    <td>
                      {#if mr._gates?.total > 0}
                        <div class="gate-cell-ws" title={mr._gates.details?.map(g => `${g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'} ${g.name}${g.required === false ? ' (advisory)' : ''}`).join('\n') ?? ''}>
                          <span class="gate-summary-inline">
                            {#if mr._gates.failed > 0}<span class="gate-fail-inline">✗{mr._gates.failed}</span>{/if}
                            {#if mr._gates.passed > 0}<span class="gate-pass-inline">✓{mr._gates.passed}</span>{/if}
                            {#if mr._gates.total - mr._gates.passed - mr._gates.failed > 0}<span class="gate-pending-inline">○{mr._gates.total - mr._gates.passed - mr._gates.failed}</span>{/if}
                          </span>
                          {#if mr._gates.details?.length > 0}
                            <span class="gate-names-ws">
                              {#each mr._gates.details as g}
                                <span class="gate-name-tag-ws gate-tag-{g.status}">{g.name}</span>
                              {/each}
                            </span>
                          {/if}
                        </div>
                      {/if}
                    </td>
                    <td class="ws-cell-diff">
                      {#if mr.diff_stats}
                        <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      {/if}
                    </td>
                    <td class="ws-cell-mono ws-cell-link">
                      {#if mr.spec_ref}
                        {@const specPath = mr.spec_ref.split('@')[0]}
                        <button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'spec', id: specPath, data: { path: specPath, repo_id: mr.repository_id ?? mr.repo_id } }); }} title={mr.spec_ref}>{specPath.split('/').pop()}</button>
                      {/if}
                    </td>
                    <td class="ws-cell-mono">{repoMap[mr.repository_id]?.name ?? ''}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if wsMrs.length > 10}
              <p class="show-more-hint">{wsMrs.length - 10} more MRs not shown</p>
            {/if}
          {/if}
        </div>
      </section>

      <!-- ── Agents ──────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-agents" data-testid="section-agents">
        <div class="section-header">
          <h2 class="section-title" id="section-agents">Agents
            {#if !agentsLoading && wsAgents.length > 0}
              <span class="section-badge">{wsAgents.length}</span>
            {/if}
          </h2>
        </div>
        <div class="section-body">
          {#if agentsLoading}
            <div class="skeleton-row"></div>
          {:else if wsAgents.length === 0}
            <p class="empty-text">No agents in this workspace.</p>
          {:else}
            <table class="ws-entity-table">
              <thead>
                <tr>
                  <th>Status</th>
                  <th>Name</th>
                  <th>Task</th>
                  <th>Branch</th>
                  <th>Duration</th>
                  <th>MR</th>
                  <th>Repo</th>
                </tr>
              </thead>
              <tbody>
                {#each wsAgents.slice(0, 10) as agent}
                  {@const taskId = agent.task_id ?? agent.current_task_id}
                  {@const spawnedAt = agent.created_at ?? agent.spawned_at}
                  <tr class="ws-entity-row" onclick={() => openDetailPanel?.({ type: 'agent', id: agent.id, data: agent })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'agent', id: agent.id, data: agent }); }}>
                    <td><span class="status-badge status-{agent.status ?? 'active'}" title={agentStatusTooltip(agent.status)}>{agent.status ?? 'active'}</span></td>
                    <td class="ws-cell-title">{agent.name ?? shortId(agent.id)}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if taskId}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'task', id: taskId, data: {} }); }} title={taskId}>{entityName('task', taskId)}</button>{/if}</td>
                    <td class="ws-cell-mono"><span class="branch-ref">{agent.branch ?? ''}</span></td>
                    <td class="ws-cell-time">{fmtDuration(spawnedAt, agent.completed_at)}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if agent.mr_id}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); openDetailPanel?.({ type: 'mr', id: agent.mr_id, data: {} }); }} title={agent.mr_id}>{entityName('mr', agent.mr_id)}</button>{/if}</td>
                    <td class="ws-cell-mono">{repoMap[agent.repo_id]?.name ?? ''}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if wsAgents.length > 10}
              <p class="show-more-hint">{wsAgents.length - 10} more agents not shown</p>
            {/if}
          {/if}
        </div>
      </section>

      <!-- ── Budget & Cost ──────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-budget" data-testid="section-budget">
        <div class="section-header">
          <h2 class="section-title" id="section-budget">Budget & Cost</h2>
        </div>
        <div class="section-body">
          {#if budgetLoading}
            <div class="skeleton-row"></div>
          {:else if budgetData || costData}
            <div class="budget-overview">
              {#if budgetData}
                {@const config = budgetData.config ?? budgetData}
                {@const usage = budgetData.usage ?? {}}
                <div class="budget-meters">
                  {#if config.max_concurrent_agents != null}
                    {@const activeCount = wsAgents.filter(a => a.status === 'active').length}
                    {@const pct = config.max_concurrent_agents > 0 ? Math.round((activeCount / config.max_concurrent_agents) * 100) : 0}
                    <div class="budget-meter">
                      <div class="budget-meter-header">
                        <span class="budget-meter-label">Concurrent Agents</span>
                        <span class="budget-meter-value">{activeCount} / {config.max_concurrent_agents}</span>
                      </div>
                      <div class="progress-bar-track" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
                        <div class="progress-bar-fill" class:progress-bar-warn={pct > 80} class:progress-bar-danger={pct > 95} style="width: {Math.min(pct, 100)}%"></div>
                      </div>
                    </div>
                  {/if}
                  {#if config.max_tokens_per_day != null}
                    {@const usedTokens = usage.tokens_today ?? 0}
                    {@const pct = config.max_tokens_per_day > 0 ? Math.round((usedTokens / config.max_tokens_per_day) * 100) : 0}
                    <div class="budget-meter">
                      <div class="budget-meter-header">
                        <span class="budget-meter-label">Tokens Today</span>
                        <span class="budget-meter-value">{usedTokens.toLocaleString()} / {config.max_tokens_per_day.toLocaleString()}</span>
                      </div>
                      <div class="progress-bar-track" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
                        <div class="progress-bar-fill" class:progress-bar-warn={pct > 80} class:progress-bar-danger={pct > 95} style="width: {Math.min(pct, 100)}%"></div>
                      </div>
                    </div>
                  {/if}
                  {#if config.max_cost_per_day != null}
                    {@const usedCost = usage.cost_today ?? 0}
                    {@const pct = config.max_cost_per_day > 0 ? Math.round((usedCost / config.max_cost_per_day) * 100) : 0}
                    <div class="budget-meter">
                      <div class="budget-meter-header">
                        <span class="budget-meter-label">Cost Today</span>
                        <span class="budget-meter-value">${usedCost.toFixed(2)} / ${config.max_cost_per_day.toFixed(2)}</span>
                      </div>
                      <div class="progress-bar-track" role="progressbar" aria-valuenow={pct} aria-valuemin="0" aria-valuemax="100">
                        <div class="progress-bar-fill" class:progress-bar-warn={pct > 80} class:progress-bar-danger={pct > 95} style="width: {Math.min(pct, 100)}%"></div>
                      </div>
                    </div>
                  {/if}
                </div>
              {:else}
                <p class="empty-text">No budget limits configured for this workspace.</p>
              {/if}
              {#if costData}
                {@const entries = Array.isArray(costData) ? costData : (costData.entries ?? costData.agents ?? [])}
                {#if entries.length > 0}
                  <div class="cost-breakdown">
                    <span class="progress-section-label">Cost by Agent</span>
                    <table class="entity-table entity-table-compact">
                      <thead>
                        <tr>
                          <th>Agent</th>
                          <th>Tokens</th>
                          <th>Cost</th>
                        </tr>
                      </thead>
                      <tbody>
                        {#each entries.slice(0, 5) as entry}
                          <tr class="entity-row" onclick={() => openDetailPanel?.({ type: 'agent', id: entry.agent_id, data: {} })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') openDetailPanel?.({ type: 'agent', id: entry.agent_id, data: {} }); }}>
                            <td class="cell-title">{entityName('agent', entry.agent_id)}</td>
                            <td>{(entry.total_tokens ?? entry.tokens ?? 0).toLocaleString()}</td>
                            <td>${(entry.total_cost ?? entry.cost ?? 0).toFixed(2)}</td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                {/if}
              {/if}
            </div>
          {:else}
            <p class="empty-text">No budget configured. Set limits in workspace settings to track agent resource usage.</p>
          {/if}
        </div>
      </section>

      <!-- ── Agent Rules ───────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-agent-rules" data-testid="section-agent-rules">
        <div class="section-header">
          <h2 class="section-title" id="section-agent-rules">{$t('workspace_home.sections.agent_rules')}</h2>
          <button
            class="section-action-btn"
            data-testid="manage-rules-link"
            onclick={() => goToAgentRules?.()}
          >{$t('workspace_home.manage_rules')}</button>
        </div>
        <div class="section-body">
          {#if rulesLoading}
            <div class="skeleton-row"></div>
          {:else if rulesError}
            <div class="error-row" role="alert">
              <p class="error-text">{rulesError}</p>
              <button class="retry-btn" onclick={loadRules} aria-label={$t('workspace_home.retry_loading_rules')}>{$t('common.retry')}</button>
            </div>
          {:else}
            <p class="rules-summary" data-testid="rules-summary">
              {$t('workspace_home.rules_summary', { values: { count: allMetaSpecs.length } })}
              {#if requiredMetaSpecs.length > 0}
                {$t('workspace_home.rules_summary_required', { values: { count: requiredMetaSpecs.length } })}
              {/if}
            </p>

            {#if recentlyUpdated.length > 0}
              <div class="reconcile-status" role="status" data-testid="reconcile-status">
                {$t('workspace_home.rules_reconciling', { values: { count: recentlyUpdated.length } })}
              </div>
            {/if}

            {#if requiredMetaSpecs.length > 0}
              <ul class="rules-list" role="list" data-testid="rules-list">
                {#each requiredMetaSpecs as ms (ms.id)}
                  <li class="rule-item" data-testid="rule-item">
                    <span class="rule-lock" aria-label={$t('workspace_home.rule_required_label')} aria-hidden="true">🔒</span>
                    <span class="rule-name">{ms.name}</span>
                    <span class="rule-scope" data-testid="rule-scope">{ms._scope === 'tenant' ? $t('workspace_home.scope_tenant') : $t('workspace_home.scope_workspace')}</span>
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
              <p class="empty-text">{$t('workspace_home.rules_no_metaspecs')}</p>
            {/if}
          {/if}
        </div>
      </section>

    </div>
  {/if}
</div>

<!-- Create Workspace modal -->
<Modal bind:open={createWsOpen} title={$t('workspace_home.create_ws_title')} size="sm">
  <div class="create-ws-form">
    <label class="create-ws-label">{$t('workspace_home.create_ws_name_label')}
      <input
        class="create-ws-input"
        bind:value={createWsForm.name}
        placeholder={$t('workspace_home.create_ws_name_placeholder')}
        onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspace()}
      />
    </label>
    <label class="create-ws-label">{$t('workspace_home.create_ws_desc_label')}
      <input
        class="create-ws-input"
        bind:value={createWsForm.description}
        placeholder={$t('workspace_home.create_ws_desc_placeholder')}
        onkeydown={(e) => e.key === 'Enter' && handleCreateWorkspace()}
      />
    </label>
    <div class="create-ws-actions">
      <button class="create-ws-cancel" onclick={() => (createWsOpen = false)}>{$t('workspace_home.create_ws_cancel')}</button>
      <button
        class="create-ws-submit"
        onclick={handleCreateWorkspace}
        disabled={createWsSaving || !createWsForm.name?.trim()}
      >
        {createWsSaving ? $t('workspace_home.create_ws_creating') : $t('workspace_home.create_ws_submit')}
      </button>
    </div>
  </div>
</Modal>

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

  .create-ws-btn {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
    margin-top: var(--space-2);
  }

  .create-ws-btn:hover { background: var(--color-primary-hover); }

  .create-ws-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Create Workspace modal form ───────────────────────────────────── */
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

  .section-action-btn {
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    padding: 0;
  }

  .section-action-btn:hover {
    text-decoration: underline;
  }

  .section-action-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
  .error-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .error-text {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .retry-btn {
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .retry-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .decision-repo-link {
    font-size: var(--text-xs);
    color: var(--color-link, var(--color-primary));
    font-family: var(--font-mono);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
    text-align: left;
  }

  .decision-repo-link:hover {
    color: var(--color-link-hover, var(--color-primary));
  }

  .decision-repo-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
    padding: 0;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-2);
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

  .progress-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 50px;
  }

  .progress-text {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
  }

  .progress-mini-bar {
    height: 3px;
    background: var(--color-surface-elevated);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-mini-fill {
    height: 100%;
    background: var(--color-warning);
    border-radius: 2px;
    transition: width var(--transition-fast);
  }
  .progress-mini-fill.progress-complete { background: var(--color-success); }

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

  .rule-scope {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--color-info) 25%, transparent);
    text-transform: capitalize;
    flex-shrink: 0;
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

  /* ── Architecture ──────────────────────────────────────────────────── */
  .arch-toggle-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    font-family: var(--font-body);
    text-align: left;
    gap: var(--space-2);
    transition: background var(--transition-fast);
  }

  .arch-toggle-header:hover {
    background: color-mix(in srgb, var(--color-surface-elevated) 80%, var(--color-border));
  }

  .arch-toggle-header:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* When not expanded, remove bottom border (section has no body) */
  .arch-toggle-header[aria-expanded="false"] {
    border-bottom: none;
  }

  .arch-toggle-label {
    font-size: var(--text-xs);
    color: var(--color-primary);
    flex-shrink: 0;
    font-family: var(--font-body);
  }

  .arch-body {
    padding: 0;
  }

  .arch-canvas-wrap {
    height: 320px;
    position: relative;
    overflow: hidden;
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

  .section-btn:hover:not(:disabled) {
    background: var(--color-surface);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .section-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .section-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .section-btn.primary {
    background: var(--color-primary);
    border-color: var(--color-primary);
    color: var(--color-text-inverse);
  }

  .section-btn.primary:hover:not(:disabled) {
    background: var(--color-primary-hover, var(--color-primary));
    border-color: var(--color-primary-hover, var(--color-primary));
    color: var(--color-text-inverse);
  }

  /* ── Inline forms ───────────────────────────────────────────────────── */
  .inline-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
  }

  .inline-form-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-1);
  }

  .inline-form-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .inline-form-close {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    line-height: 1;
    padding: 2px var(--space-1);
    border-radius: var(--radius);
  }

  .inline-form-close:hover {
    color: var(--color-text);
    background: var(--color-surface);
  }

  .inline-form-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .required {
    color: var(--color-danger);
  }

  .inline-form-input {
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    width: 100%;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .inline-form-input:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .inline-form-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .inline-form-error {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-danger);
  }

  .inline-form-actions {
    display: flex;
    gap: var(--space-2);
    padding-top: var(--space-1);
  }

  /* ── Entity tables (Tasks, MRs, Agents) ──────────────────────────────── */
  .ws-entity-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .ws-entity-table thead th {
    text-align: left;
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .ws-entity-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .ws-entity-row:hover {
    background: var(--color-surface-elevated);
  }

  .ws-entity-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .ws-entity-table td {
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .ws-cell-title {
    font-weight: 500;
    color: var(--color-text);
    max-width: 250px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-cell-mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .ws-cell-type {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .status-badge {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  .status-done, .status-merged, .status-completed, .status-active {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .status-in_progress, .status-open, .status-running {
    background: color-mix(in srgb, var(--color-info) 15%, transparent);
    color: var(--color-info);
  }

  .status-queued, .status-enqueued {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .status-blocked, .status-failed, .status-closed, .status-dead {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  .status-idle, .status-stopped {
    background: color-mix(in srgb, var(--color-info) 15%, transparent);
    color: var(--color-info);
  }

  .status-review {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .status-backlog, .status-review, .status-pending {
    background: color-mix(in srgb, var(--color-text-muted) 15%, transparent);
    color: var(--color-text-muted);
  }

  .priority-badge {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
  }

  .priority-high, .priority-critical {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  .priority-low {
    background: color-mix(in srgb, var(--color-text-muted) 10%, transparent);
    color: var(--color-text-muted);
  }

  /* ── Entity link buttons in tables ──────────────────────────────────── */
  .ws-entity-link {
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
    max-width: 120px;
    display: inline-block;
    vertical-align: middle;
    text-align: left;
  }

  .ws-entity-link:hover {
    text-decoration: underline;
    color: var(--color-primary);
  }

  .ws-entity-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
    border-radius: var(--radius-sm);
  }

  .ws-cell-link {
    max-width: 130px;
  }

  .ws-cell-diff {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .ws-cell-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .diff-ins {
    color: var(--color-success);
    font-weight: 600;
  }

  .diff-del {
    color: var(--color-danger);
    font-weight: 600;
    margin-left: var(--space-1);
  }

  .gate-summary-inline {
    display: inline-flex;
    gap: var(--space-1);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .gate-pass-inline { color: var(--color-success); font-weight: 600; }
  .gate-fail-inline { color: var(--color-danger); font-weight: 600; }
  .gate-pending-inline { color: var(--color-text-muted); }

  .gate-cell-ws { display: flex; flex-direction: column; gap: 2px; }
  .gate-names-ws { display: flex; flex-wrap: wrap; gap: 2px; }
  .gate-name-tag-ws {
    font-size: 10px;
    padding: 0 3px;
    border-radius: var(--radius);
    white-space: nowrap;
    line-height: 1.4;
  }
  .gate-tag-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-tag-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-tag-pending { color: var(--color-text-muted); background: var(--color-surface-elevated); }

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

  .show-more-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
    margin: var(--space-2) 0 0;
    font-style: italic;
  }

  @media (max-width: 768px) {
    .workspace-home {
      padding: var(--space-4);
    }

    .spec-progress,
    .spec-activity {
      display: none;
    }
  }

  /* ── Provenance Flow Bar ───────────────────────────────────────────── */
  .prov-flow-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-2);
    flex-wrap: wrap;
  }

  .prov-flow-node {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    min-width: 70px;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .prov-flow-clickable {
    cursor: pointer;
    font-family: inherit;
    transition: border-color var(--transition-fast), background var(--transition-fast), box-shadow var(--transition-fast);
  }

  .prov-flow-clickable:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 5%, var(--color-surface-elevated));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .prov-flow-clickable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .prov-flow-count {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-display);
  }

  .prov-flow-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
  }

  .prov-flow-sub {
    font-size: 10px;
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .prov-sub-pending { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 12%, transparent); }
  .prov-sub-active { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 12%, transparent); }
  .prov-sub-merged { color: var(--color-info); background: color-mix(in srgb, var(--color-info) 12%, transparent); }

  .prov-flow-arrow {
    font-size: var(--text-lg);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  /* ── Activity Timeline ───────────────────────────────────────────── */
  .activity-timeline {
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: var(--space-2) 0;
  }

  .activity-item {
    display: flex;
    position: relative;
    padding-left: 24px;
    min-height: 32px;
  }

  .activity-dot {
    position: absolute;
    left: 6px;
    top: 6px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    z-index: 1;
    border: 2px solid var(--color-surface);
    background: var(--color-text-muted);
  }

  .activity-dot-success { background: var(--color-success); }
  .activity-dot-danger { background: var(--color-danger); }
  .activity-dot-warning { background: var(--color-warning); }
  .activity-dot-info { background: var(--color-info); }

  .activity-line {
    position: absolute;
    left: 10px;
    top: 18px;
    bottom: -4px;
    width: 2px;
    background: var(--color-border);
  }

  .activity-content {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding: 2px 0 var(--space-2) var(--space-2);
    font-size: var(--text-xs);
    flex-wrap: wrap;
    min-width: 0;
  }

  .activity-icon {
    flex-shrink: 0;
    width: 16px;
    text-align: center;
  }

  .activity-label {
    color: var(--color-text-secondary);
    font-weight: 500;
    text-transform: capitalize;
  }

  .activity-detail {
    color: var(--color-text);
    font-weight: 500;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-entity-link {
    font-size: var(--text-xs);
  }

  .activity-time {
    color: var(--color-text-muted);
    font-size: 10px;
    white-space: nowrap;
    margin-left: auto;
  }

  /* ── Budget & Cost ──────────────────────────────────────────────────── */
  .budget-overview {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .budget-meters {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: var(--space-4);
  }

  .budget-meter {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .budget-meter-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .budget-meter-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  .budget-meter-value {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .progress-bar-track {
    height: 6px;
    background: var(--color-border);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: 3px;
    transition: width var(--transition-normal);
  }

  .progress-bar-fill.progress-bar-warn {
    background: var(--color-warning);
  }

  .progress-bar-fill.progress-bar-danger {
    background: var(--color-danger);
  }

  .cost-breakdown {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .entity-table-compact {
    font-size: var(--text-sm);
  }

  .entity-table-compact th,
  .entity-table-compact td {
    padding: var(--space-1) var(--space-2);
  }

  .progress-section-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  @media (prefers-reduced-motion: reduce) {
    .skeleton-row { animation: none; }
    .inline-btn, .section-btn, .repo-btn, .filter-select, .retry-btn { transition: none; }
  }
</style>
