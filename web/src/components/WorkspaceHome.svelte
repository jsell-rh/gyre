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
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  /** Navigate to full-page entity detail view (falls back to side panel) */
  function nav(type, id, data) {
    if (goToEntityDetail) {
      goToEntityDetail(type, id, data ?? {});
    } else if (openDetailPanel) {
      openDetailPanel({ type, id, data: data ?? {} });
    }
  }

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
      case 'open': {
        if (mr._gates?.failed > 0) return `MR blocked — ${mr._gates.failed} gate(s) failed: ${mr._gates.details?.filter(g => g.status === 'failed').map(g => g.name).join(', ') ?? 'unknown'}`;
        if (mr.has_conflicts) return 'MR has merge conflicts with the target branch';
        return 'MR is open and ready to be enqueued for merge';
      }
      case 'merged': {
        const parts = ['MR passed all required gates and was merged'];
        if (mr.merge_commit_sha) parts.push(`commit ${mr.merge_commit_sha.slice(0, 7)}`);
        if (mr._gates?.total > 0) parts.push(`${mr._gates.passed}/${mr._gates.total} gates passed`);
        return parts.join(' — ');
      }
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
  let archExpanded = $state(true);
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

  // ── Audit Log state ─────────────────────────────────────────────────
  let auditLoading = $state(false);
  let auditEvents = $state([]);

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
      let raw = await api.myNotifications();
      let data = Array.isArray(raw) ? raw : (raw?.notifications ?? []);
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
      // The API enriches gate_name, gate_type, required, command from definitions
      const toEnrich = mrList.slice(0, 10);
      const gatePromises = toEnrich.map(mr =>
        api.mrGates(mr.id).then(gates => {
          const arr = Array.isArray(gates) ? gates : (gates?.gates ?? []);
          const passed = arr.filter(g => g.status === 'Passed' || g.status === 'passed').length;
          const failed = arr.filter(g => g.status === 'Failed' || g.status === 'failed').length;
          const details = arr.map(g => {
            const gateType = (g.gate_type ?? '').replace(/_/g, ' ');
            return {
              name: g.gate_name ?? g.name ?? (gateType || 'Quality gate'),
              status: (g.status === 'Passed' || g.status === 'passed') ? 'passed' : (g.status === 'Failed' || g.status === 'failed') ? 'failed' : 'pending',
              gate_type: g.gate_type,
              required: g.required,
            };
          });
          return { id: mr.id, passed, failed, total: arr.length, details };
        }).catch(() => ({ id: mr.id, passed: 0, failed: 0, total: 0, details: [] }))
      );
      const gateResults = await Promise.all(gatePromises);
      const gateMap = Object.fromEntries(gateResults.map(g => [g.id, g]));
      wsMrs = mrList.map(mr => gateMap[mr.id] ? { ...mr, _gates: gateMap[mr.id] } : mr);
      // Enrich diff_stats for MRs that lack them (best-effort, parallel)
      const needDiff = wsMrs.filter(mr => !mr.diff_stats).slice(0, 10);
      if (needDiff.length > 0) {
        Promise.all(needDiff.map(mr =>
          api.mrDiff(mr.id).then(d => ({ id: mr.id, diff_stats: { files_changed: d?.files_changed ?? 0, insertions: d?.insertions ?? 0, deletions: d?.deletions ?? 0 } })).catch(() => null)
        )).then(results => {
          const diffMap = Object.fromEntries(results.filter(Boolean).map(r => [r.id, r.diff_stats]));
          if (Object.keys(diffMap).length > 0) {
            wsMrs = wsMrs.map(mr => diffMap[mr.id] ? { ...mr, diff_stats: diffMap[mr.id] } : mr);
          }
        });
      }
    } catch {
      wsMrs = [];
    } finally {
      mrsLoading = false;
    }
  }

  // ── MR actions ────────────────────────────────────────────────────────
  let enqueuingMrId = $state(null);
  async function quickEnqueueMr(mr, e) {
    e?.stopPropagation();
    if (enqueuingMrId) return;
    enqueuingMrId = mr.id;
    try {
      await api.enqueue(mr.id);
      toastSuccess(`MR "${mr.title ?? 'Untitled'}" enqueued for merge`);
      wsMrs = wsMrs.map(m => m.id === mr.id ? { ...m, queue_position: 0 } : m);
    } catch (err) {
      toastError('Enqueue failed: ' + (err.message ?? err));
    } finally {
      enqueuingMrId = null;
    }
  }

  // ── Task status transitions ──────────────────────────────────────────
  const WS_TASK_TRANSITIONS = {
    backlog: ['in_progress'],
    in_progress: ['done', 'blocked'],
    blocked: ['in_progress'],
    review: ['done'],
  };
  let changingWsTaskId = $state(null);
  async function quickChangeWsTaskStatus(task, newStatus, e) {
    e?.stopPropagation();
    if (changingWsTaskId) return;
    changingWsTaskId = task.id;
    try {
      await api.updateTaskStatus(task.id, newStatus);
      wsTasks = wsTasks.map(t => t.id === task.id ? { ...t, status: newStatus } : t);
    } catch (err) {
      toastError('Status change failed: ' + (err.message ?? err));
    } finally {
      changingWsTaskId = null;
    }
  }

  // ── Agents: load ──────────────────────────────────────────────────────
  async function loadAgents() {
    if (!workspace?.id) return;
    agentsLoading = true;
    try {
      const data = await api.agents({ workspaceId: workspace.id });
      let agentList = Array.isArray(data) ? data : [];
      // Enrich agents that lack spec_path by resolving from their task (best-effort)
      const needsSpec = agentList.filter(a => !a.spec_path && (a.task_id ?? a.current_task_id));
      if (needsSpec.length > 0) {
        const taskPromises = needsSpec.map(a => {
          const taskId = a.task_id ?? a.current_task_id;
          return api.task(taskId).then(t => ({ agentId: a.id, spec_path: t?.spec_path })).catch(() => null);
        });
        const results = await Promise.all(taskPromises);
        const specMap = Object.fromEntries(results.filter(r => r?.spec_path).map(r => [r.agentId, r.spec_path]));
        if (Object.keys(specMap).length > 0) {
          agentList = agentList.map(a => specMap[a.id] ? { ...a, spec_path: specMap[a.id] } : a);
        }
      }
      wsAgents = agentList;
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

  // ── Audit Log: load ─────────────────────────────────────────────────
  async function loadAudit() {
    auditLoading = true;
    try {
      const data = await api.adminAudit({ limit: '10' });
      auditEvents = Array.isArray(data) ? data : (data?.items ?? []);
    } catch {
      auditEvents = [];
    } finally {
      auditLoading = false;
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
  function viewAllForTab(tab) {
    if (repos.length === 1 && onSelectRepo) {
      onSelectRepo(repos[0], tab);
    }
  }

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

  // ── Spec quick actions ─────────────────────────────────────────────────
  let specActionLoading = $state(null); // spec path being acted on

  async function quickApproveSpec(spec, e) {
    e?.stopPropagation();
    const path = normalizeSpecPath(spec.path);
    const sha = spec.current_sha;
    if (!path || !sha) { toastError('Missing spec path or SHA'); return; }
    specActionLoading = spec.path;
    try {
      await api.approveSpec(path, sha);
      toastSuccess(`Spec "${spec.path.split('/').pop()}" approved`);
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'approved' } : s);
    } catch (e) {
      toastError('Failed to approve: ' + (e.message || e));
    } finally {
      specActionLoading = null;
    }
  }

  async function quickRejectSpec(spec, e) {
    e?.stopPropagation();
    const path = normalizeSpecPath(spec.path);
    if (!path) return;
    const reason = prompt('Rejection reason (required):', '');
    if (reason === null) return; // cancelled
    if (!reason.trim()) {
      toastError('A rejection reason is required');
      return;
    }
    specActionLoading = spec.path;
    try {
      await api.rejectSpec(path, reason.trim());
      toastSuccess(`Spec "${spec.path.split('/').pop()}" rejected`);
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'rejected' } : s);
    } catch (e) {
      toastError('Failed to reject: ' + (e.message || e));
    } finally {
      specActionLoading = null;
    }
  }

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
                      type === 'repo' ? api.repo(id).then(r => r?.name) :
                      type === 'workspace' ? api.workspace(id).then(w => w?.name) :
                      Promise.resolve(null);
      fetcher.then(name => {
        if (name) entityNameCache = { ...entityNameCache, [key]: name };
      }).catch(() => {});
    });
  }

  function entityName(type, id) {
    if (!id) return '';
    // Check repo map first for repos
    if (type === 'repo' && repoMap[id]?.name) return repoMap[id].name;
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

  // ── Merge Queue state ───────────────────────────────────────────────────
  let mergeQueueLoading = $state(true);
  let mergeQueueItems = $state([]);

  async function loadMergeQueue() {
    mergeQueueLoading = true;
    try {
      const [all, graph] = await Promise.all([
        api.mergeQueue().catch(() => []),
        api.mergeQueueGraph().catch(() => ({ nodes: [], edges: [] })),
      ]);
      const allItems = Array.isArray(all) ? all : [];
      // Enrich queue items with MR details
      const mrIds = allItems.map(e => e.merge_request_id ?? e.mr_id).filter(Boolean);
      const mrDetails = {};
      await Promise.all(mrIds.slice(0, 20).map(id =>
        api.mergeRequest(id).then(mr => { mrDetails[id] = mr; }).catch(() => {})
      ));
      // Filter to workspace repos if we have a repo list
      const repoIds = new Set(repos.map(r => r.id));
      mergeQueueItems = allItems
        .filter(e => {
          if (repoIds.size === 0) return true;
          const mrId = e.merge_request_id ?? e.mr_id;
          const mr = mrDetails[mrId];
          return mr ? repoIds.has(mr.repository_id ?? mr.repo_id) : true;
        })
        .map(e => {
          const mrId = e.merge_request_id ?? e.mr_id;
          const mr = mrDetails[mrId] ?? {};
          // Find deps from graph edges
          const graphEdges = graph?.edges ?? [];
          const deps = graphEdges.filter(edge => (edge.target ?? edge.to) === mrId).map(edge => edge.source ?? edge.from);
          const blocks = graphEdges.filter(edge => (edge.source ?? edge.from) === mrId).map(edge => edge.target ?? edge.to);
          return {
            ...e,
            _mr: mr,
            _title: mr.title ?? shortId(mrId),
            _status: mr.status,
            _branch: mr.source_branch,
            _agent: mr.author_agent_id,
            _spec_ref: mr.spec_ref,
            _deps: deps,
            _blocks: blocks,
          };
        })
        .sort((a, b) => (a.position ?? a.priority ?? 0) - (b.position ?? b.priority ?? 0));
    } catch {
      mergeQueueItems = [];
    } finally {
      mergeQueueLoading = false;
    }
  }

  // ── Activity feed state ─────────────────────────────────────────────────
  let activityLoading = $state(true);
  let activityEvents = $state([]);

  async function loadActivity() {
    activityLoading = true;
    try {
      const data = await api.activity(30);
      const events = Array.isArray(data) ? data : [];
      if (events.length > 0) {
        activityEvents = events;
      } else {
        // Activity API may return empty — synthesize from notifications
        // which contain rich event data (agent completions, gate failures, etc.)
        const wsId = workspace?.id;
        const notifs = wsId
          ? await api.myNotifications({ workspace_id: wsId, limit: 30 }).catch(() => ({ notifications: [] }))
          : await api.myNotifications({ limit: 30 }).catch(() => ({ notifications: [] }));
        const notifList = notifs?.notifications ?? (Array.isArray(notifs) ? notifs : []);
        activityEvents = notifList.map(n => {
          const body = parseNotifBody(n);
          const typeMap = {
            'AgentCompleted': 'agent_completed',
            'AgentFailed': 'agent_failed',
            'MrMerged': 'merged',
            'MrCreated': 'mr_created',
            'SpecApproved': 'spec_approved',
            'SpecRejected': 'spec_rejected',
            'GateFailure': 'gate_failed',
            'TaskCreated': 'task_created',
            'spec_approval': 'spec_approval',
            'gate_failure': 'gate_failed',
            'agent_clarification': 'agent_clarification',
            'budget_warning': 'budget_warning',
          };
          return {
            event_type: typeMap[n.notification_type] ?? n.notification_type,
            title: n.title ?? '',
            description: n.message ?? n.description ?? body.description ?? '',
            entity_type: body.mr_id ? 'mr' : body.agent_id ? 'agent' : body.spec_path ? 'spec' : body.task_id ? 'task' : null,
            entity_id: body.mr_id ?? body.agent_id ?? body.spec_path ?? body.task_id ?? n.entity_ref ?? null,
            entity_name: body.mr_title ?? body.agent_name ?? (body.spec_path ? body.spec_path.split('/').pop() : null),
            agent_id: body.agent_id,
            mr_id: body.mr_id,
            task_id: body.task_id,
            spec_path: body.spec_path,
            repo_id: n.repo_id,
            timestamp: n.created_at,
          };
        });
      }
    } catch {
      activityEvents = [];
    } finally {
      activityLoading = false;
    }
  }

  /** Parse notification body JSON safely */
  function parseNotifBody(n) {
    if (!n.body) return {};
    try { return typeof n.body === 'string' ? JSON.parse(n.body) : n.body; }
    catch { return {}; }
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

  const ACTIVITY_LABELS = {
    'created': 'MR created',
    'mr_created': 'MR created',
    'commit_pushed': 'Commits pushed',
    'gate_started': 'Gate started',
    'gate_passed': 'Gate passed',
    'gate_failed': 'Gate failed',
    'GateResult': 'Gate completed',
    'enqueued': 'Enqueued for merge',
    'merged': 'Merged',
    'Merged': 'Merged to main',
    'closed': 'Closed',
    'review_submitted': 'Review submitted',
    'comment_added': 'Comment added',
    'graph_extracted': 'Graph extracted',
    'GraphDelta': 'Architecture updated',
    'GitPush': 'Code pushed',
    'attestation_created': 'Attestation signed',
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
    'SuggestedSpecLink': 'Spec link suggested',
    'merged': 'MR merged',
    'budget_warning': 'Budget warning',
    'agent_clarification': 'Agent needs input',
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
    loadAudit();
    loadArchGraph();
    loadMergeQueue();
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

      <!-- ── Provenance Pipeline Summary ──────────────────────────────── -->
      {#if specs.length > 0 || wsTasks.length > 0 || wsAgents.length > 0 || wsMrs.length > 0}
      {@const pendingSpecs = specs.filter(s => (s.approval_status ?? s.status) === 'pending').length}
      {@const approvedSpecs = specs.filter(s => (s.approval_status ?? s.status) === 'approved').length}
      {@const activeTasks = wsTasks.filter(t => t.status === 'in_progress').length}
      {@const doneTasks = wsTasks.filter(t => t.status === 'done').length}
      {@const activeAgentCount = wsAgents.filter(a => a.status === 'active').length}
      {@const idleAgentCount = wsAgents.filter(a => a.status === 'idle' || a.status === 'completed').length}
      {@const openMrs = wsMrs.filter(m => m.status === 'open').length}
      {@const mergedMrs = wsMrs.filter(m => m.status === 'merged').length}
        <div class="provenance-pipeline" data-testid="provenance-pipeline">
          <div class="pipeline-stage" class:pipeline-has-pending={pendingSpecs > 0}>
            <div class="pipeline-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="20" height="20"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
            </div>
            <div class="pipeline-count">{specs.length}</div>
            <div class="pipeline-label">Specs</div>
            {#if pendingSpecs > 0}
              <div class="pipeline-detail pipeline-detail-warn">{pendingSpecs} pending</div>
            {:else if approvedSpecs > 0}
              <div class="pipeline-detail pipeline-detail-ok">{approvedSpecs} approved</div>
            {/if}
          </div>
          <div class="pipeline-arrow">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          </div>
          <div class="pipeline-stage">
            <div class="pipeline-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="20" height="20"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>
            </div>
            <div class="pipeline-count">{wsTasks.length}</div>
            <div class="pipeline-label">Tasks</div>
            {#if activeTasks > 0}
              <div class="pipeline-detail pipeline-detail-active">{activeTasks} active</div>
            {:else if doneTasks > 0}
              <div class="pipeline-detail pipeline-detail-ok">{doneTasks} done</div>
            {/if}
          </div>
          <div class="pipeline-arrow">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          </div>
          <div class="pipeline-stage" class:pipeline-has-active={activeAgentCount > 0}>
            <div class="pipeline-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="20" height="20"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
            </div>
            <div class="pipeline-count">{wsAgents.length}</div>
            <div class="pipeline-label">Agents</div>
            {#if activeAgentCount > 0}
              <div class="pipeline-detail pipeline-detail-active">{activeAgentCount} running</div>
            {:else if idleAgentCount > 0}
              <div class="pipeline-detail pipeline-detail-ok">{idleAgentCount} completed</div>
            {/if}
          </div>
          <div class="pipeline-arrow">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
          </div>
          <div class="pipeline-stage">
            <div class="pipeline-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="20" height="20"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 012 2v7"/><path d="M6 9v12"/></svg>
            </div>
            <div class="pipeline-count">{wsMrs.length}</div>
            <div class="pipeline-label">Merge Requests</div>
            {#if openMrs > 0}
              <div class="pipeline-detail pipeline-detail-warn">{openMrs} open</div>
            {:else if mergedMrs > 0}
              <div class="pipeline-detail pipeline-detail-ok">{mergedMrs} merged</div>
            {/if}
          </div>
        </div>
      {/if}

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
                    <span class="decision-desc">{n.title ?? n.message ?? n.description ?? body.description ?? ''}</span>
                    <div class="decision-refs">
                      {#if body.spec_path}
                        <button class="decision-entity-link" onclick={(e) => { e.stopPropagation(); nav('spec', normalizeSpecPath(body.spec_path), { path: normalizeSpecPath(body.spec_path), repo_id: n.repo_id }); }} title="View spec: {body.spec_path}">📋 {normalizeSpecPath(body.spec_path).split('/').pop()}</button>
                      {/if}
                      {#if body.mr_id}
                        <button class="decision-entity-link" onclick={(e) => { e.stopPropagation(); nav('mr', body.mr_id, { repository_id: n.repo_id }); }} title="View merge request">🔀 {entityName('mr', body.mr_id)}</button>
                      {/if}
                      {#if body.agent_id}
                        <button class="decision-entity-link" onclick={(e) => { e.stopPropagation(); nav('agent', body.agent_id, { repo_id: n.repo_id }); }} title="View agent">▶ {entityName('agent', body.agent_id)}</button>
                      {/if}
                      {#if body.task_id}
                        <button class="decision-entity-link" onclick={(e) => { e.stopPropagation(); nav('task', body.task_id, { repo_id: n.repo_id }); }} title="View task">☑ {entityName('task', body.task_id)}</button>
                      {/if}
                      {#if n.repo_id && repoMap[n.repo_id]}
                        <button
                          class="decision-repo-link"
                          onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[n.repo_id], 'decisions'); }}
                          aria-label={$t('workspace_home.go_to_repo_decisions', { values: { name: repoMap[n.repo_id].name } })}
                        >{repoMap[n.repo_id].name}</button>
                      {/if}
                    </div>
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
                        <button
                          class="inline-btn secondary"
                          onclick={() => nav('mr', body.mr_id, { _openTab: 'gates', repository_id: n.repo_id })}
                          title="View gate details"
                        >View Gates</button>
                      {:else if n.notification_type === 'agent_clarification' && body.agent_id}
                        <button
                          class="inline-btn"
                          onclick={() => nav('agent', body.agent_id, { _openTab: 'chat', repo_id: n.repo_id })}
                          title="View agent messages"
                        >Respond</button>
                      {:else if n.notification_type === 'budget_warning'}
                        <button
                          class="inline-btn secondary"
                          onclick={() => { const el = document.getElementById('section-budget'); if (el) el.scrollIntoView({ behavior: 'smooth' }); }}
                          title="View budget details"
                        >View Budget</button>
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
              <button class="prov-flow-node prov-flow-clickable" onclick={() => repos.length === 1 ? viewAllForTab('specs') : document.getElementById('section-specs')?.scrollIntoView({ behavior: 'smooth' })} title={repos.length === 1 ? 'Open Specs tab' : 'Jump to Specs section'}>
                <span class="prov-flow-count">{provenanceSummary.approved + provenanceSummary.pending}</span>
                <span class="prov-flow-label">Specs</span>
                {#if provenanceSummary.pending > 0}
                  <span class="prov-flow-sub prov-sub-pending">{provenanceSummary.pending} pending</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => repos.length === 1 ? viewAllForTab('tasks') : document.getElementById('section-tasks')?.scrollIntoView({ behavior: 'smooth' })} title={repos.length === 1 ? 'Open Tasks tab' : 'Jump to Tasks section'}>
                <span class="prov-flow-count">{provenanceSummary.totalTasks}</span>
                <span class="prov-flow-label">Tasks</span>
                {#if provenanceSummary.inProgressTasks > 0}
                  <span class="prov-flow-sub prov-sub-active">{provenanceSummary.inProgressTasks} in progress</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => repos.length === 1 ? viewAllForTab('agents') : document.getElementById('section-agents')?.scrollIntoView({ behavior: 'smooth' })} title={repos.length === 1 ? 'Open Agents tab' : 'Jump to Agents section'}>
                <span class="prov-flow-count">{wsAgents.length}</span>
                <span class="prov-flow-label">Agents</span>
                {#if provenanceSummary.activeAgentCount > 0}
                  <span class="prov-flow-sub prov-sub-active">{provenanceSummary.activeAgentCount} active</span>
                {/if}
              </button>
              <span class="prov-flow-arrow">→</span>
              <button class="prov-flow-node prov-flow-clickable" onclick={() => repos.length === 1 ? viewAllForTab('mrs') : document.getElementById('section-mrs')?.scrollIntoView({ behavior: 'smooth' })} title={repos.length === 1 ? 'Open MRs tab' : 'Jump to MRs section'}>
                <span class="prov-flow-count">{wsMrs.length}</span>
                <span class="prov-flow-label">MRs</span>
                {#if provenanceSummary.openMrs > 0}
                  <span class="prov-flow-sub prov-sub-pending">{provenanceSummary.openMrs} open</span>
                {/if}
                {#if provenanceSummary.mergedMrs > 0}
                  <span class="prov-flow-sub prov-sub-merged">{provenanceSummary.mergedMrs} merged</span>
                {/if}
              </button>
              {#if mergeQueueItems.length > 0}
                <span class="prov-flow-arrow">→</span>
                <button class="prov-flow-node prov-flow-clickable prov-flow-queue" onclick={() => document.getElementById('section-merge-queue')?.scrollIntoView({ behavior: 'smooth' })} title="Jump to Merge Queue">
                  <span class="prov-flow-count">{mergeQueueItems.length}</span>
                  <span class="prov-flow-label">Queue</span>
                  <span class="prov-flow-sub prov-sub-active">processing</span>
                </button>
              {/if}
              <span class="prov-flow-arrow">→</span>
              <span class="prov-flow-node prov-flow-code">
                <span class="prov-flow-count">{provenanceSummary.mergedMrs}</span>
                <span class="prov-flow-label">Merged</span>
              </span>
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
                {@const primaryType = event.entity_type ?? (event.agent_id ? 'agent' : event.mr_id ? 'mr' : event.task_id ? 'task' : event.spec_path ? 'spec' : null)}
                {@const primaryId = event.entity_id ?? event.agent_id ?? event.mr_id ?? event.task_id ?? event.spec_path ?? null}
                <button
                  class="activity-item activity-item-clickable"
                  onclick={() => {
                    if (primaryType && primaryId) {
                      const data = event.entity_id ? event : primaryType === 'spec' ? { path: event.spec_path, repo_id: event.repo_id } : {};
                      nav(primaryType, primaryId, data);
                    }
                  }}
                >
                  <div class="activity-dot activity-dot-{variant}"></div>
                  {#if i < Math.min(activityEvents.length, 15) - 1}<div class="activity-line"></div>{/if}
                  <div class="activity-content">
                    <span class="activity-icon">{activityIcon(event)}</span>
                    <span class="activity-label">{activityLabel(event)}</span>
                    {#if event.entity_name ?? event.title ?? event.description}
                      <span class="activity-detail">{event.entity_name ?? event.title ?? event.description}</span>
                    {/if}
                    {#if event.entity_id && event.entity_type}
                      <span class="activity-entity-name">{entityName(event.entity_type, event.entity_id)}</span>
                    {:else}
                      {#if event.agent_id}
                        <span class="activity-entity-name">{entityName('agent', event.agent_id)}</span>
                      {/if}
                      {#if event.mr_id}
                        <span class="activity-entity-name">{entityName('mr', event.mr_id)}</span>
                      {/if}
                      {#if event.task_id && !event.agent_id}
                        <span class="activity-entity-name">{entityName('task', event.task_id)}</span>
                      {/if}
                      {#if event.spec_path && !event.agent_id && !event.mr_id}
                        <span class="activity-entity-name">{event.spec_path.split('/').pop()}</span>
                      {/if}
                    {/if}
                    {#if event.repo_id && repoMap[event.repo_id]}
                      <span class="activity-repo-tag">{repoMap[event.repo_id].name}</span>
                    {/if}
                    {#if event.timestamp ?? event.created_at}
                      <span class="activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
                    {/if}
                  </div>
                </button>
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
                {@const repoTaskCount = wsTasks.filter(t => t.repo_id === repo.id).length}
                {@const repoActiveTaskCount = wsTasks.filter(t => t.repo_id === repo.id && t.status === 'in_progress').length}
                {@const repoMrCount = wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id).length}
                {@const repoOpenMrCount = wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id && m.status === 'open').length}
                {@const repoMergedMrCount = wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id && m.status === 'merged').length}
                {@const repoAgentCount = wsAgents.filter(a => a.repo_id === repo.id).length}
                {@const repoActiveAgentCount = wsAgents.filter(a => a.repo_id === repo.id && a.status === 'active').length}
                {@const repoSpecCount = specs.filter(s => s.repo_id === repo.id).length}
                {@const repoPendingSpecs = specs.filter(s => s.repo_id === repo.id && (s.approval_status === 'pending' || s.status === 'pending')).length}
                <li class="repo-row" data-testid="repo-row">
                  <div
                    class="repo-btn"
                    onclick={() => onSelectRepo?.(repo)}
                    role="button"
                    tabindex="0"
                    aria-label={$t('workspace_home.open_repo', { values: { name: repo.name } })}
                    data-testid="repo-link"
                    onkeydown={(e) => { if (e.key === 'Enter') onSelectRepo?.(repo); }}
                  >
                    <div class="repo-btn-top">
                      <span class="repo-name">{repo.name}</span>
                      <span class="repo-health health-{health}" aria-label={$t('workspace_home.repo_status', { values: { status: health } })} data-testid="repo-health">
                        {#if health === 'healthy'}● {$t('workspace_home.repo_health_healthy')}
                        {:else if health === 'gate'}⚠ {$t('workspace_home.repo_health_gate')}
                        {:else}○ {$t('workspace_home.repo_health_idle')}
                        {/if}
                      </span>
                    </div>
                    {#if repo.description}
                      <span class="repo-description">{repo.description}</span>
                    {/if}
                    <div class="repo-stats-row">
                      {#if repoSpecCount > 0}
                        <button class="repo-stat-chip repo-stat-clickable" title="{repoSpecCount} specs{repoPendingSpecs > 0 ? `, ${repoPendingSpecs} pending approval` : ''} — click to view" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repo, 'specs'); }}>
                          <span class="repo-stat-icon">📋</span>
                          <span class="repo-stat-num">{repoSpecCount}</span>
                          <span class="repo-stat-label">specs</span>
                          {#if repoPendingSpecs > 0}<span class="repo-stat-alert">{repoPendingSpecs} pending</span>{/if}
                        </button>
                      {/if}
                      {#if repoTaskCount > 0}
                        <button class="repo-stat-chip repo-stat-clickable" title="{repoTaskCount} tasks{repoActiveTaskCount > 0 ? `, ${repoActiveTaskCount} in progress` : ''} — click to view" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repo, 'tasks'); }}>
                          <span class="repo-stat-icon">☑</span>
                          <span class="repo-stat-num">{repoTaskCount}</span>
                          <span class="repo-stat-label">tasks</span>
                          {#if repoActiveTaskCount > 0}<span class="repo-stat-active">{repoActiveTaskCount} active</span>{/if}
                        </button>
                      {/if}
                      {#if repoAgentCount > 0}
                        <button class="repo-stat-chip repo-stat-clickable" title="{repoAgentCount} agents{repoActiveAgentCount > 0 ? `, ${repoActiveAgentCount} running` : ''} — click to view" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repo, 'agents'); }}>
                          <span class="repo-stat-icon">▶</span>
                          <span class="repo-stat-num">{repoAgentCount}</span>
                          <span class="repo-stat-label">agents</span>
                          {#if repoActiveAgentCount > 0}<span class="repo-stat-active">{repoActiveAgentCount} running</span>{/if}
                        </button>
                      {/if}
                      {#if repoMrCount > 0}
                        <button class="repo-stat-chip repo-stat-clickable" title="{repoMrCount} MRs: {repoMergedMrCount} merged, {repoOpenMrCount} open — click to view" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repo, 'mrs'); }}>
                          <span class="repo-stat-icon">🔀</span>
                          <span class="repo-stat-num">{repoMrCount}</span>
                          <span class="repo-stat-label">MRs</span>
                          {#if repoOpenMrCount > 0}<span class="repo-stat-alert">{repoOpenMrCount} open</span>{/if}
                          {#if repoMergedMrCount > 0}<span class="repo-stat-merged">{repoMergedMrCount} merged</span>{/if}
                        </button>
                      {/if}
                      {#if repoSpecCount === 0 && repoTaskCount === 0 && repoAgentCount === 0 && repoMrCount === 0}
                        <span class="repo-stat-chip repo-stat-empty">No activity yet</span>
                      {/if}
                    </div>
                  </div>
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
                  <th scope="col">Owner</th>
                  <th scope="col" aria-sort={specsSortCol === 'updated_at' ? (specsSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                    <button class="sort-btn" onclick={() => toggleSpecsSort('updated_at')}>{$t('workspace_home.col_last_activity')} <span class="sort-arrow" aria-hidden="true">{specsSortArrow('updated_at')}</span></button>
                  </th>
                  <th scope="col" class="ws-th-action"></th>
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
                    <td class="spec-repo ws-cell-link">{#if spec.repo_id && repoMap[spec.repo_id]}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[spec.repo_id]); }} title="Go to repo">{repoMap[spec.repo_id].name}</button>{:else if spec.repo_id}<span class="mono" title={spec.repo_id}>{entityName('repo', spec.repo_id)}</span>{:else}—{/if}</td>
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
                    <td class="spec-owner ws-cell-mono">{spec.owner ?? ''}</td>
                    <td class="spec-activity">{relTime(spec.updated_at)}</td>
                    <td class="ws-cell-action">
                      {#if (spec.approval_status ?? spec.status) === 'pending'}
                        <button class="ws-quick-action-btn ws-quick-action-in_progress" onclick={(e) => quickApproveSpec(spec, e)} disabled={specActionLoading === spec.path} title="Approve this spec">
                          {specActionLoading === spec.path ? '...' : 'Approve'}
                        </button>
                        <button class="ws-quick-action-btn ws-quick-action-blocked" onclick={(e) => quickRejectSpec(spec, e)} disabled={specActionLoading === spec.path} title="Reject this spec">
                          Reject
                        </button>
                      {/if}
                    </td>
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
          {#if archGraph}
            <span class="arch-stats-inline">
              {archGraph.nodes?.length ?? 0} nodes · {archGraph.edges?.length ?? 0} edges
            </span>
          {/if}
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
          {#if wsTasks.length > 10 && repos.length === 1}
            <button class="section-action-btn" onclick={() => viewAllForTab('tasks')}>View All</button>
          {/if}
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
                  <th>Created</th>
                  <th class="ws-th-action"></th>
                </tr>
              </thead>
              <tbody>
                {#each wsTasks.slice(0, 10) as task}
                  <tr class="ws-entity-row" onclick={() => nav('task', task.id, task)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') nav('task', task.id, task); }}>
                    <td><span class="status-badge status-{task.status ?? 'backlog'}" title={taskStatusTooltip(task.status)}>{task.status ?? 'backlog'}</span></td>
                    <td class="ws-cell-title">{task.title ?? 'Untitled'}</td>
                    <td>{#if task.priority}<span class="priority-badge priority-{task.priority}">{task.priority}</span>{/if}</td>
                    <td class="ws-cell-type">{task.task_type ?? ''}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if task.spec_path}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('spec', task.spec_path, { path: task.spec_path, repo_id: task.repo_id }); }} title={task.spec_path}>{task.spec_path.split('/').pop()}</button>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if task.assigned_to}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('agent', task.assigned_to, { repo_id: task.repo_id }); }} title={task.assigned_to}>{entityName('agent', task.assigned_to)}</button>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if task.repo_id && repoMap[task.repo_id]}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[task.repo_id]); }} title="Go to repo">{repoMap[task.repo_id].name}</button>{:else}{repoMap[task.repo_id]?.name ?? ''}{/if}</td>
                    <td class="ws-cell-time">{relTime(task.created_at)}</td>
                    <td class="ws-cell-action">
                      {#if WS_TASK_TRANSITIONS[task.status]?.length}
                        {#each WS_TASK_TRANSITIONS[task.status] as nextStatus}
                          <button class="ws-quick-action-btn ws-quick-action-{nextStatus}" onclick={(e) => quickChangeWsTaskStatus(task, nextStatus, e)} disabled={changingWsTaskId === task.id} title="Move to {nextStatus.replace(/_/g, ' ')}">{changingWsTaskId === task.id ? '...' : nextStatus === 'in_progress' ? 'Start' : nextStatus === 'done' ? 'Done' : nextStatus === 'blocked' ? 'Block' : nextStatus.replace(/_/g, ' ')}</button>
                        {/each}
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if wsTasks.length > 10}
              <p class="show-more-hint">
                {wsTasks.length - 10} more tasks not shown
                {#if repos.length === 1}
                  <button class="view-all-link" onclick={() => viewAllForTab('tasks')}>View All</button>
                {/if}
              </p>
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
          {#if wsMrs.length > 10 && repos.length === 1}
            <button class="section-action-btn" onclick={() => viewAllForTab('mrs')}>View All</button>
          {/if}
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
                  <th>Created</th>
                  <th class="ws-th-action"></th>
                </tr>
              </thead>
              <tbody>
                {#each wsMrs.slice(0, 10) as mr}
                  <tr class="ws-entity-row" onclick={() => nav('mr', mr.id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') nav('mr', mr.id, mr); }}>
                    <td><span class="status-badge status-{mr.queue_position != null ? 'queued' : (mr.status ?? 'open')}" title={mrStatusTooltip(mr)}>{mr.queue_position != null ? `queued #${mr.queue_position + 1}` : (mr.status ?? 'open')}</span>{#if mr.status === 'merged' && mr.merge_commit_sha}<code class="sha-inline mono" title={mr.merge_commit_sha}>{mr.merge_commit_sha.slice(0, 7)}</code>{/if}</td>
                    <td class="ws-cell-title">{mr.title ?? 'Untitled MR'}</td>
                    <td class="ws-cell-mono"><span class="branch-ref">{mr.source_branch ?? ''}</span>{#if mr.target_branch}<span class="branch-arrow">→</span><span class="branch-ref">{mr.target_branch}</span>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if mr.author_agent_id}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('agent', mr.author_agent_id, { repo_id: mr.repository_id ?? mr.repo_id }); }} title={mr.author_agent_id}>{entityName('agent', mr.author_agent_id)}</button>{/if}</td>
                    <td>
                      {#if mr._gates?.total > 0}
                        <button class="gate-cell-ws gate-cell-clickable" title={mr._gates.details?.map(g => `${g.status === 'passed' ? '✓' : g.status === 'failed' ? '✗' : '○'} ${g.name}${g.required === false ? ' (advisory)' : ''}`).join('\n') ?? ''} onclick={(e) => { e.stopPropagation(); nav('mr', mr.id, { ...mr, _openTab: 'gates' }); }}>
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
                        </button>
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
                        <button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('spec', specPath, { path: specPath, repo_id: mr.repository_id ?? mr.repo_id }); }} title={mr.spec_ref}>{specPath.split('/').pop()}</button>
                      {/if}
                    </td>
                    <td class="ws-cell-mono ws-cell-link">{#if mr.repository_id && repoMap[mr.repository_id]}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[mr.repository_id]); }} title="Go to repo">{repoMap[mr.repository_id].name}</button>{:else}{repoMap[mr.repository_id]?.name ?? ''}{/if}</td>
                    <td class="ws-cell-time">{relTime(mr.created_at)}</td>
                    <td class="ws-cell-action">
                      {#if mr.status === 'open' && mr.queue_position == null}
                        <button class="ws-quick-action-btn" onclick={(e) => quickEnqueueMr(mr, e)} disabled={enqueuingMrId === mr.id} title="Enqueue for merge">{enqueuingMrId === mr.id ? '...' : 'Enqueue'}</button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
            {#if wsMrs.length > 10}
              <p class="show-more-hint">
                {wsMrs.length - 10} more MRs not shown
                {#if repos.length === 1}
                  <button class="view-all-link" onclick={() => viewAllForTab('mrs')}>View All</button>
                {/if}
              </p>
            {/if}
          {/if}
        </div>
      </section>

      <!-- ── Merge Queue ──────────────────────────────────────────────────── -->
      {#if !mergeQueueLoading && mergeQueueItems.length > 0}
        <section class="home-section" aria-labelledby="section-merge-queue" data-testid="section-merge-queue">
          <div class="section-header">
            <h2 class="section-title" id="section-merge-queue">Merge Queue
              <span class="section-badge">{mergeQueueItems.length}</span>
            </h2>
          </div>
          <div class="section-body">
            <div class="merge-queue-pipeline">
              {#each mergeQueueItems as item, idx}
                {@const mrId = item.merge_request_id ?? item.mr_id}
                <div class="mq-item" class:mq-item-first={idx === 0}>
                  <div class="mq-item-position">#{idx + 1}</div>
                  <div class="mq-item-content">
                    <button class="mq-item-title" onclick={() => nav('mr', mrId, item._mr)} title="View merge request">
                      {item._title}
                    </button>
                    <div class="mq-item-meta">
                      {#if item._branch}
                        <span class="mq-branch mono">{item._branch}</span>
                      {/if}
                      {#if item._agent}
                        <button class="ws-entity-link mq-agent" onclick={(e) => { e.stopPropagation(); nav('agent', item._agent, { repo_id: item.repository_id ?? item.repo_id }); }} title={item._agent}>
                          {entityName('agent', item._agent)}
                        </button>
                      {/if}
                      {#if item._spec_ref}
                        {@const specPath = item._spec_ref.split('@')[0]}
                        <button class="ws-entity-link mq-spec" onclick={(e) => { e.stopPropagation(); nav('spec', specPath, { path: specPath }); }} title={item._spec_ref}>
                          {specPath.split('/').pop()}
                        </button>
                      {/if}
                    </div>
                    {#if item._deps.length > 0 || item._blocks.length > 0}
                      <div class="mq-deps">
                        {#if item._deps.length > 0}
                          <span class="mq-dep-label">waits for</span>
                          {#each item._deps as depId}
                            <button class="mq-dep-link" onclick={(e) => { e.stopPropagation(); nav('mr', depId, { repository_id: item.repository_id ?? item.repo_id }); }} title="View dependency">{entityName('mr', depId)}</button>
                          {/each}
                        {/if}
                        {#if item._blocks.length > 0}
                          <span class="mq-dep-label">blocks</span>
                          {#each item._blocks as blockId}
                            <button class="mq-dep-link" onclick={(e) => { e.stopPropagation(); nav('mr', blockId, { repository_id: item.repository_id ?? item.repo_id }); }} title="View dependent">{entityName('mr', blockId)}</button>
                          {/each}
                        {/if}
                      </div>
                    {/if}
                  </div>
                  <div class="mq-item-status">
                    <span class="mq-status-badge mq-status-{item.status ?? 'waiting'}">{item.status ?? 'waiting'}</span>
                  </div>
                  {#if idx < mergeQueueItems.length - 1}
                    <div class="mq-connector"></div>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        </section>
      {/if}

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
                  <th>Spec</th>
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
                  <tr class="ws-entity-row" onclick={() => nav('agent', agent.id, agent)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') nav('agent', agent.id, agent); }}>
                    <td><span class="status-badge status-{agent.status ?? 'active'}" title={agentStatusTooltip(agent.status)}>{agent.status ?? 'active'}</span></td>
                    <td class="ws-cell-title">{agent.name ?? shortId(agent.id)}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if agent.spec_path}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('spec', agent.spec_path, { path: agent.spec_path, repo_id: agent.repo_id }); }} title={agent.spec_path}>{agent.spec_path.split('/').pop()}</button>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if taskId}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('task', taskId, { repo_id: agent.repo_id }); }} title={taskId}>{entityName('task', taskId)}</button>{/if}</td>
                    <td class="ws-cell-mono"><span class="branch-ref">{agent.branch ?? ''}</span></td>
                    <td class="ws-cell-time">{fmtDuration(spawnedAt, agent.completed_at)}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if agent.mr_id}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); nav('mr', agent.mr_id, { repository_id: agent.repo_id }); }} title={agent.mr_id}>{entityName('mr', agent.mr_id)}</button>{/if}</td>
                    <td class="ws-cell-mono ws-cell-link">{#if agent.repo_id && repoMap[agent.repo_id]}<button class="ws-entity-link" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[agent.repo_id]); }} title="Go to repo">{repoMap[agent.repo_id].name}</button>{:else}{repoMap[agent.repo_id]?.name ?? ''}{/if}</td>
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
                        <span class="budget-meter-value">{activeCount} of {config.max_concurrent_agents} agent {config.max_concurrent_agents === 1 ? 'slot' : 'slots'} used</span>
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
                        <span class="budget-meter-value">{usedTokens.toLocaleString()} of {config.max_tokens_per_day.toLocaleString()} tokens used ({pct}%)</span>
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
                        <span class="budget-meter-value">${usedCost.toFixed(2)} of ${config.max_cost_per_day.toFixed(2)} budget used ({pct}%)</span>
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
                          <tr class="entity-row" onclick={() => nav('agent', entry.agent_id, { repo_id: entry.repo_id })} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') nav('agent', entry.agent_id, { repo_id: entry.repo_id }); }}>
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

      <!-- ── Audit Log ──────────────────────────────────────────────────── -->
      {#if !auditLoading && auditEvents.length > 0}
        <section class="home-section" aria-labelledby="section-audit" data-testid="section-audit">
          <div class="section-header">
            <h2 class="section-title" id="section-audit">Audit Log
              <span class="section-badge">{auditEvents.length}</span>
            </h2>
          </div>
          <div class="section-body">
            <div class="activity-timeline">
              {#each auditEvents.slice(0, 8) as event}
                {@const eventType = event.event_type ?? event.action ?? event.type ?? 'event'}
                {@const actor = event.actor ?? event.user_id ?? event.agent_id ?? '—'}
                {@const target = event.target ?? event.entity_type ?? ''}
                {@const targetId = event.target_id ?? event.entity_id ?? ''}
                {@const clickableType = target === 'agent' || target === 'mr' || target === 'task' || target === 'spec' ? target : null}
                <button
                  class="activity-item activity-item-clickable"
                  onclick={() => {
                    if (clickableType && targetId) {
                      const data = clickableType === 'spec' ? { path: targetId, repo_id: event.repo_id } : {};
                      nav(clickableType, targetId, data);
                    }
                  }}
                  disabled={!clickableType || !targetId}
                >
                  <div class="activity-dot activity-dot-info"></div>
                  <div class="activity-content">
                    <span class="activity-label">{eventType.replace(/_/g, ' ')}</span>
                    {#if target}
                      <span class="activity-detail">{target}{#if targetId}: <span class="mono">{entityName(target, targetId)}</span>{/if}</span>
                    {/if}
                    <span class="activity-entity-name mono">{actor.length > 12 ? entityName('agent', actor) : actor}</span>
                    {#if event.timestamp ?? event.created_at}
                      <span class="activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
                    {/if}
                  </div>
                </button>
              {/each}
            </div>
          </div>
        </section>
      {/if}

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

  /* ── Provenance Pipeline ─────────────────────────────────────────────── */
  .provenance-pipeline {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    flex-wrap: wrap;
  }

  .pipeline-stage {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius);
    min-width: 90px;
    transition: background var(--transition-fast);
  }

  .pipeline-stage:hover {
    background: var(--color-surface-elevated);
  }

  .pipeline-has-pending {
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
  }

  .pipeline-has-active {
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
  }

  .pipeline-icon {
    color: var(--color-text-muted);
  }

  .pipeline-count {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    line-height: 1;
  }

  .pipeline-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
  }

  .pipeline-detail {
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    white-space: nowrap;
  }

  .pipeline-detail-warn {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .pipeline-detail-ok {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .pipeline-detail-active {
    background: color-mix(in srgb, var(--color-info) 15%, transparent);
    color: var(--color-info);
  }

  .pipeline-arrow {
    color: var(--color-text-muted);
    flex-shrink: 0;
    display: flex;
    align-items: center;
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

  .decision-refs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: 2px;
  }

  .decision-entity-link {
    font-size: var(--text-xs);
    color: var(--color-link, var(--color-primary));
    font-family: var(--font-mono);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .decision-entity-link:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .decision-entity-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3);
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    text-align: left;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .repo-btn:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .repo-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .repo-btn-top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
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

  .repo-description {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .repo-stats-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .repo-stat-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    font-family: var(--font-body);
    cursor: default;
  }

  button.repo-stat-clickable {
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  button.repo-stat-clickable:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    color: var(--color-text);
  }

  .repo-stat-empty {
    color: var(--color-text-muted);
    border: none;
    background: none;
    font-style: italic;
  }

  .repo-stat-icon {
    font-size: 10px;
  }

  .repo-stat-num {
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .repo-stat-label {
    color: var(--color-text-muted);
  }

  .repo-stat-alert {
    color: var(--color-warning);
    font-weight: 500;
    margin-left: 2px;
  }

  .repo-stat-active {
    color: var(--color-success);
    font-weight: 500;
    margin-left: 2px;
  }

  .repo-stat-merged {
    color: var(--color-primary);
    font-weight: 500;
    margin-left: 2px;
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

  .arch-stats-inline {
    font-size: var(--text-xs);
    color: var(--color-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
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

  .gate-cell-ws { display: flex; flex-direction: column; gap: 2px; background: none; border: none; padding: 0; text-align: left; }
  .gate-cell-clickable { cursor: pointer; border-radius: var(--radius); }
  .gate-cell-clickable:hover { background: var(--color-surface-elevated); }

  .ws-th-action { width: 70px; }
  .ws-cell-action { white-space: nowrap; }
  .ws-quick-action-btn {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-text);
    cursor: pointer;
    margin-right: 2px;
  }
  .ws-quick-action-btn:hover:not(:disabled) { background: var(--color-surface-elevated); border-color: var(--color-primary); color: var(--color-primary); }
  .ws-quick-action-btn:disabled { opacity: 0.5; cursor: default; }
  .ws-quick-action-done:hover:not(:disabled) { border-color: var(--color-success); color: var(--color-success); }
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

  .show-more-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
    margin: var(--space-2) 0 0;
    font-style: italic;
  }

  .view-all-link {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    font-style: normal;
    font-weight: 500;
    padding: 0;
    margin-left: var(--space-2);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .view-all-link:hover {
    color: var(--color-primary-hover);
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

  .prov-flow-code {
    border-color: var(--color-success);
    opacity: 0.8;
    cursor: default;
  }

  .prov-flow-queue {
    border-color: var(--color-warning);
  }

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
    border: none;
    background: none;
    text-align: left;
    width: 100%;
    font-family: inherit;
  }

  .activity-item-clickable {
    cursor: pointer;
    border-radius: var(--radius);
    transition: background var(--transition-fast);
  }

  .activity-item-clickable:hover {
    background: var(--color-surface-elevated);
  }

  .activity-item-clickable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .activity-entity-name {
    color: var(--color-primary);
    font-weight: 500;
    font-size: var(--text-xs);
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

  .activity-repo-tag {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    white-space: nowrap;
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

  /* ── Merge Queue pipeline ──────────────────────────────────────────── */
  .merge-queue-pipeline {
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: var(--space-2) 0;
  }

  .mq-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-left: 3px solid var(--color-border);
    position: relative;
    transition: background var(--transition-fast);
  }

  .mq-item:hover {
    background: var(--color-surface-hover, rgba(255,255,255,0.03));
  }

  .mq-item-first {
    border-left-color: var(--color-primary);
  }

  .mq-item-position {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    min-width: 24px;
    text-align: center;
    padding-top: 2px;
    font-weight: 600;
  }

  .mq-item-content {
    flex: 1;
    min-width: 0;
  }

  .mq-item-title {
    background: none;
    border: none;
    padding: 0;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-link);
    cursor: pointer;
    text-align: left;
    text-decoration: none;
  }

  .mq-item-title:hover {
    text-decoration: underline;
  }

  .mq-item-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-1);
    flex-wrap: wrap;
  }

  .mq-branch {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-alt, rgba(255,255,255,0.05));
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
  }

  .mq-agent, .mq-spec {
    font-size: var(--text-xs);
  }

  .mq-deps {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  .mq-dep-label {
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .mq-dep-link {
    font-size: 10px;
    color: var(--color-primary);
    background: var(--color-surface-alt, rgba(255,255,255,0.04));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-1);
    cursor: pointer;
    font-family: var(--font-mono);
  }

  .mq-dep-link:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .mq-item-status {
    flex-shrink: 0;
    padding-top: 2px;
  }

  .mq-status-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-alt, rgba(255,255,255,0.06));
    color: var(--color-text-secondary);
  }

  .mq-status-running, .mq-status-processing {
    background: var(--color-warning-bg, rgba(234,179,8,0.15));
    color: var(--color-warning, #eab308);
  }

  .mq-status-passed, .mq-status-ready {
    background: var(--color-success-bg, rgba(34,197,94,0.15));
    color: var(--color-success, #22c55e);
  }

  .mq-status-failed, .mq-status-blocked {
    background: var(--color-danger-bg, rgba(239,68,68,0.15));
    color: var(--color-danger, #ef4444);
  }

  .mq-connector {
    display: none; /* vertical border-left provides visual continuity */
  }

  @media (prefers-reduced-motion: reduce) {
    .skeleton-row { animation: none; }
    .inline-btn, .section-btn, .repo-btn, .filter-select, .retry-btn { transition: none; }
  }
</style>
