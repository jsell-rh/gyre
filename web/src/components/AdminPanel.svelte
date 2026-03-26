<script>
  import { getContext, tick } from 'svelte';
  import { api } from '../lib/api.js';
  import Tabs from '../lib/Tabs.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError, toastInfo } from '../lib/toast.svelte.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Shell context (available when rendered inside AppShell)
  const navigate = getContext('navigate') ?? (() => {});
  const getScope = getContext('getScope') ?? (() => ({}));

  // Derive effective scope from props — repo > workspace > tenant
  const effectiveScope = $derived(
    repoId ? 'repo' : workspaceId ? 'workspace' : 'tenant'
  );

  // ---- TENANT STATE ----
  let tenantCompute = $state([]);
  let tenantBudget = $state(null);
  let tenantAudit = $state([]);
  let tenantWorkspaces = $state([]);
  let tenantTab = $state('workspaces');
  const TENANT_TABS = [
    { id: 'workspaces', label: 'Workspaces' },
    { id: 'compute',    label: 'Compute' },
    { id: 'budget',     label: 'Budget' },
    { id: 'audit',      label: 'Audit' },
  ];

  // ---- WORKSPACE STATE ----
  let workspace = $state(null);
  let wsBudget = $state(null);
  let wsMembers = $state([]);
  let wsPolicies = $state([]);
  let wsTrustLevel = $state('Autonomous');
  let wsTab = $state('settings');
  const WS_TABS = [
    { id: 'settings', label: 'Settings' },
    { id: 'budget',   label: 'Budget' },
    { id: 'trust',    label: 'Trust Level' },
    { id: 'teams',    label: 'Teams' },
    { id: 'policies', label: 'Policies' },
  ];

  // Workspace settings form
  let wsSettingsForm = $state({ name: '', description: '' });
  let wsSettingsSaving = $state(false);
  let wsDeleteConfirm = $state(false);

  // ---- REPO STATE ----
  let repoGates = $state([]);
  let repoPolicies = $state([]);
  let repoTab = $state('settings');
  const REPO_TABS = [
    { id: 'settings', label: 'Settings' },
    { id: 'gates',    label: 'Gates' },
    { id: 'policies', label: 'Policies' },
  ];

  // ---- SHARED ----
  let loading = $state(true);
  let error = $state(null);

  // ---- TRUST LEVEL ----
  const TRUST_LEVELS = [
    { id: 'Supervised', label: 'Supervised', desc: 'I review everything before it merges' },
    { id: 'Guided',     label: 'Guided',     desc: 'Agents merge if gates pass, alert me on failures' },
    { id: 'Autonomous', label: 'Autonomous', desc: 'Only interrupt me for exceptions' },
    { id: 'Custom',     label: 'Custom',     desc: 'Configure policies manually' },
  ];
  let pendingTrustLevel = $state(null);
  let trustConfirmModal = $state(null);
  let trustChanging = $state(false);

  // ---- BUDGET MODAL ----
  let budgetModal = $state(false);
  let budgetLimit = $state('');
  let budgetSaving = $state(false);

  // ---- MEMBERS MODAL ----
  let newMemberModal = $state(false);
  let memberForm = $state({ email: '' });
  let memberFormLoading = $state(false);

  // ---- POLICY STATE ----
  const ACTIONS = ['merge', 'approve', 'read', 'write', 'delete', 'push', 'spawn'];
  const RESOURCE_TYPES = ['mr', 'spec', 'repo', 'agent', 'workspace', 'task'];

  let policyModal = $state(null);
  let policyForm = $state({ name: '', effect: 'Allow', actions: [], resource_types: [] });
  let policyFormLoading = $state(false);
  let deleteConfirmModal = $state(null);

  let simulateForm = $state({ action: 'merge', resource_type: 'mr' });
  let simulateResult = $state(null);
  let simulateLoading = $state(false);

  const policyGroups = $derived({
    builtin: wsPolicies.filter(p => p.name?.startsWith('builtin:')),
    trust:   wsPolicies.filter(p => p.name?.startsWith('trust:')),
    custom:  wsPolicies.filter(p => !p.name?.startsWith('builtin:') && !p.name?.startsWith('trust:')),
  });

  // ---- NEW WORKSPACE MODAL ----
  let newWorkspaceModal = $state(false);
  let wsForm = $state({ name: '', description: '' });
  let wsFormLoading = $state(false);

  // ---- GATE MODAL ----
  let gateModal = $state(false);
  let gateForm = $state({ name: '', command: '', timeout: 300 });
  let gateSaving = $state(false);
  let gateDeleting = $state({});

  // ---- MODAL REFS (auto-focus) ----
  let budgetModalEl = $state(null);
  let trustModalEl = $state(null);
  let memberModalEl = $state(null);
  let deleteConfirmModalEl = $state(null);
  let newWorkspaceModalEl = $state(null);

  $effect(() => {
    if (budgetModal) {
      tick().then(() => budgetModalEl?.focus());
    }
  });

  $effect(() => {
    if (trustConfirmModal) {
      tick().then(() => trustModalEl?.focus());
    }
  });

  $effect(() => {
    if (newMemberModal) {
      tick().then(() => memberModalEl?.focus());
    }
  });

  $effect(() => {
    if (deleteConfirmModal) {
      tick().then(() => deleteConfirmModalEl?.focus());
    }
  });

  $effect(() => {
    if (newWorkspaceModal) {
      tick().then(() => newWorkspaceModalEl?.focus());
    }
  });

  // ---- COMPUTE MODAL (tenant) ----
  let computeModal = $state(false);
  let computeForm = $state({ name: '', target_type: 'local', host: '' });
  let computeLoading = $state(false);

  // ---- LOAD ON SCOPE CHANGE ----
  $effect(() => {
    if (effectiveScope === 'tenant') loadTenant();
    else if (effectiveScope === 'workspace') loadWorkspace();
    else if (effectiveScope === 'repo') loadRepo();
  });

  async function loadTenant() {
    loading = true; error = null;
    try {
      const [compute, budget, audit, wsList] = await Promise.all([
        api.computeList().catch(() => []),
        api.budgetSummary().catch(() => null),
        api.auditEvents({ limit: 50 }).catch(() => ({ events: [] })),
        api.workspaces().catch(() => []),
      ]);
      tenantCompute   = Array.isArray(compute) ? compute : (compute?.targets ?? []);
      tenantBudget    = budget;
      tenantAudit     = audit?.events ?? [];
      tenantWorkspaces = Array.isArray(wsList) ? wsList : [];
    } catch (e) { error = e.message; }
    finally { loading = false; }
  }

  async function loadWorkspace() {
    if (!workspaceId) return;
    loading = true; error = null;
    try {
      const [ws, budget, members, policies] = await Promise.all([
        api.workspace(workspaceId),
        api.workspaceBudget(workspaceId).catch(() => null),
        api.workspaceMembers(workspaceId).catch(() => []),
        api.workspaceAbacPolicies(workspaceId).catch(() => []),
      ]);
      workspace   = ws;
      wsBudget    = budget;
      wsMembers   = Array.isArray(members) ? members : (members?.members ?? []);
      wsPolicies  = Array.isArray(policies) ? policies : (policies?.policies ?? []);
      wsTrustLevel = ws?.trust_level ?? 'Autonomous';
      wsSettingsForm = { name: ws?.name ?? '', description: ws?.description ?? '' };
    } catch (e) { error = e.message; }
    finally { loading = false; }
  }

  async function loadRepo() {
    if (!repoId) return;
    loading = true; error = null;
    try {
      const [gates, policies] = await Promise.all([
        api.repoGates(repoId).catch(() => []),
        api.repoAbacPolicy(repoId).catch(() => []),
      ]);
      repoGates    = Array.isArray(gates) ? gates : (gates?.gates ?? []);
      repoPolicies = Array.isArray(policies) ? policies : [];
    } catch (e) { error = e.message; }
    finally { loading = false; }
  }

  // ---- TRUST LEVEL ----
  function selectTrustLevel(level) {
    if (level === wsTrustLevel) return;
    pendingTrustLevel = level;
    trustConfirmModal = { from: wsTrustLevel, to: level };
  }

  function cancelTrustChange() {
    trustConfirmModal = null;
    pendingTrustLevel = null;
  }

  async function confirmTrustChange() {
    trustChanging = true;
    try {
      await api.updateWorkspace(workspaceId, { trust_level: pendingTrustLevel });
      wsTrustLevel = pendingTrustLevel;
      workspace = { ...workspace, trust_level: pendingTrustLevel };
      toastSuccess(`Trust level updated to ${pendingTrustLevel}`);
      trustConfirmModal = null;
      pendingTrustLevel = null;
      // Reload policies — trust transition rewrites trust: policies
      wsPolicies = await api.workspaceAbacPolicies(workspaceId)
        .then(r => Array.isArray(r) ? r : (r?.policies ?? []))
        .catch(() => wsPolicies);
    } catch (e) {
      if (e.message?.includes('409')) {
        toastError('Trust level transition failed — policies could not be created');
      } else {
        toastError(e.message);
      }
    } finally {
      trustChanging = false;
    }
  }

  function trustChangeDescription(to) {
    if (to === 'Guided')     return 'Switching to Guided removes the human MR review requirement. Agents will merge automatically when all gates pass. Continue?';
    if (to === 'Autonomous') return 'Switching to Autonomous means agents will merge without interrupting you for each MR. You will only be notified on exceptions. Continue?';
    if (to === 'Supervised') return 'Switching to Supervised requires human approval for every MR before it merges. Continue?';
    if (to === 'Custom')     return 'Switching to Custom preserves current trust policies as a starting point so you can edit them manually. Continue?';
    return `Switch trust level to ${to}?`;
  }

  // ---- WORKSPACE SETTINGS ----
  async function saveWsSettings() {
    wsSettingsSaving = true;
    try {
      workspace = await api.updateWorkspace(workspaceId, {
        name: wsSettingsForm.name,
        description: wsSettingsForm.description,
      });
      toastSuccess('Workspace settings saved.');
    } catch (e) { toastError(e.message); }
    finally { wsSettingsSaving = false; }
  }

  // ---- BUDGET ----
  function openBudgetModal() {
    budgetLimit = String(wsBudget?.limit ?? '');
    budgetModal = true;
  }

  async function saveBudget() {
    budgetSaving = true;
    try {
      await api.setWorkspaceBudget(workspaceId, { limit: Number(budgetLimit), currency: wsBudget?.currency ?? 'USD' });
      wsBudget = await api.workspaceBudget(workspaceId);
      toastSuccess('Budget limit updated.');
      budgetModal = false;
    } catch (e) { toastError(e.message); }
    finally { budgetSaving = false; }
  }

  function budgetPercent(b) {
    if (!b?.limit || b.used == null) return 0;
    return Math.min(100, Math.round((b.used / b.limit) * 100));
  }

  // ---- MEMBERS ----
  function removeMember(userId, userName) {
    deleteConfirmModal = {
      kind: 'member',
      userId,
      label: `Remove ${userName || 'this member'} from the workspace? This cannot be undone.`,
    };
  }

  async function addMember() {
    memberFormLoading = true;
    try {
      await api.addWorkspaceMember(workspaceId, { email: memberForm.email });
      wsMembers = await api.workspaceMembers(workspaceId).then(r => Array.isArray(r) ? r : (r?.members ?? []));
      toastSuccess('Member added.');
      newMemberModal = false;
      memberForm = { email: '' };
    } catch (e) { toastError(e.message); }
    finally { memberFormLoading = false; }
  }

  // ---- POLICIES ----
  function openNewPolicy() {
    policyForm = { name: '', effect: 'Allow', actions: [], resource_types: [] };
    policyModal = { mode: 'create' };
  }

  function openEditPolicy(policy) {
    policyForm = {
      name: policy.name,
      effect: policy.effect ?? 'Allow',
      actions: [...(policy.actions ?? [])],
      resource_types: [...(policy.resource_types ?? [])],
    };
    policyModal = { mode: 'edit', policy };
  }

  async function savePolicy() {
    policyFormLoading = true;
    try {
      if (policyModal.mode === 'create') {
        await api.createWorkspaceAbacPolicy(workspaceId, policyForm);
        toastSuccess('Policy created.');
      } else {
        await api.deleteWorkspaceAbacPolicy(workspaceId, policyModal.policy.id);
        await api.createWorkspaceAbacPolicy(workspaceId, policyForm);
        toastSuccess('Policy updated.');
      }
      wsPolicies = await api.workspaceAbacPolicies(workspaceId)
        .then(r => Array.isArray(r) ? r : (r?.policies ?? []));
      policyModal = null;
    } catch (e) { toastError(e.message); }
    finally { policyFormLoading = false; }
  }

  async function deletePolicy(policyId) {
    try {
      await api.deleteWorkspaceAbacPolicy(workspaceId, policyId);
      wsPolicies = wsPolicies.filter(p => p.id !== policyId);
      toastSuccess('Policy deleted.');
    } catch (e) { toastError(e.message); }
    finally { deleteConfirmModal = null; }
  }

  function toggleChip(arr, val) {
    return arr.includes(val) ? arr.filter(x => x !== val) : [...arr, val];
  }

  async function simulatePolicy() {
    simulateLoading = true;
    simulateResult = null;
    try {
      simulateResult = await api.simulateAbacPolicy(workspaceId, simulateForm);
    } catch (e) {
      simulateResult = { error: e.message };
    } finally { simulateLoading = false; }
  }

  // ---- WORKSPACE CREATION (tenant scope) ----
  async function createWorkspace() {
    wsFormLoading = true;
    try {
      const newWs = await api.createWorkspace(wsForm);
      tenantWorkspaces = [...tenantWorkspaces, newWs];
      toastSuccess(`Workspace "${newWs.name ?? wsForm.name}" created.`);
      newWorkspaceModal = false;
      wsForm = { name: '', description: '' };
      // Navigate to new workspace if shell context available
      if (newWs.id) navigate('workspace-detail', { workspace: newWs });
    } catch (e) { toastError(e.message); }
    finally { wsFormLoading = false; }
  }

  // ---- REPO GATES ----
  async function createGate() {
    gateSaving = true;
    try {
      await api.createRepoGate(repoId, {
        name: gateForm.name,
        command: gateForm.command,
        timeout_secs: Number(gateForm.timeout),
      });
      repoGates = await api.repoGates(repoId).then(r => Array.isArray(r) ? r : (r?.gates ?? []));
      toastSuccess('Gate added.');
      gateModal = false;
    } catch (e) { toastError(e.message); }
    finally { gateSaving = false; }
  }

  function deleteGate(gateId) {
    deleteConfirmModal = { kind: 'gate', gateId, label: 'Remove this gate? This cannot be undone.' };
  }

  async function confirmDeleteGate(gateId) {
    gateDeleting = { ...gateDeleting, [gateId]: true };
    try {
      await api.deleteRepoGate(repoId, gateId);
      repoGates = repoGates.filter(g => g.id !== gateId);
      toastSuccess('Gate removed.');
    } catch (e) { toastError(e.message); }
    finally { gateDeleting = { ...gateDeleting, [gateId]: false }; }
  }

  // ---- COMPUTE (tenant) ----
  async function saveCompute() {
    computeLoading = true;
    try {
      const body = { name: computeForm.name, target_type: computeForm.target_type };
      if (computeForm.host) body.config = { host: computeForm.host };
      await api.computeCreate(body);
      tenantCompute = await api.computeList().then(r => Array.isArray(r) ? r : (r?.targets ?? []));
      toastSuccess('Compute target created.');
      computeModal = false;
    } catch (e) { toastError(e.message); }
    finally { computeLoading = false; }
  }

  function deleteCompute(id) {
    deleteConfirmModal = { kind: 'compute', computeId: id, label: 'Delete this compute target? This cannot be undone.' };
  }

  async function confirmDelete() {
    const modal = deleteConfirmModal;
    if (!modal) return;

    if (modal.kind === 'policy') {
      await deletePolicy(modal.policyId);
      return;
    }

    deleteConfirmModal = null;

    if (modal.kind === 'member') {
      try {
        await api.removeWorkspaceMember(workspaceId, modal.userId);
        wsMembers = wsMembers.filter(m => (m.id ?? m.user_id) !== modal.userId);
        toastSuccess('Member removed.');
      } catch (e) { toastError(e.message); }
    } else if (modal.kind === 'gate') {
      await confirmDeleteGate(modal.gateId);
    } else if (modal.kind === 'compute') {
      try {
        await api.computeDelete(modal.computeId);
        tenantCompute = tenantCompute.filter(t => t.id !== modal.computeId);
        toastSuccess('Compute target deleted.');
      } catch (e) { toastError(e.message); }
    }
  }

  // ---- HELPERS ----
  function relativeTime(ts) {
    if (!ts) return '—';
    const diff = Math.floor((Date.now() - ts * 1000) / 1000);
    if (diff < 60)    return `${diff}s ago`;
    if (diff < 3600)  return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }
