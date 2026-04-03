<script>
  /**
   * WorkspaceHome — workspace dashboard (§2 of ui-navigation.md)
   *
   * Zones: ActionNeeded, PipelineOverview, Decisions, Repos grid, Tabbed secondary (Specs/Tasks/MRs/Agents/Activity/Budget).
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
  import { entityName, shortId, seedEntityName, seedFromEntities } from '../lib/entityNames.svelte.js';
  import { relativeTime, formatDuration } from '../lib/timeFormat.js';
  import { specStatusTooltip, taskStatusTooltip, mrStatusTooltip, agentStatusTooltip, SPEC_STATUS_ICONS } from '../lib/statusTooltips.js';
  import ActionNeeded from './ActionNeeded.svelte';
  import PipelineOverview from './PipelineOverview.svelte';
  import RepoCard from './RepoCard.svelte';
  import Modal from '../lib/Modal.svelte';
  import Icon from '../lib/Icon.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

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
    spec_approval: '!',
    gate_failure: '!',
    cross_workspace_change: '~',
    conflicting_interpretations: '!',
    meta_spec_drift: '~',
    budget_warning: '$',
    trust_suggestion: '*',
    spec_assertion_failure: '✗',
    suggested_link: '~',
    agent_completed: '✓',
    agent_failed: '✗',
    mr_merged: '✓',
    mr_created: '+',
    mr_needs_review: '>',
    spec_approved: '✓',
    spec_rejected: '✗',
    spec_changed: '~',
    task_created: '+',
  };

  // Normalize PascalCase notification types from the server
  const NOTIF_TYPE_NORM = {
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

  function typeLabel(type) {
    const key = `workspace_home.type_labels.${type}`;
    const val = $t(key);
    return val !== key ? val : type;
  }

  // SPEC_STATUS_ICONS, specStatusTooltip, taskStatusTooltip, mrStatusTooltip,
  // agentStatusTooltip are imported from ../lib/statusTooltips.js

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

  // ── Budget/Cost state ───────────────────────────────────────────────────
  let budgetLoading = $state(true);
  let budgetData = $state(null); // { config, usage }
  let costData = $state(null);   // cost summary

  // ── Briefing state ─────────────────────────────────────────────────────
  let briefingLoading = $state(true);
  let briefingData = $state(null);

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
      data = data.map(n => ({ ...n, notification_type: NOTIF_TYPE_NORM[n.notification_type] ?? n.notification_type }));
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
      seedRepoNames();
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
      seedFromEntities('task', wsTasks);
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
      seedFromEntities('mr', wsMrs);
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
      seedFromEntities('agent', wsAgents);
    } catch {
      wsAgents = [];
    } finally {
      agentsLoading = false;
    }
  }

  // ── Briefing: load ─────────────────────────────────────────────────────
  async function loadBriefing() {
    if (!workspace?.id) return;
    briefingLoading = true;
    try {
      briefingData = await api.getWorkspaceBriefing(workspace.id);
    } catch {
      briefingData = null;
    } finally {
      briefingLoading = false;
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


  // ── Notification inline actions ────────────────────────────────────────
  async function handleApproveSpec(n) {
    const body = getBody(n);
    // Extract spec path from body, or parse from title like "Spec pending approval: system/foo.md"
    let specPath = body.spec_path ?? (n.title?.match(/:\s*(.+\.md)\s*$/)?.[1]);
    if (!specPath) return;
    specPath = normalizeSpecPath(specPath);
    // Get SHA: from body, or fetch from spec ledger
    let sha = body.spec_sha;
    if (!sha) {
      try {
        const spec = await api.getSpec(specPath);
        sha = spec?.current_sha ?? spec?.sha;
      } catch { /* best effort */ }
    }
    if (!sha) return;
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'approve' } };
    try {
      await api.approveSpec(specPath, sha);
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
    let specPath = body.spec_path ?? (n.title?.match(/:\s*(.+\.md)\s*$/)?.[1]);
    if (!specPath) return;
    specPath = normalizeSpecPath(specPath);
    actionStates = { ...actionStates, [n.id]: { loading: true, action: 'reject' } };
    try {
      await api.revokeSpec(specPath, 'Rejected');
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

  // ── Quick spec approve/reject from sidebar ──────────────────────────────
  let specActionStates = $state({});

  async function quickApproveSpec(spec, e) {
    e?.stopPropagation();
    const sha = spec.current_sha ?? spec.sha;
    if (!sha) { toastError('Cannot approve: spec SHA not available'); return; }
    specActionStates = { ...specActionStates, [spec.path]: 'loading' };
    try {
      await api.approveSpec(spec.path, sha);
      specActionStates = { ...specActionStates, [spec.path]: 'approved' };
      toastSuccess(`Spec "${spec.path.split('/').pop()?.replace(/\.md$/, '')}" approved`);
      // Update specs list to reflect the change
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'approved', status: 'approved' } : s);
    } catch (err) {
      specActionStates = { ...specActionStates, [spec.path]: 'error' };
      toastError('Approve failed: ' + (err.message ?? err));
    }
  }

  async function quickRejectSpec(spec, e) {
    e?.stopPropagation();
    specActionStates = { ...specActionStates, [spec.path]: 'loading' };
    try {
      await api.rejectSpec(spec.path, 'Rejected from dashboard');
      specActionStates = { ...specActionStates, [spec.path]: 'rejected' };
      toastSuccess(`Spec "${spec.path.split('/').pop()?.replace(/\.md$/, '')}" rejected`);
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'rejected', status: 'rejected' } : s);
    } catch (err) {
      specActionStates = { ...specActionStates, [spec.path]: 'error' };
      toastError('Reject failed: ' + (err.message ?? err));
    }
  }

  // ── Quick MR enqueue from sidebar ──────────────────────────────────────
  let mrEnqueueStates = $state({});

  async function quickEnqueueMr(mr, e) {
    e?.stopPropagation();
    mrEnqueueStates = { ...mrEnqueueStates, [mr.id]: 'loading' };
    try {
      await api.enqueue(mr.id);
      mrEnqueueStates = { ...mrEnqueueStates, [mr.id]: 'queued' };
      toastSuccess(`MR "${mr.title ?? 'Untitled'}" enqueued for merge`);
      // Update MR in list
      const updated = await api.mergeRequest(mr.id).catch(() => null);
      if (updated) {
        wsMrs = wsMrs.map(m => m.id === mr.id ? { ...m, ...updated } : m);
      }
    } catch (err) {
      mrEnqueueStates = { ...mrEnqueueStates, [mr.id]: 'error' };
      toastError('Enqueue failed: ' + (err.message ?? err));
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

  // ── Relative time helper (i18n-aware wrapper around shared module) ──────
  function relTime(ts) {
    if (!ts) return '';
    return relativeTime(ts);
  }

  // Entity name resolution + time formatting imported from shared modules.
  // Seed repo names from loaded data so they're immediately available.
  function seedRepoNames() {
    for (const r of repos) {
      if (r.id && r.name) seedEntityName('repo', r.id, r.name);
    }
  }

  // ── Pipeline overview computed stats ────────────────────────────────────
  let pipelineSpecs = $derived({
    total: specs.length,
    pending: specs.filter(s => (s.approval_status ?? s.status) === 'pending').length,
    approved: specs.filter(s => (s.approval_status ?? s.status) === 'approved').length,
  });
  let pipelineTasks = $derived({
    total: wsTasks.length,
    in_progress: wsTasks.filter(t => t.status === 'in_progress').length,
    blocked: wsTasks.filter(t => t.status === 'blocked').length,
    done: wsTasks.filter(t => t.status === 'done').length,
  });
  let pipelineAgents = $derived({
    total: wsAgents.length,
    active: wsAgents.filter(a => a.status === 'active').length,
  });
  let pipelineMrs = $derived({
    total: wsMrs.length,
    open: wsMrs.filter(m => m.status === 'open').length,
    merged: wsMrs.filter(m => m.status === 'merged').length,
    failed_gates: wsMrs.filter(m => m._gates?.failed > 0).length,
  });

  // ── Activity filter + pagination ──────────────────────────────────────
  let activityFilter = $state('');
  let activityLimit = $state(10);

  let filteredActivity = $derived.by(() => {
    if (!activityFilter) return activityEvents;
    return activityEvents.filter(e => {
      const t = e.event_type ?? e.type ?? '';
      if (activityFilter === 'spec') return t.includes('spec');
      if (activityFilter === 'task') return t.includes('task');
      if (activityFilter === 'agent') return t.includes('agent');
      if (activityFilter === 'mr') return t.includes('mr') || t.includes('merg') || t.includes('creat');
      if (activityFilter === 'gate') return t.includes('gate');
      return true;
    });
  });

  // ── Repo card data ────────────────────────────────────────────────────
  // repoHealth(repo) function already defined above (line ~265)

  function repoStats(repo) {
    const repoMrs = wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id);
    const repoAgents = wsAgents.filter(a => a.repo_id === repo.id);
    const repoTasks = wsTasks.filter(t => t.repo_id === repo.id);
    // Compute last activity from most recent MR/agent/task
    const times = [
      ...repoMrs.map(m => m.created_at ?? m.updated_at),
      ...repoAgents.map(a => a.created_at ?? a.spawned_at),
      ...repoTasks.map(t => t.created_at),
    ].filter(Boolean).sort().reverse();
    return {
      specs: specs.filter(s => s.repo_id === repo.id).length,
      tasks: repoTasks.length,
      agents: repoAgents.filter(a => a.status === 'active').length,
      mrs: repoMrs.length,
      openMrs: repoMrs.filter(m => m.status === 'open').length,
      failedGates: repoMrs.filter(m => m._gates?.failed > 0).length,
      last_activity: times[0] ?? null,
    };
  }

  function repoActiveAgentNames(repo) {
    return wsAgents
      .filter(a => a.repo_id === repo.id && a.status === 'active')
      .map(a => a.name ?? shortId(a.id));
  }

  function repoLatestMr(repo) {
    const mrs = wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id);
    if (mrs.length === 0) return null;
    // Sort by most recent first
    return mrs.sort((a, b) => {
      const aTime = a.merged_at ?? a.updated_at ?? a.created_at ?? 0;
      const bTime = b.merged_at ?? b.updated_at ?? b.created_at ?? 0;
      return bTime - aTime;
    })[0];
  }

  function repoSpecBreakdown(repo) {
    const repoSpecs = specs.filter(s => s.repo_id === repo.id);
    if (repoSpecs.length === 0) return null;
    return {
      pending: repoSpecs.filter(s => (s.approval_status ?? s.status) === 'pending').length,
      approved: repoSpecs.filter(s => (s.approval_status ?? s.status) === 'approved').length,
      draft: repoSpecs.filter(s => (s.approval_status ?? s.status) === 'draft').length,
    };
  }

  function handlePipelineStageClick(stageId) {
    const tabMap = { specs: 'specs', tasks: 'tasks', mrs: 'mrs', agents: 'agents', merged: 'mrs' };
    const tab = tabMap[stageId];
    if (!tab) return;
    // Single repo: navigate directly to that repo's tab
    if (repos.length === 1 && onSelectRepo) {
      onSelectRepo(repos[0], tab);
      return;
    }
    // Multiple repos: scroll to activity feed (which shows cross-repo data)
    // and set a filter matching the stage type
    const filterMap = { specs: 'spec', tasks: 'task', agents: 'agent', mrs: 'mr', merged: 'mr' };
    if (filterMap[stageId]) activityFilter = filterMap[stageId];
    document.querySelector('[data-testid="ws-tabbed-panel"]')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
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
          // Enrich with gate data from already-loaded workspace MRs
          const wsMr = wsMrs.find(m => m.id === mrId);
          if (wsMr?._gates && !mr._gates) mr._gates = wsMr._gates;
          // Find deps from graph edges
          const graphEdges = graph?.edges ?? [];
          const deps = graphEdges.filter(edge => (edge.target ?? edge.to) === mrId).map(edge => edge.source ?? edge.from);
          const blocks = graphEdges.filter(edge => (edge.source ?? edge.from) === mrId).map(edge => edge.target ?? edge.to);
          return {
            ...e,
            _mr: mr,
            _title: mr.title ?? entityName('mr', mrId),
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
        // Sanitize: strip raw JSON from description fields — show human-readable text only
        activityEvents = events.map(e => {
          const desc = e.description ?? e.detail ?? '';
          if (desc && (desc.startsWith('{') || desc.startsWith('['))) {
            // Try to extract something useful from the JSON
            try {
              const parsed = JSON.parse(desc);
              const agentName = parsed.agent_name ?? '';
              const mrTitle = parsed.mr_title ?? '';
              return { ...e, description: agentName ? `Agent: ${agentName}` : mrTitle ? `MR: ${mrTitle}` : '' };
            } catch {
              return { ...e, description: '' };
            }
          }
          return e;
        });
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
          // Build a human-readable description — never show raw JSON
          const rawDesc = n.message ?? n.description ?? body.description ?? '';
          const humanDesc = (rawDesc && !rawDesc.startsWith('{') && !rawDesc.startsWith('['))
            ? rawDesc
            : synthesizeDescription(n.notification_type, body);
          return {
            event_type: typeMap[n.notification_type] ?? n.notification_type,
            title: n.title ?? '',
            description: humanDesc,
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

  /** Synthesize a human-readable description from notification type + parsed body */
  function synthesizeDescription(notifType, body) {
    const agentName = body.agent_name ?? '';
    const mrTitle = body.mr_title ?? '';
    const specPath = body.spec_path ? body.spec_path.split('/').pop()?.replace(/\.md$/, '') : '';
    switch (notifType) {
      case 'AgentCompleted': return agentName ? `Agent "${agentName}" finished implementing` : '';
      case 'AgentFailed': return agentName ? `Agent "${agentName}" encountered an error` : '';
      case 'MrMerged': return mrTitle ? `"${mrTitle}" passed all gates and was merged` : '';
      case 'MrCreated': return mrTitle ? `"${mrTitle}" created from agent work` : '';
      case 'MrNeedsReview': return mrTitle ? `"${mrTitle}" is ready for review` : '';
      case 'SpecApproved': return specPath ? `"${specPath}" approved — agents can begin` : '';
      case 'SpecRejected': return specPath ? `"${specPath}" rejected — implementation blocked` : '';
      case 'SpecChanged': return specPath ? `"${specPath}" was updated` : '';
      case 'GateFailure': return body.gate_name ? `Gate "${body.gate_name}" failed` : 'A quality gate failed';
      case 'TaskCreated': return specPath ? `Task created from spec "${specPath}"` : '';
      case 'SuggestedSpecLink': return mrTitle ? `MR "${mrTitle}" may relate to a spec` : '';
      case 'BudgetWarning': return 'Budget threshold exceeded';
      default: return '';
    }
  }

  /** Parse notification body JSON safely */
  function parseNotifBody(n) {
    if (!n.body) return {};
    try { return typeof n.body === 'string' ? JSON.parse(n.body) : n.body; }
    catch { return {}; }
  }

  function activityIconName(event) {
    const t = (event.event_type ?? event.event ?? event.type ?? '').toLowerCase();
    if (t.includes('spec') && t.includes('approv')) return 'check';
    if (t.includes('spec') && t.includes('reject')) return 'x';
    if (t.includes('spec') && t.includes('link')) return 'link';
    if (t.includes('spec') && t.includes('pending')) return 'clock';
    if (t.includes('spec')) return 'spec';
    if (t.includes('task')) return 'task';
    if (t.includes('agent') && t.includes('spawn')) return 'play';
    if (t.includes('agent') && t.includes('complet')) return 'agent';
    if (t.includes('agent') && t.includes('fail')) return 'alert-triangle';
    if (t.includes('mr') && t.includes('merg')) return 'git-merge';
    if (t.includes('mr') && t.includes('creat')) return 'plus';
    if (t.includes('gate')) return 'gate';
    if (t.includes('push')) return 'code';
    if (t.includes('graph')) return 'hash';
    if (t.includes('budget')) return 'dollar';
    return 'activity';
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
    'spec_approval': 'Spec needs approval',
    'suggested_link': 'Spec link suggested',
    'SuggestedSpecLink': 'Spec link suggested',
    'SpecPendingApproval': 'Spec needs approval',
    'SpecApproved': 'Spec approved',
    'SpecRejected': 'Spec rejected',
    'SpecChanged': 'Spec updated',
    'AgentCompleted': 'Agent completed',
    'AgentFailed': 'Agent failed',
    'MrMerged': 'MR merged',
    'MrCreated': 'MR created',
    'MrNeedsReview': 'MR needs review',
    'GateFailure': 'Gate failed',
    'TaskCreated': 'Task created',
    'BudgetWarning': 'Budget warning',
    'MetaSpecDrift': 'Agent rules drifted',
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
    loadTasks();
    loadMrs();
    loadAgents();
    loadActivity();
    loadBudget();
    loadBriefing();
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
    <!-- ═══ Focused Dashboard (replaces cluttered section soup) ═══════ -->
    <div class="focused-dashboard">

      <!-- Workspace header with description -->
      {#if workspace.description}
        <p class="ws-description">{workspace.description}</p>
      {/if}

      <!-- Zone 1: Action Needed (with inline approve/reject/retry/dismiss) -->
      <ActionNeeded
        items={notifications}
        onApproveSpec={handleApproveSpec}
        onRejectSpec={handleRejectSpec}
        onRetryGate={handleRetry}
        onDismiss={handleDismiss}
      />

      <!-- Zone 2: Pipeline Overview (expandable hero — replaces old sidebar) -->
      <PipelineOverview
        specs={pipelineSpecs}
        tasks={pipelineTasks}
        agents={pipelineAgents}
        mrs={pipelineMrs}
        budget={budgetData}
        specsList={specs}
        tasksList={wsTasks}
        agentsList={wsAgents}
        mrsList={wsMrs}
        onStageClick={handlePipelineStageClick}
        onApproveSpec={quickApproveSpec}
        onRejectSpec={quickRejectSpec}
        onEnqueueMr={quickEnqueueMr}
        onNavigateSpec={navigateToSpec}
      />

      <!-- Provenance summary bar — compact one-liner of workspace state -->
      {#if provenanceSummary.approved > 0 || provenanceSummary.activeAgentCount > 0 || provenanceSummary.mergedMrs > 0}
        <div class="provenance-summary-bar">
          <span class="prov-item">{provenanceSummary.approved} spec{provenanceSummary.approved !== 1 ? 's' : ''} approved</span>
          <span class="prov-sep" aria-hidden="true">·</span>
          <span class="prov-item">{provenanceSummary.inProgressTasks} task{provenanceSummary.inProgressTasks !== 1 ? 's' : ''} active</span>
          <span class="prov-sep" aria-hidden="true">·</span>
          <span class="prov-item">{provenanceSummary.activeAgentCount} agent{provenanceSummary.activeAgentCount !== 1 ? 's' : ''} running</span>
          <span class="prov-sep" aria-hidden="true">·</span>
          <span class="prov-item prov-merged">{provenanceSummary.mergedMrs} merged</span>
          {#if provenanceSummary.pending > 0}
            <span class="prov-sep" aria-hidden="true">·</span>
            <span class="prov-item prov-pending">{provenanceSummary.pending} awaiting approval</span>
          {/if}
        </div>
      {/if}

      <!-- Briefing one-liner — compact summary replacing the full briefing section -->
      {#if briefingData && !briefingLoading && (briefingData.summary || briefingData.narrative || briefingData.exceptions?.length > 0)}
        <p class="briefing-oneliner" data-testid="section-briefing">
          {briefingData.summary ?? briefingData.narrative ?? ''}
          {#if briefingData.exceptions?.length > 0}
            <span class="briefing-exception-count"> — {briefingData.exceptions.length} exception{briefingData.exceptions.length !== 1 ? 's' : ''} flagged</span>
          {/if}
        </p>
      {/if}

      <!-- ── Zone 3: Repos + Merge Queue (two-column when queue active) ── -->
      <div class="repos-and-queue" class:has-queue={mergeQueueItems.length > 0}>
        <!-- Repositories -->
        <section class="home-section" aria-labelledby="section-repos" data-testid="section-repos">
          <div class="section-header">
            <h2 class="section-title" id="section-repos">{$t('workspace_home.sections.repos')}</h2>
            <div class="repo-header-actions">
              <button class="section-btn" onclick={() => { newRepoOpen = !newRepoOpen; importOpen = false; }} data-testid="btn-new-repo">{$t('workspace_home.new_repo')}</button>
              <button class="section-btn" onclick={() => { importOpen = !importOpen; newRepoOpen = false; }} data-testid="btn-import-repo">{$t('workspace_home.import')}</button>
            </div>
          </div>
        <div class="section-body">
          {#if reposLoading}
            <div class="skeleton-row"></div>
          {:else if reposError}
            <div class="error-row" role="alert">
              <p class="error-text">{reposError}</p>
              <button class="retry-btn" onclick={loadRepos}>{$t('common.retry')}</button>
            </div>
          {:else if repos.length === 0}
            <p class="empty-text" data-testid="repos-empty">{$t('workspace_home.repos_empty')}</p>
          {:else}
            <div class="repo-cards-grid">
              {#each repos as repo (repo.id)}
                <RepoCard
                  {repo}
                  health={repoHealth(repo)}
                  stats={repoStats(repo)}
                  activeAgentNames={repoActiveAgentNames(repo)}
                  specBreakdown={repoSpecBreakdown(repo)}
                  latestMr={repoLatestMr(repo)}
                  onclick={() => onSelectRepo?.(repo)}
                />
              {/each}
            </div>
          {/if}

          {#if newRepoOpen}
            <form class="inline-form" data-testid="new-repo-form" onsubmit={(e) => { e.preventDefault(); handleCreateRepo(); }}>
              <div class="inline-form-header">
                <span class="inline-form-title">{$t('workspace_home.new_repo_title')}</span>
                <button type="button" class="inline-form-close" onclick={() => { newRepoOpen = false; newRepoError = null; }}>✕</button>
              </div>
              <input id="new-repo-name" class="inline-form-input" type="text" placeholder={$t('workspace_home.new_repo_name_placeholder')} bind:value={newRepoName} required disabled={newRepoLoading} data-testid="new-repo-name-input" />
              <input id="new-repo-desc" class="inline-form-input" type="text" placeholder={$t('workspace_home.new_repo_desc_placeholder')} bind:value={newRepoDescription} disabled={newRepoLoading} data-testid="new-repo-description-input" />
              {#if newRepoError}<p class="inline-form-error" role="alert">{newRepoError}</p>{/if}
              <div class="inline-form-actions">
                <button type="submit" class="section-btn primary" disabled={newRepoLoading || !newRepoName.trim()}>{newRepoLoading ? $t('workspace_home.new_repo_creating') : $t('workspace_home.new_repo_create')}</button>
                <button type="button" class="section-btn" onclick={() => { newRepoOpen = false; newRepoError = null; }}>{$t('common.cancel')}</button>
              </div>
            </form>
          {/if}

          {#if importOpen}
            <form class="inline-form" data-testid="import-repo-form" onsubmit={(e) => { e.preventDefault(); handleImportRepo(); }}>
              <div class="inline-form-header">
                <span class="inline-form-title">{$t('workspace_home.import_repo_title')}</span>
                <button type="button" class="inline-form-close" onclick={() => { importOpen = false; importError = null; }}>✕</button>
              </div>
              <input id="import-url" class="inline-form-input" type="url" placeholder={$t('workspace_home.import_url_placeholder')} bind:value={importUrl} required disabled={importLoading} data-testid="import-url-input" />
              <input id="import-name" class="inline-form-input" type="text" placeholder={$t('workspace_home.import_name_placeholder')} bind:value={importName} disabled={importLoading} data-testid="import-name-input" />
              {#if importError}<p class="inline-form-error" role="alert">{importError}</p>{/if}
              <div class="inline-form-actions">
                <button type="submit" class="section-btn primary" disabled={importLoading || !importUrl.trim()}>{importLoading ? $t('workspace_home.import_importing') : $t('workspace_home.import_submit')}</button>
                <button type="button" class="section-btn" onclick={() => { importOpen = false; importError = null; }}>{$t('common.cancel')}</button>
              </div>
            </form>
          {/if}
        </div>
      </section>

        <!-- Merge Queue sidebar (only when items are queued) -->
        {#if mergeQueueItems.length > 0}
          <aside class="merge-queue-sidebar" aria-labelledby="section-merge-queue">
            <h3 class="mq-sidebar-title" id="section-merge-queue">
              <Icon name="git-merge" size={14} />
              Merge Queue
              <span class="mq-sidebar-count">{mergeQueueItems.length}</span>
            </h3>
            <div class="mq-sidebar-list">
              {#each mergeQueueItems.slice(0, 8) as item, i}
                {@const mrId = item.merge_request_id ?? item.mr_id}
                <button class="mq-sidebar-item" onclick={() => nav('mr', mrId, item._mr)} title="View merge request">
                  <span class="mq-item-position">#{i + 1}</span>
                  <div class="mq-sidebar-item-body">
                    <span class="mq-sidebar-item-title">{item._title}</span>
                    <span class="mq-sidebar-item-meta">
                      {#if item._branch}<span class="mq-branch">{item._branch}</span>{/if}
                      {#if item._mr?._gates?.total > 0}
                        <span class="mq-gates-mini">
                          {#if item._mr._gates.failed > 0}<span class="gate-fail-inline">✗{item._mr._gates.failed}</span>{/if}
                          {#if item._mr._gates.passed > 0}<span class="gate-pass-inline">✓{item._mr._gates.passed}</span>{/if}
                        </span>
                      {/if}
                    </span>
                  </div>
                </button>
              {/each}
              {#if mergeQueueItems.length > 8}
                <p class="show-more-hint">{mergeQueueItems.length - 8} more in queue</p>
              {/if}
            </div>
          </aside>
        {/if}
      </div><!-- .repos-and-queue -->

      <!-- ── Zone 5: Activity feed (full-width) ── -->
      <section class="ws-feed-panel" aria-labelledby="feed-title" data-testid="ws-tabbed-panel">
        <div class="feed-header">
          <h2 class="feed-title" id="feed-title">
            <Icon name="activity" size={14} />
            Activity
          </h2>
          <select class="filter-select" bind:value={activityFilter} aria-label="Filter activity">
            <option value="">All</option>
            <option value="spec">Specs</option>
            <option value="task">Tasks</option>
            <option value="agent">Agents</option>
            <option value="mr">MRs</option>
            <option value="gate">Gates</option>
          </select>
        </div>
        <div class="feed-body">
          {#if activityLoading}
            <div class="skeleton-row"></div>
            <div class="skeleton-row"></div>
          {:else if filteredActivity.length === 0}
            <p class="empty-text">No recent activity. Push specs and approve them to get started.</p>
          {:else}
            <div class="activity-timeline">
              {#each filteredActivity.slice(0, activityLimit) as event, i}
                {@const variant = activityVariant(event)}
                {@const primaryType = event.entity_type ?? (event.agent_id ? 'agent' : event.mr_id ? 'mr' : event.task_id ? 'task' : event.spec_path ? 'spec' : null)}
                {@const primaryId = event.entity_id ?? event.agent_id ?? event.mr_id ?? event.task_id ?? event.spec_path ?? null}
                <button
                  class="activity-item activity-item-clickable"
                  onclick={() => {
                    if (primaryType && primaryId) {
                      const data = primaryType === 'spec' ? { path: event.spec_path, repo_id: event.repo_id } : { repo_id: event.repo_id };
                      nav(primaryType, primaryId, data);
                    }
                  }}
                >
                  <div class="activity-dot activity-dot-{variant}"></div>
                  {#if i < Math.min(filteredActivity.length, activityLimit) - 1}<div class="activity-line"></div>{/if}
                  <div class="activity-content">
                    <div class="activity-main-row">
                      <span class="activity-icon"><Icon name={activityIconName(event)} size={12} /></span>
                      <span class="activity-label">{activityLabel(event)}</span>
                      {#if event.entity_name ?? event.title}
                        <span class="activity-detail">{event.entity_name ?? event.title}</span>
                      {/if}
                      <span class="activity-entity-badges">
                        {#if event.agent_id}
                          <button class="activity-entity-badge" onclick={(e) => { e.stopPropagation(); nav('agent', event.agent_id, { repo_id: event.repo_id }); }} title="View agent">
                            <Icon name="agent" size={10} /> {event.entity_name && event.entity_type === 'agent' ? event.entity_name : event.agent_id.slice(0, 8)}
                          </button>
                        {/if}
                        {#if event.mr_id}
                          <button class="activity-entity-badge" onclick={(e) => { e.stopPropagation(); nav('mr', event.mr_id, { repo_id: event.repo_id }); }} title="View merge request">
                            <Icon name="git-merge" size={10} /> {event.entity_name && event.entity_type === 'mr' ? event.entity_name : 'MR'}
                          </button>
                        {/if}
                        {#if event.task_id}
                          <button class="activity-entity-badge" onclick={(e) => { e.stopPropagation(); nav('task', event.task_id, { repo_id: event.repo_id }); }} title="View task">
                            <Icon name="task" size={10} /> {event.entity_name && event.entity_type === 'task' ? event.entity_name : event.task_id.slice(0, 8)}
                          </button>
                        {/if}
                        {#if event.spec_path}
                          <button class="activity-entity-badge" onclick={(e) => { e.stopPropagation(); nav('spec', event.spec_path, { path: event.spec_path, repo_id: event.repo_id }); }} title="View spec">
                            <Icon name="spec" size={10} /> {event.spec_path.split('/').pop()?.replace(/\.md$/, '')}
                          </button>
                        {/if}
                      </span>
                      {#if event.repo_id && repoMap[event.repo_id]}
                        <button class="activity-repo-tag activity-repo-tag-clickable" onclick={(e) => { e.stopPropagation(); onSelectRepo?.(repoMap[event.repo_id]); }} title="Go to repo">
                          {repoMap[event.repo_id].name}
                        </button>
                      {/if}
                      {#if event.timestamp ?? event.created_at}
                        <span class="activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
                      {/if}
                    </div>
                    {#if event.description && event.description !== event.title && event.description !== event.entity_name && !event.description.startsWith('{')}
                      <p class="activity-reason">{event.description.length > 140 ? event.description.slice(0, 140) + '...' : event.description}</p>
                    {/if}
                  </div>
                </button>
              {/each}
            </div>
            {#if filteredActivity.length > activityLimit}
              <button class="show-more-btn" onclick={() => { activityLimit += 20; }}>
                Show more ({filteredActivity.length - activityLimit} remaining)
              </button>
            {/if}
          {/if}
        </div>
      </section>

    </div><!-- .focused-dashboard -->
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
  /* ═══ Focused Dashboard ═══════════════════════════════════════════════ */
  .workspace-home {
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }

  .focused-dashboard {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
  }

  .ws-description {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: calc(-1 * var(--space-3)) 0 0 0;
    line-height: 1.4;
  }

  /* ── Briefing compact ────────────────────────────────────────────────── */
  /* ── Provenance summary bar ──────────────────────────────────────────── */
  .provenance-summary-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-wrap: wrap;
  }

  .prov-sep {
    color: var(--color-border-strong);
  }

  .prov-item {
    white-space: nowrap;
  }

  .prov-merged {
    color: var(--color-success);
    font-weight: 500;
  }

  .prov-pending {
    color: var(--color-warning);
    font-weight: 500;
  }

  /* ── Compact briefing one-liner ────────────────────────────────────────── */
  .briefing-oneliner {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    line-height: 1.4;
    padding: 0 var(--space-1);
  }

  .briefing-exception-count {
    color: var(--color-warning);
    font-weight: 500;
  }

  /* ── Sidebar spec items with approve/reject ─────────────────────────── */
  .sidebar-spec-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: nowrap;
  }

  .sidebar-spec-name {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    color: var(--color-text);
    text-align: left;
    flex: 1;
    min-width: 0;
    padding: 0;
  }

  .sidebar-spec-name:hover .sidebar-item-name { color: var(--color-primary); text-decoration: underline; }

  .sidebar-spec-actions {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  .sidebar-approve-btn,
  .sidebar-reject-btn {
    font-family: var(--font-body);
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    border: none;
    transition: background var(--transition-fast);
  }

  .sidebar-approve-btn {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }
  .sidebar-approve-btn:hover { background: color-mix(in srgb, var(--color-success) 25%, transparent); }

  .sidebar-reject-btn {
    color: var(--color-text-muted);
    background: transparent;
  }
  .sidebar-reject-btn:hover { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 12%, transparent); }

  .sidebar-action-done {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-success);
    flex-shrink: 0;
  }

  .sidebar-action-rejected {
    color: var(--color-text-muted);
  }

  /* ── Repos + Merge Queue two-column layout ─────────────────────────── */
  .repos-and-queue {
    display: grid;
    grid-template-columns: 1fr;
    gap: var(--space-4);
  }

  .repos-and-queue.has-queue {
    grid-template-columns: 1fr 280px;
  }

  @media (max-width: 900px) {
    .repos-and-queue.has-queue {
      grid-template-columns: 1fr;
    }
  }

  .merge-queue-sidebar {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
    align-self: start;
  }

  .mq-sidebar-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3);
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .mq-sidebar-count {
    font-size: var(--text-xs);
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border-radius: 8px;
    padding: 0 5px;
    min-width: 16px;
    text-align: center;
    line-height: 16px;
    font-weight: 700;
  }

  .mq-sidebar-list {
    display: flex;
    flex-direction: column;
  }

  .mq-sidebar-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: background var(--transition-fast);
  }

  .mq-sidebar-item:last-child { border-bottom: none; }
  .mq-sidebar-item:hover { background: var(--color-surface-elevated); }

  .mq-sidebar-item-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .mq-sidebar-item-title {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mq-sidebar-item-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 10px;
    color: var(--color-text-muted);
  }

  /* ── Repo cards grid (responsive) ──────────────────────────────────── */
  .repo-cards-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-2);
  }

  .repo-header-actions {
    display: flex;
    gap: var(--space-2);
  }

  /* ── Activity dots ─────────────────────────────────────────────────── */
  .activity-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--color-text-muted);
  }

  .activity-dot-success { background: var(--color-success); }
  .activity-dot-danger { background: var(--color-danger); }
  .activity-dot-warning { background: var(--color-warning); }
  .activity-dot-info { background: var(--color-info, #1e90ff); }

  /* ── Overview layout (unused, kept for compat) ──────────────────── */

  .show-more-btn {
    display: block;
    width: 100%;
    padding: var(--space-2);
    background: transparent;
    border: none;
    border-top: 1px solid var(--color-border);
    color: var(--color-link);
    font-size: var(--text-xs);
    cursor: pointer;
    text-align: center;
    font-family: var(--font-body);
  }

  .show-more-btn:hover { background: var(--color-surface-elevated); text-decoration: underline; }

  /* Summary cards */
  .overview-summary {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .summary-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .summary-card:hover { border-color: var(--color-primary); box-shadow: var(--shadow-sm); }
  .summary-card:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .summary-card-budget { cursor: default; }
  .summary-card-budget:hover { border-color: var(--color-border); box-shadow: none; }

  .summary-card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .summary-card-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    font-size: 10px;
    font-weight: 700;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .summary-card-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    flex: 1;
  }

  .summary-card-count {
    font-size: var(--text-lg);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .summary-card-breakdown {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .summary-stat {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
  }

  .summary-stat-warn { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 10%, transparent); }
  .summary-stat-ok { color: var(--color-success); }
  .summary-stat-active { color: var(--color-success); font-weight: 600; }
  .summary-stat-danger { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .summary-stat-info { color: var(--color-info, #1e90ff); }

  .budget-mini {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-xs);
    width: 100%;
  }

  .budget-mini-label { color: var(--color-text-muted); }
  .budget-mini-value { font-weight: 600; font-family: var(--font-mono); color: var(--color-text); }
  .budget-mini-value.budget-warn { color: var(--color-warning); }

  /* ── Activity feed (full-width) ──────────────────── */
  .ws-feed-panel {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .feed-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .feed-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .feed-body {
    max-height: 400px;
    overflow-y: auto;
  }

  /* Sidebar styles removed — entity summaries moved to PipelineOverview expansion */

  /* ═══ Original styles ══════════════════════════════════════════════════ */

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
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* Briefing section (unused — section removed) */
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

  .agent-elapsed {
    font-size: 10px;
    color: var(--color-text-muted);
    margin-left: var(--space-1);
    font-family: var(--font-mono);
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
    min-height: 26px;
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
    flex-direction: column;
    gap: 0;
    padding: 1px 0 var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    min-width: 0;
  }

  .activity-main-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .activity-reason {
    margin: 0;
    font-size: 10px;
    color: var(--color-text-muted);
    line-height: 1.4;
    padding-left: calc(16px + var(--space-2));
  }

  .activity-icon {
    flex-shrink: 0;
    width: 16px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 700;
    opacity: 0.7;
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

  .activity-entity-badges {
    display: inline-flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .activity-entity-badge {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-accent) 25%, transparent);
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    cursor: pointer;
    white-space: nowrap;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.6;
  }

  .activity-entity-badge:hover {
    background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    border-color: var(--color-accent);
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

  .activity-repo-tag-clickable {
    cursor: pointer;
  }

  .activity-repo-tag-clickable:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
    background: var(--color-surface-hover);
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

  .merge-queue-section {
    border-left: 3px solid var(--color-primary);
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

  .mq-gates-mini {
    display: flex;
    gap: 2px;
    font-size: 10px;
  }

  .mq-connector {
    display: none; /* vertical border-left provides visual continuity */
  }

  @media (prefers-reduced-motion: reduce) {
    .skeleton-row { animation: none; }
    .inline-btn, .section-btn, .repo-btn, .filter-select, .retry-btn { transition: none; }
  }
</style>
