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
  import { entityName, shortId, formatId, seedEntityName, seedFromEntities } from '../lib/entityNames.svelte.js';
  import { relativeTime, formatDuration } from '../lib/timeFormat.js';
  import { specStatusTooltip, taskStatusTooltip, mrStatusTooltip, agentStatusTooltip, SPEC_STATUS_ICONS } from '../lib/statusTooltips.js';
  import RepoCard from './RepoCard.svelte';
  import Modal from '../lib/Modal.svelte';
  import Icon from '../lib/Icon.svelte';
  import CopyableId from '../lib/CopyableId.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  const goToAgentRules = getContext('goToAgentRules') ?? null;
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings') ?? null;

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

  // ── Workspace overview tab state ────────────────────────────────────────
  // Pipeline stages navigate directly to the single repo's tab when there's only one repo.
  // When multiple repos exist, clicking a stage expands a minimal summary below the bar.
  let wsTab = $state(null); // 'specs' | 'tasks' | 'mrs' | 'agents' | 'budget' | null

  const goToRepoTab = getContext('goToRepoTab') ?? null;

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
          const details = arr.map((g, idx) => {
            const gateType = g.gate_type ?? '';
            const gateTypeLabel = gateType ? gateType.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) : '';
            const gateCommand = g.command ?? '';
            // Build a descriptive name: prefer gate_name, then formatted gate_type, then extract from command
            const name = g.gate_name ?? g.name ?? (gateTypeLabel
              || (gateCommand ? gateCommand.split(' ')[0].split('/').pop() : '')
              || `Check #${idx + 1}`);
            return {
              name,
              status: (g.status === 'Passed' || g.status === 'passed') ? 'passed' : (g.status === 'Failed' || g.status === 'failed') ? 'failed' : 'pending',
              gate_type: g.gate_type,
              required: g.required,
              output: g.output,
              error: g.error,
              command: g.command,
              duration_ms: (g.started_at && g.finished_at) ? Math.round((g.finished_at - g.started_at) * 1000) : g.duration_ms,
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

  /** Format an absolute timestamp for tooltip display */
  function absTime(ts) {
    if (!ts) return '';
    try {
      const d = new Date(typeof ts === 'number' ? (ts < 1e12 ? ts * 1000 : ts) : ts);
      return d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
    } catch { return ''; }
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

  // ── Activity pagination ──────────────────────────────────────
  let activityLimit = $state(5);  // show enough activity to be useful

  // (pipeline-detail list expansion removed — entities are browsed per-repo)

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
      .map(a => a.name ?? formatId('agent', a.id));
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
    const taskTitle = body.task_title ?? '';
    const gateName = body.gate_name ?? '';
    switch (notifType) {
      case 'AgentCompleted': return agentName ? `Agent "${agentName}" finished implementing` : 'Agent completed work';
      case 'AgentFailed': return agentName ? `Agent "${agentName}" encountered an error` : 'Agent failed';
      case 'MrMerged': return mrTitle ? `"${mrTitle}" passed all gates and was merged` : 'MR merged successfully';
      case 'MrCreated': return mrTitle ? `"${mrTitle}" created from agent work` : 'New MR created';
      case 'MrNeedsReview': return mrTitle ? `"${mrTitle}" is ready for review` : 'MR needs review';
      case 'SpecApproved': return specPath ? `"${specPath}" approved — agents can begin` : 'Spec approved';
      case 'SpecRejected': return specPath ? `"${specPath}" rejected — implementation blocked` : 'Spec rejected';
      case 'SpecChanged': return specPath ? `"${specPath}" was updated` : 'Spec updated';
      case 'SpecPendingApproval': return specPath ? `"${specPath}" pushed — needs your approval` : 'Spec needs approval';
      case 'GateFailure': return gateName ? `Gate "${gateName}" failed — merge blocked` : 'A quality gate failed';
      case 'TaskCreated': return taskTitle ? `"${taskTitle}" created${specPath ? ` from spec "${specPath}"` : ''}` : (specPath ? `Task created from spec "${specPath}"` : 'Task created');
      case 'SuggestedSpecLink': return mrTitle ? `MR "${mrTitle}" may relate to a spec` : 'Spec link suggested';
      case 'BudgetWarning': return 'Budget threshold exceeded — consider adjusting limits';
      case 'MetaSpecDrift': return 'Agent rules changed — repos may need reconciliation';
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

  // ── Derived: actionable notifications for sidebar visibility ─────────
  const ACTIONABLE_TYPES = new Set(['spec_approval', 'gate_failure', 'agent_clarification', 'budget_warning', 'mr_needs_review', 'agent_failed']);
  let actionableNotifications = $derived(notifications.filter(n => {
    const nt = NOTIF_TYPE_NORM[n.notification_type] ?? n.notification_type;
    return ACTIONABLE_TYPES.has(nt) && !n.dismissed_at && !n.resolved_at;
  }));

  // ── Derived: provenance summary counts ──────────────────────────────
  let provenanceSummary = $derived.by(() => {
    const approved = specs.filter(s => s.approval_status === 'approved' || s.status === 'approved').length;
    const pending = specs.filter(s => s.approval_status === 'pending' || s.status === 'pending').length;
    const activeAgentCount = wsAgents.filter(a => a.status === 'active').length;
    const mergedMrs = wsMrs.filter(m => m.status === 'merged').length;
    const openMrs = wsMrs.filter(m => m.status === 'open').length;
    const inProgressTasks = wsTasks.filter(t => t.status === 'in_progress').length;
    const failedGates = wsMrs.filter(m => m._gates?.failed > 0).length;
    return { approved, pending, activeAgentCount, mergedMrs, openMrs, failedGates, inProgressTasks, totalTasks: wsTasks.length };
  });

  // ── Structured status items (each clickable) ───────────────────────────
  let statusItems = $derived.by(() => {
    const s = provenanceSummary;
    const items = [];
    if (s.failedGates > 0) items.push({ text: `${s.failedGates} MR${s.failedGates !== 1 ? 's have' : ' has'} failed gates`, variant: 'danger', tab: 'mrs', icon: '✗' });
    if (s.pending > 0) items.push({ text: `${s.pending} spec${s.pending !== 1 ? 's need' : ' needs'} approval`, variant: 'warning', tab: 'specs', icon: '!' });
    if (s.openMrs > 0 && s.failedGates === 0) items.push({ text: `${s.openMrs} MR${s.openMrs !== 1 ? 's' : ''} ready to merge`, variant: 'info', tab: 'mrs', icon: '→' });
    if (s.activeAgentCount > 0) items.push({ text: `${s.activeAgentCount} agent${s.activeAgentCount !== 1 ? 's' : ''} implementing code`, variant: 'success', tab: 'agents', icon: '⚙' });
    if (s.mergedMrs > 0 && items.length < 3) items.push({ text: `${s.mergedMrs} MR${s.mergedMrs !== 1 ? 's' : ''} merged`, variant: 'muted', tab: 'mrs', icon: '✓' });
    return items;
  });

  let statusSentence = $derived.by(() => {
    const s = provenanceSummary;
    // Build a natural sentence summarizing the workspace state
    if (specs.length === 0 && repos.length === 0) return 'Get started by creating a repo and pushing specs.';
    if (specs.length === 0 && repos.length > 0) return `${repos.length} repo${repos.length !== 1 ? 's' : ''} ready. Push specs to start the autonomous pipeline.`;

    const parts = [];
    if (s.failedGates > 0) parts.push(`${s.failedGates} MR${s.failedGates !== 1 ? 's have' : ' has'} failed gates`);
    if (s.pending > 0) parts.push(`${s.pending} spec${s.pending !== 1 ? 's' : ''} awaiting approval`);
    if (s.activeAgentCount > 0) parts.push(`${s.activeAgentCount} agent${s.activeAgentCount !== 1 ? 's' : ''} running`);
    if (s.openMrs > 0 && s.failedGates === 0) parts.push(`${s.openMrs} open MR${s.openMrs !== 1 ? 's' : ''}`);

    if (parts.length > 0) return parts.join(' · ') + '.';

    if (s.mergedMrs > 0 && s.activeAgentCount === 0 && s.openMrs === 0 && s.pending === 0) {
      return `All clear — ${s.mergedMrs} MR${s.mergedMrs !== 1 ? 's' : ''} merged across ${repos.length} repo${repos.length !== 1 ? 's' : ''}.`;
    }
    if (s.approved > 0 && s.totalTasks === 0) return `${s.approved} spec${s.approved !== 1 ? 's' : ''} approved — tasks will be created automatically.`;
    if (s.totalTasks > 0 && s.activeAgentCount === 0) return `${s.totalTasks} task${s.totalTasks !== 1 ? 's' : ''} tracked, no agents running.`;
    return `${repos.length} repo${repos.length !== 1 ? 's' : ''}, no active work.`;
  });

  // ── Budget percentage ──────────────────────────────────────────────────
  let budgetPct = $derived.by(() => {
    if (!budgetData) return null;
    const maxTokens = budgetData.max_tokens_per_day ?? 0;
    if (!maxTokens) return null;
    const used = budgetData.tokens_used_today ?? 0;
    return Math.min(100, Math.round((used / maxTokens) * 100));
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
    <div class="focused-dashboard">

      <!-- ── Workspace header (lean: name + status + actions) ──────── -->
      <header class="ws-header">
        <div class="ws-header-top-row">
          <h1 class="ws-header-name">{workspace.name ?? workspace.slug ?? 'Workspace'}</h1>
          {#if budgetPct !== null}
            <span class="ws-budget-indicator" class:ws-budget-warn={budgetPct > 70} class:ws-budget-danger={budgetPct > 90} title="Budget: {budgetPct}% of daily token limit used">
              <span class="ws-budget-bar"><span class="ws-budget-fill" style="width: {budgetPct}%"></span></span>
              <span class="ws-budget-label">{budgetPct}%</span>
            </span>
          {/if}
        </div>
        {#if !specsLoading && !tasksLoading && !mrsLoading && !agentsLoading}
          {#if statusItems.length > 0}
            <div class="ws-header-status-items">
              {#each statusItems as item}
                {@const singleRepo = repos.length === 1 ? repos[0] : null}
                <button class="ws-status-chip ws-status-chip-{item.variant}" onclick={() => { if (singleRepo && item.tab) { onSelectRepo?.(singleRepo, item.tab); } else { wsTab = item.tab; } }}>
                  <span class="ws-status-icon">{item.icon}</span>
                  {item.text}
                </button>
              {/each}
            </div>
          {:else}
            <p class="ws-header-status">{statusSentence}</p>
          {/if}
        {/if}
      </header>

      <!-- ── Pipeline flow bar (compact) ──────────────────────────── -->
      {#if !specsLoading && !tasksLoading && !mrsLoading && !agentsLoading && (specs.length > 0 || wsTasks.length > 0 || wsAgents.length > 0 || wsMrs.length > 0)}
        {@const singleRepo = repos.length === 1 ? repos[0] : null}
        <nav class="pipeline-bar" aria-label="Pipeline status">
          <button class="pipeline-stage" class:pipeline-stage-active={pipelineSpecs.pending > 0} class:pipeline-stage-done={pipelineSpecs.approved > 0 && pipelineSpecs.pending === 0} onclick={() => { if (singleRepo) { onSelectRepo?.(singleRepo, 'specs'); } else { wsTab = wsTab === 'specs' ? null : 'specs'; } }} title="{pipelineSpecs.approved}/{pipelineSpecs.total} specs approved{pipelineSpecs.pending > 0 ? `, ${pipelineSpecs.pending} need approval` : ''}">
            <span class="pipeline-stage-count">{pipelineSpecs.total}</span>
            <span class="pipeline-stage-label">Specs</span>
            {#if pipelineSpecs.pending > 0}<span class="pipeline-stage-badge pipeline-badge-warning">{pipelineSpecs.pending}</span>{/if}
          </button>
          <span class="pipeline-arrow" aria-hidden="true">→</span>
          <button class="pipeline-stage" class:pipeline-stage-active={pipelineTasks.in_progress > 0} class:pipeline-stage-alert={pipelineTasks.blocked > 0} onclick={() => { if (singleRepo) { onSelectRepo?.(singleRepo, 'tasks'); } else { wsTab = wsTab === 'tasks' ? null : 'tasks'; } }} title="{pipelineTasks.done}/{pipelineTasks.total} tasks done{pipelineTasks.in_progress > 0 ? `, ${pipelineTasks.in_progress} active` : ''}{pipelineTasks.blocked > 0 ? `, ${pipelineTasks.blocked} blocked` : ''}">
            <span class="pipeline-stage-count">{pipelineTasks.total}</span>
            <span class="pipeline-stage-label">Tasks</span>
            {#if pipelineTasks.blocked > 0}<span class="pipeline-stage-badge pipeline-badge-danger">{pipelineTasks.blocked}</span>
            {:else if pipelineTasks.in_progress > 0}<span class="pipeline-stage-badge pipeline-badge-active">{pipelineTasks.in_progress}</span>{/if}
          </button>
          <span class="pipeline-arrow" aria-hidden="true">→</span>
          <button class="pipeline-stage" class:pipeline-stage-active={pipelineAgents.active > 0} onclick={() => { if (singleRepo) { onSelectRepo?.(singleRepo, 'agents'); } else { wsTab = wsTab === 'agents' ? null : 'agents'; } }} title="{pipelineAgents.active} of {pipelineAgents.total} agents running">
            <span class="pipeline-stage-count">{pipelineAgents.total}</span>
            <span class="pipeline-stage-label">Agents</span>
            {#if pipelineAgents.active > 0}<span class="pipeline-stage-badge pipeline-badge-success">{pipelineAgents.active}</span>{/if}
          </button>
          <span class="pipeline-arrow" aria-hidden="true">→</span>
          <button class="pipeline-stage" class:pipeline-stage-active={pipelineMrs.open > 0} class:pipeline-stage-alert={pipelineMrs.failed_gates > 0} onclick={() => { if (singleRepo) { onSelectRepo?.(singleRepo, 'mrs'); } else { wsTab = wsTab === 'mrs' ? null : 'mrs'; } }} title="{pipelineMrs.merged}/{pipelineMrs.total} MRs merged{pipelineMrs.open > 0 ? `, ${pipelineMrs.open} open` : ''}{pipelineMrs.failed_gates > 0 ? `, ${pipelineMrs.failed_gates} gate failures` : ''}">
            <span class="pipeline-stage-count">{pipelineMrs.total}</span>
            <span class="pipeline-stage-label">MRs</span>
            {#if pipelineMrs.failed_gates > 0}<span class="pipeline-stage-badge pipeline-badge-danger">{pipelineMrs.failed_gates}</span>
            {:else if pipelineMrs.open > 0}<span class="pipeline-stage-badge pipeline-badge-active">{pipelineMrs.open}</span>{/if}
          </button>
          {#if mergeQueueItems.length > 0}
            <span class="pipeline-arrow" aria-hidden="true">→</span>
            <button class="pipeline-stage pipeline-stage-active" onclick={() => { if (singleRepo) { onSelectRepo?.(singleRepo, 'mrs'); } else { wsTab = wsTab === 'mrs' ? null : 'mrs'; } }} title="{mergeQueueItems.length} MR{mergeQueueItems.length !== 1 ? 's' : ''} in merge queue">
              <span class="pipeline-stage-count">{mergeQueueItems.length}</span>
              <span class="pipeline-stage-label">Queue</span>
            </button>
          {/if}
          {#if pipelineMrs.merged > 0}
            <span class="pipeline-arrow" aria-hidden="true">→</span>
            <span class="pipeline-stage pipeline-stage-done" title="{pipelineMrs.merged} MR{pipelineMrs.merged !== 1 ? 's' : ''} merged">
              <span class="pipeline-stage-count">{pipelineMrs.merged}</span>
              <span class="pipeline-stage-label">Merged</span>
            </span>
          {/if}
        </nav>
      {/if}

      <!-- ── Pipeline detail: compact multi-repo summary (only for multi-repo workspaces) ──── -->
      {#if wsTab && repos.length > 1}
        <div class="pipeline-detail-compact" data-testid="pipeline-detail">
          <div class="pipeline-detail-header">
            <span class="pipeline-detail-title">
              {wsTab === 'specs' ? 'Specs' : wsTab === 'tasks' ? 'Tasks' : wsTab === 'mrs' ? 'Merge Requests' : wsTab === 'agents' ? 'Agents' : 'Budget'} across {repos.length} repos
            </span>
            <button class="pipeline-detail-close" onclick={() => { wsTab = null; }} title="Close" aria-label="Close panel">✕</button>
          </div>
          <div class="pipeline-detail-body">
            <!-- Group by repo for quick navigation -->
            {#each repos as repo (repo.id)}
              {@const repoSpecs = wsTab === 'specs' ? specs.filter(s => s.repo_id === repo.id) : []}
              {@const repoTasksFiltered = wsTab === 'tasks' ? wsTasks.filter(t => t.repo_id === repo.id) : []}
              {@const repoMrsFiltered = wsTab === 'mrs' ? wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id) : []}
              {@const repoAgentsFiltered = wsTab === 'agents' ? wsAgents.filter(a => a.repo_id === repo.id) : []}
              {@const count = wsTab === 'specs' ? repoSpecs.length : wsTab === 'tasks' ? repoTasksFiltered.length : wsTab === 'mrs' ? repoMrsFiltered.length : repoAgentsFiltered.length}
              {#if count > 0}
                <button class="pipeline-repo-row" onclick={() => onSelectRepo?.(repo, wsTab === 'mrs' ? 'mrs' : wsTab)}>
                  <span class="pipeline-repo-name">{repo.name}</span>
                  <span class="pipeline-repo-count">{count}</span>
                  {#if wsTab === 'specs'}
                    {@const pending = repoSpecs.filter(s => (s.approval_status ?? s.status) === 'pending').length}
                    {#if pending > 0}<span class="pipeline-repo-badge pipeline-badge-warning">{pending} pending</span>{/if}
                  {:else if wsTab === 'tasks'}
                    {@const blocked = repoTasksFiltered.filter(t => t.status === 'blocked').length}
                    {@const active = repoTasksFiltered.filter(t => t.status === 'in_progress').length}
                    {#if blocked > 0}<span class="pipeline-repo-badge pipeline-badge-danger">{blocked} blocked</span>
                    {:else if active > 0}<span class="pipeline-repo-badge pipeline-badge-active">{active} active</span>{/if}
                  {:else if wsTab === 'mrs'}
                    {@const failed = repoMrsFiltered.filter(m => m._gates?.failed > 0).length}
                    {@const open = repoMrsFiltered.filter(m => m.status === 'open').length}
                    {#if failed > 0}<span class="pipeline-repo-badge pipeline-badge-danger">{failed} failed gates</span>
                    {:else if open > 0}<span class="pipeline-repo-badge pipeline-badge-active">{open} open</span>{/if}
                  {:else if wsTab === 'agents'}
                    {@const running = repoAgentsFiltered.filter(a => a.status === 'active' || a.status === 'running').length}
                    {#if running > 0}<span class="pipeline-repo-badge pipeline-badge-success">{running} running</span>{/if}
                  {/if}
                  <span class="pipeline-repo-arrow">→</span>
                </button>
              {/if}
            {/each}
          </div>
        </div>
      {/if}

      <!-- ── Main content: single-column layout ──────────────────── -->
      <div class="ws-main-content">

          <!-- Action Needed (compact, dismissible) -->
          {#if !decisionsLoading && actionableNotifications.length > 0}
            {@const hasDangerDecision = actionableNotifications.some(n => { const nt = NOTIF_TYPE_NORM[n.notification_type] ?? n.notification_type; return nt === 'gate_failure' || nt === 'agent_failed'; })}
            <section class="ws-decisions-section" class:decisions-danger={hasDangerDecision} data-testid="section-decisions">
              <div class="decisions-header">
                <h2 class="decisions-title">
                  {actionableNotifications.length} item{actionableNotifications.length !== 1 ? 's need' : ' needs'} attention
                </h2>
                {#if actionableNotifications.length > 3}
                  <button class="section-btn" onclick={() => showAllDecisions = !showAllDecisions}>
                    {showAllDecisions ? 'Show less' : `Show all`}
                  </button>
                {/if}
              </div>
              <div class="decisions-list">
                {#each (showAllDecisions ? actionableNotifications : actionableNotifications.slice(0, 3)) as n (n.id)}
                  {@const nt = NOTIF_TYPE_NORM[n.notification_type] ?? n.notification_type}
                  {@const body = getBody(n)}
                  {@const aState = actionStates[n.id]}
                  {@const severity = nt === 'gate_failure' || nt === 'agent_failed' ? 'danger' : nt === 'spec_approval' || nt === 'mr_needs_review' ? 'action' : 'warn'}
                  <div class="decision-item decision-severity-{severity}" class:decision-resolved={aState?.success}>
                    <div class="decision-icon decision-icon-{severity}">
                      {TYPE_ICONS[nt] ?? '?'}
                    </div>
                    <button class="decision-body" onclick={() => {
                      if (body.mr_id) nav('mr', body.mr_id, { repo_id: n.repo_id, _openTab: nt === 'gate_failure' ? 'gates' : undefined });
                      else if (body.agent_id) nav('agent', body.agent_id, { repo_id: n.repo_id, _openTab: nt === 'agent_failed' ? 'history' : undefined });
                      else if (body.task_id) nav('task', body.task_id, { repo_id: n.repo_id });
                      else if (body.spec_path) {
                        const sp = normalizeSpecPath(body.spec_path);
                        nav('spec', sp, { path: sp, repo_id: n.repo_id });
                      }
                    }}>
                      <span class="decision-type">{typeLabel(nt)}{#if n.repo_id && repoMap[n.repo_id]} · {repoMap[n.repo_id].name}{/if}</span>
                      <span class="decision-title">{n.title ?? n.message ?? ''}</span>
                      {#if nt === 'gate_failure' && body.gate_name}
                        <span class="decision-detail">
                          Gate "{body.gate_name}" failed{#if body.gate_type} ({body.gate_type.replace(/_/g, ' ')}){/if} — merge blocked
                          {#if body.error}<code class="decision-error-preview">{body.error.split('\n')[0]?.slice(0, 100)}</code>{/if}
                        </span>
                      {:else if nt === 'gate_failure' && body.mr_id}
                        <span class="decision-detail">Quality gate failed on {entityName('mr', body.mr_id)}</span>
                      {:else if nt === 'agent_failed' && body.agent_name}
                        <span class="decision-detail">Agent "{body.agent_name}" stopped — check logs for root cause</span>
                      {:else if nt === 'agent_failed' && body.agent_id}
                        <span class="decision-detail">{entityName('agent', body.agent_id)} encountered an error</span>
                      {:else if nt === 'spec_approval' && body.spec_path}
                        <span class="decision-detail">Agents cannot begin until "{body.spec_path.split('/').pop()?.replace(/\.md$/, '')}" is approved</span>
                      {:else if nt === 'mr_needs_review' && body.mr_id}
                        <span class="decision-detail">{entityName('mr', body.mr_id)} is ready for human review</span>
                      {:else if nt === 'budget_warning'}
                        <span class="decision-detail">Budget threshold exceeded — consider adjusting limits</span>
                      {/if}
                      {#if n.created_at}
                        <span class="decision-time">{relTime(n.created_at)}</span>
                      {/if}
                    </button>
                    <div class="decision-actions">
                      {#if aState?.success}
                        <span class="decision-done">{aState.message}</span>
                      {:else if aState?.loading}
                        <span class="decision-loading">...</span>
                      {:else}
                        {#if nt === 'spec_approval'}
                          <button class="inline-action-btn inline-action-approve" onclick={() => handleApproveSpec(n)}>Approve</button>
                          <button class="inline-action-btn inline-action-reject" onclick={() => handleRejectSpec(n)}>Reject</button>
                        {:else if nt === 'gate_failure'}
                          <button class="inline-action-btn inline-action-approve" onclick={() => handleRetry(n)}>Retry</button>
                        {:else if nt === 'mr_needs_review'}
                          <button class="inline-action-btn inline-action-approve" onclick={() => { const mrId = body.mr_id; if (mrId) nav('mr', mrId, { repo_id: n.repo_id }); }}>Review</button>
                        {/if}
                        <button class="inline-action-btn inline-action-reject" onclick={() => handleDismiss(n)} title="Dismiss">✕</button>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          <!-- Specs Awaiting Review (primary human touchpoint) -->
          {#if !specsLoading}
            {@const pendingSpecs = specs.filter(s => (s.approval_status ?? s.status) === 'pending')}
            {#if pendingSpecs.length > 0}
              <section class="ws-pending-specs-section" data-testid="section-pending-specs">
                <h2 class="section-heading">Specs awaiting review <span class="pending-specs-badge">{pendingSpecs.length}</span></h2>
                <div class="pending-specs-list">
                  {#each pendingSpecs as spec (spec.path)}
                    {@const specRepo = repoMap[spec.repo_id]}
                    {@const actionState = specActionStates[spec.path]}
                    <div class="pending-spec-item" class:pending-spec-resolved={actionState === 'approved' || actionState === 'rejected'}>
                      <button class="pending-spec-body" onclick={() => nav('spec', spec.path, { path: spec.path, repo_id: spec.repo_id })}>
                        <span class="pending-spec-name">{spec.path.split('/').pop()?.replace(/\.md$/, '') ?? spec.path}</span>
                        <span class="pending-spec-meta">
                          {#if specRepo}<span class="pending-spec-repo">{specRepo.name}</span>{/if}
                          {#if spec.kind}<span class="pending-spec-kind">{spec.kind}</span>{/if}
                          {#if spec.created_at ?? spec.synced_at}
                            <span class="pending-spec-time">pushed {relTime(spec.created_at ?? spec.synced_at)}</span>
                          {/if}
                        </span>
                      </button>
                      <div class="pending-spec-actions">
                        {#if actionState === 'loading'}
                          <span class="decision-loading">...</span>
                        {:else if actionState === 'approved'}
                          <span class="pending-spec-done">Approved</span>
                        {:else if actionState === 'rejected'}
                          <span class="pending-spec-done">Rejected</span>
                        {:else if actionState === 'error'}
                          <span class="pending-spec-error">Failed</span>
                        {:else}
                          <button class="inline-action-btn inline-action-approve" onclick={(e) => quickApproveSpec(spec, e)}>Approve</button>
                          <button class="inline-action-btn inline-action-reject" onclick={(e) => quickRejectSpec(spec, e)}>Reject</button>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </section>
            {/if}
          {/if}

          <!-- Active Agents (live work indicator) -->
          {#if !agentsLoading && wsAgents.filter(a => a.status === 'active').length > 0}
            {@const activeList = wsAgents.filter(a => a.status === 'active')}
            <section class="ws-active-agents" data-testid="section-active-agents">
              <h2 class="section-heading"><span class="live-dot"></span> {activeList.length} agent{activeList.length !== 1 ? 's' : ''} working</h2>
              <div class="active-agents-grid">
                {#each activeList as agent (agent.id)}
                  {@const agentRepo = repoMap[agent.repo_id]}
                  <button class="active-agent-card" onclick={() => nav('agent', agent.id, { repo_id: agent.repo_id, name: agent.name })}>
                    <div class="active-agent-header">
                      <span class="active-agent-name">{agent.name ?? formatId('agent', agent.id)}</span>
                      {#if agent.spawned_at ?? agent.created_at}
                        <span class="active-agent-time">{formatDuration(agent.spawned_at ?? agent.created_at)}</span>
                      {/if}
                    </div>
                    <div class="active-agent-context">
                      {#if agent.spec_path}
                        <span class="active-agent-spec">{agent.spec_path.split('/').pop()?.replace(/\.md$/, '')}</span>
                      {/if}
                      {#if agent.current_task_id ?? agent.task_id}
                        <span class="active-agent-task">{entityName('task', agent.current_task_id ?? agent.task_id)}</span>
                      {/if}
                    </div>
                    <div class="active-agent-actions">
                      {#if agentRepo}<span class="active-agent-repo">{agentRepo.name}</span>{/if}
                      <span class="active-agent-view">View logs</span>
                    </div>
                  </button>
                {/each}
              </div>
            </section>
          {/if}

          <!-- Merge Queue (items awaiting merge) -->
          {#if !mergeQueueLoading && mergeQueueItems.length > 0}
            <section class="ws-merge-queue-section" data-testid="section-merge-queue">
              <h2 class="section-heading">Merge Queue <span class="queue-count-badge">{mergeQueueItems.length}</span></h2>
              <div class="merge-queue-list">
                {#each mergeQueueItems as item, i (item.merge_request_id ?? item.mr_id ?? i)}
                  {@const mrId = item.merge_request_id ?? item.mr_id}
                  {@const mr = item._mr ?? {}}
                  {@const mrRepo = repoMap[mr.repository_id ?? mr.repo_id]}
                  <button class="merge-queue-item" onclick={() => nav('mr', mrId, { repo_id: mr.repository_id ?? mr.repo_id, title: item._title })}>
                    <span class="queue-position">#{i + 1}</span>
                    <div class="queue-item-info">
                      <span class="queue-item-title">{item._title}</span>
                      <span class="queue-item-meta">
                        {#if item._branch}<span class="queue-item-branch">{item._branch}</span>{/if}
                        {#if mrRepo}<span class="queue-item-repo">{mrRepo.name}</span>{/if}
                        {#if item._spec_ref}<span class="queue-item-spec">{item._spec_ref.split('/').pop()?.replace(/\.md$/, '').replace(/@.*$/, '')}</span>{/if}
                      </span>
                    </div>
                    {#if item._deps?.length > 0}
                      <span class="queue-item-deps" title="Depends on {item._deps.length} other MR{item._deps.length !== 1 ? 's' : ''}">
                        {item._deps.length} dep{item._deps.length !== 1 ? 's' : ''}
                      </span>
                    {/if}
                    <span class="queue-item-status queue-item-status-{item._status ?? 'pending'}">{item._status === 'merged' ? 'Merged' : item._status === 'open' ? 'Open' : 'Queued'}</span>
                  </button>
                {/each}
              </div>
            </section>
          {/if}

          <!-- Repos (primary content) -->
          <section class="repos-section" data-testid="section-repos">
            <div class="section-header-row">
              <h2 class="section-heading">Repositories</h2>
              {#if repos.length > 0}
                <div class="section-header-actions">
                  <button class="section-btn section-btn-compact" onclick={() => { newRepoOpen = !newRepoOpen; importOpen = false; }} data-testid="btn-new-repo-top">+ New</button>
                  <button class="section-btn section-btn-compact" onclick={() => { importOpen = !importOpen; newRepoOpen = false; }} data-testid="btn-import-repo-top">Import</button>
                </div>
              {/if}
            </div>
            <div class="feed-body">
              {#if reposLoading}
                <div class="skeleton-row"></div>
              {:else if reposError}
                <div class="error-row" role="alert">
                  <p class="error-text">{reposError}</p>
                  <button class="retry-btn" onclick={loadRepos}>{$t('common.retry')}</button>
                </div>
              {:else if repos.length === 0}
                <div class="empty-state-guided" data-testid="repos-empty">
                  <p class="empty-text">No repositories yet</p>
                  <p class="empty-guide">Create a repo, push your code with specs, and Gyre handles the rest.</p>
                  <div class="pipeline-guide">
                    <div class="pipeline-step"><span class="pipeline-step-num">1</span><span class="pipeline-step-title">Create a repo</span><span class="pipeline-step-desc">Click "+ New" above</span></div>
                    <span class="pipeline-arrow">→</span>
                    <div class="pipeline-step"><span class="pipeline-step-num">2</span><span class="pipeline-step-title">Push specs</span><span class="pipeline-step-desc">specs/manifest.yaml + .md files</span></div>
                    <span class="pipeline-arrow">→</span>
                    <div class="pipeline-step"><span class="pipeline-step-num">3</span><span class="pipeline-step-title">Approve</span><span class="pipeline-step-desc">Review & approve specs</span></div>
                    <span class="pipeline-arrow">→</span>
                    <div class="pipeline-step"><span class="pipeline-step-num">4</span><span class="pipeline-step-title">Agents implement</span><span class="pipeline-step-desc">Autonomous code + MR</span></div>
                    <span class="pipeline-arrow">→</span>
                    <div class="pipeline-step"><span class="pipeline-step-num">5</span><span class="pipeline-step-title">Gates & merge</span><span class="pipeline-step-desc">Test, lint, attest, merge</span></div>
                  </div>
                  <div class="empty-actions">
                    <button class="section-btn primary" onclick={() => { newRepoOpen = true; }}>Create your first repo</button>
                  </div>
                </div>
              {:else}
                <div class="repo-cards-list">
                  {#each repos.slice().sort((a, b) => {
                    const aStats = repoStats(a);
                    const bStats = repoStats(b);
                    if (bStats.agents !== aStats.agents) return bStats.agents - aStats.agents;
                    const aTime = aStats.last_activity ? new Date(aStats.last_activity).getTime() : 0;
                    const bTime = bStats.last_activity ? new Date(bStats.last_activity).getTime() : 0;
                    return bTime - aTime;
                  }) as repo (repo.id)}
                    <RepoCard
                      {repo}
                      health={repoHealth(repo)}
                      stats={repoStats(repo)}
                      activeAgentNames={repoActiveAgentNames(repo)}
                      activeAgents={wsAgents.filter(a => a.repo_id === repo.id && a.status === 'active')}
                      failedMrs={wsMrs.filter(m => (m.repository_id ?? m.repo_id) === repo.id && m._gates?.failed > 0)}
                      specBreakdown={repoSpecBreakdown(repo)}
                      latestMr={repoLatestMr(repo)}
                      onclick={() => onSelectRepo?.(repo)}
                      onStatClick={(r, tab) => onSelectRepo?.(r, tab)}
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

          <!-- Recently Merged — celebrate shipped autonomous work -->
          {#if !mrsLoading}
            {@const recentlyMerged = wsMrs.filter(m => m.status === 'merged').sort((a, b) => (b.merged_at ?? b.updated_at ?? 0) - (a.merged_at ?? a.updated_at ?? 0)).slice(0, 5)}
            {#if recentlyMerged.length > 0}
              <details class="ws-activity-details" open>
                <summary class="ws-activity-summary">
                  <h2 class="section-heading section-heading-inline">Recently Shipped</h2>
                  <span class="activity-count-badge">{recentlyMerged.length}</span>
                </summary>
                <div class="recently-merged-list">
                  {#each recentlyMerged as mr (mr.id)}
                    {@const mrRepo = repoMap[mr.repository_id ?? mr.repo_id]}
                    {@const specPath = mr.spec_ref?.split('@')[0]}
                    {@const agentId = mr.author_agent_id ?? mr.agent_id}
                    <button class="recently-merged-item" onclick={() => nav('mr', mr.id, { repo_id: mr.repository_id ?? mr.repo_id, title: mr.title })}>
                      <div class="merged-item-main">
                        <span class="merged-item-check">✓</span>
                        <div class="merged-item-info">
                          <span class="merged-item-title">{mr.title ?? 'Untitled'}</span>
                          <span class="merged-item-meta">
                            {#if specPath}
                              <span class="merged-meta-spec">{specPath.split('/').pop()?.replace(/\.md$/, '')}</span>
                              <span class="merged-meta-arrow">→</span>
                            {/if}
                            {#if agentId}
                              <span class="merged-meta-agent">{entityName('agent', agentId)}</span>
                              <span class="merged-meta-arrow">→</span>
                            {/if}
                            <span class="merged-meta-status">merged</span>
                            {#if mr.merged_at ?? mr.updated_at}
                              <span class="merged-meta-time">{relTime(mr.merged_at ?? mr.updated_at)}</span>
                            {/if}
                          </span>
                        </div>
                      </div>
                      <div class="merged-item-right">
                        {#if mr.diff_stats}
                          <span class="merged-diff-stats">
                            <span class="diff-ins-tiny">+{mr.diff_stats.insertions ?? 0}</span>
                            <span class="diff-del-tiny">-{mr.diff_stats.deletions ?? 0}</span>
                          </span>
                        {/if}
                        {#if mr._gates?.total > 0}
                          <span class="merged-gates-mini" title="{mr._gates.passed}/{mr._gates.total} gates passed">
                            ✓{mr._gates.passed}
                          </span>
                        {/if}
                        {#if mrRepo}
                          <span class="merged-repo-badge">{mrRepo.name}</span>
                        {/if}
                      </div>
                    </button>
                  {/each}
                </div>
              </details>
            {/if}
          {/if}

          <!-- ── Activity feed (always visible, collapsible) ────── -->
          {#if activityEvents.length > 0}
            <details class="ws-activity-details" open>
              <summary class="ws-activity-summary">
                <h2 class="section-heading section-heading-inline">Activity</h2>
                <span class="activity-count-badge">{activityEvents.length}</span>
              </summary>
              <div class="activity-timeline">
                {#each activityEvents.slice(0, activityLimit) as event, i}
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
                    {#if i < Math.min(activityEvents.length, activityLimit) - 1}<div class="activity-line"></div>{/if}
                    <div class="activity-content">
                      <div class="activity-main-row">
                        <span class="activity-icon"><Icon name={activityIconName(event)} size={11} /></span>
                        <span class="activity-label">{activityLabel(event)}</span>
                        {#if event.repo_id && repoMap[event.repo_id]}<span class="activity-repo-badge">{repoMap[event.repo_id].name}</span>{/if}
                        {#if event.timestamp ?? event.created_at}
                          <span class="activity-time">{relTime(event.timestamp ?? event.created_at)}</span>
                        {/if}
                      </div>
                      {#if event.entity_name ?? event.title}
                        <p class="activity-entity-name">{event.entity_name ?? event.title}</p>
                      {/if}
                      {#if event.description && event.description !== event.title && event.description !== event.entity_name && !event.description.startsWith('{')}
                        <p class="activity-reason">{event.description.length > 120 ? event.description.slice(0, 117) + '...' : event.description}</p>
                      {/if}
                    </div>
                  </button>
                {/each}
              </div>
              {#if activityEvents.length > activityLimit}
                <button class="ws-overview-more-btn" onclick={() => { activityLimit = activityLimit <= 5 ? 30 : 5; }}>
                  {activityLimit <= 5 ? `Show all ${activityEvents.length} events` : 'Show less'}
                </button>
              {/if}
            </details>
          {/if}

      </div><!-- .ws-main-content -->

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
  /* ── Section headings ──────────────────────────────────────── */
  .section-heading {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 0 0 var(--space-2) 0;
    padding: 0 var(--space-1);
  }

  .section-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .section-header-actions {
    display: flex;
    gap: var(--space-1);
  }

  .repos-section {
    display: flex;
    flex-direction: column;
  }

  .activity-section,
  .ws-activity-section {
    display: flex;
    flex-direction: column;
  }

  .ws-activity-details {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .ws-activity-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    user-select: none;
    list-style: none;
  }

  .ws-activity-summary::-webkit-details-marker { display: none; }

  .ws-activity-summary::marker { content: ''; }

  .ws-activity-details[open] .ws-activity-summary {
    border-bottom: 1px solid var(--color-border);
  }

  .ws-activity-details .activity-timeline {
    padding: var(--space-2) var(--space-3);
    max-height: 400px;
    overflow-y: auto;
  }

  .activity-details {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .activity-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    user-select: none;
    list-style: none;
  }

  .activity-summary::-webkit-details-marker { display: none; }

  .activity-summary::marker { content: ''; }

  .section-heading-inline {
    margin: 0;
    padding: 0;
  }

  .activity-count-badge {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
  }


  .activity-details .activity-timeline {
    padding: var(--space-2) var(--space-3);
  }

  /* merge queue compact section removed — items shown on repo cards */

  /* ── Pipeline attention section ──────────────────────────────────── */
  .pipeline-attention {
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, var(--color-border));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-warning) 3%, var(--color-surface));
    overflow: hidden;
  }

  .pipeline-attention-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-warning);
    border-bottom: 1px solid color-mix(in srgb, var(--color-warning) 15%, transparent);
  }

  .pipeline-attention-icon {
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--color-warning);
    color: white;
    font-size: 11px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .pipeline-attention-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    width: 100%;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    font-family: var(--font-body);
    text-align: left;
    transition: background var(--transition-fast);
  }

  .pipeline-attention-item:last-child,
  .pipeline-attention-item:has(+ .pipeline-attention-more) { border-bottom: none; }
  .pipeline-attention-item:hover { background: var(--color-surface-elevated); }

  .attention-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    font-size: 11px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .attention-icon-warning { background: color-mix(in srgb, var(--color-warning) 15%, transparent); color: var(--color-warning); }
  .attention-icon-danger { background: color-mix(in srgb, var(--color-danger) 15%, transparent); color: var(--color-danger); }

  .attention-content {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .attention-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .attention-title {
    font-size: var(--text-xs);
    color: var(--color-text);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attention-repo {
    font-size: 10px;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .attention-why {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .attention-action {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-primary);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .pipeline-attention-more {
    display: block;
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
  }

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
    padding: var(--space-2) var(--space-5);
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
  }

  .ws-main-col {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
    min-height: 0;
  }

  /* ── Single-column main content ──────────────────────────────── */
  .ws-main-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  /* ── Unified pipeline tabs (replaces separate pipeline hero + entity tab bar) ── */
  .ws-entity-tabs {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .entity-panel-overflow {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
    padding: var(--space-2) 0;
    margin: 0;
    font-style: italic;
  }

  /* ── Active agents section ──────────────────────────────────── */
  .ws-active-agents {
    display: flex;
    flex-direction: column;
  }

  .live-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-success);
    animation: hstat-pulse 2s ease-in-out infinite;
    margin-right: 2px;
    vertical-align: middle;
  }

  .active-agents-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-2);
  }

  .active-agent-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-success) 3%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-success) 20%, var(--color-border));
    border-left: 3px solid var(--color-success);
    border-radius: var(--radius);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: all var(--transition-fast);
    width: 100%;
  }

  .active-agent-card:hover {
    border-color: var(--color-success);
    box-shadow: var(--shadow-sm);
  }

  .active-agent-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .active-agent-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .active-agent-time {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-success);
    font-weight: 600;
    flex-shrink: 0;
  }

  .active-agent-context {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .active-agent-spec { font-style: italic; }
  .active-agent-task { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .active-agent-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-1);
    font-size: 10px;
  }

  .active-agent-repo {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .active-agent-view {
    color: var(--color-primary);
    font-weight: 600;
  }

  /* ── Merge queue section ────────────────────────────────────── */
  .ws-merge-queue-section {
    display: flex;
    flex-direction: column;
  }

  .queue-count-badge {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--color-info, #1e90ff);
    background: color-mix(in srgb, var(--color-info, #1e90ff) 10%, transparent);
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    margin-left: var(--space-1);
  }

  .merge-queue-list {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .merge-queue-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
  }

  .merge-queue-item:last-child { border-bottom: none; }
  .merge-queue-item:hover { background: var(--color-surface-elevated); }

  .queue-position {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--color-text-muted);
    width: 24px;
    text-align: center;
    flex-shrink: 0;
  }

  .queue-item-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-width: 0;
  }

  .queue-item-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .queue-item-meta {
    display: flex;
    gap: var(--space-2);
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .queue-item-branch {
    font-family: var(--font-mono);
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }

  .queue-item-repo {
    font-weight: 600;
  }

  .queue-item-spec {
    font-style: italic;
  }

  .queue-item-deps {
    font-size: 10px;
    font-weight: 600;
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    padding: 0 6px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .queue-item-status {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 1px 8px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .queue-item-status-open { color: var(--color-info, #1e90ff); background: color-mix(in srgb, var(--color-info, #1e90ff) 10%, transparent); }
  .queue-item-status-merged { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 10%, transparent); }
  .queue-item-status-pending { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  /* ── Pending specs section ──────────────────────────────────── */
  .ws-pending-specs-section {
    display: flex;
    flex-direction: column;
  }

  .pending-specs-badge {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    margin-left: var(--space-1);
  }

  .pending-specs-list {
    display: flex;
    flex-direction: column;
    border: 1px solid color-mix(in srgb, var(--color-warning) 25%, var(--color-border));
    border-radius: var(--radius);
    overflow: hidden;
  }

  .pending-spec-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    transition: opacity var(--transition-fast);
  }

  .pending-spec-item:last-child { border-bottom: none; }

  .pending-spec-resolved { opacity: 0.5; }

  .pending-spec-body {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .pending-spec-body:hover { background: var(--color-surface-elevated); }

  .pending-spec-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pending-spec-meta {
    display: flex;
    gap: var(--space-2);
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .pending-spec-repo {
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .pending-spec-kind {
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }

  .pending-spec-actions {
    display: flex;
    gap: var(--space-1);
    padding-right: var(--space-3);
    flex-shrink: 0;
  }

  .pending-spec-done {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-success);
  }

  .pending-spec-error {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-danger);
  }

  /* ── Recently merged section ────────────────────────────────── */
  .recently-merged-list {
    display: flex;
    flex-direction: column;
    padding: var(--space-1) 0;
  }

  .recently-merged-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
  }

  .recently-merged-item:last-child { border-bottom: none; }
  .recently-merged-item:hover { background: var(--color-surface-elevated); }

  .merged-item-main {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    flex: 1;
    min-width: 0;
  }

  .merged-item-check {
    color: var(--color-success);
    font-weight: 700;
    font-size: var(--text-sm);
    flex-shrink: 0;
    margin-top: 1px;
  }

  .merged-item-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .merged-item-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .merged-item-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    color: var(--color-text-muted);
    overflow: hidden;
  }

  .merged-meta-spec {
    font-style: italic;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .merged-meta-agent {
    font-family: var(--font-mono);
    max-width: 100px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .merged-meta-arrow { color: var(--color-text-muted); }
  .merged-meta-status { color: var(--color-success); font-weight: 600; }
  .merged-meta-time { white-space: nowrap; }

  .merged-item-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .merged-diff-stats {
    font-family: var(--font-mono);
    font-size: 10px;
    display: flex;
    gap: 3px;
  }

  .merged-gates-mini {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }

  .merged-repo-badge {
    font-size: 10px;
    font-weight: 600;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 6px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .diff-ins-tiny { color: var(--color-success); font-weight: 600; }
  .diff-del-tiny { color: var(--color-danger); font-weight: 600; }
  /* ── Pipeline detail panel (compact multi-repo navigator) ──────── */
  .pipeline-detail-compact {
    border: 1px solid var(--color-border);
    border-top: none;
    border-radius: 0 0 var(--radius) var(--radius);
    background: var(--color-surface);
    overflow: hidden;
    margin-top: -1px;
    animation: pipeline-detail-in 0.15s ease-out;
  }

  @keyframes pipeline-detail-in {
    from { opacity: 0; max-height: 0; }
    to { opacity: 1; max-height: 300px; }
  }

  .pipeline-detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .pipeline-detail-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .pipeline-detail-close {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    transition: all var(--transition-fast);
  }

  .pipeline-detail-close:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .pipeline-detail-body {
    display: flex;
    flex-direction: column;
  }

  .pipeline-repo-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
  }

  .pipeline-repo-row:last-child { border-bottom: none; }
  .pipeline-repo-row:hover { background: var(--color-surface-elevated); }

  .pipeline-repo-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-primary);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pipeline-repo-count {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--color-text);
    flex-shrink: 0;
  }

  .pipeline-repo-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 0 6px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .pipeline-repo-arrow {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  /* ── Pipeline flow bar ───────────────────────────────────────── */
  .pipeline-bar {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow-x: auto;
  }

  .pipeline-stage {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px var(--space-2);
    border-radius: var(--radius-sm);
    background: transparent;
    border: 1px solid transparent;
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .pipeline-stage:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }

  .pipeline-stage-active { color: var(--color-text); font-weight: 600; }
  .pipeline-stage-done { color: var(--color-success); }
  .pipeline-stage-alert { color: var(--color-danger); }
  .pipeline-stage-count {
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: var(--text-sm);
    color: inherit;
  }

  .pipeline-stage-label { font-weight: 500; }

  .pipeline-progress-bar {
    display: block;
    width: 100%;
    height: 3px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
    margin-top: 2px;
  }
  .pipeline-progress-fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    transition: width 0.5s ease;
  }
  .pipeline-fill-ok { background: var(--color-success); }
  .pipeline-fill-warn { background: var(--color-warning); }
  .pipeline-fill-danger { background: var(--color-danger); }

  .pipeline-stage-badge {
    font-size: 10px;
    font-weight: 700;
    font-family: var(--font-mono);
    padding: 0 4px;
    border-radius: 8px;
    line-height: 1.5;
    min-width: 16px;
    text-align: center;
  }

  .pipeline-badge-warning { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 12%, transparent); }
  .pipeline-badge-active { color: var(--color-info, #1e90ff); background: color-mix(in srgb, var(--color-info, #1e90ff) 12%, transparent); }
  .pipeline-badge-success { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 12%, transparent); }
  .pipeline-badge-danger { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 12%, transparent); }

  .pipeline-arrow {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    flex-shrink: 0;
    opacity: 0.5;
  }

  /* ── Workspace overview tabs ────────────────────────────────── */
  .ws-overview-section {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .ws-overview-tabs {
    display: flex;
    border-bottom: 1px solid var(--color-border);
    overflow-x: auto;
  }

  .ws-overview-tab {
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .ws-overview-tab:hover { color: var(--color-text); background: var(--color-surface-elevated); }
  .ws-overview-tab-active { color: var(--color-primary); border-bottom-color: var(--color-primary); font-weight: 600; }

  .ws-overview-tab-count {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-text-muted);
    margin-left: 2px;
  }

  .ws-overview-content {
    padding: var(--space-2);
    max-height: 400px;
    overflow-y: auto;
  }

  .ws-overview-more-btn {
    display: block;
    width: 100%;
    padding: var(--space-2);
    background: transparent;
    border: none;
    border-top: 1px solid var(--color-border);
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    color: var(--color-primary);
    font-weight: 500;
    transition: background var(--transition-fast);
  }

  .ws-overview-more-btn:hover { background: var(--color-surface-elevated); }

  .ws-overview-empty {
    text-align: center;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    padding: var(--space-4);
    margin: 0;
  }

  .ws-overview-more {
    text-align: center;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    padding: var(--space-2);
    margin: 0;
    font-style: italic;
  }


  .ws-status-dot {
    display: inline-block;
    margin-right: 3px;
    font-size: 10px;
  }

  .ws-status-approved, .ws-status-merged, .ws-status-done, .ws-status-completed, .ws-status-idle { color: var(--color-success); }
  .ws-status-pending, .ws-status-open, .ws-status-in_progress, .ws-status-active, .ws-status-running { color: var(--color-warning); }
  .ws-status-rejected, .ws-status-closed, .ws-status-blocked, .ws-status-failed, .ws-status-dead { color: var(--color-danger); }
  .ws-status-draft, .ws-status-backlog, .ws-status-spawning, .ws-status-unknown { color: var(--color-text-muted); }

  .cell-title { font-weight: 500; color: var(--color-text); max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cell-mono { font-family: var(--font-mono); font-size: 10px; color: var(--color-text-muted); max-width: 150px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cell-time { font-size: 10px; color: var(--color-text-muted); white-space: nowrap; }
  .cell-action { white-space: nowrap; }
  .cell-diff { white-space: nowrap; font-family: var(--font-mono); font-size: 10px; }
  .diff-stat-clickable {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    padding: 1px 4px;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 10px;
    transition: all var(--transition-fast);
  }
  .diff-stat-clickable:hover { background: var(--color-surface-elevated); border-color: var(--color-border); }
  .diff-ins-tiny { color: var(--color-success); }
  .diff-del-tiny { color: var(--color-danger); }
  .diff-files-tiny { color: var(--color-text-muted); }

  /* ── Budget panel ──────────────────────────────────────────── */
  .ws-budget-panel { padding: var(--space-2); }

  .budget-stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-2);
  }

  .budget-stat {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
  }

  .budget-stat-label { font-size: 10px; color: var(--color-text-muted); font-weight: 500; text-transform: uppercase; letter-spacing: 0.04em; }
  .budget-stat-value { font-size: var(--text-sm); font-weight: 700; font-family: var(--font-mono); color: var(--color-text); }

  .budget-cost-summary { margin-top: var(--space-3); }
  .budget-cost-heading { font-size: var(--text-xs); font-weight: 600; color: var(--color-text-secondary); margin: 0 0 var(--space-2) 0; }

  .priority-pill {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 0 6px;
    border-radius: var(--radius-sm);
  }

  .priority-critical, .priority-high { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 12%, transparent); }
  .priority-medium { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 12%, transparent); }
  .priority-low { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  .gate-summary {
    display: inline-flex;
    gap: 2px;
  }

  .gate-mini {
    font-weight: 600;
    padding: 0 3px;
    border-radius: 3px;
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .gate-mini-pass { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-mini-fail { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }

  /* Workspace gate badges (named, clickable) */
  .gate-names-ws { display: flex; flex-wrap: wrap; gap: 3px; align-items: center; }
  .gate-badge-ws {
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
  .gate-badge-ws:hover { border-color: var(--color-border); }
  .gate-badge-ws-icon { font-weight: 700; }
  .gate-badge-ws-name { font-weight: 500; max-width: 100px; overflow: hidden; text-overflow: ellipsis; }
  .gate-badge-ws-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-badge-ws-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-badge-ws-pending, .gate-badge-ws-running { color: var(--color-text-muted); background: var(--color-surface-elevated); }
  .gate-badge-ws-more { color: var(--color-text-muted); background: var(--color-surface-elevated); font-size: 10px; font-weight: 600; }
  .gate-error-snippet {
    display: block;
    width: 100%;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 5%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 15%, transparent);
    border-radius: var(--radius-sm);
    padding: 2px 6px;
    cursor: pointer;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }
  .gate-error-snippet:hover { background: color-mix(in srgb, var(--color-danger) 10%, transparent); }

  /* (pipeline-detail entity cards removed — entities are browsed per-repo in RepoMode) */

  .activity-timeline-full {
    padding: var(--space-2) 0;
  }

  .activity-repo-badge {
    font-size: 10px;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    flex-shrink: 0;
    position: sticky;
    top: var(--space-2);
    max-height: calc(100vh - 120px);
    overflow-y: auto;
  }

  .repo-cards-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* ── Workspace header ────────────────────────────────────────────── */
  .ws-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding-bottom: var(--space-1);
    border-bottom: 1px solid var(--color-border);
  }

  .ws-header-top-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .ws-header-name {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 700;
    color: var(--color-text);
    font-family: var(--font-display);
  }

  .ws-header-stats {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .ws-hstat {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .ws-hstat-num {
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
  }

  .ws-hstat-warn .ws-hstat-num { color: var(--color-warning); }
  .ws-hstat-danger .ws-hstat-num { color: var(--color-danger); }

  .ws-hstat-live {
    color: var(--color-success);
  }

  .ws-hstat-live .ws-hstat-num {
    color: var(--color-success);
  }

  .ws-hstat-pulse {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-success);
    animation: hstat-pulse 2s ease-in-out infinite;
    flex-shrink: 0;
  }

  @keyframes hstat-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.7); }
  }

  .ws-header-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    max-width: 600px;
    line-height: 1.4;
  }

  .ws-header-status {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    max-width: 700px;
    line-height: 1.4;
  }

  .ws-header-status-items {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .ws-status-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: transparent;
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-secondary);
    transition: all var(--transition-fast);
  }

  .ws-status-chip:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .ws-status-icon { font-size: var(--text-sm); }

  .ws-status-chip-danger { color: var(--color-danger); border-color: color-mix(in srgb, var(--color-danger) 30%, var(--color-border)); }
  .ws-status-chip-danger:hover { border-color: var(--color-danger); }
  .ws-status-chip-warning { color: var(--color-warning); border-color: color-mix(in srgb, var(--color-warning) 30%, var(--color-border)); }
  .ws-status-chip-warning:hover { border-color: var(--color-warning); }
  .ws-status-chip-success { color: var(--color-success); border-color: color-mix(in srgb, var(--color-success) 30%, var(--color-border)); }
  .ws-status-chip-success:hover { border-color: var(--color-success); }
  .ws-status-chip-info { color: var(--color-info, #1e90ff); border-color: color-mix(in srgb, var(--color-info, #1e90ff) 30%, var(--color-border)); }
  .ws-status-chip-info:hover { border-color: var(--color-info, #1e90ff); }
  .ws-status-chip-muted { color: var(--color-text-muted); }

  .ws-status-badges {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .ws-status-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
    background: transparent;
    cursor: pointer;
    font-family: inherit;
    font-size: var(--text-xs);
    font-weight: 500;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .ws-status-badge:hover { border-color: var(--color-border); background: var(--color-surface-elevated); }

  .ws-status-badge-icon { font-weight: 700; }

  .ws-status-badge-danger { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 6%, transparent); }
  .ws-status-badge-warning { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 6%, transparent); }
  .ws-status-badge-success { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 6%, transparent); }
  .ws-status-badge-info { color: var(--color-info, #1e90ff); background: color-mix(in srgb, var(--color-info, #1e90ff) 6%, transparent); }
  .ws-status-badge-muted { color: var(--color-text-muted); }

  /* ── Budget indicator ─────────────────────────────────────────────── */
  .ws-budget-indicator {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: var(--space-1) var(--space-2);
  }

  .ws-budget-bar {
    width: 40px;
    height: 4px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  .ws-budget-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .ws-budget-warn .ws-budget-fill { background: var(--color-warning); }
  .ws-budget-danger .ws-budget-fill { background: var(--color-danger); }

  .ws-budget-label {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    font-weight: 500;
  }

  .ws-budget-warn .ws-budget-label { color: var(--color-warning); }
  .ws-budget-danger .ws-budget-label { color: var(--color-danger); }

  /* ── Status hero — prominent workspace summary ──────────────────── */
  .ws-status-hero {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--space-2) 0;
  }

  .ws-status-sentence {
    font-size: var(--text-base);
    color: var(--color-text);
    margin: 0;
    line-height: 1.4;
    font-weight: 500;
  }

  .ws-briefing {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-info);
    background: color-mix(in srgb, var(--color-info) 4%, var(--color-surface));
    border-radius: 0 var(--radius) var(--radius) 0;
  }

  .ws-briefing-icon {
    flex-shrink: 0;
    color: var(--color-info);
    margin-top: 1px;
  }

  .ws-briefing-text {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
    max-width: 800px;
  }

  .ws-briefing-banner {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 3%, var(--color-surface));
    border-radius: 0 var(--radius) var(--radius) 0;
  }

  .ws-briefing-banner-text {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    line-height: 1.4;
    font-style: italic;
    max-width: 800px;
  }

  .ws-briefing-inline {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    padding: 0 var(--space-1);
    line-height: 1.4;
    font-style: italic;
    max-width: 700px;
  }

  .ws-briefing-idle {
    color: var(--color-text-muted);
    font-style: normal;
  }

  /* ── Status chips (clickable status items) ── */
  .status-chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .status-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    border-radius: 999px;
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-body);
    cursor: pointer;
    border: 1px solid transparent;
    transition: all var(--transition-fast);
    line-height: 1.4;
  }

  .status-chip:hover {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }

  .status-chip-icon {
    font-size: 11px;
  }

  .status-chip-danger {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
  }

  .status-chip-warning {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
    border-color: color-mix(in srgb, var(--color-warning) 30%, transparent);
  }

  .status-chip-success {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .status-chip-info {
    background: color-mix(in srgb, var(--color-info, #1e90ff) 15%, transparent);
    color: var(--color-info, #1e90ff);
    border-color: color-mix(in srgb, var(--color-info, #1e90ff) 30%, transparent);
  }

  .status-chip-muted {
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border-color: var(--color-border);
  }

  /* ── Main layout ────────────────────────────────────────────────── */
  .dashboard-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }

  /* browse-toggle removed — entity tabs are always visible */

  /* ── Pipeline progress bar (compact inline stepper) ────────────── */
  .pipeline-progress {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow-x: auto;
    height: 32px;
  }

  .ws-health-indicator {
    display: flex;
    align-items: center;
    padding: 0 var(--space-1);
    flex-shrink: 0;
  }

  .ws-health-danger { color: var(--color-danger); }
  .ws-health-active { color: var(--color-warning); animation: pulse-health 2s ease-in-out infinite; }
  .ws-health-healthy { color: var(--color-success); }
  .ws-health-idle { color: var(--color-text-muted); opacity: 0.5; }

  @keyframes pulse-health {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .pipeline-stage {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--space-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    transition: all var(--transition-fast);
    white-space: nowrap;
    position: relative;
  }

  .pipeline-stage:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }

  .pipeline-stage-terminal {
    cursor: default;
  }

  .pipeline-stage-terminal:hover {
    background: transparent;
    border-color: transparent;
  }

  .pipeline-stage-count {
    font-size: var(--text-xs);
    font-weight: 700;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    line-height: 1;
  }

  .pipeline-stage-active .pipeline-stage-count {
    color: var(--color-primary);
  }

  .pipeline-stage-done .pipeline-stage-count {
    color: var(--color-success);
  }

  .pipeline-stage-warn .pipeline-stage-count {
    color: var(--color-danger);
  }

  .pipeline-stage-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-muted);
  }

  .pipeline-stage-badge {
    font-size: 9px;
    font-weight: 600;
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .pipeline-badge-warn {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
  }

  .pipeline-badge-danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  }

  .pipeline-badge-success {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
  }

  .pipeline-arrow {
    color: var(--color-text-muted);
    font-size: 11px;
    flex-shrink: 0;
    padding: 0 1px;
    opacity: 0.4;
  }

  .pipeline-budget-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: auto;
    padding: 0 var(--space-2);
    flex-shrink: 0;
  }

  .pipeline-budget-bar {
    width: 40px;
    height: 3px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  .pipeline-budget-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .pipeline-budget-fill.budget-warn { background: var(--color-warning); }
  .pipeline-budget-fill.budget-danger { background: var(--color-danger); }

  .pipeline-budget-label {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    font-weight: 500;
  }

  .pipeline-budget-label.budget-warn { color: var(--color-warning); }
  .pipeline-budget-label.budget-danger { color: var(--color-danger); }

  /* ── Workspace briefing (inline) ─────────────────────────────────── */
  /* ── Compact workspace briefing ────────────────────────────────── */
  .ws-briefing-compact {
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 3%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-primary) 15%, var(--color-border));
    border-radius: var(--radius);
    border-left: 3px solid var(--color-primary);
  }

  .ws-briefing-summary {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .ws-briefing-section {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    border-left: 3px solid var(--color-primary);
  }

  .ws-briefing-body {
    padding: var(--space-2) var(--space-3);
  }

  .ws-briefing-text {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
    margin: 0 0 var(--space-2) 0;
  }

  .ws-briefing-narrative {
    font-style: italic;
    color: var(--color-text-muted);
  }

  /* ── Repos section ──────────────────────────────────────────────── */
  .ws-repos-section {
    min-width: 0;
  }

  /* ── Activity toolbar ─────────────────────────────────────────── */
  .activity-toolbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: var(--space-1) var(--space-3);
    border-bottom: 1px solid var(--color-border);
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

  /* ── Single-column dashboard flow ─────────────────────────────────── */
  .dashboard-flow {
    display: flex;
    flex-direction: column;
    min-width: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .dashboard-flow-collapsed {
    border-color: var(--color-border);
  }

  .browse-section-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
    width: 100%;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .browse-section-toggle:hover {
    background: color-mix(in srgb, var(--color-primary) 6%, var(--color-surface-elevated));
  }

  .browse-toggle-chevron {
    font-size: 10px;
    color: var(--color-text-muted);
    transition: transform 0.15s ease;
    display: inline-block;
  }

  .browse-toggle-open {
    transform: rotate(90deg);
  }

  .browse-toggle-label {
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .browse-toggle-hint {
    font-weight: 400;
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .browse-toggle-hint-muted {
    opacity: 0.6;
  }

  /* ── Queue list (legacy, used by standalone queue view) ─────────── */
  .mq-list {
    display: flex;
    flex-direction: column;
  }

  .mq-list-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: background var(--transition-fast);
  }

  .mq-list-item:last-child { border-bottom: none; }
  .mq-list-item:hover { background: var(--color-surface-elevated); }

  .mq-list-item-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mq-list-item-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mq-list-item-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-wrap: wrap;
  }

  .mq-list-item-gates {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .mq-dep-badge {
    font-size: 10px;
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    padding: 0 4px;
    border-radius: var(--radius-sm);
  }

  /* ── Repo cards grid (responsive — list layout for fewer repos) ────── */
  .repo-cards-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-3);
    padding: var(--space-2) 0;
  }

  .repo-cards-few {
    grid-template-columns: 1fr;
    max-width: 500px;
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

  /* ── Merge queue banner (promoted) ───────────────── */
  .merge-queue-banner {
    border: 1px solid var(--color-border);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .mq-banner-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-warning) 4%, var(--color-surface-elevated));
    border-bottom: 1px solid var(--color-border);
    color: var(--color-warning);
  }

  .mq-banner-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .mq-banner-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .mq-banner-list {
    display: flex;
    flex-direction: column;
  }

  .mq-banner-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    width: 100%;
    transition: background var(--transition-fast);
  }

  .mq-banner-item:last-child { border-bottom: none; }
  .mq-banner-item:hover { background: var(--color-surface-elevated); }

  .mq-banner-item-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
  }

  .mq-banner-gates {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .mq-banner-overflow {
    padding: var(--space-1) var(--space-3);
    font-size: 10px;
    color: var(--color-text-muted);
    text-align: center;
  }

  /* ── Feed header bar ──────────────────────────────── */
  .feed-header-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
  }

  .feed-header-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .feed-header-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .browse-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    transition: all var(--transition-fast);
  }

  .browse-toggle:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .browse-toggle-active {
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .browse-panel {
    border-color: var(--color-primary);
    border-top: 2px solid var(--color-primary);
  }

  /* ── Activity feed collapsible ──────────────────── */
  .activity-details {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
  }

  .activity-count-badge {
    font-size: 10px;
    font-weight: 700;
    background: var(--color-surface);
    color: var(--color-text-muted);
    border-radius: 8px;
    padding: 0 5px;
    min-width: 14px;
    text-align: center;
    line-height: 16px;
    border: 1px solid var(--color-border);
  }


  /* ── Main content area (always visible) ────────────────────── */
  .ws-main-content {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface);
    overflow: hidden;
    flex: 1;
    min-height: 0;
  }

  /* ── Workspace quick links ──────────────────────────────────── */
  .ws-quick-links {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  .ws-quick-link {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: 10px;
    color: var(--color-text-muted);
    transition: all var(--transition-fast);
  }

  .ws-quick-link:hover {
    color: var(--color-primary);
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 4%, transparent);
  }

  /* ── Activity feed (full-width) ──────────────────── */
  .ws-feed-panel {
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
    max-height: calc(100vh - 250px);
    min-height: 120px;
    overflow-y: auto;
    padding: var(--space-1) 0;
  }

  /* Tab bar CSS removed — workspace home no longer uses tabs */

  .ws-tab-toolbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding: var(--space-1) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  /* ── Entity list (compact list layout replacing wide tables) ─────── */
  .entity-list {
    display: flex;
    flex-direction: column;
  }

  .entity-list-item {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 6px var(--space-3);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
  }

  .entity-list-item:last-child { border-bottom: none; }
  .entity-list-item:hover { background: color-mix(in srgb, var(--color-primary) 4%, transparent); }
  .entity-list-item:focus-visible { outline: 2px solid var(--color-focus); outline-offset: -2px; }

  .entity-list-main {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    flex: 1;
    min-width: 0;
  }

  .entity-list-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }

  .entity-list-title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .entity-list-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entity-list-path {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .entity-list-repo {
    font-size: 10px;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .entity-list-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: var(--text-xs);
  }

  .entity-list-meta-secondary {
    margin-top: 2px;
    font-size: var(--text-xs);
  }

  .entity-list-context {
    color: var(--color-text-muted);
    font-style: italic;
  }

  .entity-list-context-action { color: var(--color-warning); font-weight: 500; font-style: normal; }
  .entity-list-context-success { color: var(--color-success); }
  .entity-list-context-danger { color: var(--color-danger); }

  .entity-list-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: 10px;
    font-weight: 500;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    white-space: nowrap;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .entity-list-chip:hover { text-decoration: underline; color: var(--color-primary); }
  .entity-list-chip-active { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .entity-list-chip-merged { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .entity-list-chip-open { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 8%, transparent); }

  .entity-list-progress {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
  }

  .entity-list-time {
    color: var(--color-text-muted);
    white-space: nowrap;
    margin-left: auto;
    flex-shrink: 0;
  }

  .entity-list-tokens {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    font-size: 10px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .entity-list-provenance {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
  }

  .entity-list-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  /* ── Shared chips (used by merge queue and entity cards) ─────────────── */
  .ec-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    border: none;
    font-family: var(--font-body);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .ec-chip:hover {
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }

  .ec-chip-mono {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0;
  }

  .ec-chip-agent {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
  }

  .ec-chip-agent:hover {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 16%, transparent);
  }

  .ec-chip-spec {
    color: var(--color-info);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
  }

  .ec-chip-spec:hover {
    color: var(--color-info);
    background: color-mix(in srgb, var(--color-info) 16%, transparent);
  }

  .ec-chip-merged {
    color: var(--color-blocked, #5e40be);
    background: color-mix(in srgb, var(--color-blocked, #5e40be) 8%, transparent);
  }

  .ec-chip-muted {
    color: var(--color-text-muted);
    font-style: italic;
  }

  .ec-gate-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-weight: 500;
    white-space: nowrap;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .ec-gate-chip:hover { opacity: 0.8; }

  .ec-gate-icon {
    font-size: 12px;
    font-weight: 700;
  }

  .ec-gate-passed, .ec-gate-pass { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .ec-gate-failed, .ec-gate-fail { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .ec-gate-pending { color: var(--color-text-muted); background: color-mix(in srgb, var(--color-text-muted) 8%, transparent); }
  .ec-gate-more { color: var(--color-text-muted); font-style: italic; }

  .ec-gate-dur {
    font-size: 10px;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .entity-secondary-path {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .entity-agent-chip {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    white-space: nowrap;
    max-width: 100px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .entity-agent-chip:hover {
    background: color-mix(in srgb, var(--color-success) 16%, transparent);
    text-decoration: underline;
  }

  .entity-branch-tag {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .entity-time {
    color: var(--color-text-muted);
    white-space: nowrap;
    font-size: 10px;
  }

  .duration-running {
    color: var(--color-success);
    font-weight: 500;
  }

  .entity-repo-link,
  .entity-spec-link {
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    color: var(--color-primary);
    padding: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 140px;
  }

  .entity-repo-link:hover,
  .entity-spec-link:hover {
    text-decoration: underline;
  }

  /* ── Status pills ──────────────────────────────────────────────────── */
  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    text-transform: capitalize;
  }

  .status-pill-approved, .status-pill-merged, .status-pill-done, .status-pill-completed, .status-pill-idle {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
  }

  .status-pill-pending, .status-pill-in_progress, .status-pill-spawning, .status-pill-open, .status-pill-queued {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
  }

  .status-pill-rejected, .status-pill-failed, .status-pill-dead, .status-pill-closed, .status-pill-blocked, .status-pill-cancelled {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }

  .status-pill-active {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
  }

  .status-pill-backlog, .status-pill-draft, .status-pill-deprecated, .status-pill-review {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
  }

  .status-pulse {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-success);
    animation: pulse 1.5s ease-in-out infinite;
  }

  /* ── Priority pills ────────────────────────────────────────────────── */
  .priority-pill {
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
    text-transform: capitalize;
  }

  .priority-critical { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .priority-high { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 10%, transparent); }
  .priority-medium { color: var(--color-text-secondary); background: var(--color-surface-elevated); }
  .priority-low { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  /* ── Gates mini display ────────────────────────────────────────────── */
  .gates-inline {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0;
    font-family: var(--font-body);
    flex-wrap: wrap;
  }

  .gates-inline:hover .gate-chip { text-decoration: underline; }

  .gate-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: var(--text-xs);
    font-weight: 500;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .gate-chip-passed { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .gate-chip-failed { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .gate-chip-pending { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  .gate-chip-more {
    font-size: 10px;
    color: var(--color-text-muted);
    padding: 1px 4px;
  }

  .gate-adv-tag {
    font-size: 8px;
    opacity: 0.6;
    font-style: italic;
  }

  .entity-list-chip-gates {
    gap: 3px;
  }

  .gate-cmd-inline {
    font-family: var(--font-mono);
    font-size: 9px;
    opacity: 0.8;
  }

  .gate-error-preview {
    display: block;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 6%, transparent);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    margin-top: 2px;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gates-mini {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .gates-mini-clickable {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .gates-mini-clickable:hover {
    background: var(--color-surface-elevated);
    text-decoration: underline;
  }

  .gate-fail-count { color: var(--color-danger); }
  .gate-pass-count { color: var(--color-success); }
  .gate-total-count { color: var(--color-text-muted); }

  /* ── Diff stats mini ───────────────────────────────────────────────── */
  .diff-stats-mini {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 10px;
    font-family: var(--font-mono);
    font-weight: 500;
  }

  .diff-stats-link {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
  }

  .diff-stats-link:hover {
    background: var(--color-surface-elevated);
    text-decoration: underline;
  }

  .diff-files-mini { color: var(--color-text-muted); }
  .diff-ins-mini { color: var(--color-success); }
  .diff-del-mini { color: var(--color-danger); }

  .text-muted { color: var(--color-text-muted); font-size: 10px; }

  /* ── Spec downstream chips ─────────────────────────────────────────── */
  .spec-downstream {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .downstream-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    font-weight: 500;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    white-space: nowrap;
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .downstream-chip:hover { text-decoration: underline; }
  .downstream-chip-active { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .downstream-chip-merged { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .downstream-chip-open { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 8%, transparent); }
  .downstream-chip-closed { color: var(--color-text-muted); background: var(--color-surface-elevated); }

  .status-pulse-tiny {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--color-success);
    animation: pulse 1.5s ease-in-out infinite;
  }

  /* ── Guided empty states ───────────────────────────────────────────── */
  .empty-state-guided {
    padding: var(--space-3) var(--space-4);
  }

  .empty-state-guided .empty-text {
    padding: 0;
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .pipeline-guide {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
  }

  .pipeline-step {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    white-space: nowrap;
    font-weight: 500;
    min-width: 90px;
    text-align: center;
  }

  .pipeline-step-num {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 700;
    color: var(--color-primary);
    width: 18px;
    height: 18px;
    line-height: 18px;
    text-align: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
  }

  .pipeline-step-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .pipeline-step-desc {
    font-size: 10px;
    color: var(--color-text-muted);
    font-weight: 400;
  }

  .empty-actions {
    margin-top: var(--space-3);
  }

  .pipeline-arrow {
    color: var(--color-text-muted);
    opacity: 0.5;
    flex-shrink: 0;
  }

  .empty-guide {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: var(--space-1) 0 0 0;
    line-height: 1.5;
  }

  .empty-guide code {
    font-family: var(--font-mono);
    font-size: 10px;
    padding: 1px 4px;
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
  }

  /* ── Inline action buttons ─────────────────────────────────────────── */
  .td-actions {
    white-space: nowrap;
  }

  .inline-action-btn {
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 3px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    border: none;
    transition: background var(--transition-fast);
  }

  .inline-action-view {
    color: var(--color-link);
    background: color-mix(in srgb, var(--color-link) 8%, transparent);
  }
  .inline-action-view:hover { background: color-mix(in srgb, var(--color-link) 18%, transparent); }

  .inline-action-danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
  }
  .inline-action-danger:hover { background: color-mix(in srgb, var(--color-danger) 18%, transparent); }

  .td-actions-mr {
    display: flex;
    gap: 2px;
    flex-wrap: nowrap;
  }

  .inline-action-approve {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
  }
  .inline-action-approve:hover { background: color-mix(in srgb, var(--color-success) 20%, transparent); }

  .inline-action-reject {
    color: var(--color-text-muted);
    background: transparent;
  }
  .inline-action-reject:hover { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }

  .inline-action-done {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-success);
  }

  .inline-action-rejected { color: var(--color-text-muted); }

  .inline-action-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  /* ── Status with context ───────────────────────────────────────────── */
  .status-with-context {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .status-context {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 140px;
  }

  .status-context-danger { color: var(--color-danger); }
  .status-context-success { color: var(--color-success); }
  .status-context-action { color: var(--color-warning); font-weight: 600; }

  .status-context-link {
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: 10px;
    color: var(--color-primary);
    padding: 0;
    white-space: nowrap;
  }

  .status-context-link:hover {
    text-decoration: underline;
  }

  .merged-provenance {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 9px;
    color: var(--color-text-muted);
  }

  .prov-step {
    color: var(--color-success);
    font-weight: 500;
  }

  .prov-arrow {
    opacity: 0.4;
    font-size: 8px;
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
    padding: var(--space-1) 0;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .section-header-repos {
    padding: 0;
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .section-title-sm {
    font-size: 10px;
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

  /* ── Decisions / Action Needed section ────────────────────────────── */
  /* ── Active agents strip ────────────────────────────────────── */
  .ws-decisions-section {
    border: 1px solid var(--color-warning);
    border-left: 3px solid var(--color-warning);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-warning) 3%, var(--color-surface));
    overflow: hidden;
  }

  .ws-decisions-section.decisions-danger {
    border-color: var(--color-danger);
    border-left-color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 3%, var(--color-surface));
  }

  .decisions-danger .decisions-header {
    background: color-mix(in srgb, var(--color-danger) 6%, var(--color-surface-elevated));
  }

  .decisions-danger .decisions-count-badge {
    background: var(--color-danger);
  }

  .decisions-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-warning) 6%, var(--color-surface-elevated));
    border-bottom: 1px solid var(--color-border);
  }

  .decisions-title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .decisions-count-badge {
    font-size: 10px;
    font-weight: 700;
    background: var(--color-warning);
    color: var(--color-text-inverse);
    border-radius: 8px;
    padding: 0 5px;
    min-width: 14px;
    text-align: center;
    line-height: 16px;
  }

  .decisions-list {
    display: flex;
    flex-direction: column;
  }

  .decision-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast), opacity var(--transition-fast);
  }

  .decision-item:last-child { border-bottom: none; }
  .decision-item:hover { background: color-mix(in srgb, var(--color-warning) 4%, transparent); }
  .decision-resolved { opacity: 0.5; }

  .decision-severity-danger { border-left: 3px solid var(--color-danger); }
  .decision-severity-action { border-left: 3px solid var(--color-warning); }
  .decision-severity-warn { border-left: 3px solid var(--color-primary); }

  .decision-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 700;
    flex-shrink: 0;
  }

  .decision-icon-action {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
  }

  .decision-icon-danger {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
  }

  .decision-icon-warn {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
  }

  .decision-body {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    flex-wrap: wrap;
    background: transparent;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
    text-align: left;
    padding: 0;
  }

  .decision-body:hover .decision-title {
    color: var(--color-primary);
    text-decoration: underline;
  }

  .decision-type {
    font-size: 10px;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  .decision-title {
    font-size: var(--text-xs);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .decision-detail {
    font-size: 10px;
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .decision-error-preview {
    display: block;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-danger);
    opacity: 0.8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-top: 1px;
  }

  .decision-time {
    font-size: 10px;
    color: var(--color-text-muted);
    white-space: nowrap;
    margin-left: auto;
  }

  .decision-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .decision-done {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-success);
  }

  .decision-loading {
    font-size: 10px;
    color: var(--color-text-muted);
  }

  /* ── Provenance flow (visual pipeline) ─────────────────────────────── */
  .provenance-flow {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow-x: auto;
  }

  .flow-stage {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    transition: all var(--transition-fast);
    min-width: 60px;
  }

  .flow-stage:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 4%, transparent);
  }

  .flow-stage-active {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 6%, transparent);
  }

  .flow-stage-count {
    font-size: var(--text-lg);
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text);
    line-height: 1;
  }

  .flow-stage-label {
    font-size: 10px;
    font-weight: 500;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .flow-stage-badge {
    font-size: 9px;
    font-weight: 600;
    color: var(--color-warning);
    white-space: nowrap;
  }

  .flow-badge-warn { color: var(--color-warning); }
  .flow-badge-danger { color: var(--color-danger); }
  .flow-badge-success { color: var(--color-success); }

  .flow-arrow {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    flex-shrink: 0;
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
    min-width: 50px;
  }

  .progress-mini {
    display: flex;
    align-items: center;
    gap: var(--space-1);
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

  .section-btn-compact {
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
  }

  .section-btn-subtle {
    background: transparent;
    border-color: transparent;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .section-btn-subtle:hover:not(:disabled) {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
    color: var(--color-text-secondary);
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
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
    vertical-align: middle;
    user-select: all;
  }

  .sha-copyable {
    cursor: pointer;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .sha-copyable:hover {
    color: var(--color-text);
    background: var(--color-border);
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
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .activity-entity-name {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text);
    font-weight: 500;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-reason {
    margin: 0;
    font-size: 10px;
    color: var(--color-text-muted);
    line-height: 1.3;
  }

  .activity-refs {
    display: flex;
    gap: 4px;
    padding-left: calc(16px + var(--space-2));
    flex-wrap: wrap;
  }

  .activity-ref-chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: 10px;
    color: var(--color-text-secondary);
    transition: all var(--transition-fast);
  }

  .activity-ref-chip:hover {
    color: var(--color-primary);
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
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

  .activity-repo {
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
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
