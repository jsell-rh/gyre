<script>
  /**
   * TenantSettings — full-page tenant administration (§10 of ui-navigation.md)
   *
   * URL: /all/settings
   * Only visible to tenant Admin role users.
   * Tabs: Users | Compute Targets | Budget | Audit | Health | Jobs
   *
   * Spec ref: ui-navigation.md §10
   *   "Tenant administration is accessed via a gear icon on the cross-workspace view header.
   *    Only visible to tenant Admin role users. Tabs: Users, Compute Targets, Budget, Audit, Health, Jobs."
   */
  import { getContext, untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName as sharedEntityName, shortId as sharedShortId } from '../lib/entityNames.svelte.js';
  import { absoluteTime } from '../lib/timeFormat.js';

  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  function nav(type, id, data) {
    if (goToEntityDetail) goToEntityDetail(type, id, data ?? {});
    else if (openDetailPanel) openDetailPanel({ type, id, data: data ?? {} });
  }
  import { toast as showToast } from '../lib/toast.svelte.js';

  let {
    onBack = undefined,
  } = $props();

  const TABS = [
    { id: 'users',      labelKey: 'tenant_settings.tabs.users' },
    { id: 'compute',    labelKey: 'tenant_settings.tabs.compute' },
    { id: 'budget',     labelKey: 'tenant_settings.tabs.budget' },
    { id: 'policies',   label: 'Policies' },
    { id: 'llm',        label: 'LLM Defaults' },
    { id: 'analytics',  label: 'Analytics' },
    { id: 'audit',      labelKey: 'tenant_settings.tabs.audit' },
    { id: 'health',     labelKey: 'tenant_settings.tabs.health' },
    { id: 'jobs',       labelKey: 'tenant_settings.tabs.jobs' },
    { id: 'bcp',        label: 'BCP' },
  ];

  let activeTab = $state('users');

  // ── Users ─────────────────────────────────────────────────────────────
  let currentUser = $state(null);
  let usersLoading = $state(false);
  let usersError = $state(null);

  // ── Compute Targets ───────────────────────────────────────────────────
  let computeTargets = $state([]);
  let computeLoading = $state(false);
  let computeError = $state(null);

  // ── Budget ────────────────────────────────────────────────────────────
  let budgetSummary = $state(null);
  let budgetLoading = $state(false);
  let budgetError = $state(null);

  // ── Audit ─────────────────────────────────────────────────────────────
  let auditEvents = $state([]);
  let auditLoading = $state(false);
  let auditError = $state(null);
  let auditFilterType = $state('');

  // ── Health ────────────────────────────────────────────────────────────
  let health = $state(null);
  let healthLoading = $state(false);
  let healthError = $state(null);
  let versionInfo = $state(null);

  // ── Jobs ──────────────────────────────────────────────────────────────
  let jobs = $state([]);
  let jobsLoading = $state(false);
  let jobsError = $state(null);
  let runningJob = $state(null);

  // ── Compute CRUD ─────────────────────────────────────────────────────
  let showComputeForm = $state(false);
  let newComputeName = $state('');
  let newComputeType = $state('local');
  let computeCreating = $state(false);
  let computeDeleting = $state(null);

  // ── Policies (ABAC) ──────────────────────────────────────────────────
  let policies = $state([]);
  let policiesLoading = $state(false);
  let policiesError = $state(null);
  let policyDecisions = $state([]);
  let policyDecisionsLoading = $state(false);
  let showPolicyForm = $state(false);
  let policyFormSaving = $state(false);
  let policyForm = $state({ name: '', scope: 'tenant', effect: 'allow', actions: 'push', resource_types: 'repo', priority: 100, condition_attr: 'subject.type', condition_op: 'equals', condition_val: 'agent' });
  let deletingPolicyId = $state(null);

  async function createPolicy() {
    if (!policyForm.name.trim()) return;
    policyFormSaving = true;
    try {
      await api.createPolicy({
        name: policyForm.name.trim(),
        scope: policyForm.scope,
        effect: policyForm.effect,
        conditions: [{ attribute: policyForm.condition_attr, operator: policyForm.condition_op, value: policyForm.condition_val }],
        actions: policyForm.actions.split(',').map(s => s.trim()).filter(Boolean),
        resource_types: policyForm.resource_types.split(',').map(s => s.trim()).filter(Boolean),
        priority: parseInt(policyForm.priority) || 100,
      });
      showPolicyForm = false;
      policyForm = { name: '', scope: 'tenant', effect: 'allow', actions: 'push', resource_types: 'repo', priority: 100, condition_attr: 'subject.type', condition_op: 'equals', condition_val: 'agent' };
      loadPolicies();
    } catch (e) {
      policiesError = 'Create failed: ' + (e?.message ?? e);
    } finally {
      policyFormSaving = false;
    }
  }

  async function deletePolicy(id) {
    deletingPolicyId = id;
    try {
      await api.deletePolicy(id);
      policies = policies.filter(p => p.id !== id);
    } catch (e) {
      policiesError = 'Delete failed: ' + (e?.message ?? e);
    } finally {
      deletingPolicyId = null;
    }
  }

  // ── LLM Defaults ──────────────────────────────────────────────────────
  const LLM_FEATURES = ['briefing-ask', 'spec-assist', 'explorer-generate', 'graph-predict'];
  let adminLlmConfigs = $state({});
  let adminLlmPrompts = $state({});
  let adminLlmLoading = $state(false);
  let adminLlmError = $state(null);
  let adminLlmEditFeature = $state(null);
  let adminLlmEditModel = $state('');
  let adminLlmEditMaxTokens = $state('');
  let adminLlmEditPrompt = $state('');
  let adminLlmSaving = $state(false);
  let adminLlmSaved = $state(false);

  // ── Analytics ────────────────────────────────────────────────────────
  let analyticsTop = $state([]);
  let analyticsLoading = $state(false);
  let analyticsError = $state(null);
  let costSummary = $state([]);
  let costLoading = $state(false);
  let activityLog = $state([]);
  let activityLoading = $state(false);

  // ── BCP (Business Continuity) ─────────────────────────────────────────
  let bcpTargets = $state(null);
  let bcpLoading = $state(false);
  let bcpDrillRunning = $state(false);
  let bcpDrillResult = $state(null);
  let snapshots = $state([]);
  let snapshotsLoading = $state(false);
  let retention = $state(null);
  let retentionLoading = $state(false);
  let creatingSnapshot = $state(false);

  // ── Live Audit Stream ───────────────────────────────────────────────
  let auditStreamEvents = $state([]);
  let auditStreaming = $state(false);
  let auditStreamSource = null;

  // ── Audit detail expansion ───────────────────────────────────────────
  let expandedAuditId = $state(null);

  // ── Sorting (per-table) ───────────────────────────────────────────────
  let computeSortCol = $state('name');
  let computeSortDir = $state(1);
  let budgetSortCol = $state('workspace_name');
  let budgetSortDir = $state(1);
  let auditSortCol = $state('timestamp');
  let auditSortDir = $state(-1);
  let jobsSortCol = $state('name');
  let jobsSortDir = $state(1);

  function toggleSort(col, currentCol, currentDir, setCol, setDir) {
    if (col === currentCol) { setDir(currentDir * -1); }
    else { setCol(col); setDir(1); }
  }

  function sortedBy(arr, col, dir) {
    return [...arr].sort((a, b) => {
      const av = a[col] ?? '';
      const bv = b[col] ?? '';
      if (av < bv) return -1 * dir;
      if (av > bv) return 1 * dir;
      return 0;
    });
  }

  function sortArrow(col, activeCol, dir) {
    return col === activeCol ? (dir === 1 ? ' ↑' : ' ↓') : '';
  }

  // ── Data loading driven by tab ─────────────────────────────────────────
  $effect(() => {
    const tab = activeTab;

    if (tab === 'users') {
      if (untrack(() => !currentUser && !usersLoading)) loadUsers();
    }
    if (tab === 'compute') {
      if (untrack(() => computeTargets.length === 0 && !computeLoading)) loadCompute();
    }
    if (tab === 'budget') {
      if (untrack(() => !budgetSummary && !budgetLoading)) loadBudget();
    }
    if (tab === 'audit') {
      if (untrack(() => auditEvents.length === 0 && !auditLoading)) loadAudit();
    }
    if (tab === 'health') {
      if (untrack(() => !health && !healthLoading)) loadHealth();
    }
    if (tab === 'jobs') {
      if (untrack(() => jobs.length === 0 && !jobsLoading)) loadJobs();
    }
    if (tab === 'policies') {
      if (untrack(() => policies.length === 0 && !policiesLoading)) loadPolicies();
    }
    if (tab === 'llm') {
      if (untrack(() => Object.keys(adminLlmConfigs).length === 0 && !adminLlmLoading)) loadAdminLlm();
    }
    if (tab === 'analytics') {
      if (untrack(() => analyticsTop.length === 0 && !analyticsLoading)) loadAnalytics();
    }
    if (tab === 'bcp') {
      if (untrack(() => !bcpTargets && !bcpLoading)) loadBcp();
    }
    // Start/stop audit stream based on tab
    if (tab === 'audit') {
      startAuditStream();
    } else {
      stopAuditStream();
    }
  });

  async function loadUsers() {
    usersLoading = true;
    usersError = null;
    try {
      currentUser = await api.me();
    } catch (e) {
      usersError = e?.message ?? $t('tenant_settings.error_load_users');
    } finally {
      usersLoading = false;
    }
  }

  async function loadCompute() {
    computeLoading = true;
    computeError = null;
    try {
      const data = await api.computeList();
      computeTargets = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      computeError = e?.message ?? $t('tenant_settings.error_load_compute');
    } finally {
      computeLoading = false;
    }
  }

  async function loadBudget() {
    budgetLoading = true;
    budgetError = null;
    try {
      budgetSummary = await api.budgetSummary();
    } catch (e) {
      budgetError = e?.message ?? $t('tenant_settings.error_load_budget');
    } finally {
      budgetLoading = false;
    }
  }

  async function loadAudit() {
    auditLoading = true;
    auditError = null;
    try {
      const params = auditFilterType ? { event_type: auditFilterType } : {};
      const data = await api.adminAudit(params);
      auditEvents = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      auditError = e?.message ?? $t('tenant_settings.error_load_audit');
    } finally {
      auditLoading = false;
    }
  }

  async function refreshAudit() {
    auditEvents = [];
    await loadAudit();
  }

  async function loadHealth() {
    healthLoading = true;
    healthError = null;
    try {
      const [h, v] = await Promise.all([
        api.adminHealth(),
        api.version().catch(() => null),
      ]);
      health = h;
      versionInfo = v;
    } catch (e) {
      healthError = e?.message ?? $t('tenant_settings.error_load_health');
    } finally {
      healthLoading = false;
    }
  }

  async function loadJobs() {
    jobsLoading = true;
    jobsError = null;
    try {
      const data = await api.adminJobs();
      jobs = Array.isArray(data) ? data : (data?.jobs ?? []);
    } catch (e) {
      jobsError = e?.message ?? $t('tenant_settings.error_load_jobs');
    } finally {
      jobsLoading = false;
    }
  }

  async function runJob(jobName) {
    runningJob = jobName;
    try {
      await api.adminRunJob(jobName);
      showToast($t('tenant_settings.jobs.job_triggered', { values: { name: jobName } }), { type: 'success' });
      jobs = [];
      await loadJobs();
    } catch (e) {
      showToast($t('tenant_settings.jobs.job_failed', { values: { error: e?.message ?? 'Unknown error' } }), { type: 'error' });
    } finally {
      runningJob = null;
    }
  }

  // ── Compute CRUD ───────────────────────────────────────────────────────
  async function createComputeTarget() {
    if (!newComputeName.trim()) return;
    computeCreating = true;
    try {
      await api.computeCreate({ name: newComputeName.trim(), target_type: newComputeType, config: {} });
      showToast('Compute target created', { type: 'success' });
      newComputeName = '';
      showComputeForm = false;
      computeTargets = [];
      await loadCompute();
    } catch (e) {
      showToast(`Failed to create: ${e?.message ?? 'Unknown error'}`, { type: 'error' });
    } finally {
      computeCreating = false;
    }
  }

  async function deleteComputeTarget(id) {
    computeDeleting = id;
    try {
      await api.computeDelete(id);
      showToast('Compute target deleted', { type: 'success' });
      computeTargets = computeTargets.filter(ct => ct.id !== id);
    } catch (e) {
      showToast(`Failed to delete: ${e?.message ?? 'Unknown error'}`, { type: 'error' });
    } finally {
      computeDeleting = null;
    }
  }

  // ── Policies ──────────────────────────────────────────────────────────
  async function loadPolicies() {
    policiesLoading = true;
    policiesError = null;
    try {
      const [pols, decs] = await Promise.all([
        api.policies().catch(() => []),
        api.policyDecisions({ limit: 20 }).catch(() => []),
      ]);
      policies = Array.isArray(pols) ? pols : (pols?.items ?? []);
      policyDecisions = Array.isArray(decs) ? decs : (decs?.items ?? []);
    } catch (e) {
      policiesError = e?.message ?? 'Failed to load policies';
    } finally {
      policiesLoading = false;
    }
  }

  // ── LLM Defaults ─────────────────────────────────────────────────────
  async function loadAdminLlm() {
    adminLlmLoading = true;
    adminLlmError = null;
    try {
      const cfgMap = {};
      const promptMap = {};
      await Promise.all(LLM_FEATURES.map(async (f) => {
        try {
          const cfg = await api.adminLlmConfigGet(f);
          if (cfg?.model_name) cfgMap[f] = cfg;
        } catch { /* not configured */ }
        try {
          const p = await api.adminLlmPromptGet(f);
          if (p?.content) promptMap[f] = p;
        } catch { /* not configured */ }
      }));
      adminLlmConfigs = cfgMap;
      adminLlmPrompts = promptMap;
    } catch (e) {
      adminLlmError = e?.message ?? 'Failed to load LLM defaults';
    } finally {
      adminLlmLoading = false;
    }
  }

  function editAdminLlm(feature) {
    adminLlmEditFeature = feature;
    const cfg = adminLlmConfigs[feature];
    adminLlmEditModel = cfg?.model_name ?? '';
    adminLlmEditMaxTokens = cfg?.max_tokens ? String(cfg.max_tokens) : '';
    adminLlmEditPrompt = adminLlmPrompts[feature]?.content ?? '';
    adminLlmSaved = false;
  }

  async function saveAdminLlm() {
    if (!adminLlmEditFeature) return;
    adminLlmSaving = true;
    adminLlmSaved = false;
    try {
      if (adminLlmEditModel.trim()) {
        const data = { model_name: adminLlmEditModel.trim() };
        if (adminLlmEditMaxTokens.trim()) data.max_tokens = parseInt(adminLlmEditMaxTokens);
        await api.adminLlmConfigSet(adminLlmEditFeature, data);
      }
      if (adminLlmEditPrompt.trim()) {
        await api.adminLlmPromptSet(adminLlmEditFeature, { content: adminLlmEditPrompt.trim() });
      }
      adminLlmSaved = true;
      setTimeout(() => { adminLlmSaved = false; }, 2000);
      await loadAdminLlm();
    } catch (e) {
      showToast('Failed to save: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      adminLlmSaving = false;
    }
  }

  // ── Analytics ─────────────────────────────────────────────────────────
  async function loadAnalytics() {
    analyticsLoading = true;
    analyticsError = null;
    try {
      const [top, costs, activity] = await Promise.all([
        api.analyticsTop({ limit: 10 }).catch(() => []),
        api.costSummary().catch(() => []),
        api.activity(20).catch(() => []),
      ]);
      analyticsTop = Array.isArray(top) ? top : (top?.items ?? []);
      costSummary = Array.isArray(costs) ? costs : (costs?.items ?? []);
      activityLog = Array.isArray(activity) ? activity : (activity?.items ?? []);
    } catch (e) {
      analyticsError = e?.message ?? 'Failed to load analytics';
    } finally {
      analyticsLoading = false;
    }
  }

  // ── BCP ────────────────────────────────────────────────────────────────
  async function loadBcp() {
    bcpLoading = true;
    try {
      const [targets, snaps, ret] = await Promise.all([
        api.bcpTargets().catch(() => null),
        api.adminListSnapshots().catch(() => []),
        api.adminRetention().catch(() => null),
      ]);
      bcpTargets = targets;
      snapshots = Array.isArray(snaps) ? snaps : (snaps?.items ?? []);
      retention = ret;
    } catch {
      bcpTargets = null;
    } finally {
      bcpLoading = false;
    }
  }

  async function runBcpDrill() {
    bcpDrillRunning = true;
    bcpDrillResult = null;
    try {
      const result = await api.bcpDrill();
      bcpDrillResult = result;
      showToast('BCP drill completed', { type: 'success' });
      // Refresh snapshots
      api.adminListSnapshots().then(s => { snapshots = Array.isArray(s) ? s : []; }).catch(() => {});
    } catch (e) {
      showToast('BCP drill failed: ' + (e?.message ?? e), { type: 'error' });
      bcpDrillResult = { error: e?.message ?? 'Unknown error' };
    } finally {
      bcpDrillRunning = false;
    }
  }

  async function createSnapshot() {
    creatingSnapshot = true;
    try {
      await api.adminCreateSnapshot();
      showToast('Snapshot created', { type: 'success' });
      api.adminListSnapshots().then(s => { snapshots = Array.isArray(s) ? s : []; }).catch(() => {});
    } catch (e) {
      showToast('Snapshot failed: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      creatingSnapshot = false;
    }
  }

  async function deleteSnapshot(id) {
    try {
      await api.adminDeleteSnapshot(id);
      snapshots = snapshots.filter(s => s.id !== id);
      showToast('Snapshot deleted', { type: 'success' });
    } catch (e) {
      showToast('Delete failed: ' + (e?.message ?? e), { type: 'error' });
    }
  }

  // ── Live Audit Stream ────────────────────────────────────────────────
  function startAuditStream() {
    if (auditStreaming || auditStreamSource) return;
    if (typeof EventSource === 'undefined') return;
    const token = localStorage.getItem('gyre_token') ?? '';
    const url = api.auditStreamUrl();
    const es = new EventSource(`${url}?token=${encodeURIComponent(token)}`);
    auditStreaming = true;
    auditStreamSource = es;
    es.onmessage = (evt) => {
      try {
        const event = JSON.parse(evt.data);
        auditStreamEvents = [event, ...auditStreamEvents].slice(0, 50);
      } catch { /* ignore parse errors */ }
    };
    es.onerror = () => {
      auditStreaming = false;
      es.close();
      auditStreamSource = null;
    };
  }

  function stopAuditStream() {
    if (auditStreamSource) {
      auditStreamSource.close();
      auditStreamSource = null;
      auditStreaming = false;
    }
  }

  function fmtTimestamp(ts) {
    if (!ts) return '—';
    const d = typeof ts === 'number' ? new Date(ts < 1e12 ? ts * 1000 : ts) : new Date(ts);
    return d.toLocaleString();
  }

  function shortId(id) {
    if (!id || typeof id !== 'string') return '—';
    return sharedShortId(id);
  }

  // Entity name resolution uses shared singleton cache
  function resolveWorkspaceName(id) {
    if (!id) return '—';
    return sharedEntityName('workspace', id);
  }

  function resolveEntityName(type, id) {
    if (!id) return shortId(id);
    return sharedEntityName(type, id);
  }

  // ── Tab keyboard navigation ────────────────────────────────────────────
  let tabListEl = $state(null);

  function onTabKeydown(e) {
    const tabs = tabListEl?.querySelectorAll('[role="tab"]');
    if (!tabs?.length) return;
    const arr = Array.from(tabs);
    const current = arr.indexOf(document.activeElement);
    if (e.key === 'ArrowRight') { e.preventDefault(); arr[(current + 1) % arr.length]?.focus(); }
    else if (e.key === 'ArrowLeft') { e.preventDefault(); arr[(current - 1 + arr.length) % arr.length]?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); arr[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); arr[arr.length - 1]?.focus(); }
  }
</script>

<div class="tenant-settings" data-testid="tenant-settings">
  <!-- Page header with back button -->
  <header class="settings-header">
    <button
      class="back-btn"
      onclick={() => onBack?.()}
      aria-label={$t('topbar.back_to_all_workspaces')}
      data-testid="tenant-settings-back"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 5l-7 7 7 7"/>
      </svg>
    </button>
    <div class="header-text">
      <h1 class="settings-title">{$t('tenant_settings.title')}</h1>
      <p class="settings-subtitle">{$t('tenant_settings.subtitle')}</p>
    </div>
  </header>

  <!-- Tab bar -->
  <div
    class="tab-bar"
    role="tablist"
    aria-label={$t('tenant_settings.sections_label')}
    tabindex="-1"
    bind:this={tabListEl}
    onkeydown={onTabKeydown}
    data-testid="tenant-settings-tabs"
  >
    {#each TABS as tab (tab.id)}
      <button
        class="tab-btn"
        class:active={activeTab === tab.id}
        role="tab"
        tabindex={activeTab === tab.id ? 0 : -1}
        aria-selected={activeTab === tab.id}
        aria-controls="tab-panel-{tab.id}"
        onclick={() => { activeTab = tab.id; }}
        data-testid="tenant-settings-tab-{tab.id}"
      >
        {tab.labelKey ? $t(tab.labelKey) : tab.label}
      </button>
    {/each}
  </div>

  <!-- Tab content -->
  <div class="tab-content">

    <!-- ── Users ──────────────────────────────────────────────────────── -->
    {#if activeTab === 'users'}
      <div id="tab-panel-users" role="tabpanel" aria-label="Users" class="tab-panel" data-testid="tenant-tab-users">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.users.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.users.desc')}</p>
        </div>

        {#if usersLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.users.loading')}</div>
        {:else if usersError}
          <div class="panel-error" role="alert">{usersError}</div>
        {:else if currentUser}
          <div class="info-card">
            <div class="info-row">
              <span class="info-label">{$t('tenant_settings.users.current_user')}</span>
              <span class="info-value">{currentUser.username ?? currentUser.name ?? currentUser.email ?? '—'}</span>
            </div>
            {#if currentUser.email}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.email')}</span>
                <span class="info-value">{currentUser.email}</span>
              </div>
            {/if}
            {#if currentUser.role}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.role')}</span>
                <span class="info-value role-badge">{currentUser.role}</span>
              </div>
            {/if}
            {#if currentUser.tenant_id}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.tenant_id')}</span>
                <span class="info-value mono">{currentUser.tenant_id}</span>
              </div>
            {/if}
          </div>
          <div class="panel-note">
            <p>{$t('tenant_settings.users.provisioning_note')}</p>
          </div>
        {:else}
          <div class="panel-empty">{$t('tenant_settings.users.no_user_info')}</div>
        {/if}
      </div>

    <!-- ── Compute Targets ────────────────────────────────────────────── -->
    {:else if activeTab === 'compute'}
      <div id="tab-panel-compute" role="tabpanel" aria-label="Compute Targets" class="tab-panel" data-testid="tenant-tab-compute">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.compute.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.compute.desc')}</p>
        </div>

        <!-- Create compute target -->
        <div class="action-bar">
          {#if showComputeForm}
            <div class="inline-form">
              <input type="text" class="form-input" bind:value={newComputeName} placeholder="Target name" disabled={computeCreating} />
              <select class="filter-select" bind:value={newComputeType} disabled={computeCreating}>
                <option value="local">Local</option>
                <option value="ssh">SSH</option>
                <option value="container">Container</option>
              </select>
              <button class="run-btn" onclick={createComputeTarget} disabled={computeCreating || !newComputeName.trim()}>
                {computeCreating ? 'Creating…' : 'Create'}
              </button>
              <button class="run-btn" onclick={() => { showComputeForm = false; }}>Cancel</button>
            </div>
          {:else}
            <button class="run-btn" onclick={() => { showComputeForm = true; }}>+ Add Compute Target</button>
          {/if}
        </div>

        {#if computeLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.compute.loading')}</div>
        {:else if computeError}
          <div class="panel-error" role="alert">{computeError}</div>
        {:else if computeTargets.length === 0}
          <div class="panel-empty">{$t('tenant_settings.compute.empty')}</div>
        {:else}
          <table class="data-table" data-testid="compute-targets-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={computeSortCol === 'name' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('name', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_name')}{sortArrow('name', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'kind' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('kind', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_kind')}{sortArrow('kind', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'status' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('status', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_status')}{sortArrow('status', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'capacity' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('capacity', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_capacity')}{sortArrow('capacity', computeSortCol, computeSortDir)}</button></th>
                <th scope="col">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(computeTargets, computeSortCol, computeSortDir) as ct (ct.id ?? ct.name)}
                <tr>
                  <td class="td-name">{ct.name ?? '—'}</td>
                  <td>{ct.kind ?? ct.target_type ?? ct.type ?? '—'}</td>
                  <td>
                    <span class="status-pill" class:status-ok={ct.status === 'healthy' || ct.status === 'active'} class:status-warn={ct.status === 'degraded'} class:status-err={ct.status === 'error' || ct.status === 'offline'}>
                      {ct.status ?? '—'}
                    </span>
                  </td>
                  <td>{ct.capacity ?? ct.max_agents ?? '—'}</td>
                  <td>
                    <button class="delete-btn" onclick={() => deleteComputeTarget(ct.id)} disabled={computeDeleting === ct.id} title="Delete {ct.name}">
                      {computeDeleting === ct.id ? '…' : '✕'}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- ── Budget ─────────────────────────────────────────────────────── -->
    {:else if activeTab === 'budget'}
      <div id="tab-panel-budget" role="tabpanel" aria-label="Budget" class="tab-panel" data-testid="tenant-tab-budget">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.budget.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.budget.desc')}</p>
        </div>

        {#if budgetLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.budget.loading')}</div>
        {:else if budgetError}
          <div class="panel-error" role="alert">{budgetError}</div>
        {:else if !budgetSummary}
          <div class="panel-empty">{$t('tenant_settings.budget.empty')}</div>
        {:else}
          <div class="budget-grid">
            {#if budgetSummary.total_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.total_credits')}</span>
                <span class="budget-value">{budgetSummary.total_credits.toLocaleString()}</span>
              </div>
            {/if}
            {#if budgetSummary.used_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.used_credits')}</span>
                <span class="budget-value">{budgetSummary.used_credits.toLocaleString()}</span>
              </div>
            {/if}
            {#if budgetSummary.total_credits && budgetSummary.used_credits != null}
              {@const pct = Math.round((budgetSummary.used_credits / budgetSummary.total_credits) * 100)}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.usage')}</span>
                <span class="budget-value" class:danger={pct > 90} class:warn={pct > 70 && pct <= 90}>{pct}%</span>
              </div>
            {/if}
            {#if budgetSummary.remaining_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.remaining')}</span>
                <span class="budget-value">{budgetSummary.remaining_credits.toLocaleString()}</span>
              </div>
            {/if}
          </div>
          {#if budgetSummary.workspace_breakdown}
            <h3 class="sub-heading">{$t('tenant_settings.budget.per_workspace')}</h3>
            <table class="data-table" data-testid="budget-breakdown-table">
              <thead>
                <tr>
                  <th scope="col" aria-sort={budgetSortCol === 'workspace_name' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('workspace_name', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_workspace')}{sortArrow('workspace_name', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'allocated' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('allocated', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_allocated')}{sortArrow('allocated', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'used' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('used', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_used')}{sortArrow('used', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'pct' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('pct', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_usage_pct')}{sortArrow('pct', budgetSortCol, budgetSortDir)}</button></th>
                </tr>
              </thead>
              <tbody>
                {#each sortedBy(budgetSummary.workspace_breakdown, budgetSortCol, budgetSortDir) as row (row.workspace_id ?? row.workspace_name)}
                  <tr>
                    <td>{row.workspace_name ?? row.workspace_id ?? '—'}</td>
                    <td>{row.allocated ?? '—'}</td>
                    <td>{row.used ?? '—'}</td>
                    <td>{row.pct != null ? row.pct + '%' : '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      </div>

    <!-- ── Audit ──────────────────────────────────────────────────────── -->
    {:else if activeTab === 'audit'}
      <div id="tab-panel-audit" role="tabpanel" aria-label="Audit" class="tab-panel" data-testid="tenant-tab-audit">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.audit.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.audit.desc')}</p>
        </div>

        {#if auditStreaming && auditStreamEvents.length > 0}
          <div class="live-stream-section">
            <div class="live-stream-header">
              <span class="live-dot"></span>
              <span class="live-label">Live Stream ({auditStreamEvents.length} events)</span>
            </div>
            <div class="live-stream-list">
              {#each auditStreamEvents.slice(0, 10) as event}
                <div class="live-event">
                  <span class="live-event-time">{fmtTimestamp(event.timestamp ?? event.created_at)}</span>
                  <span class="live-event-type">{(event.event_type ?? event.type ?? '—').replace(/_/g, ' ')}</span>
                  {#if event.actor ?? event.user_id ?? event.agent_id}
                    <span class="live-event-actor mono">{event.actor ?? resolveEntityName('agent', event.agent_id ?? event.user_id)}</span>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {:else if auditStreaming}
          <div class="live-stream-section">
            <div class="live-stream-header">
              <span class="live-dot"></span>
              <span class="live-label">Connected — waiting for events...</span>
            </div>
          </div>
        {/if}

        <div class="filter-bar" data-testid="audit-filter-bar">
          <label for="audit-filter-type" class="filter-label">{$t('tenant_settings.audit.event_type')}</label>
          <select
            id="audit-filter-type"
            class="filter-select"
            bind:value={auditFilterType}
            onchange={refreshAudit}
          >
            <option value="">{$t('tenant_settings.audit.all_events')}</option>
            <option value="tenant_created">{$t('tenant_settings.audit_event_types.tenant_created')}</option>
            <option value="tenant_updated">{$t('tenant_settings.audit_event_types.tenant_updated')}</option>
            <option value="user_role_changed">{$t('tenant_settings.audit_event_types.user_role_changed')}</option>
            <option value="compute_target_added">{$t('tenant_settings.audit_event_types.compute_target_added')}</option>
            <option value="budget_updated">{$t('tenant_settings.audit_event_types.budget_updated')}</option>
            <option value="agent_killed">{$t('tenant_settings.audit_event_types.agent_killed')}</option>
            <option value="snapshot_created">{$t('tenant_settings.audit_event_types.snapshot_created')}</option>
            <option value="job_run">{$t('tenant_settings.audit_event_types.job_run')}</option>
          </select>
          <button class="refresh-btn" onclick={refreshAudit} aria-label={$t('tenant_settings.refresh')} data-testid="audit-refresh">
            {$t('tenant_settings.refresh')}
          </button>
        </div>

        {#if auditLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.audit.loading')}</div>
        {:else if auditError}
          <div class="panel-error" role="alert">{auditError}</div>
        {:else if auditEvents.length === 0}
          <div class="panel-empty">{$t('tenant_settings.audit_empty')}</div>
        {:else}
          <table class="data-table" data-testid="audit-events-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={auditSortCol === 'timestamp' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('timestamp', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_time')}{sortArrow('timestamp', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'event_type' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('event_type', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_event')}{sortArrow('event_type', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'actor' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('actor', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_actor')}{sortArrow('actor', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'detail' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('detail', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_details')}{sortArrow('detail', auditSortCol, auditSortDir)}</button></th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(auditEvents, auditSortCol, auditSortDir) as ev (ev.id ?? ev.timestamp)}
                {@const evId = ev.id ?? ev.timestamp}
                <tr class="clickable-row" onclick={() => { expandedAuditId = expandedAuditId === evId ? null : evId; }} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') expandedAuditId = expandedAuditId === evId ? null : evId; }}>
                  <td class="td-mono">{fmtTimestamp(ev.timestamp)}</td>
                  <td><span class="event-type">{(ev.event_type ?? ev.kind ?? '—').replace(/_/g, ' ')}</span></td>
                  <td>{ev.actor ?? ev.user ?? '—'}</td>
                  <td class="td-detail">{ev.detail ?? ev.message ?? '—'}</td>
                </tr>
                {#if expandedAuditId === evId}
                  <tr class="audit-detail-row">
                    <td colspan="4">
                      <div class="audit-detail-content">
                        <dl class="audit-dl">
                          {#if ev.id}<dt>ID</dt><dd class="mono">{sharedShortId(ev.id)}</dd>{/if}
                          {#if ev.event_type}<dt>Event</dt><dd>{ev.event_type}</dd>{/if}
                          {#if ev.actor}<dt>Actor</dt><dd>{ev.actor}</dd>{/if}
                          {#if ev.ip_address}<dt>IP</dt><dd class="mono">{ev.ip_address}</dd>{/if}
                          {#if ev.resource_type}
                            {@const refType = ev.resource_type === 'repository' ? 'repo' : ev.resource_type}
                            {@const clickable = refType === 'agent' || refType === 'mr' || refType === 'task' || refType === 'spec'}
                            <dt>Resource</dt>
                            <dd>
                              {ev.resource_type}
                              {#if ev.resource_id}
                                {#if clickable}
                                  : <button class="audit-entity-btn" onclick={(e) => { e.stopPropagation(); nav(refType, ev.resource_id, refType === 'spec' ? { path: ev.resource_id } : {}); }} title="View {refType}">{resolveEntityName(refType, ev.resource_id)}</button>
                                {:else}
                                  : {resolveEntityName(refType, ev.resource_id)}
                                {/if}
                              {/if}
                            </dd>
                          {/if}
                          {#if ev.workspace_id}<dt>Workspace</dt><dd class="mono" title={ev.workspace_id}>{resolveWorkspaceName(ev.workspace_id)}</dd>{/if}
                          {#if ev.detail ?? ev.message}<dt>Detail</dt><dd class="audit-full-detail">{ev.detail ?? ev.message}</dd>{/if}
                          {#if ev.metadata}
                            <dt>Metadata</dt><dd><pre class="audit-meta-pre">{JSON.stringify(ev.metadata, null, 2)}</pre></dd>
                          {/if}
                        </dl>
                      </div>
                    </td>
                  </tr>
                {/if}
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- ── Health ─────────────────────────────────────────────────────── -->
    {:else if activeTab === 'health'}
      <div id="tab-panel-health" role="tabpanel" aria-label="Health" class="tab-panel" data-testid="tenant-tab-health">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.health.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.health.subtitle')}</p>
        </div>

        {#if healthLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.health.loading')}</div>
        {:else if healthError}
          <div class="panel-error" role="alert">{healthError}</div>
        {:else if !health}
          <div class="panel-empty">{$t('tenant_settings.health.empty')}</div>
        {:else}
          {#if versionInfo}
            <div class="version-banner" data-testid="version-banner">
              <span class="version-label">Gyre Server</span>
              {#if versionInfo.version}<span class="version-value">{versionInfo.version}</span>{/if}
              {#if versionInfo.commit}<span class="version-commit mono">{typeof versionInfo.commit === 'string' ? versionInfo.commit.slice(0, 7) : versionInfo.commit}</span>{/if}
              {#if versionInfo.build_date}<span class="version-date">{versionInfo.build_date}</span>{/if}
              {#if versionInfo.rust_version}<span class="version-rust">Rust {versionInfo.rust_version}</span>{/if}
            </div>
          {/if}
          <div class="health-grid" data-testid="health-grid">
            {#each Object.entries(health) as [component, status] (component)}
              {@const ok = status === 'ok' || status === 'healthy' || status === true}
              {@const degraded = status === 'degraded' || status === 'warn'}
              <div class="health-card" class:health-ok={ok} class:health-warn={degraded} class:health-err={!ok && !degraded}>
                <span class="health-dot" aria-hidden="true"></span>
                <span class="health-component">{component}</span>
                <span class="health-status">{typeof status === 'boolean' ? (status ? $t('tenant_settings.health.status_ok') : $t('tenant_settings.health.status_error')) : (status ?? '—')}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

    <!-- ── Jobs ───────────────────────────────────────────────────────── -->
    {:else if activeTab === 'jobs'}
      <div id="tab-panel-jobs" role="tabpanel" aria-label="Jobs" class="tab-panel" data-testid="tenant-tab-jobs">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.jobs.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.jobs.subtitle')}</p>
        </div>

        {#if jobsLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.jobs.loading')}</div>
        {:else if jobsError}
          <div class="panel-error" role="alert">{jobsError}</div>
        {:else if jobs.length === 0}
          <div class="panel-empty">{$t('tenant_settings.jobs.empty')}</div>
        {:else}
          <table class="data-table" data-testid="jobs-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={jobsSortCol === 'name' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('name', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_job')}{sortArrow('name', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'schedule' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('schedule', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_schedule')}{sortArrow('schedule', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'last_run' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('last_run', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_last_run')}{sortArrow('last_run', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'status' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('status', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_status')}{sortArrow('status', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col">{$t('tenant_settings.audit.col_action')}</th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(jobs, jobsSortCol, jobsSortDir) as job (job.name ?? job.id)}
                <tr>
                  <td class="td-name">{job.name ?? job.id ?? '—'}</td>
                  <td class="td-mono">{job.schedule ?? '—'}</td>
                  <td class="td-mono">{job.last_run ? new Date(job.last_run).toLocaleString() : '—'}</td>
                  <td>
                    <span class="status-pill" class:status-ok={job.status === 'ok' || job.status === 'success'} class:status-warn={job.status === 'running'} class:status-err={job.status === 'error' || job.status === 'failed'}>
                      {job.status ?? '—'}
                    </span>
                  </td>
                  <td>
                    <button
                      class="run-btn"
                      onclick={() => runJob(job.name ?? job.id)}
                      disabled={runningJob === (job.name ?? job.id)}
                      aria-label="Run job {job.name ?? job.id}"
                      data-testid="run-job-{job.name ?? job.id}"
                    >
                      {runningJob === (job.name ?? job.id) ? $t('tenant_settings.jobs.running') : $t('tenant_settings.jobs.run_now')}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- ── Policies (ABAC) ─────────────────────────────────────────────── -->
    {:else if activeTab === 'policies'}
      <div id="tab-panel-policies" role="tabpanel" aria-label="Policies" class="tab-panel" data-testid="tenant-tab-policies">
        <div class="panel-header">
          <h2 class="panel-title">Access Policies (ABAC)</h2>
          <p class="panel-desc">Attribute-based access control policies governing agent actions, git push, and resource access across the tenant.</p>
        </div>

        <!-- Create Policy button -->
        <div class="panel-actions" style="margin-bottom: var(--space-4)">
          <button class="action-btn" onclick={() => (showPolicyForm = !showPolicyForm)}>
            {showPolicyForm ? 'Cancel' : '+ Create Policy'}
          </button>
        </div>

        {#if showPolicyForm}
          <div class="create-form" style="margin-bottom: var(--space-6)">
            <div class="form-row">
              <label class="form-label">Name <input class="form-input" bind:value={policyForm.name} placeholder="e.g. allow-agent-push" /></label>
              <label class="form-label">Effect
                <select class="form-input" bind:value={policyForm.effect}>
                  <option value="allow">Allow</option>
                  <option value="deny">Deny</option>
                </select>
              </label>
              <label class="form-label">Priority <input class="form-input" type="number" bind:value={policyForm.priority} /></label>
            </div>
            <div class="form-row">
              <label class="form-label">Actions <input class="form-input" bind:value={policyForm.actions} placeholder="push, read, spawn" /></label>
              <label class="form-label">Resource Types <input class="form-input" bind:value={policyForm.resource_types} placeholder="repo, agent" /></label>
              <label class="form-label">Scope
                <select class="form-input" bind:value={policyForm.scope}>
                  <option value="tenant">Tenant</option>
                  <option value="workspace">Workspace</option>
                </select>
              </label>
            </div>
            <div class="form-row">
              <label class="form-label">Condition Attribute <input class="form-input" bind:value={policyForm.condition_attr} placeholder="subject.type" /></label>
              <label class="form-label">Operator
                <select class="form-input" bind:value={policyForm.condition_op}>
                  <option value="equals">equals</option>
                  <option value="not_equals">not_equals</option>
                  <option value="contains">contains</option>
                  <option value="in">in</option>
                </select>
              </label>
              <label class="form-label">Value <input class="form-input" bind:value={policyForm.condition_val} placeholder="agent" /></label>
            </div>
            <button class="action-btn action-btn-primary" onclick={createPolicy} disabled={policyFormSaving || !policyForm.name.trim()}>
              {policyFormSaving ? 'Creating...' : 'Create Policy'}
            </button>
          </div>
        {/if}

        {#if policiesLoading}
          <div class="panel-loading" aria-live="polite">Loading policies…</div>
        {:else if policiesError}
          <div class="panel-error" role="alert">{policiesError}</div>
        {:else}
          {#if policies.length > 0}
            <h3 class="sub-heading">Active Policies ({policies.length})</h3>
            <table class="data-table">
              <thead>
                <tr>
                  <th scope="col"><button class="sort-btn">Name</button></th>
                  <th scope="col"><button class="sort-btn">Scope</button></th>
                  <th scope="col"><button class="sort-btn">Effect</button></th>
                  <th scope="col"><button class="sort-btn">Actions</button></th>
                  <th scope="col"><button class="sort-btn">Resources</button></th>
                  <th scope="col"><button class="sort-btn">Priority</button></th>
                  <th scope="col"></th>
                </tr>
              </thead>
              <tbody>
                {#each policies as policy (policy.id ?? policy.name)}
                  <tr>
                    <td class="td-name">{policy.name ?? '—'}</td>
                    <td>{policy.scope ?? '—'}</td>
                    <td>
                      <span class="status-pill" class:status-ok={policy.effect === 'allow'} class:status-err={policy.effect === 'deny'}>
                        {policy.effect ?? '—'}
                      </span>
                    </td>
                    <td class="mono">{(policy.actions ?? []).join(', ') || '—'}</td>
                    <td class="mono">{(policy.resource_types ?? []).join(', ') || '—'}</td>
                    <td>{policy.priority ?? '—'}</td>
                    <td>
                      {#if policy.id}
                        <button class="delete-btn" onclick={() => deletePolicy(policy.id)} disabled={deletingPolicyId === policy.id} title="Delete policy">
                          {deletingPolicyId === policy.id ? '...' : '✕'}
                        </button>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <div class="panel-empty">No ABAC policies configured. Create one above to control agent access.</div>
          {/if}

          {#if policyDecisions.length > 0}
            <h3 class="sub-heading" style="margin-top: var(--space-6)">Recent Decisions ({policyDecisions.length})</h3>
            <table class="data-table">
              <thead>
                <tr>
                  <th scope="col"><button class="sort-btn">Time</button></th>
                  <th scope="col"><button class="sort-btn">Decision</button></th>
                  <th scope="col"><button class="sort-btn">Subject</button></th>
                  <th scope="col"><button class="sort-btn">Action</button></th>
                  <th scope="col"><button class="sort-btn">Resource</button></th>
                  <th scope="col"><button class="sort-btn">Policy</button></th>
                </tr>
              </thead>
              <tbody>
                {#each policyDecisions as dec (dec.id ?? dec.timestamp)}
                  <tr>
                    <td class="td-mono">{fmtTimestamp(dec.timestamp ?? dec.evaluated_at)}</td>
                    <td>
                      <span class="status-pill" class:status-ok={dec.decision === 'allow'} class:status-err={dec.decision === 'deny'}>
                        {dec.decision ?? '—'}
                      </span>
                    </td>
                    <td class="mono" title={dec.subject?.id ?? dec.subject_id ?? ''}>
                      {#if dec.subject?.type && dec.subject?.id}
                        {dec.subject.type}: <button class="audit-entity-btn" onclick={() => nav(dec.subject.type, dec.subject.id, {})}>{resolveEntityName(dec.subject.type, dec.subject.id)}</button>
                      {:else if dec.subject_id}
                        <button class="audit-entity-btn" onclick={() => nav('agent', dec.subject_id, {})}>{resolveEntityName('agent', dec.subject_id)}</button>
                      {:else}
                        —
                      {/if}
                    </td>
                    <td>{dec.action ?? '—'}</td>
                    <td>{dec.resource?.type ?? dec.resource_type ?? '—'}</td>
                    <td class="mono">{dec.matched_policy ?? dec.policy_name ?? '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      </div>

    <!-- ── LLM Defaults ─────────────────────────────────────────────── -->
    {:else if activeTab === 'llm'}
      <div id="tab-panel-llm" role="tabpanel" aria-label="LLM Defaults" class="tab-panel" data-testid="tenant-tab-llm">
        <div class="panel-header">
          <h2 class="panel-title">LLM Default Configuration</h2>
          <p class="panel-desc">Set tenant-wide default models and prompt templates. Workspaces can override these per-feature.</p>
        </div>

        {#if adminLlmLoading}
          <div class="panel-loading" aria-live="polite">Loading LLM defaults...</div>
        {:else if adminLlmError}
          <div class="panel-error" role="alert">{adminLlmError}</div>
        {:else}
          <table class="data-table">
            <thead>
              <tr>
                <th>Feature</th>
                <th>Default Model</th>
                <th>Default Prompt</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each LLM_FEATURES as feature}
                {@const cfg = adminLlmConfigs[feature]}
                {@const prompt = adminLlmPrompts[feature]}
                <tr>
                  <td>
                    <div class="llm-feature-cell">
                      <span class="llm-feature-name">{feature.replace(/-/g, ' ')}</span>
                    </div>
                  </td>
                  <td>
                    {#if cfg?.model_name}
                      <code class="mono">{cfg.model_name}</code>
                    {:else}
                      <span class="muted-text">not set</span>
                    {/if}
                  </td>
                  <td>
                    {#if prompt?.content}
                      <span class="prompt-preview-text" title={prompt.content}>{prompt.content.slice(0, 50)}...</span>
                    {:else}
                      <span class="muted-text">built-in</span>
                    {/if}
                  </td>
                  <td>
                    <button class="edit-btn" onclick={() => editAdminLlm(feature)}>Configure</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>

          {#if adminLlmEditFeature}
            <div class="llm-edit-panel">
              <h3 class="llm-edit-heading">Configure: {adminLlmEditFeature.replace(/-/g, ' ')}</h3>
              <div class="llm-field">
                <label class="llm-field-label" for="admin-llm-model">Default model</label>
                <input id="admin-llm-model" class="llm-field-input" bind:value={adminLlmEditModel} placeholder="e.g. claude-sonnet-4-20250514" />
              </div>
              <div class="llm-field">
                <label class="llm-field-label" for="admin-llm-tokens">Max tokens</label>
                <input id="admin-llm-tokens" class="llm-field-input" type="number" bind:value={adminLlmEditMaxTokens} placeholder="e.g. 4096" />
              </div>
              <div class="llm-field">
                <label class="llm-field-label" for="admin-llm-prompt">Default prompt template</label>
                <textarea id="admin-llm-prompt" class="llm-field-textarea" rows="5" bind:value={adminLlmEditPrompt} placeholder="Use {{context}} and {{question}} as template variables"></textarea>
              </div>
              <div class="llm-edit-actions">
                <button class="edit-btn" onclick={() => { adminLlmEditFeature = null; }}>Cancel</button>
                <button class="edit-btn edit-btn-primary" onclick={saveAdminLlm} disabled={adminLlmSaving}>
                  {adminLlmSaving ? 'Saving...' : adminLlmSaved ? 'Saved!' : 'Save Defaults'}
                </button>
              </div>
            </div>
          {/if}
        {/if}
      </div>

    <!-- ── Analytics ──────────────────────────────────────────────────── -->
    {:else if activeTab === 'analytics'}
      <div id="tab-panel-analytics" role="tabpanel" aria-label="Analytics" class="tab-panel" data-testid="tenant-tab-analytics">
        <div class="panel-header">
          <h2 class="panel-title">Analytics & Activity</h2>
          <p class="panel-desc">Platform usage, cost tracking, and recent activity across all workspaces.</p>
        </div>

        {#if analyticsLoading}
          <div class="panel-loading" aria-live="polite">Loading analytics…</div>
        {:else if analyticsError}
          <div class="panel-error" role="alert">{analyticsError}</div>
        {:else}
          <!-- Cost Summary -->
          {#if costSummary.length > 0}
            <h3 class="sub-heading">Cost by Agent</h3>
            <table class="data-table">
              <thead>
                <tr>
                  <th scope="col"><button class="sort-btn">Agent</button></th>
                  <th scope="col"><button class="sort-btn">Total Cost</button></th>
                  <th scope="col"><button class="sort-btn">Tokens</button></th>
                  <th scope="col"><button class="sort-btn">Entries</button></th>
                </tr>
              </thead>
              <tbody>
                {#each costSummary as entry (entry.agent_id ?? entry.id)}
                  <tr>
                    <td class="mono" title={entry.agent_id ?? entry.id ?? ''}><button class="audit-entity-btn" onclick={() => nav('agent', entry.agent_id ?? entry.id, {})}>{resolveEntityName('agent', entry.agent_id ?? entry.id)}</button></td>
                    <td>{entry.total_cost != null ? `$${entry.total_cost.toFixed(4)}` : '—'}</td>
                    <td>{entry.total_tokens?.toLocaleString() ?? entry.tokens?.toLocaleString() ?? '—'}</td>
                    <td>{entry.count ?? entry.entries ?? '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}

          <!-- Top Event Types -->
          {#if analyticsTop.length > 0}
            <h3 class="sub-heading" style="margin-top: var(--space-6)">Top Event Types</h3>
            <div class="analytics-bars">
              {#each analyticsTop as item (item.event_name ?? item.name)}
                {@const maxCount = Math.max(...analyticsTop.map(i => i.count ?? 0))}
                {@const pct = maxCount > 0 ? Math.round(((item.count ?? 0) / maxCount) * 100) : 0}
                <div class="analytics-bar-row">
                  <span class="analytics-bar-label">{(item.event_name ?? item.name ?? '—').replace(/\./g, ' ')}</span>
                  <div class="analytics-bar-track">
                    <div class="analytics-bar-fill" style="width: {pct}%"></div>
                  </div>
                  <span class="analytics-bar-count">{(item.count ?? 0).toLocaleString()}</span>
                </div>
              {/each}
            </div>
          {/if}

          <!-- Recent Activity -->
          {#if activityLog.length > 0}
            <h3 class="sub-heading" style="margin-top: var(--space-6)">Recent Activity</h3>
            <div class="activity-list">
              {#each activityLog as entry (entry.id ?? entry.timestamp)}
                <div class="activity-item">
                  <span class="activity-time">{fmtTimestamp(entry.timestamp ?? entry.created_at)}</span>
                  <span class="event-type">{(entry.event_type ?? entry.kind ?? '—').replace(/_/g, ' ')}</span>
                  {#if entry.actor ?? entry.user_id ?? entry.agent_id}
                    <span class="activity-actor mono" title={entry.user_id ?? entry.agent_id ?? ''}>{entry.actor ?? (entry.agent_id ? resolveEntityName('agent', entry.agent_id) : shortId(entry.user_id))}</span>
                  {/if}
                  {#if entry.detail ?? entry.message ?? entry.description}
                    <span class="activity-detail">{entry.detail ?? entry.message ?? entry.description}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {:else if costSummary.length === 0 && analyticsTop.length === 0}
            <div class="panel-empty">No analytics data available yet. Activity will appear here as agents work.</div>
          {/if}
        {/if}
      </div>

    {:else if activeTab === 'bcp'}
      <div id="tab-panel-bcp" role="tabpanel" aria-label="BCP" class="tab-panel" data-testid="tenant-tab-bcp">
        <div class="panel-header">
          <h2 class="panel-title">Business Continuity</h2>
          <p class="panel-desc">Disaster recovery targets, database snapshots, and data retention policies.</p>
        </div>

        <!-- BCP Targets (RTO/RPO) -->
        {#if bcpLoading}
          <p class="loading-text">Loading...</p>
        {:else}
          {#if bcpTargets}
            <h3 class="sub-heading">Recovery Targets</h3>
            <dl class="bcp-targets">
              {#if bcpTargets.rto_seconds != null}
                <dt>RTO (Recovery Time Objective)</dt>
                <dd>{bcpTargets.rto_seconds < 3600 ? `${Math.round(bcpTargets.rto_seconds / 60)}m` : `${(bcpTargets.rto_seconds / 3600).toFixed(1)}h`}</dd>
              {/if}
              {#if bcpTargets.rpo_seconds != null}
                <dt>RPO (Recovery Point Objective)</dt>
                <dd>{bcpTargets.rpo_seconds < 3600 ? `${Math.round(bcpTargets.rpo_seconds / 60)}m` : `${(bcpTargets.rpo_seconds / 3600).toFixed(1)}h`}</dd>
              {/if}
            </dl>
          {/if}

          <!-- BCP Drill -->
          <h3 class="sub-heading" style="margin-top: var(--space-4)">Drill</h3>
          <p class="panel-desc">Run a BCP drill to verify backup/restore works. Creates a snapshot and verifies it.</p>
          <div class="bcp-drill-section">
            <button class="btn btn-primary" onclick={runBcpDrill} disabled={bcpDrillRunning}>
              {bcpDrillRunning ? 'Running drill...' : 'Run BCP Drill'}
            </button>
            {#if bcpDrillResult}
              <div class="bcp-drill-result" class:bcp-drill-error={bcpDrillResult.error}>
                {#if bcpDrillResult.error}
                  <span class="bcp-result-icon">✗</span> Drill failed: {bcpDrillResult.error}
                {:else}
                  <span class="bcp-result-icon bcp-result-ok">✓</span> Drill completed successfully
                  {#if bcpDrillResult.snapshot_id}
                    <span class="bcp-result-detail mono">Snapshot: {shortId(bcpDrillResult.snapshot_id)}</span>
                  {/if}
                {/if}
              </div>
            {/if}
          </div>

          <!-- Snapshots -->
          <h3 class="sub-heading" style="margin-top: var(--space-4)">Snapshots</h3>
          <div class="bcp-snapshot-actions">
            <button class="btn btn-secondary" onclick={createSnapshot} disabled={creatingSnapshot}>
              {creatingSnapshot ? 'Creating...' : '+ Create Snapshot'}
            </button>
          </div>
          {#if snapshots.length > 0}
            <table class="settings-table">
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Created</th>
                  <th>Size</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each snapshots as snap}
                  <tr>
                    <td class="mono">{shortId(snap.id ?? snap.snapshot_id)}</td>
                    <td>{fmtTimestamp(snap.created_at ?? snap.timestamp)}</td>
                    <td>{snap.size_bytes ? `${(snap.size_bytes / 1024).toFixed(0)} KB` : '—'}</td>
                    <td>
                      <button class="btn-danger-sm" onclick={() => deleteSnapshot(snap.id ?? snap.snapshot_id)} title="Delete snapshot">Delete</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <p class="panel-empty">No snapshots. Create one or run a BCP drill.</p>
          {/if}

          <!-- Retention Policies -->
          {#if retention}
            <h3 class="sub-heading" style="margin-top: var(--space-4)">Retention Policies</h3>
            <dl class="bcp-targets">
              {#each Object.entries(retention) as [key, value]}
                <dt>{key.replace(/_/g, ' ')}</dt>
                <dd>{typeof value === 'number' ? (value < 86400 ? `${Math.round(value / 3600)}h` : `${Math.round(value / 86400)}d`) : JSON.stringify(value)}</dd>
              {/each}
            </dl>
          {/if}
        {/if}
      </div>
    {/if}

  </div>
</div>

<style>
  .tenant-settings {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  /* ── Header ────────────────────────────────────────────────────────────── */
  .settings-header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-6) var(--space-8);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .header-text { min-width: 0; }

  .settings-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
  }

  .settings-subtitle {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* ── Tab bar ────────────────────────────────────────────────────────────── */
  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    padding: 0 var(--space-8);
    overflow-x: auto;
  }

  .tab-btn {
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    white-space: nowrap;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .tab-btn:hover {
    color: var(--color-text-secondary);
  }

  .tab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  /* ── Tab content ────────────────────────────────────────────────────────── */
  .tab-content {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .tab-panel {
    padding: var(--space-6) var(--space-8);
    max-width: 900px;
  }

  /* ── Panel header ────────────────────────────────────────────────────────── */
  .panel-header {
    margin-bottom: var(--space-6);
  }

  .panel-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
  }

  .panel-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .panel-loading,
  .panel-empty {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-4) 0;
  }

  .panel-error {
    font-size: var(--text-sm);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-left: 3px solid var(--color-danger);
    padding: var(--space-3) var(--space-4);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    margin-bottom: var(--space-4);
  }

  .panel-note {
    margin-top: var(--space-4);
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .panel-note p { margin: 0; }

  /* ── Info card (Users tab) ────────────────────────────────────────────── */
  .info-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 480px;
  }

  .info-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .info-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    width: 100px;
    flex-shrink: 0;
  }

  .info-value {
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .info-value.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .role-badge {
    display: inline-block;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-primary);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  /* ── Data tables ─────────────────────────────────────────────────────── */
  .data-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .data-table th {
    padding: 0;
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .data-table td {
    padding: var(--space-3) var(--space-4);
    color: var(--color-text-secondary);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .data-table tr:last-child td { border-bottom: none; }

  .data-table tr:hover td { background: var(--color-surface-elevated); }

  .td-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .td-mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .td-detail {
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Status pills ────────────────────────────────────────────────────── */
  .status-pill {
    display: inline-block;
    padding: 2px var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    background: var(--color-border);
    color: var(--color-text-muted);
  }

  .status-pill.status-ok {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .status-pill.status-warn {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .status-pill.status-err {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  /* ── Event type badge ────────────────────────────────────────────────── */
  .event-type {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-border);
    color: var(--color-text-secondary);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
  }

  /* ── Budget grid ─────────────────────────────────────────────────────── */
  .budget-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-4);
    margin-bottom: var(--space-6);
  }

  .budget-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .budget-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .budget-value {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
  }

  .budget-value.danger { color: var(--color-danger); }
  .budget-value.warn { color: var(--color-warning); }

  .sub-heading {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0 0 var(--space-3) 0;
  }

  /* ── Version banner ─────────────────────────────────────────────────── */
  .version-banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    margin-bottom: var(--space-4);
    font-size: var(--text-sm);
    flex-wrap: wrap;
  }

  .version-label {
    font-weight: 600;
    color: var(--color-text);
  }

  .version-value {
    font-weight: 600;
    color: var(--color-primary);
  }

  .version-commit, .version-date, .version-rust {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  /* ── Health grid ─────────────────────────────────────────────────────── */
  .health-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
  }

  .health-card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }

  .health-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-text-muted);
    flex-shrink: 0;
  }

  .health-card.health-ok .health-dot { background: var(--color-success); }
  .health-card.health-warn .health-dot { background: var(--color-warning); }
  .health-card.health-err .health-dot { background: var(--color-danger); }

  .health-component {
    flex: 1;
    color: var(--color-text);
    font-weight: 500;
  }

  .health-status {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* ── Audit filter bar ────────────────────────────────────────────────── */
  .filter-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
    flex-wrap: wrap;
  }

  .filter-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .filter-select {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .filter-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .refresh-btn {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .refresh-btn:hover { background: var(--color-surface-elevated); }

  .refresh-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Run job button ──────────────────────────────────────────────────── */
  .run-btn {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .run-btn:hover:not(:disabled) {
    background: var(--color-surface-elevated);
    border-color: var(--color-text-muted);
  }

  .run-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .run-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Create policy form ──────────────────────────────────────────────── */
  .create-form {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .form-row {
    display: flex;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .form-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex: 1;
    min-width: 150px;
  }

  .panel-actions {
    display: flex;
    gap: var(--space-2);
  }

  .action-btn-primary {
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border-color: var(--color-primary);
  }

  .action-btn-primary:hover:not(:disabled) { background: var(--color-primary-hover); }
  .action-btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  /* ── Inline forms ────────────────────────────────────────────────────── */
  .action-bar {
    margin-bottom: var(--space-4);
  }

  .inline-form {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .form-input {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    min-width: 200px;
  }

  .form-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .delete-btn {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .delete-btn:hover:not(:disabled) {
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .delete-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  /* ── Clickable audit rows ──────────────────────────────────────────── */
  .clickable-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .clickable-row:hover td { background: var(--color-surface-elevated); }

  .audit-detail-row td {
    background: var(--color-surface-elevated);
    padding: var(--space-4) !important;
    border-bottom: 1px solid var(--color-border);
  }

  .audit-detail-content { max-width: 600px; }

  .audit-dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-3);
    font-size: var(--text-sm);
  }

  .audit-dl dt {
    font-weight: 600;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .audit-dl dd {
    color: var(--color-text-secondary);
    margin: 0;
    word-break: break-word;
  }

  .audit-full-detail { white-space: pre-wrap; }

  .audit-entity-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px 6px;
    font-size: inherit;
    font-family: var(--font-mono);
    color: var(--color-primary);
    cursor: pointer;
  }

  .audit-entity-btn:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .audit-meta-pre {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface);
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    margin: 0;
    overflow-x: auto;
    max-height: 200px;
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  /* ── Analytics ─────────────────────────────────────────────────────── */
  .analytics-bars {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .analytics-bar-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .analytics-bar-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    width: 140px;
    flex-shrink: 0;
    text-transform: capitalize;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .analytics-bar-track {
    flex: 1;
    height: 8px;
    background: var(--color-border);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .analytics-bar-fill {
    height: 100%;
    background: var(--color-primary);
    border-radius: var(--radius-full);
    transition: width 0.3s ease;
  }

  .analytics-bar-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    width: 50px;
    text-align: right;
  }

  /* ── Activity list ─────────────────────────────────────────────────── */
  .activity-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .activity-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-left: 2px solid var(--color-border);
    font-size: var(--text-sm);
  }

  .activity-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .activity-actor {
    color: var(--color-text-secondary);
    flex-shrink: 0;
  }

  .activity-detail {
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Responsive ──────────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .settings-header { padding: var(--space-4); }
    .tab-bar { padding: 0 var(--space-3); }
    .tab-panel { padding: var(--space-4); }
    .budget-grid { grid-template-columns: repeat(2, 1fr); }
    .td-detail { max-width: 150px; }
  }

  /* ── LLM Defaults ─────────────────────────────────────────────────── */
  .llm-feature-cell { display: flex; flex-direction: column; }
  .llm-feature-name { font-weight: 600; text-transform: capitalize; }
  .muted-text { color: var(--color-text-muted); font-size: var(--text-xs); font-style: italic; }
  .prompt-preview-text { font-size: var(--text-xs); color: var(--color-text-secondary); font-family: var(--font-mono); }
  .edit-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
  }
  .edit-btn:hover { background: var(--color-surface-elevated); }
  .edit-btn-primary { background: var(--color-primary); color: white; border-color: var(--color-primary); }
  .edit-btn-primary:hover { opacity: 0.9; }
  .llm-edit-panel {
    margin-top: var(--space-4);
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
  }
  .llm-edit-heading { font-size: var(--text-sm); font-weight: 600; margin: 0 0 var(--space-3); text-transform: capitalize; }
  .llm-field { margin-bottom: var(--space-3); }
  .llm-field-label { display: block; font-size: var(--text-xs); font-weight: 600; color: var(--color-text-secondary); margin-bottom: var(--space-1); }
  .llm-field-input {
    width: 100%; max-width: 400px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    background: var(--color-surface);
    color: var(--color-text);
    font: inherit; font-size: var(--text-sm);
  }
  .llm-field-input:focus { outline: 2px solid var(--color-focus); outline-offset: 1px; }
  .llm-field-textarea {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-mono); font-size: var(--text-xs);
    resize: vertical;
  }
  .llm-field-textarea:focus { outline: 2px solid var(--color-focus); outline-offset: 1px; }
  .llm-edit-actions { display: flex; gap: var(--space-2); justify-content: flex-end; margin-top: var(--space-3); }

  /* ── Live audit stream ─────────────────────────────────────────────── */
  .live-stream-section {
    margin-bottom: var(--space-4);
    border: 1px solid color-mix(in srgb, var(--color-success) 25%, transparent);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-success) 5%, transparent);
    overflow: hidden;
  }

  .live-stream-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid color-mix(in srgb, var(--color-success) 15%, transparent);
  }

  .live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-success);
    animation: live-pulse 1.5s ease-in-out infinite;
    flex-shrink: 0;
  }

  @keyframes live-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .live-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-success);
  }

  .live-stream-list {
    padding: var(--space-1) 0;
    max-height: 200px;
    overflow-y: auto;
  }

  .live-event {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
  }

  .live-event-time { color: var(--color-text-muted); font-family: var(--font-mono); white-space: nowrap; flex-shrink: 0; }
  .live-event-type { font-weight: 600; color: var(--color-text); }
  .live-event-actor { color: var(--color-text-secondary); }

  /* ── BCP tab ─────────────────────────────────────────────────────────── */
  .bcp-targets {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-4);
    font-size: var(--text-sm);
    padding: var(--space-2) 0;
  }

  .bcp-targets dt { font-weight: 600; color: var(--color-text-muted); }
  .bcp-targets dd { margin: 0; color: var(--color-text); }

  .bcp-drill-section {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
  }

  .bcp-drill-result {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-success) 25%, transparent);
  }

  .bcp-drill-error {
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 25%, transparent);
  }

  .bcp-result-icon { font-size: var(--text-base); }
  .bcp-result-ok { color: var(--color-success); }
  .bcp-drill-error .bcp-result-icon { color: var(--color-danger); }
  .bcp-result-detail { font-size: var(--text-xs); color: var(--color-text-muted); }

  .bcp-snapshot-actions {
    margin-bottom: var(--space-2);
  }

  .btn {
    padding: var(--space-2) var(--space-4);
    border: none;
    border-radius: var(--radius);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-primary { background: var(--color-primary); color: white; }
  .btn-primary:hover:not(:disabled) { opacity: 0.85; }
  .btn-secondary { background: var(--color-surface-elevated); color: var(--color-text); border: 1px solid var(--color-border); }
  .btn-secondary:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .btn-danger-sm {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-danger);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .btn-danger-sm:hover { background: color-mix(in srgb, var(--color-danger) 10%, transparent); }

  @media (prefers-reduced-motion: reduce) {
    .back-btn, .tab-btn, .refresh-btn, .run-btn { transition: none; }
    .live-dot { animation: none; }
  }
</style>