</script>

<div class="panel">
  <div class="panel-header">
    <h2>
      {#if effectiveScope === 'repo'}Repo Admin
      {:else if effectiveScope === 'workspace'}Workspace Admin
      {:else}Admin{/if}
    </h2>
    <button class="refresh-btn" onclick={() => {
      if (effectiveScope === 'tenant') loadTenant();
      else if (effectiveScope === 'workspace') loadWorkspace();
      else loadRepo();
    }} disabled={loading}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M23 4v6h-6M1 20v-6h6"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
      </svg>
      {loading ? 'Loading…' : 'Refresh'}
    </button>
  </div>

  {#if effectiveScope === 'tenant'}
    <Tabs tabs={TENANT_TABS} bind:active={tenantTab} />
  {:else if effectiveScope === 'workspace'}
    <Tabs tabs={WS_TABS} bind:active={wsTab} />
  {:else}
    <Tabs tabs={REPO_TABS} bind:active={repoTab} />
  {/if}

  <div class="admin-content" role="tabpanel" id="tabpanel-{effectiveScope === 'tenant' ? tenantTab : effectiveScope === 'workspace' ? wsTab : repoTab}" aria-labelledby="tab-{effectiveScope === 'tenant' ? tenantTab : effectiveScope === 'workspace' ? wsTab : repoTab}">
    {#if error}
      <div class="error-banner" role="alert">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
          <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
        </svg>
        {error}
      </div>
    {/if}

    <!-- ==================== TENANT SCOPE ==================== -->
    {#if effectiveScope === 'tenant'}

      {#if tenantTab === 'workspaces'}
        <div class="section-actions">
          <p class="section-desc">All workspaces in this tenant. Admins see all.</p>
          <button class="primary-btn" onclick={() => { wsForm = { name: '', description: '' }; newWorkspaceModal = true; }}>
            + New Workspace
          </button>
        </div>
        {#if loading}
          <Skeleton height="200px" />
        {:else if tenantWorkspaces.length === 0}
          <EmptyState title="No workspaces" description="Create a workspace to get started." />
        {:else}
          <table class="data-table">
            <thead>
              <tr><th>Name</th><th>Trust Level</th><th>Description</th></tr>
            </thead>
            <tbody>
              {#each tenantWorkspaces as ws}
                <tr>
                  <td class="agent-name">{ws.name}</td>
                  <td>
                    <span class="trust-badge trust-{(ws.trust_level ?? 'autonomous').toLowerCase()}">
                      {ws.trust_level ?? 'Autonomous'}
                    </span>
                  </td>
                  <td class="dim">{ws.description ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

      {:else if tenantTab === 'compute'}
        <div class="section-actions">
          <p class="section-desc">Register remote compute targets for agent workload dispatch.</p>
          <button class="primary-btn" onclick={() => { computeForm = { name: '', target_type: 'local', host: '' }; computeModal = true; }}>
            + Add Target
          </button>
        </div>
        {#if loading}
          <Skeleton height="150px" />
        {:else if tenantCompute.length === 0}
          <EmptyState title="No compute targets" description="Register local, Docker, or SSH compute targets." />
        {:else}
          <table class="data-table">
            <thead><tr><th>Name</th><th>Type</th><th>Host</th><th>Status</th><th>Actions</th></tr></thead>
            <tbody>
              {#each tenantCompute as ct}
                <tr>
                  <td class="agent-name">{ct.name ?? ct.id}</td>
                  <td><Badge value={ct.target_type ?? ct.type ?? 'local'} /></td>
                  <td class="mono dim">{ct.host ?? ct.config?.host ?? '—'}</td>
                  <td><Badge value={ct.status ?? 'active'} /></td>
                  <td>
                    <button class="kill-btn" onclick={() => deleteCompute(ct.id)}>Delete</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

      {:else if tenantTab === 'budget'}
        {#if loading}
          <Skeleton height="150px" />
        {:else if !tenantBudget}
          <EmptyState title="No budget data" description="Budget summary requires Admin role." />
        {:else}
          <div class="metric-grid">
            <div class="metric-card">
              <span class="metric-label">Total Used</span>
              <span class="metric-value">{tenantBudget.used ?? '—'} {tenantBudget.currency ?? ''}</span>
            </div>
            <div class="metric-card">
              <span class="metric-label">Limit</span>
              <span class="metric-value">{tenantBudget.limit ?? '∞'} {tenantBudget.currency ?? ''}</span>
            </div>
          </div>
        {/if}

      {:else if tenantTab === 'audit'}
        {#if loading}
          <Skeleton height="200px" />
        {:else if tenantAudit.length === 0}
          <EmptyState title="No audit events" description="Audit events will appear here." />
        {:else}
          <div class="table-scroll">
            <table class="data-table">
              <thead><tr><th>Time</th><th>Actor</th><th>Event</th><th>Description</th></tr></thead>
              <tbody>
                {#each tenantAudit as evt}
                  <tr>
                    <td class="dim">{relativeTime(evt.timestamp)}</td>
                    <td class="mono dim">{evt.actor_id ?? evt.agent_id ?? '—'}</td>
                    <td><Badge value={evt.event_type ?? 'info'} /></td>
                    <td class="dim">{evt.description ?? evt.message ?? '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      {/if}

    <!-- ==================== WORKSPACE SCOPE ==================== -->
    {:else if effectiveScope === 'workspace'}

      {#if wsTab === 'settings'}
        {#if loading}
          <Skeleton height="200px" />
        {:else if !workspace}
          <EmptyState title="Workspace not found" description="Could not load workspace settings." />
        {:else}
          <div class="form-section">
            <h3 class="section-title">General</h3>
            <div class="form-field">
              <label class="form-label" for="ws-name">Name</label>
              <input id="ws-name" class="filter-input full-width" bind:value={wsSettingsForm.name} />
            </div>
            <div class="form-field">
              <label class="form-label" for="ws-desc">Description</label>
              <textarea id="ws-desc" class="filter-input full-width textarea" bind:value={wsSettingsForm.description} rows="3"></textarea>
            </div>
            <div class="form-actions">
              <button class="primary-btn" onclick={saveWsSettings} disabled={wsSettingsSaving}>
                {wsSettingsSaving ? 'Saving…' : 'Save Settings'}
              </button>
            </div>
          </div>

          <div class="danger-zone">
            <h3 class="danger-title">Danger Zone</h3>
            {#if wsDeleteConfirm}
              <p class="danger-desc">Are you sure? This permanently deletes the workspace and all its data.</p>
              <div class="form-actions">
                <button class="secondary-btn" onclick={() => wsDeleteConfirm = false}>Cancel</button>
                <button class="kill-btn" onclick={() => { toastInfo('Workspace deletion not yet implemented.'); wsDeleteConfirm = false; }}>
                  Confirm Delete
                </button>
              </div>
            {:else}
              <p class="danger-desc">Permanently delete this workspace and all associated data.</p>
              <button class="kill-btn" onclick={() => wsDeleteConfirm = true}>Delete Workspace</button>
            {/if}
          </div>
        {/if}

      {:else if wsTab === 'budget'}
        {#if loading}
          <Skeleton height="150px" />
        {:else if !wsBudget}
          <EmptyState title="No budget configured" description="Set a budget limit to track spending." />
        {:else}
          {@const pct = budgetPercent(wsBudget)}
          <div class="budget-card">
            <div class="budget-header">
              <span class="budget-label">Token Usage</span>
              <span class="budget-amount">{wsBudget.used ?? 0} / {wsBudget.limit ?? '∞'} {wsBudget.currency ?? ''}</span>
            </div>
            <div
              class="budget-bar-track"
              role="progressbar"
              aria-valuenow={Math.round(pct)}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-label="Budget usage"
            >
              <div
                class="budget-bar-fill {pct >= 90 ? 'danger' : pct >= 70 ? 'warning' : ''}"
                style="width: {pct}%"
              ></div>
            </div>
            <p class="budget-pct">{pct}% used</p>
          </div>
          <div class="section-actions" style="margin-top: var(--space-2);">
            <button class="primary-btn" onclick={openBudgetModal}>Adjust Limit</button>
          </div>
        {/if}

      {:else if wsTab === 'trust'}
        {#if loading}
          <Skeleton height="280px" />
        {:else}
          <div class="trust-section">
            <h3 class="section-title">Workspace Trust Level</h3>
            <p class="section-desc">
              Controls how much autonomy agents have in this workspace.
              One click — no ABAC knowledge required.
            </p>

            <div class="trust-options" role="radiogroup" aria-label="Trust level">
              {#each TRUST_LEVELS as level}
                {@const isSelected = wsTrustLevel === level.id}
                <button
                  class="trust-option {isSelected ? 'selected' : ''}"
                  onclick={() => selectTrustLevel(level.id)}
                  role="radio"
                  aria-checked={isSelected}
                >
                  <span class="trust-radio">
                    <span class="trust-radio-dot {isSelected ? 'active' : ''}"></span>
                  </span>
                  <span class="trust-option-body">
                    <span class="trust-option-label">{level.label}</span>
                    <span class="trust-option-desc">{level.desc}</span>
                  </span>
                </button>
              {/each}
            </div>

            <div class="trust-current">
              Current: <strong>{wsTrustLevel}</strong>{#if wsTrustLevel === 'Supervised'}
                — every MR requires your approval before merging.
              {:else if wsTrustLevel === 'Guided'}
                — agents merge when all gates pass; you're alerted on failures.
              {:else if wsTrustLevel === 'Autonomous'}
                — only exceptions surface to you.
              {:else if wsTrustLevel === 'Custom'}
                — policies configured manually in the Policies tab.
              {/if}
            </div>
          </div>
        {/if}

      {:else if wsTab === 'teams'}
        <div class="section-actions">
          <p class="section-desc">Members with access to this workspace.</p>
          <button class="primary-btn" onclick={() => { memberForm = { email: '' }; newMemberModal = true; }}>
            + Add Member
          </button>
        </div>
        {#if loading}
          <Skeleton height="200px" />
        {:else if wsMembers.length === 0}
          <EmptyState title="No members" description="Add members to grant access to this workspace." />
        {:else}
          <table class="data-table">
            <thead><tr><th>User</th><th>Role</th><th>Last Active</th><th>Actions</th></tr></thead>
            <tbody>
              {#each wsMembers as member}
                <tr>
                  <td>
                    <div class="member-row">
                      <div class="member-avatar">{(member.name ?? member.email ?? 'U')[0].toUpperCase()}</div>
                      <div>
                        <div class="agent-name">{member.name ?? member.email ?? member.user_id ?? '—'}</div>
                        {#if member.email && member.name}<div class="dim">{member.email}</div>{/if}
                      </div>
                    </div>
                  </td>
                  <td><Badge value={member.role ?? 'member'} /></td>
                  <td class="dim">{relativeTime(member.last_active ?? member.last_seen_at)}</td>
                  <td>
                    <button class="kill-btn" onclick={() => removeMember(member.id ?? member.user_id, member.name ?? member.email)}>
                      Remove
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

      {:else if wsTab === 'policies'}
        <div class="section-actions">
          <p class="section-desc">
            ABAC policies for this workspace.
            {wsTrustLevel === 'Custom' ? 'Custom trust — full policy editor enabled.' : 'Switch to Custom trust to edit policies.'}
          </p>
          {#if wsTrustLevel === 'Custom'}
            <button class="primary-btn" onclick={openNewPolicy}>+ New Policy</button>
          {/if}
        </div>

        {#if loading}
          <Skeleton height="200px" />
        {:else}
          {#if policyGroups.builtin.length > 0}
            <div class="policy-group">
              <div class="policy-group-header">
                <span class="policy-prefix-badge builtin">builtin:</span>
                <span class="policy-group-label">System-managed — immutable</span>
              </div>
              {#each policyGroups.builtin as policy}
                <div class="policy-row readonly">
                  <span class="policy-name mono">{policy.name}</span>
                  <span class="policy-effect {(policy.effect ?? '').toLowerCase()}">{policy.effect ?? '—'}</span>
                  <span class="policy-detail dim">{(policy.actions ?? []).join(', ')} on {(policy.resource_types ?? []).join(', ')}</span>
                </div>
              {/each}
            </div>
          {/if}

          {#if policyGroups.trust.length > 0}
            <div class="policy-group">
              <div class="policy-group-header">
                <span class="policy-prefix-badge trust">trust:</span>
                <span class="policy-group-label">
                  Trust-preset-managed — {wsTrustLevel === 'Custom' ? 'editable in Custom mode' : 'read-only'}
                </span>
              </div>
              {#each policyGroups.trust as policy}
                <div class="policy-row {wsTrustLevel !== 'Custom' ? 'readonly' : ''}">
                  <span class="policy-name mono">{policy.name}</span>
                  <span class="policy-effect {(policy.effect ?? '').toLowerCase()}">{policy.effect ?? '—'}</span>
                  <span class="policy-detail dim">{(policy.actions ?? []).join(', ')} on {(policy.resource_types ?? []).join(', ')}</span>
                  {#if wsTrustLevel === 'Custom'}
                    <button class="secondary-btn small" onclick={() => openEditPolicy(policy)}>Edit</button>
                    <button class="kill-btn small" onclick={() => deleteConfirmModal = { kind: 'policy', policyId: policy.id, label: 'This policy will be permanently removed. This cannot be undone.' }}>Delete</button>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

          <div class="policy-group">
            <div class="policy-group-header">
              <span class="policy-prefix-badge custom">user</span>
              <span class="policy-group-label">User-created policies</span>
            </div>
            {#if policyGroups.custom.length === 0}
              <p class="dim policy-empty">
                {wsTrustLevel === 'Custom' ? 'No user policies yet. Use "+ New Policy" to add one.' : 'No user-created policies.'}
              </p>
            {:else}
              {#each policyGroups.custom as policy}
                <div class="policy-row">
                  <span class="policy-name mono">{policy.name}</span>
                  <span class="policy-effect {(policy.effect ?? '').toLowerCase()}">{policy.effect ?? '—'}</span>
                  <span class="policy-detail dim">{(policy.actions ?? []).join(', ')} on {(policy.resource_types ?? []).join(', ')}</span>
                  {#if wsTrustLevel === 'Custom'}
                    <button class="secondary-btn small" onclick={() => openEditPolicy(policy)}>Edit</button>
                    <button class="kill-btn small" onclick={() => deleteConfirmModal = { kind: 'policy', policyId: policy.id, label: 'This policy will be permanently removed. This cannot be undone.' }}>Delete</button>
                  {/if}
                </div>
              {/each}
            {/if}
          </div>

          {#if wsTrustLevel !== 'Custom'}
            <div class="policy-locked-note">
              Switch to <strong>Custom</strong> trust level (Trust Level tab) to create and edit policies.
            </div>
          {:else}
            <div class="simulator-section">
              <h4 class="simulator-title">Dry-run Simulator</h4>
              <div class="simulator-row">
                <div class="form-field">
                  <label class="form-label" for="sim-action">Action</label>
                  <select id="sim-action" class="target-select narrow" bind:value={simulateForm.action}>
                    {#each ACTIONS as a}<option value={a}>{a}</option>{/each}
                  </select>
                </div>
                <div class="form-field">
                  <label class="form-label" for="sim-resource">Resource type</label>
                  <select id="sim-resource" class="target-select narrow" bind:value={simulateForm.resource_type}>
                    {#each RESOURCE_TYPES as r}<option value={r}>{r}</option>{/each}
                  </select>
                </div>
                <button class="primary-btn" onclick={simulatePolicy} disabled={simulateLoading}>
                  {simulateLoading ? 'Simulating…' : 'Simulate'}
                </button>
              </div>
              {#if simulateResult}
                <div class="simulate-result {simulateResult.error ? 'error' : (simulateResult.outcome ?? '').toLowerCase() === 'deny' ? 'deny' : 'allow'}">
                  {#if simulateResult.error}
                    Error: {simulateResult.error}
                  {:else}
                    Outcome: <strong>{simulateResult.outcome ?? 'Unknown'}</strong>
                    {#if simulateResult.matched_policies?.length}
                      — matched: {simulateResult.matched_policies.join(', ')}
                    {/if}
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      {/if}

    <!-- ==================== REPO SCOPE ==================== -->
    {:else if effectiveScope === 'repo'}

      {#if repoTab === 'settings'}
        {#if loading}
          <Skeleton height="150px" />
        {:else}
          <EmptyState title="Repo settings" description="Edit repo name and description — coming soon." />
        {/if}

      {:else if repoTab === 'gates'}
        <div class="section-actions">
          <p class="section-desc">Gates run before a MR can merge. All enabled gates must pass.</p>
          <button class="primary-btn" onclick={() => { gateForm = { name: '', command: '', timeout: 300 }; gateModal = true; }}>
            + Add Gate
          </button>
        </div>
        {#if loading}
          <Skeleton height="200px" />
        {:else if repoGates.length === 0}
          <EmptyState title="No gates configured" description="Add gates to require checks before merging." />
        {:else}
          <table class="data-table">
            <thead><tr><th>Name</th><th>Command</th><th>Timeout</th><th>Status</th><th>Actions</th></tr></thead>
            <tbody>
              {#each repoGates as gate}
                <tr>
                  <td class="agent-name">{gate.name}</td>
                  <td class="mono dim">{gate.command ?? '—'}</td>
                  <td class="dim">{gate.timeout_secs ?? gate.timeout ?? '—'}s</td>
                  <td><Badge value={gate.enabled === false ? 'idle' : 'active'} /></td>
                  <td>
                    <button class="kill-btn" onclick={() => deleteGate(gate.id)} disabled={gateDeleting[gate.id]}>
                      {gateDeleting[gate.id] ? 'Removing…' : 'Remove'}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}

      {:else if repoTab === 'policies'}
        <div class="section-actions">
          <p class="section-desc">ABAC policies scoped to this repository.</p>
        </div>
        {#if loading}
          <Skeleton height="200px" />
        {:else if repoPolicies.length === 0}
          <EmptyState title="No repo policies" description="No ABAC policies configured for this repository." />
        {:else}
          <div class="policy-group">
            {#each repoPolicies as policy}
              <div class="policy-row readonly">
                <span class="policy-name mono">{policy.name}</span>
                <span class="policy-effect {(policy.effect ?? '').toLowerCase()}">{policy.effect ?? '—'}</span>
                <span class="policy-detail dim">{(policy.actions ?? []).join(', ')} on {(policy.resource_types ?? []).join(', ')}</span>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    {/if}
  </div>
</div>

<!-- TRUST CONFIRM MODAL -->
{#if trustConfirmModal}
  <div class="modal-backdrop" role="presentation" onclick={cancelTrustChange}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="Change Trust Level"
    bind:this={trustModalEl}
    onkeydown={(e) => { if (e.key === 'Escape') cancelTrustChange(); }}>
    <h3 class="modal-title">Change Trust Level</h3>
    <p class="modal-desc">{trustChangeDescription(trustConfirmModal.to)}</p>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={cancelTrustChange}>Cancel</button>
      <button class="primary-btn" onclick={confirmTrustChange} disabled={trustChanging}>
        {trustChanging ? 'Applying…' : `Switch to ${trustConfirmModal.to}`}
      </button>
    </div>
  </div>
{/if}

<!-- BUDGET MODAL -->
{#if budgetModal}
  <div class="modal-backdrop" role="presentation" onclick={() => budgetModal = false}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="Adjust Budget Limit"
    bind:this={budgetModalEl}
    onkeydown={(e) => { if (e.key === 'Escape') budgetModal = false; }}>
    <h3 class="modal-title">Adjust Budget Limit</h3>
    <div class="form-field">
      <label class="form-label" for="budget-limit">Limit ({wsBudget?.currency ?? 'USD'})</label>
      <input id="budget-limit" type="number" class="filter-input full-width" bind:value={budgetLimit} min="0" />
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => budgetModal = false}>Cancel</button>
      <button class="primary-btn" onclick={saveBudget} disabled={budgetSaving || !budgetLimit}>
        {budgetSaving ? 'Saving…' : 'Update Limit'}
      </button>
    </div>
  </div>
{/if}

<!-- ADD MEMBER MODAL -->
{#if newMemberModal}
  <div class="modal-backdrop" role="presentation" onclick={() => newMemberModal = false}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="Add Member"
    bind:this={memberModalEl}
    onkeydown={(e) => { if (e.key === 'Escape') newMemberModal = false; }}>
    <h3 class="modal-title">Add Member</h3>
    <div class="form-field">
      <label class="form-label" for="member-email">Email address</label>
      <input id="member-email" class="filter-input full-width" bind:value={memberForm.email}
        placeholder="user@example.com"
        onkeydown={(e) => e.key === 'Enter' && addMember()} />
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => newMemberModal = false}>Cancel</button>
      <button class="primary-btn" onclick={addMember} disabled={memberFormLoading || !memberForm.email}>
        {memberFormLoading ? 'Adding…' : 'Add Member'}
      </button>
    </div>
  </div>
{/if}

<!-- POLICY EDITOR MODAL -->
{#if policyModal}
  <div class="modal-backdrop" role="presentation" onclick={() => policyModal = null}></div>
  <div class="modal modal-lg" role="dialog" aria-modal="true" tabindex="-1" aria-label="Policy Editor"
    onkeydown={(e) => { if (e.key === 'Escape') policyModal = null; }}>
    <h3 class="modal-title">{policyModal.mode === 'create' ? 'New Policy' : 'Edit Policy'}</h3>
    <div class="form-field">
      <label class="form-label" for="policy-name">Name</label>
      <input id="policy-name" class="filter-input full-width" bind:value={policyForm.name} placeholder="e.g. my-allow-reads" />
    </div>
    <div class="form-field">
      <label class="form-label" for="policy-effect">Effect</label>
      <select id="policy-effect" class="target-select" bind:value={policyForm.effect}>
        <option value="Allow">Allow</option>
        <option value="Deny">Deny</option>
      </select>
    </div>
    <div class="form-field">
      <span class="form-label">Actions</span>
      <div class="chip-group">
        {#each ACTIONS as a}
          <button
            class="chip {policyForm.actions.includes(a) ? 'selected' : ''}"
            onclick={() => policyForm.actions = toggleChip(policyForm.actions, a)}
          >{a}</button>
        {/each}
      </div>
    </div>
    <div class="form-field">
      <span class="form-label">Resource Types</span>
      <div class="chip-group">
        {#each RESOURCE_TYPES as r}
          <button
            class="chip {policyForm.resource_types.includes(r) ? 'selected' : ''}"
            onclick={() => policyForm.resource_types = toggleChip(policyForm.resource_types, r)}
          >{r}</button>
        {/each}
      </div>
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => policyModal = null}>Cancel</button>
      <button class="primary-btn" onclick={savePolicy}
        disabled={policyFormLoading || !policyForm.name || policyForm.actions.length === 0 || policyForm.resource_types.length === 0}>
        {policyFormLoading ? 'Saving…' : 'Save Policy'}
      </button>
    </div>
  </div>
{/if}

<!-- DESTRUCTIVE ACTION CONFIRM -->
{#if deleteConfirmModal}
  <div class="modal-backdrop" role="presentation" onclick={() => deleteConfirmModal = null}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1"
    aria-label={deleteConfirmModal.kind === 'member' ? 'Remove Member' : deleteConfirmModal.kind === 'gate' ? 'Remove Gate' : deleteConfirmModal.kind === 'compute' ? 'Delete Compute Target' : 'Delete Policy'}
    bind:this={deleteConfirmModalEl}
    onkeydown={(e) => { if (e.key === 'Escape') deleteConfirmModal = null; }}>
    <h3 class="modal-title">
      {#if deleteConfirmModal.kind === 'member'}Remove Member
      {:else if deleteConfirmModal.kind === 'gate'}Remove Gate
      {:else if deleteConfirmModal.kind === 'compute'}Delete Compute Target
      {:else}Delete Policy{/if}
    </h3>
    <p class="modal-desc">{deleteConfirmModal.label ?? 'This cannot be undone.'}</p>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => deleteConfirmModal = null}>Cancel</button>
      <button class="kill-btn" onclick={confirmDelete}>
        {#if deleteConfirmModal.kind === 'member'}Remove Member
        {:else if deleteConfirmModal.kind === 'gate'}Remove Gate
        {:else if deleteConfirmModal.kind === 'compute'}Delete
        {:else}Delete Policy{/if}
      </button>
    </div>
  </div>
{/if}

<!-- NEW WORKSPACE MODAL -->
{#if newWorkspaceModal}
  <div class="modal-backdrop" role="presentation" onclick={() => newWorkspaceModal = false}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="New Workspace"
    bind:this={newWorkspaceModalEl}
    onkeydown={(e) => { if (e.key === 'Escape') newWorkspaceModal = false; }}>
    <h3 class="modal-title">New Workspace</h3>
    <div class="form-field">
      <label class="form-label" for="wsf-name">Name</label>
      <input id="wsf-name" class="filter-input full-width" bind:value={wsForm.name} placeholder="e.g. payments-team" />
    </div>
    <div class="form-field">
      <label class="form-label" for="wsf-desc">Description</label>
      <input id="wsf-desc" class="filter-input full-width" bind:value={wsForm.description} placeholder="Optional" />
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => newWorkspaceModal = false}>Cancel</button>
      <button class="primary-btn" onclick={createWorkspace} disabled={wsFormLoading || !wsForm.name}>
        {wsFormLoading ? 'Creating…' : 'Create Workspace'}
      </button>
    </div>
  </div>
{/if}

<!-- GATE MODAL -->
{#if gateModal}
  <div class="modal-backdrop" role="presentation" onclick={() => gateModal = false}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="Add Gate"
    onkeydown={(e) => { if (e.key === 'Escape') gateModal = false; }}>
    <h3 class="modal-title">Add Gate</h3>
    <div class="form-field">
      <label class="form-label" for="gate-name">Name</label>
      <input id="gate-name" class="filter-input full-width" bind:value={gateForm.name} placeholder="e.g. lint" />
    </div>
    <div class="form-field">
      <label class="form-label" for="gate-cmd">Command</label>
      <input id="gate-cmd" class="filter-input full-width" bind:value={gateForm.command} placeholder="e.g. cargo clippy" />
    </div>
    <div class="form-field">
      <label class="form-label" for="gate-timeout">Timeout (seconds)</label>
      <input id="gate-timeout" type="number" class="filter-input" style="width: 100px;" bind:value={gateForm.timeout} min="1" />
    </div>
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => gateModal = false}>Cancel</button>
      <button class="primary-btn" onclick={createGate} disabled={gateSaving || !gateForm.name || !gateForm.command}>
        {gateSaving ? 'Adding…' : 'Add Gate'}
      </button>
    </div>
  </div>
{/if}

<!-- COMPUTE MODAL -->
{#if computeModal}
  <div class="modal-backdrop" role="presentation" onclick={() => computeModal = false}></div>
  <div class="modal" role="dialog" aria-modal="true" tabindex="-1" aria-label="Add Compute Target"
    onkeydown={(e) => { if (e.key === 'Escape') computeModal = false; }}>
    <h3 class="modal-title">Add Compute Target</h3>
    <div class="form-field">
      <label class="form-label" for="ct-name">Name</label>
      <input id="ct-name" class="filter-input full-width" bind:value={computeForm.name} placeholder="e.g. docker-host-1" />
    </div>
    <div class="form-field">
      <label class="form-label" for="ct-type">Type</label>
      <select id="ct-type" class="target-select" bind:value={computeForm.target_type}>
        <option value="local">Local</option>
        <option value="docker">Docker</option>
        <option value="ssh">SSH</option>
      </select>
    </div>
    {#if computeForm.target_type !== 'local'}
      <div class="form-field">
        <label class="form-label" for="ct-host">Host</label>
        <input id="ct-host" class="filter-input full-width" bind:value={computeForm.host} placeholder="host:port" />
      </div>
    {/if}
    <div class="modal-actions">
      <button class="secondary-btn" onclick={() => computeModal = false}>Cancel</button>
      <button class="primary-btn" onclick={saveCompute} disabled={computeLoading || !computeForm.name}>
        {computeLoading ? 'Creating…' : 'Create'}
      </button>
    </div>
  </div>
{/if}

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }
  .refresh-btn:hover:not(:disabled) { border-color: var(--color-border-strong); }
  .refresh-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .admin-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 960px;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  /* Section layout */
  .section-actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .section-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    flex: 1;
    margin: 0;
  }
  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3);
  }

  /* Metrics */
  .metric-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--space-4);
  }
  .metric-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .metric-value {
    font-size: var(--text-xl);
    font-weight: 700;
    font-family: var(--font-display);
    color: var(--color-text);
  }

  /* Data table */
  .data-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .data-table thead th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }
  .data-table tbody tr {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }
  .data-table tbody tr:last-child { border-bottom: none; }
  .data-table tbody tr:hover { background: var(--color-surface-elevated); }
  .data-table td { padding: var(--space-3) var(--space-4); vertical-align: middle; color: var(--color-text); }
  .table-scroll { overflow-x: auto; }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .dim { color: var(--color-text-muted); font-size: var(--text-xs); }
  .agent-name { font-weight: 500; }

  /* Buttons */
  .primary-btn {
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-surface, #fff);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-4);
    font-family: var(--font-body);
    font-weight: 500;
    transition: opacity var(--transition-fast);
    white-space: nowrap;
  }
  .primary-btn:hover { opacity: 0.88; }
  .primary-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .secondary-btn {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), color var(--transition-fast);
    white-space: nowrap;
  }
  .secondary-btn:hover { border-color: var(--color-border-strong); color: var(--color-text); }
  .secondary-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .secondary-btn.small { font-size: var(--text-xs); padding: var(--space-1) var(--space-2); }

  .kill-btn {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-3);
    font-family: var(--font-body);
    transition: background var(--transition-fast);
    white-space: nowrap;
  }
  .kill-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--color-danger) 20%, transparent); }
  .kill-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .kill-btn.small { font-size: var(--text-xs); padding: var(--space-1) var(--space-2); }

  /* Forms */
  .form-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .form-field { display: flex; flex-direction: column; gap: var(--space-1); }
  .form-label { font-size: var(--text-xs); color: var(--color-text-muted); font-weight: 500; }
  .form-actions { display: flex; gap: var(--space-3); margin-top: var(--space-2); }
  .filter-input {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-3);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }
  .filter-input:focus:not(:focus-visible) { outline: none; }
  .filter-input:focus-visible {
    outline: none;
    border-color: var(--color-primary);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 30%, transparent);
  }
  .filter-input.full-width { width: 100%; box-sizing: border-box; }
  .textarea { resize: vertical; min-height: 72px; }

  /* Danger zone */
  .danger-zone {
    background: color-mix(in srgb, var(--color-danger) 5%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 25%, transparent);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .danger-title { font-family: var(--font-display); font-size: var(--text-base); font-weight: 600; color: var(--color-danger); margin: 0; }
  .danger-desc { font-size: var(--text-sm); color: var(--color-text-muted); margin: 0; }

  /* Budget */
  .budget-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .budget-header { display: flex; justify-content: space-between; align-items: baseline; }
  .budget-label { font-size: var(--text-sm); color: var(--color-text-muted); }
  .budget-amount { font-size: var(--text-lg); font-weight: 600; font-family: var(--font-display); color: var(--color-text); }
  .budget-bar-track { height: 8px; background: var(--color-surface-elevated); border-radius: 4px; overflow: hidden; }
  .budget-bar-fill { height: 100%; background: var(--color-success); border-radius: 4px; transition: width var(--transition-normal); }
  .budget-bar-fill.warning { background: var(--color-warning); }
  .budget-bar-fill.danger  { background: var(--color-danger); }
  .budget-pct { font-size: var(--text-xs); color: var(--color-text-muted); margin: 0; }

  /* Trust level */
  .trust-section { display: flex; flex-direction: column; gap: var(--space-5); }
  .trust-options { display: flex; flex-direction: column; gap: var(--space-3); }
  .trust-option {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-5);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .trust-option:hover { border-color: var(--color-border-strong); background: var(--color-surface-elevated); }
  .trust-option.selected { border-color: var(--color-primary); background: color-mix(in srgb, var(--color-primary) 4%, transparent); }
  .trust-radio {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: border-color var(--transition-fast);
  }
  .trust-option.selected .trust-radio { border-color: var(--color-primary); }
  .trust-radio-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: transparent;
    transition: background var(--transition-fast);
  }
  .trust-radio-dot.active { background: var(--color-primary); }
  .trust-option-body { display: flex; flex-direction: column; gap: 2px; }
  .trust-option-label { font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .trust-option-desc { font-size: var(--text-xs); color: var(--color-text-muted); }
  .trust-current {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }
  .trust-current strong { color: var(--color-text); }

  /* Trust badges in workspace list */
  .trust-badge {
    font-size: var(--text-xs);
    font-weight: 500;
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
  }
  .trust-badge.trust-supervised { background: color-mix(in srgb, var(--color-info, #8b5cf6) 15%, transparent); color: var(--color-blocked); }
  .trust-badge.trust-guided     { background: color-mix(in srgb, var(--color-info, #60a5fa) 15%, transparent); color: var(--color-link); }
  .trust-badge.trust-autonomous { background: color-mix(in srgb, var(--color-success) 15%, transparent);  color: var(--color-success); }
  .trust-badge.trust-custom     { background: color-mix(in srgb, var(--color-warning) 15%, transparent); color: var(--color-warning); }

  /* Members */
  .member-row { display: flex; align-items: center; gap: var(--space-3); }
  .member-avatar {
    width: 28px; height: 28px; border-radius: 50%;
    background: var(--color-primary); color: var(--color-surface, #fff);
    display: flex; align-items: center; justify-content: center;
    font-size: var(--text-xs); font-weight: 700; flex-shrink: 0;
  }

  /* Policies */
  .policy-group {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .policy-group-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
  }
  .policy-group-label { font-size: var(--text-xs); color: var(--color-text-muted); }
  .policy-prefix-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
  }
  .policy-prefix-badge.builtin { background: var(--color-bg); color: var(--color-text-muted); border: 1px solid var(--color-border); }
  .policy-prefix-badge.trust   { background: color-mix(in srgb, var(--color-info, #60a5fa) 15%, transparent); color: var(--color-link); }
  .policy-prefix-badge.custom  { background: color-mix(in srgb, var(--color-success) 15%, transparent);  color: var(--color-success); }
  .policy-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-sm);
    transition: background var(--transition-fast);
  }
  .policy-row:last-child { border-bottom: none; }
  .policy-row:not(.readonly):hover { background: var(--color-surface-elevated); }
  .policy-row.readonly { opacity: 0.8; }
  .policy-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .policy-effect { font-size: var(--text-xs); font-weight: 600; min-width: 36px; }
  .policy-effect.allow { color: var(--color-success); }
  .policy-effect.deny  { color: var(--color-danger); }
  .policy-detail { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 240px; }
  .policy-empty { padding: var(--space-3) var(--space-4); margin: 0; }
  .policy-locked-note {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }
  .policy-locked-note strong { color: var(--color-text); }

  /* Simulator */
  .simulator-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .simulator-title { font-size: var(--text-sm); font-weight: 600; color: var(--color-text-secondary); margin: 0; }
  .simulator-row { display: flex; align-items: flex-end; gap: var(--space-3); flex-wrap: wrap; }
  .simulate-result { font-size: var(--text-sm); padding: var(--space-3) var(--space-3); border-radius: var(--radius); }
  .simulate-result.allow { background: color-mix(in srgb, var(--color-success) 15%, transparent); color: var(--color-success); }
  .simulate-result.deny  { background: color-mix(in srgb, var(--color-danger) 10%, transparent);  color: var(--color-danger); }
  .simulate-result.error { background: color-mix(in srgb, var(--color-danger) 10%, transparent);  color: var(--color-danger); }

  /* Chip multi-select */
  .chip-group { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .chip {
    font-size: var(--text-xs);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-mono);
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast);
  }
  .chip:hover { border-color: var(--color-border-strong); color: var(--color-text); }
  .chip.selected { background: color-mix(in srgb, var(--color-primary) 10%, transparent); border-color: var(--color-primary); color: var(--color-primary); }

  /* Modal */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--color-surface, #0a0a0a) 60%, transparent);
    z-index: 100;
  }
  .modal {
    position: fixed;
    z-index: 101;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    min-width: 360px;
    max-width: 480px;
    width: 100%;
    max-height: 90vh;
    overflow-y: auto;
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .modal.modal-lg { max-width: 540px; }
  .modal-title { font-family: var(--font-display); font-size: var(--text-lg); font-weight: 600; color: var(--color-text); margin: 0; }
  .modal-desc { font-size: var(--text-sm); color: var(--color-text-secondary); margin: 0; line-height: 1.6; }
  .modal-actions { display: flex; gap: var(--space-3); justify-content: flex-end; }

  .target-select {
    width: 100%;
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-3);
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }
  .target-select.narrow { width: auto; min-width: 120px; }
</style>
