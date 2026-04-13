<script>
  /**
   * RepoSettings — repo settings tab content (§3 Tab: ⚙ Settings of ui-navigation.md)
   *
   * Tabs: General | Gates | Policies | Budget | Audit | Danger Zone
   * Rendered inside RepoMode when the ⚙ tab is active.
   */
  import { getContext, untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { toastError } from '../lib/toast.svelte.js';
  import { shortId, entityName } from '../lib/entityNames.svelte.js';

  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  function nav(type, id, data) {
    if (goToEntityDetail) goToEntityDetail(type, id, data ?? {});
    else if (openDetailPanel) openDetailPanel({ type, id, data: data ?? {} });
  }

  let {
    workspace = null,
    repo = null,
  } = $props();

  const TAB_IDS = ['general', 'gates', 'policies', 'budget', 'dependencies', 'release', 'audit', 'danger-zone'];
  const TAB_KEYS = {
    'general': 'repo_settings.tabs.general',
    'gates': 'repo_settings.tabs.gates',
    'policies': 'repo_settings.tabs.policies',
    'budget': 'repo_settings.tabs.budget',
    'dependencies': 'Dependencies',
    'release': 'Release',
    'audit': 'repo_settings.tabs.audit',
    'danger-zone': 'repo_settings.tabs.danger_zone',
  };
  const TABS = $derived(TAB_IDS.map(id => ({ id, label: $t(TAB_KEYS[id]) })));

  let activeTab = $state('general');

  // ── General ──────────────────────────────────────────────────────────
  let repoDescription = $state(repo?.description ?? '');
  let repoDefaultBranch = $state(repo?.default_branch ?? 'main');
  let repoMaxConcurrent = $state(repo?.max_concurrent_agents ?? 3);
  let generalSaving = $state(false);
  let generalSaved = $state(false);
  let generalError = $state(null);

  // Sync form values when repo prop changes
  $effect(() => {
    if (repo) {
      repoDescription = repo.description ?? '';
      repoDefaultBranch = repo.default_branch ?? 'main';
      repoMaxConcurrent = repo.max_concurrent_agents ?? 3;
    }
  });

  // ── Gates ─────────────────────────────────────────────────────────────
  let gates = $state([]);
  let gatesLoading = $state(false);
  let gatesError = $state(null);
  let recentGateResults = $state([]);
  let gateResultsLoading = $state(false);
  let addGateOpen = $state(false);
  let newGateName = $state('');
  let newGateType = $state('test_command');
  let newGateCommand = $state('');
  let newGateRequired = $state(true);
  let gateCreating = $state(false);
  let gateCreateError = $state(null);
  let deletingGateId = $state(null);

  // ── Push Gates ────────────────────────────────────────────────────────
  let pushGates = $state([]);
  let pushGatesLoading = $state(false);
  let newPushGate = $state('');
  let pushGatesSaving = $state(false);

  // ── Policies ──────────────────────────────────────────────────────────
  let specPolicy = $state(null);
  let specPolicyLoading = $state(false);
  let specPolicyError = $state(null);
  let specPolicySaving = $state(false);
  let specPolicySaved = $state(false);

  // ── Budget ────────────────────────────────────────────────────────────
  let repoBudget = $state(null);
  let repoBudgetLoading = $state(false);
  let repoBudgetError = $state(null);

  // ── Dependencies ───────────────────────────────────────────────────────
  let repoDeps = $state([]);
  let repoDependents = $state([]);
  let blastRadius = $state([]);
  let depsLoading = $state(false);
  let depsError = $state(null);

  // ── Release ───────────────────────────────────────────────────────────
  let releaseData = $state(null);
  let releaseLoading = $state(false);
  let releaseError = $state(null);

  // ── Audit ─────────────────────────────────────────────────────────────
  let auditEvents = $state([]);
  let auditLoading = $state(false);
  let auditError = $state(null);
  let auditFilterType = $state('');
  let auditSortCol = $state('timestamp');
  let auditSortDir = $state(-1);

  function toggleAuditSort(col) {
    if (col === auditSortCol) { auditSortDir *= -1; }
    else { auditSortCol = col; auditSortDir = 1; }
  }

  const sortedAuditEvents = $derived(
    [...auditEvents].sort((a, b) => {
      const av = a[auditSortCol] ?? '';
      const bv = b[auditSortCol] ?? '';
      if (av < bv) return -1 * auditSortDir;
      if (av > bv) return 1 * auditSortDir;
      return 0;
    })
  );

  const AUDIT_EVENT_TYPES = [
    'spec_approved', 'spec_revoked', 'gate_override', 'agent_spawned',
    'agent_stopped', 'mr_merged', 'mr_opened', 'commit_pushed',
  ];

  // ── Danger Zone ───────────────────────────────────────────────────────
  let archiveConfirm = $state(false);
  let archiving = $state(false);
  let archiveError = $state(null);

  let deleteConfirmName = $state('');
  let deleting = $state(false);
  let deleteError = $state(null);

  const archiveConfirmMsg = $derived(
    $t('repo_settings.danger_zone.archive_confirm', { values: { name: repo?.name ?? 'this repo' } })
  );
  const deleteConfirmRequired = $derived(repo?.name ?? '');
  const deleteReady = $derived(deleteConfirmName === deleteConfirmRequired && deleteConfirmRequired !== '');

  // ── Data loading driven by tab ─────────────────────────────────────────
  $effect(() => {
    const repoId = repo?.id;
    if (!repoId) return;

    if (activeTab === 'gates') {
      if (untrack(() => gates.length === 0 && !gatesLoading)) loadGates(repoId);
      if (untrack(() => pushGates.length === 0 && !pushGatesLoading)) loadPushGates(repoId);
    }
    if (activeTab === 'policies') {
      if (untrack(() => !specPolicy && !specPolicyLoading)) loadSpecPolicy(repoId);
    }
    if (activeTab === 'budget') {
      if (untrack(() => !repoBudget && !repoBudgetLoading)) loadRepoBudget(repoId);
    }
    if (activeTab === 'dependencies') {
      if (untrack(() => repoDeps.length === 0 && repoDependents.length === 0 && !depsLoading)) loadDependencies(repoId);
    }
    if (activeTab === 'release') {
      // Don't auto-load — user initiates preparation
    }
    if (activeTab === 'audit') {
      // Track auditFilterType so changes trigger a reload
      void auditFilterType;
      loadAudit(repoId);
    }
  });

  async function loadGates(repoId) {
    gatesLoading = true;
    gatesError = null;
    try {
      gates = await api.repoGates(repoId) ?? [];
      // Load recent gate results from MRs (best-effort)
      loadRecentGateResults(repoId);
    } catch (e) {
      gatesError = e.message;
      gates = [];
    }
    finally { gatesLoading = false; }
  }

  async function loadRecentGateResults(repoId) {
    gateResultsLoading = true;
    try {
      const mrs = await api.mergeRequests({ repository_id: repoId });
      const mrList = (Array.isArray(mrs) ? mrs : []).slice(0, 5);
      const gateDefMap = Object.fromEntries(gates.map(g => [g.id, g]));
      const results = await Promise.all(mrList.map(async (mr) => {
        const gateData = await api.mrGates(mr.id).catch(() => []);
        const arr = Array.isArray(gateData) ? gateData : (gateData?.gates ?? []);
        return arr.map(g => ({
          ...g,
          mr_id: mr.id,
          mr_title: mr.title,
          mr_status: mr.status,
          gate_name: g.gate_name ?? gateDefMap[g.gate_id]?.name ?? g.name,
          gate_type: g.gate_type ?? gateDefMap[g.gate_id]?.gate_type,
          required: g.required ?? gateDefMap[g.gate_id]?.required,
        }));
      }));
      recentGateResults = results.flat().sort((a, b) => (b.finished_at ?? b.started_at ?? 0) - (a.finished_at ?? a.started_at ?? 0)).slice(0, 15);
    } catch { recentGateResults = []; }
    finally { gateResultsLoading = false; }
  }

  async function createGate() {
    if (!repo?.id || !newGateName.trim()) return;
    gateCreating = true;
    gateCreateError = null;
    try {
      await api.createRepoGate(repo.id, {
        name: newGateName.trim(),
        gate_type: newGateType,
        command: newGateCommand.trim() || undefined,
        required: newGateRequired,
      });
      newGateName = '';
      newGateCommand = '';
      newGateType = 'test_command';
      newGateRequired = true;
      addGateOpen = false;
      await loadGates(repo.id);
    } catch (e) {
      gateCreateError = e.message;
    } finally { gateCreating = false; }
  }

  async function deleteGate(gateId) {
    if (!repo?.id) return;
    deletingGateId = gateId;
    try {
      await api.deleteRepoGate(repo.id, gateId);
      await loadGates(repo.id);
    } catch (e) {
      toastError($t('repo_settings.gates.delete_failed', { values: { error: e.message } }));
    } finally { deletingGateId = null; }
  }

  async function loadPushGates(repoId) {
    pushGatesLoading = true;
    try {
      const resp = await api.repoPushGates(repoId);
      pushGates = Array.isArray(resp?.gates ?? resp) ? (resp?.gates ?? resp) : [];
    } catch {
      pushGates = [];
    } finally { pushGatesLoading = false; }
  }

  async function addPushGate() {
    if (!repo?.id || !newPushGate.trim()) return;
    pushGatesSaving = true;
    try {
      const updated = [...pushGates, newPushGate.trim()];
      await api.setRepoPushGates(repo.id, { gates: updated });
      pushGates = updated;
      newPushGate = '';
    } catch (e) {
      toastError('Failed to add push gate: ' + (e.message ?? e));
    } finally { pushGatesSaving = false; }
  }

  async function removePushGate(gate) {
    if (!repo?.id) return;
    pushGatesSaving = true;
    try {
      const updated = pushGates.filter(g => g !== gate);
      await api.setRepoPushGates(repo.id, { gates: updated });
      pushGates = updated;
    } catch (e) {
      toastError('Failed to remove push gate: ' + (e.message ?? e));
    } finally { pushGatesSaving = false; }
  }

  async function loadSpecPolicy(repoId) {
    specPolicyLoading = true;
    specPolicyError = null;
    try {
      specPolicy = await api.repoSpecPolicy(repoId);
    } catch (e) {
      specPolicyError = e.message;
      specPolicy = null;
    }
    finally { specPolicyLoading = false; }
  }

  async function loadRepoBudget(_repoId) {
    repoBudgetLoading = true;
    repoBudgetError = null;
    try {
      // Per-repo budget endpoint does not exist; show workspace-level budget instead
      if (workspace?.id) {
        repoBudget = await api.workspaceBudget(workspace.id);
      } else {
        repoBudget = null;
      }
    } catch (e) {
      repoBudgetError = e.message;
      repoBudget = null;
    }
    finally { repoBudgetLoading = false; }
  }

  async function loadDependencies(repoId) {
    depsLoading = true;
    depsError = null;
    try {
      const [deps, dependents, blast] = await Promise.all([
        api.repoDependencies(repoId).catch(() => []),
        api.repoDependents(repoId).catch(() => []),
        api.repoBlastRadius(repoId).catch(() => []),
      ]);
      repoDeps = Array.isArray(deps) ? deps : (deps?.dependencies ?? []);
      repoDependents = Array.isArray(dependents) ? dependents : (dependents?.dependents ?? []);
      blastRadius = Array.isArray(blast) ? blast : (blast?.repos ?? blast?.affected ?? []);
    } catch (e) {
      depsError = e.message ?? 'Failed to load dependencies';
    } finally {
      depsLoading = false;
    }
  }

  async function prepareRelease() {
    if (!repo?.id || releaseLoading) return;
    releaseLoading = true;
    releaseError = null;
    try {
      releaseData = await api.releasePrep(repo.id);
    } catch (e) {
      releaseError = e.message ?? 'Release preparation failed';
      releaseData = null;
    } finally {
      releaseLoading = false;
    }
  }

  async function loadAudit(repoId) {
    auditLoading = true;
    auditError = null;
    try {
      const params = { repo_id: repoId };
      if (auditFilterType) params.event_type = auditFilterType;
      const raw = await api.auditEvents(params);
      auditEvents = Array.isArray(raw) ? raw : (raw?.events ?? []);
    } catch (e) {
      auditError = e.message;
    }
    finally { auditLoading = false; }
  }

  async function saveGeneralSettings() {
    if (!repo?.id) return;
    generalSaving = true;
    generalError = null;
    try {
      await api.updateRepo(repo.id, {
        description: repoDescription,
        default_branch: repoDefaultBranch,
        max_concurrent_agents: Number(repoMaxConcurrent),
      });
      generalSaved = true;
      setTimeout(() => { generalSaved = false; }, 2000);
    } catch (e) {
      generalError = e.message;
    }
    finally { generalSaving = false; }
  }

  async function saveSpecPolicy() {
    if (!repo?.id || !specPolicy) return;
    specPolicySaving = true;
    try {
      await api.setRepoSpecPolicy(repo.id, specPolicy);
      specPolicySaved = true;
      setTimeout(() => { specPolicySaved = false; }, 2000);
    } catch (e) {
      toastError($t('repo_settings.policies.policy_save_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    finally { specPolicySaving = false; }
  }

  async function archiveRepo() {
    if (!repo?.id) return;
    archiving = true;
    archiveError = null;
    try {
      await api.archiveRepo(repo.id);
      archiveConfirm = false;
      // After archiving, navigate back (parent will handle re-render)
      window.history.back();
    } catch (e) {
      archiveError = e.message;
    }
    finally { archiving = false; }
  }

  async function deleteRepo() {
    if (!repo?.id || !deleteReady) return;
    deleting = true;
    deleteError = null;
    try {
      await api.deleteRepo(repo.id);
      // Navigate back to workspace home
      window.history.back();
    } catch (e) {
      deleteError = e.message;
    }
    finally { deleting = false; }
  }

  function handleTabKeydown(e) {
    const idx = TABS.findIndex(t => t.id === activeTab);
    if (idx < 0) return;
    let next = -1;
    if (e.key === 'ArrowRight') next = (idx + 1) % TABS.length;
    else if (e.key === 'ArrowLeft') next = (idx - 1 + TABS.length) % TABS.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = TABS.length - 1;
    if (next >= 0) {
      e.preventDefault();
      activeTab = TABS[next].id;
      const btn = e.currentTarget?.querySelector(`#repo-stab-${TABS[next].id}`);
      btn?.focus();
    }
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(typeof ts === 'number' ? ts * 1000 : ts).toLocaleString();
  }

  /** Format audit event details — handles objects, strings, and null */
  function fmtAuditDetail(detail) {
    if (!detail) return '';
    if (typeof detail === 'string') return detail;
    if (typeof detail !== 'object') return String(detail);
    // Extract human-readable summary from common audit detail shapes
    const parts = [];
    if (detail.path) parts.push(detail.path);
    if (detail.branch) parts.push(`branch: ${detail.branch}`);
    if (detail.sha) parts.push(detail.sha.slice(0, 7));
    if (detail.address || detail.remote_addr) parts.push(detail.address ?? detail.remote_addr);
    if (detail.reason) parts.push(detail.reason);
    if (detail.name) parts.push(detail.name);
    if (detail.gate) parts.push(`gate: ${detail.gate}`);
    if (detail.status) parts.push(detail.status);
    if (parts.length > 0) return parts.join(' · ');
    // Fallback: compact JSON
    return JSON.stringify(detail);
  }

  /** Extract clickable entity references from audit event */
  function auditEntityRefs(evt) {
    const refs = [];
    const d = evt.details ?? {};
    if (typeof d === 'object') {
      if (d.agent_id) refs.push({ type: 'agent', id: d.agent_id });
      if (d.mr_id) refs.push({ type: 'mr', id: d.mr_id });
      if (d.task_id) refs.push({ type: 'task', id: d.task_id });
      if (d.spec_path) refs.push({ type: 'spec', id: d.spec_path });
    }
    if (evt.entity_type && evt.entity_id) {
      const existing = refs.find(r => r.id === evt.entity_id);
      if (!existing) refs.push({ type: evt.entity_type, id: evt.entity_id });
    }
    return refs;
  }

  /** Entity name cache for audit */
  // Use shared entity name resolution (global singleton cache)
  function auditEntityName(type, id) {
    return entityName(type, id);
  }

  const repoBudgetPct = $derived.by(() => {
    if (!repoBudget) return null;
    const used = repoBudget.used_credits ?? 0;
    const total = repoBudget.total_credits ?? 0;
    if (!total) return null;
    return Math.round((used / total) * 100);
  });
</script>

<div class="repo-settings" data-testid="repo-settings">
  <!-- ── Inner tab bar ───────────────────────────────────────────────── -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="inner-tab-bar" role="tablist" aria-label={$t('repo_settings.tab_bar_label')} data-testid="repo-settings-tabs" onkeydown={handleTabKeydown}>
    {#each TABS as tab}
      <button
        class="inner-tab-btn"
        class:active={activeTab === tab.id}
        class:danger={tab.id === 'danger-zone'}
        role="tab"
        id="repo-stab-{tab.id}"
        aria-selected={activeTab === tab.id}
        aria-controls="repo-spanel-{tab.id}"
        tabindex={activeTab === tab.id ? 0 : -1}
        onclick={() => { activeTab = tab.id; }}
      >
        {tab.label}
      </button>
    {/each}
  </div>

  <!-- ── Tab content ────────────────────────────────────────────────── -->
  <div
    class="settings-panel"
    role="tabpanel"
    id="repo-spanel-{activeTab}"
    aria-labelledby="repo-stab-{activeTab}"
    tabindex="0"
  >

    <!-- General tab -->
    {#if activeTab === 'general'}
      <div class="tab-body" data-testid="repo-general-tab">
        <h2 class="tab-title">{$t('repo_settings.general.title')}</h2>

        <div class="field-card">
          <div class="field">
            <span class="field-label">{$t('repo_settings.general.repo_name')}</span>
            <div class="field-value" data-testid="repo-name-display">{repo?.name ?? '—'}</div>
            <p class="field-hint">{$t('repo_settings.general.repo_name_hint')}</p>
          </div>

          <div class="field">
            <label class="field-label" for="repo-desc-input">{$t('repo_settings.general.description')}</label>
            <textarea
              id="repo-desc-input"
              class="field-textarea"
              rows="3"
              placeholder={$t('repo_settings.general.description_placeholder')}
              bind:value={repoDescription}
              data-testid="repo-desc-input"
            ></textarea>
          </div>

          <div class="field">
            <label class="field-label" for="repo-branch-input">{$t('repo_settings.general.default_branch')}</label>
            <input
              id="repo-branch-input"
              class="field-input"
              type="text"
              placeholder={$t('repo_settings.general.default_branch_placeholder')}
              bind:value={repoDefaultBranch}
              data-testid="repo-branch-input"
            />
          </div>

          <div class="field">
            <label class="field-label" for="repo-max-agents-input">{$t('repo_settings.general.max_concurrent_agents')}</label>
            <input
              id="repo-max-agents-input"
              class="field-input field-input-sm"
              type="number"
              min="1"
              max="50"
              bind:value={repoMaxConcurrent}
              data-testid="repo-max-agents-input"
            />
            <p class="field-hint">{$t('repo_settings.general.max_concurrent_agents_hint')}</p>
          </div>
        </div>

        {#if generalError}
          <p class="error-text" role="alert" data-testid="general-error">{generalError}</p>
        {/if}

        <div class="action-row">
          <button
            class="btn-primary"
            onclick={saveGeneralSettings}
            disabled={generalSaving}
            data-testid="save-general-btn"
          >
            {#if generalSaving}{$t('repo_settings.general.saving')}{:else if generalSaved}{$t('repo_settings.general.saved')}{:else}{$t('repo_settings.general.save_changes')}{/if}
          </button>
        </div>
      </div>

    <!-- Gates tab -->
    {:else if activeTab === 'gates'}
      <div class="tab-body" data-testid="repo-gates-tab">
        <h2 class="tab-title">{$t('repo_settings.gates.title')}</h2>
        <p class="tab-desc">{$t('repo_settings.gates.description')}</p>

        {#if gatesLoading}
          <p class="loading-text">{$t('repo_settings.gates.loading')}</p>
        {:else if gatesError}
          <p class="error-text" role="alert">{gatesError}</p>
        {:else}
          {#if gates.length === 0}
            <p class="empty-text">{$t('repo_settings.gates.empty')}</p>
          {:else}
            <div class="gates-list" data-testid="gates-list">
              {#each gates as gate}
                <div class="gate-card" data-testid="gate-card">
                  <div class="gate-header">
                    <span class="gate-name">{gate.name ?? shortId(gate.id)}</span>
                    {#if gate.gate_type}
                      <span class="gate-kind">{gate.gate_type}</span>
                    {/if}
                    {#if gate.required !== undefined}
                      <span class="gate-required" class:required={gate.required}>
                        {gate.required ? $t('repo_settings.gates.required') : $t('repo_settings.gates.optional')}
                      </span>
                    {/if}
                    <button
                      class="btn-gate-delete"
                      onclick={() => deleteGate(gate.id)}
                      disabled={deletingGateId === gate.id}
                      aria-label="{$t('repo_settings.gates.delete_gate')} {gate.name ?? gate.id}"
                      data-testid="delete-gate-btn"
                    >
                      {deletingGateId === gate.id ? $t('repo_settings.gates.deleting') : $t('repo_settings.gates.delete_gate')}
                    </button>
                  </div>
                  {#if gate.command}
                    <code class="gate-command">{gate.command}</code>
                  {/if}
                  {#if gate.description}
                    <p class="gate-desc">{gate.description}</p>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

          <!-- Add Gate -->
          {#if !addGateOpen}
            <div class="action-row action-row-left">
              <button class="btn-secondary" onclick={() => { addGateOpen = true; }} data-testid="add-gate-btn">
                {$t('repo_settings.gates.add_gate')}
              </button>
            </div>
          {:else}
            <div class="field-card" data-testid="add-gate-form">
              <div class="field">
                <label class="field-label" for="gate-name-input">{$t('repo_settings.gates.gate_name_label')}</label>
                <input id="gate-name-input" class="field-input" type="text" placeholder={$t('repo_settings.gates.gate_name_placeholder')} bind:value={newGateName} />
              </div>
              <div class="field">
                <label class="field-label" for="gate-type-select">{$t('repo_settings.gates.gate_type_label')}</label>
                <select id="gate-type-select" class="filter-select" bind:value={newGateType}>
                  <option value="test_command">{$t('repo_settings.gates.type_test_command')}</option>
                  <option value="lint_command">{$t('repo_settings.gates.type_lint_command')}</option>
                  <option value="required_approvals">{$t('repo_settings.gates.type_required_approvals')}</option>
                  <option value="agent_review">{$t('repo_settings.gates.type_agent_review')}</option>
                  <option value="agent_validation">{$t('repo_settings.gates.type_agent_validation')}</option>
                </select>
              </div>
              {#if newGateType === 'test_command' || newGateType === 'lint_command'}
                <div class="field">
                  <label class="field-label" for="gate-command-input">{$t('repo_settings.gates.gate_command_label')}</label>
                  <input id="gate-command-input" class="field-input" type="text" placeholder={$t('repo_settings.gates.gate_command_placeholder')} bind:value={newGateCommand} />
                </div>
              {/if}
              <div class="field">
                <label class="toggle-row">
                  <input type="checkbox" bind:checked={newGateRequired} />
                  <span class="toggle-label">
                    <span class="toggle-name">{$t('repo_settings.gates.gate_required_label')}</span>
                  </span>
                </label>
              </div>
              {#if gateCreateError}
                <p class="error-text" role="alert">{$t('repo_settings.gates.create_failed', { values: { error: gateCreateError } })}</p>
              {/if}
              <div class="confirm-actions">
                <button class="btn-secondary" onclick={() => { addGateOpen = false; gateCreateError = null; }}>
                  {$t('common.cancel')}
                </button>
                <button class="btn-primary" onclick={createGate} disabled={gateCreating || !newGateName.trim()} data-testid="create-gate-btn">
                  {gateCreating ? $t('repo_settings.gates.creating') : $t('repo_settings.gates.create_gate')}
                </button>
              </div>
            </div>
          {/if}
        {/if}

        <!-- Recent Gate Results -->
        <h3 class="section-title">Recent Gate Results</h3>
        <p class="tab-desc">Results from the most recent merge request gate checks.</p>
        {#if gateResultsLoading}
          <p class="loading-text">Loading recent results...</p>
        {:else if recentGateResults.length === 0}
          <p class="empty-text">No gate results yet. Results appear after MRs are enqueued for merge.</p>
        {:else}
          <div class="gate-results-list">
            {#each recentGateResults as result}
              {@const passed = result.status === 'Passed' || result.status === 'passed'}
              {@const failed = result.status === 'Failed' || result.status === 'failed'}
              <div class="gate-result-row" class:gate-result-pass={passed} class:gate-result-fail={failed}>
                <span class="gate-result-icon">{passed ? '✓' : failed ? '✗' : '○'}</span>
                <span class="gate-result-name">{result.gate_name ?? 'Gate'}</span>
                {#if result.gate_type}
                  <span class="gate-kind">{result.gate_type.replace(/_/g, ' ')}</span>
                {/if}
                {#if result.required !== undefined}
                  <span class="gate-required" class:required={result.required}>{result.required ? 'required' : 'advisory'}</span>
                {/if}
                <button class="gate-result-mr" onclick={() => nav('mr', result.mr_id, { _openTab: 'gates' })} title="View MR: {result.mr_title}">
                  {result.mr_title ?? 'MR'}
                  <span class="gate-result-mr-status status-badge status-{result.mr_status}">{result.mr_status}</span>
                </button>
                {#if result.duration_ms || (result.started_at && result.finished_at)}
                  {@const dur = result.duration_ms ?? Math.round((result.finished_at - result.started_at) * 1000)}
                  <span class="gate-result-duration">{dur < 1000 ? dur + 'ms' : (dur / 1000).toFixed(1) + 's'}</span>
                {/if}
                {#if result.output && failed}
                  <details class="gate-result-output">
                    <summary>Output</summary>
                    <pre class="gate-output-pre">{result.output}</pre>
                  </details>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Push Gates Section -->
        <h3 class="section-title">Push Gates</h3>
        <p class="tab-desc">Push gates validate commits before they are accepted. Rejected pushes show the gate name to the user.</p>

        {#if pushGatesLoading}
          <p class="loading-text">Loading push gates...</p>
        {:else}
          {#if pushGates.length === 0}
            <p class="empty-text">No push gates configured. Commits are accepted without pre-receive checks.</p>
          {:else}
            <div class="gates-list" data-testid="push-gates-list">
              {#each pushGates as gate}
                <div class="gate-card">
                  <div class="gate-header">
                    <span class="gate-name">{gate}</span>
                    <span class="gate-kind">push gate</span>
                    <button
                      class="btn-gate-delete"
                      onclick={() => removePushGate(gate)}
                      disabled={pushGatesSaving}
                      aria-label="Remove push gate {gate}"
                    >
                      {pushGatesSaving ? 'Removing...' : 'Remove'}
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
          <div class="action-row action-row-left" style="display: flex; gap: var(--space-2); align-items: center;">
            <input
              class="field-input field-input-sm"
              type="text"
              placeholder="e.g., conventional-commit"
              bind:value={newPushGate}
              onkeydown={(e) => e.key === 'Enter' && addPushGate()}
              style="max-width: 240px;"
            />
            <button class="btn-secondary" onclick={addPushGate} disabled={pushGatesSaving || !newPushGate.trim()}>
              Add Push Gate
            </button>
          </div>
        {/if}
      </div>

    <!-- Policies tab -->
    {:else if activeTab === 'policies'}
      <div class="tab-body" data-testid="repo-policies-tab">
        <h2 class="tab-title">{$t('repo_settings.policies.title')}</h2>
        <p class="tab-desc">{$t('repo_settings.policies.description')}</p>

        {#if specPolicyLoading}
          <p class="loading-text">{$t('repo_settings.policies.loading')}</p>
        {:else if specPolicyError}
          <p class="error-text" role="alert">{specPolicyError}</p>
        {:else if !specPolicy}
          <p class="empty-text">{$t('repo_settings.policies.empty')}</p>
        {:else}
          <div class="field-card" data-testid="spec-policy-form">
            <div class="field">
              <label class="toggle-row">
                <input
                  type="checkbox"
                  bind:checked={specPolicy.require_spec_ref}
                  data-testid="toggle-require-spec-ref"
                />
                <span class="toggle-label">
                  <span class="toggle-name">{$t('repo_settings.policies.require_spec_ref')}</span>
                  <span class="toggle-hint">{$t('repo_settings.policies.require_spec_ref_hint')}</span>
                </span>
              </label>
            </div>

            <div class="field">
              <label class="toggle-row">
                <input
                  type="checkbox"
                  bind:checked={specPolicy.require_approval}
                  data-testid="toggle-require-approval"
                />
                <span class="toggle-label">
                  <span class="toggle-name">{$t('repo_settings.policies.require_approval')}</span>
                  <span class="toggle-hint">{$t('repo_settings.policies.require_approval_hint')}</span>
                </span>
              </label>
            </div>

            <div class="field">
              <label class="toggle-row">
                <input
                  type="checkbox"
                  bind:checked={specPolicy.stale_spec_warning}
                  data-testid="toggle-stale-warning"
                />
                <span class="toggle-label">
                  <span class="toggle-name">{$t('repo_settings.policies.stale_warning')}</span>
                  <span class="toggle-hint">{$t('repo_settings.policies.stale_warning_hint')}</span>
                </span>
              </label>
            </div>
          </div>

          <div class="action-row">
            <button
              class="btn-primary"
              onclick={saveSpecPolicy}
              disabled={specPolicySaving}
              data-testid="save-spec-policy-btn"
            >
              {#if specPolicySaving}{$t('repo_settings.policies.saving')}{:else if specPolicySaved}{$t('repo_settings.policies.saved')}{:else}{$t('repo_settings.policies.save_policies')}{/if}
            </button>
          </div>
        {/if}
      </div>

    <!-- Budget tab -->
    {:else if activeTab === 'budget'}
      <div class="tab-body" data-testid="repo-budget-tab">
        <h2 class="tab-title">{$t('repo_settings.budget.title')}</h2>
        <p class="tab-desc">{$t('repo_settings.budget.description')}</p>

        {#if repoBudgetLoading}
          <p class="loading-text">{$t('repo_settings.budget.loading')}</p>
        {:else if repoBudgetError}
          <p class="empty-text" data-testid="budget-unavailable">{$t('repo_settings.budget.unavailable')}</p>
        {:else if !repoBudget}
          <p class="empty-text">{$t('repo_settings.budget.empty')}</p>
        {:else}
          <div class="budget-card" data-testid="repo-budget-card">
            <div class="budget-stat-row">
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('repo_settings.budget.allocated')}</span>
                <span class="budget-stat-value">{repoBudget.total_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('repo_settings.budget.used')}</span>
                <span class="budget-stat-value">{repoBudget.used_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('repo_settings.budget.remaining')}</span>
                <span class="budget-stat-value">
                  {repoBudget.total_credits != null && repoBudget.used_credits != null
                    ? repoBudget.total_credits - repoBudget.used_credits
                    : '—'}
                </span>
              </div>
            </div>

            {#if repoBudgetPct !== null}
              <div class="budget-bar-wrap">
                <div class="budget-bar-label">
                  <span>{$t('repo_settings.budget.usage')}</span>
                  <span>{repoBudgetPct}%</span>
                </div>
                <div
                  class="budget-bar-track"
                  role="progressbar"
                  aria-valuenow={repoBudgetPct}
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-label={$t('repo_settings.budget.budget_pct_used', { values: { pct: repoBudgetPct } })}
                  data-testid="repo-budget-bar"
                >
                  <div
                    class="budget-bar-fill"
                    class:bar-danger={repoBudgetPct > 90}
                    class:bar-warn={repoBudgetPct > 70 && repoBudgetPct <= 90}
                    class:bar-ok={repoBudgetPct <= 70}
                    style="width: {repoBudgetPct}%"
                  ></div>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>

    <!-- Dependencies tab -->
    {:else if activeTab === 'dependencies'}
      <div class="tab-body" data-testid="repo-dependencies-tab">
        <h2 class="tab-title">Cross-Repo Dependencies</h2>
        <p class="tab-desc">View this repository's dependency relationships and blast radius — which other repos are affected when this one changes.</p>

        {#if depsLoading}
          <div class="loading-row">Loading dependencies...</div>
        {:else if depsError}
          <div class="error-row" role="alert">
            <p>{depsError}</p>
            <button class="btn-secondary" onclick={() => loadDependencies(repo?.id)}>Retry</button>
          </div>
        {:else}
          <div class="deps-grid">
            <div class="deps-section">
              <h3 class="deps-section-title">Depends On ({repoDeps.length})</h3>
              <p class="deps-section-hint">Repositories this repo imports or references.</p>
              {#if repoDeps.length === 0}
                <p class="empty-text">No outgoing dependencies detected.</p>
              {:else}
                <ul class="deps-list">
                  {#each repoDeps as dep}
                    <li class="dep-item">
                      <span class="dep-name">{dep.target_repo_name ?? dep.name ?? dep.target_repo_id ?? dep.repo_id ?? 'Unknown'}</span>
                      {#if dep.dep_type ?? dep.dependency_type}
                        <span class="dep-type-tag">{(dep.dep_type ?? dep.dependency_type).replace(/_/g, ' ')}</span>
                      {/if}
                      {#if dep.detection_method}
                        <span class="dep-method-tag">{dep.detection_method}</span>
                      {/if}
                      {#if dep.notes}
                        <span class="dep-notes">{dep.notes}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>

            <div class="deps-section">
              <h3 class="deps-section-title">Dependents ({repoDependents.length})</h3>
              <p class="deps-section-hint">Repositories that depend on this repo.</p>
              {#if repoDependents.length === 0}
                <p class="empty-text">No incoming dependencies detected.</p>
              {:else}
                <ul class="deps-list">
                  {#each repoDependents as dep}
                    <li class="dep-item">
                      <span class="dep-name">{dep.source_repo_name ?? dep.name ?? dep.source_repo_id ?? dep.repo_id ?? 'Unknown'}</span>
                      {#if dep.dep_type ?? dep.dependency_type}
                        <span class="dep-type-tag">{(dep.dep_type ?? dep.dependency_type).replace(/_/g, ' ')}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>

            <div class="deps-section">
              <h3 class="deps-section-title">Blast Radius</h3>
              <p class="deps-section-hint">If this repo changes, these repos may be affected (transitive dependents).</p>
              {#if blastRadius.length === 0}
                <p class="empty-text">No transitive dependents — changes to this repo are self-contained.</p>
              {:else}
                <ul class="deps-list">
                  {#each blastRadius as affected}
                    <li class="dep-item blast-item">
                      <span class="dep-name">{affected.name ?? affected.repo_name ?? affected.repo_id ?? affected.id ?? affected}</span>
                      {#if affected.depth != null}
                        <span class="dep-depth-tag">depth {affected.depth}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          </div>
        {/if}
      </div>

    <!-- Release tab -->
    {:else if activeTab === 'release'}
      <div class="tab-body" data-testid="repo-release-tab">
        <h2 class="tab-title">Release Preparation</h2>
        <p class="tab-desc">Generate a release summary with changelog, version recommendation, and attestation status for all merged changes.</p>

        {#if !releaseData && !releaseLoading}
          <div class="action-row action-row-left">
            <button class="btn-primary" onclick={prepareRelease} disabled={releaseLoading}>
              Prepare Release
            </button>
          </div>
        {/if}

        {#if releaseLoading}
          <p class="loading-text">Analyzing commits and generating changelog...</p>
        {:else if releaseError}
          <div class="error-text" role="alert">{releaseError}</div>
          <div class="action-row action-row-left">
            <button class="btn-secondary" onclick={prepareRelease}>Retry</button>
          </div>
        {:else if releaseData}
          <div class="release-result">
            {#if !releaseData.has_release}
              <p class="tab-desc">No releasable changes found since the last tag.</p>
            {/if}
            <dl class="release-meta">
              <dt>Suggested version</dt>
              <dd class="release-version">{releaseData.next_version ?? 'N/A'}</dd>
              {#if releaseData.current_tag}
                <dt>Current tag</dt>
                <dd class="mono">{releaseData.current_tag}</dd>
              {/if}
              {#if releaseData.current_version}
                <dt>Current version</dt>
                <dd class="mono">{releaseData.current_version}</dd>
              {/if}
              {#if releaseData.bump_type}
                <dt>Bump type</dt>
                <dd>{releaseData.bump_type}</dd>
              {/if}
              {#if releaseData.commit_count != null}
                <dt>Commits since last release</dt>
                <dd>{releaseData.commit_count}</dd>
              {/if}
              {#if releaseData.branch}
                <dt>Branch analyzed</dt>
                <dd class="mono">{releaseData.branch}</dd>
              {/if}
            </dl>
            {#if releaseData.sections?.length > 0}
              <div class="release-changelog">
                <h3 class="release-section-title">Changes by Category</h3>
                {#each releaseData.sections as section}
                  <details class="release-section-group" open>
                    <summary class="release-section-summary">{section.title ?? section.kind ?? 'Other'} ({section.entries?.length ?? 0})</summary>
                    <ul class="release-entry-list">
                      {#each (section.entries ?? []) as entry}
                        <li class="release-entry">
                          <span class="release-entry-msg">{entry.description ?? entry.message ?? entry.summary ?? entry}</span>
                          {#if entry.sha}<code class="mono release-entry-sha">{entry.sha.slice(0, 7)}</code>{/if}
                          {#if entry.scope}<span class="release-entry-scope">({entry.scope})</span>{/if}
                          {#if entry.is_breaking}<span class="release-breaking-badge">BREAKING</span>{/if}
                          {#if entry.agent_name}
                            <span class="release-entry-agent" title={entry.agent_id ?? ''}>by {entry.agent_name}</span>
                          {/if}
                        </li>
                      {/each}
                    </ul>
                  </details>
                {/each}
              </div>
            {/if}
            {#if releaseData.changelog}
              <div class="release-changelog">
                <h3 class="release-section-title">Full Changelog</h3>
                <pre class="release-changelog-pre">{releaseData.changelog}</pre>
              </div>
            {/if}
            {#if releaseData.mr_id}
              <p class="tab-desc">Release MR created: <code class="mono">{entityName('mr', releaseData.mr_id)}</code></p>
            {/if}
            <div class="action-row action-row-left">
              <button class="btn-secondary" onclick={prepareRelease}>Regenerate</button>
            </div>
          </div>
        {/if}
      </div>

    <!-- Audit tab -->
    {:else if activeTab === 'audit'}
      <div class="tab-body" data-testid="repo-audit-tab">
        <h2 class="tab-title">{$t('repo_settings.audit.title')}</h2>

        <div class="audit-filter-bar">
          <select
            class="filter-select"
            bind:value={auditFilterType}
            aria-label={$t('repo_settings.audit.filter_by_event_type')}
            data-testid="audit-filter-select"
          >
            <option value="">{$t('repo_settings.audit.all_event_types')}</option>
            {#each AUDIT_EVENT_TYPES as et}
              <option value={et}>{$t(`repo_settings.audit.event_types.${et}`)}</option>
            {/each}
          </select>
          <button
            class="btn-secondary"
            onclick={() => loadAudit(repo?.id)}
            disabled={auditLoading}
            data-testid="audit-refresh-btn"
          >
            {auditLoading ? $t('repo_settings.audit.loading') : $t('repo_settings.audit.refresh')}
          </button>
        </div>

        {#if auditLoading}
          <p class="loading-text">{$t('repo_settings.audit.loading_events')}</p>
        {:else if auditError}
          <p class="error-text" role="alert">{auditError}</p>
        {:else if auditEvents.length === 0}
          <p class="empty-text">{$t('repo_settings.audit.empty')}</p>
        {:else}
          <div class="audit-list" data-testid="repo-audit-list">
            <div class="audit-row audit-header">
              <button class="audit-sort-btn" aria-label="{$t('repo_settings.audit.sort_by_type')} {auditSortCol === 'event_type' ? (auditSortDir === 1 ? $t('repo_settings.audit.ascending') : $t('repo_settings.audit.descending')) : ''}" onclick={() => toggleAuditSort('event_type')}>{$t('repo_settings.audit.col_type')}{auditSortCol === 'event_type' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-label="{$t('repo_settings.audit.sort_by_actor')} {auditSortCol === 'actor' ? (auditSortDir === 1 ? $t('repo_settings.audit.ascending') : $t('repo_settings.audit.descending')) : ''}" onclick={() => toggleAuditSort('actor')}>{$t('repo_settings.audit.col_actor')}{auditSortCol === 'actor' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-label="{$t('repo_settings.audit.sort_by_detail')} {auditSortCol === 'details' ? (auditSortDir === 1 ? $t('repo_settings.audit.ascending') : $t('repo_settings.audit.descending')) : ''}" onclick={() => toggleAuditSort('details')}>{$t('repo_settings.audit.col_detail')}{auditSortCol === 'details' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-label="{$t('repo_settings.audit.sort_by_time')} {auditSortCol === 'timestamp' ? (auditSortDir === 1 ? $t('repo_settings.audit.ascending') : $t('repo_settings.audit.descending')) : ''}" onclick={() => toggleAuditSort('timestamp')}>{$t('repo_settings.audit.col_time')}{auditSortCol === 'timestamp' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
            </div>
            {#each sortedAuditEvents as evt}
              {@const refs = auditEntityRefs(evt)}
              <div class="audit-row" data-testid="audit-row">
                <span class="audit-type">{evt.event_type ?? evt.type ?? '—'}</span>
                <span class="audit-actor">{evt.actor ?? evt.user_id ?? '—'}</span>
                <span class="audit-detail">
                  {fmtAuditDetail(evt.details ?? evt.message)}
                  {#if refs.length > 0}
                    <span class="audit-refs">
                      {#each refs as ref}
                        <button class="audit-ref-link" onclick={() => nav(ref.type, ref.id, ref.type === 'spec' ? { path: ref.id, repo_id: repo?.id } : {})} title="View {ref.type}: {ref.id}">
                          {auditEntityName(ref.type, ref.id)}
                        </button>
                      {/each}
                    </span>
                  {/if}
                </span>
                <span class="audit-time">{fmtDate(evt.timestamp ?? evt.created_at)}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

    <!-- Danger Zone tab -->
    {:else if activeTab === 'danger-zone'}
      <div class="tab-body" data-testid="repo-danger-tab">
        <h2 class="tab-title danger-title">{$t('repo_settings.danger_zone.title')}</h2>

        <!-- Archive -->
        <div class="danger-card" data-testid="archive-section">
          <div class="danger-card-content">
            <div class="danger-card-info">
              <h3 class="danger-card-title">{$t('repo_settings.danger_zone.archive_title')}</h3>
              <p class="danger-card-desc">
                {$t('repo_settings.danger_zone.archive_desc')}
              </p>
            </div>
            <button
              class="btn-danger"
              onclick={() => { archiveConfirm = !archiveConfirm; deleteConfirmName = ''; }}
              data-testid="archive-btn"
            >
              {$t('repo_settings.danger_zone.archive_btn')}
            </button>
          </div>

          {#if archiveConfirm}
            <div class="confirm-box" data-testid="archive-confirm-box">
              <p class="confirm-msg">{archiveConfirmMsg}</p>
              {#if archiveError}
                <p class="error-text" role="alert">{archiveError}</p>
              {/if}
              <div class="confirm-actions">
                <button
                  class="btn-secondary"
                  onclick={() => { archiveConfirm = false; archiveError = null; }}
                >
                  {$t('repo_settings.danger_zone.cancel')}
                </button>
                <button
                  class="btn-danger"
                  onclick={archiveRepo}
                  disabled={archiving}
                  data-testid="archive-confirm-btn"
                >
                  {archiving ? $t('repo_settings.danger_zone.archiving') : $t('repo_settings.danger_zone.confirm_archive')}
                </button>
              </div>
            </div>
          {/if}
        </div>

        <!-- Delete -->
        <div class="danger-card" data-testid="delete-section">
          {#if repo?.status !== 'Archived'}
            <!-- Repo must be archived before it can be deleted -->
            <div class="danger-card-content">
              <div class="danger-card-info">
                <h3 class="danger-card-title">{$t('repo_settings.danger_zone.delete_title')}</h3>
                <p class="danger-card-desc">
                  {$t('repo_settings.danger_zone.delete_desc')}
                  <strong>{$t('repo_settings.danger_zone.delete_irreversible')}</strong>
                </p>
                <p class="danger-card-prereq" data-testid="delete-archive-required">
                  {$t('repo_settings.danger_zone.delete_prereq')}
                </p>
              </div>
              <button
                class="btn-danger"
                disabled
                data-testid="delete-btn"
                title={$t('repo_settings.danger_zone.delete_prereq_tooltip')}
              >
                {$t('repo_settings.danger_zone.delete_btn')}
              </button>
            </div>
          {:else}
            <!-- Repo is archived — deletion is allowed -->
            <div class="danger-card-content">
              <div class="danger-card-info">
                <h3 class="danger-card-title">{$t('repo_settings.danger_zone.delete_title')}</h3>
                <p class="danger-card-desc">
                  {$t('repo_settings.danger_zone.delete_desc')}
                  <strong>{$t('repo_settings.danger_zone.delete_irreversible')}</strong>
                </p>
              </div>
              <button
                class="btn-danger"
                onclick={() => { deleteConfirmName = ''; archiveConfirm = false; deleteError = null; }}
                data-testid="delete-btn"
              >
                {$t('repo_settings.danger_zone.delete_btn')}
              </button>
            </div>

            {#if deleteConfirmName !== undefined && !archiveConfirm}
              <div class="confirm-box" data-testid="delete-confirm-box">
                <p class="confirm-msg">
                  {$t('repo_settings.danger_zone.delete_confirm_prompt')}
                  <strong>{deleteConfirmRequired}</strong>
                </p>
                <input
                  class="confirm-input"
                  type="text"
                  placeholder={deleteConfirmRequired}
                  bind:value={deleteConfirmName}
                  aria-label={$t('repo_settings.danger_zone.delete_confirm_aria')}
                  data-testid="delete-confirm-input"
                />
                {#if deleteError}
                  <p class="error-text" role="alert">{deleteError}</p>
                {/if}
                <div class="confirm-actions">
                  <button
                    class="btn-secondary"
                    onclick={() => { deleteConfirmName = ''; deleteError = null; }}
                  >
                    {$t('repo_settings.danger_zone.cancel')}
                  </button>
                  <button
                    class="btn-danger"
                    onclick={deleteRepo}
                    disabled={!deleteReady || deleting}
                    data-testid="delete-confirm-btn"
                  >
                    {deleting ? $t('repo_settings.danger_zone.deleting') : $t('repo_settings.danger_zone.delete_repository')}
                  </button>
                </div>
              </div>
            {/if}
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .repo-settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* ── Inner tab bar ────────────────────────────────────────────────── */
  .inner-tab-bar {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .inner-tab-btn {
    padding: var(--space-2) var(--space-4);
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

  .inner-tab-btn:hover { color: var(--color-text); }

  .inner-tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .inner-tab-btn.danger:hover { color: var(--color-danger); }
  .inner-tab-btn.danger.active { border-bottom-color: var(--color-danger); color: var(--color-danger); }

  .inner-tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Tab panel ────────────────────────────────────────────────────── */
  .settings-panel {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-5) var(--space-6);
  }

  .settings-panel:focus { outline: none; }
  .settings-panel:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Tab body ─────────────────────────────────────────────────────── */
  .tab-body {
    max-width: 600px;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .tab-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .tab-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    margin-top: calc(-1 * var(--space-3));
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  /* ── Fields ───────────────────────────────────────────────────────── */
  .field-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    padding: var(--space-5) var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-value {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    padding: var(--space-1) 0;
  }

  .field-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    font-style: italic;
  }

  .field-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    width: 100%;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .field-input-sm { width: 80px; }

  .field-input:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .field-textarea {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    min-height: 80px;
    transition: border-color var(--transition-fast);
  }

  .field-textarea:focus:not(:focus-visible) { outline: none; }
  .field-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  /* ── Toggles ──────────────────────────────────────────────────────── */
  .toggle-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    cursor: pointer;
  }

  .toggle-row input[type="checkbox"] {
    margin-top: 2px;
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    cursor: pointer;
    accent-color: var(--color-primary);
  }

  .toggle-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .toggle-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .toggle-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* ── Gates ────────────────────────────────────────────────────────── */
  .gates-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .gate-card {
    padding: var(--space-4) var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .gate-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .gate-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .gate-kind {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-family: var(--font-mono);
  }

  .gate-required {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .gate-required.required { color: var(--color-warning); }

  .gate-command {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    display: block;
    overflow-x: auto;
    white-space: pre;
  }

  .gate-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .gate-results-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: var(--space-4);
  }

  .gate-result-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    flex-wrap: wrap;
  }

  .gate-result-pass { border-left: 3px solid var(--color-success); }
  .gate-result-fail { border-left: 3px solid var(--color-danger); }

  .gate-result-icon {
    font-weight: 700;
    width: 16px;
    text-align: center;
  }

  .gate-result-pass .gate-result-icon { color: var(--color-success); }
  .gate-result-fail .gate-result-icon { color: var(--color-danger); }

  .gate-result-name {
    font-weight: 500;
  }

  .gate-result-mr {
    background: none;
    border: none;
    font-size: var(--text-xs);
    color: var(--color-primary);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
  }

  .gate-result-mr:hover { text-decoration: underline; }

  .gate-result-mr-status {
    font-size: 10px;
    padding: 1px 4px;
    border-radius: 3px;
  }

  .gate-result-duration {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .gate-result-output {
    width: 100%;
    margin-top: 4px;
  }

  .gate-result-output summary {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .gate-output-pre {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    overflow-x: auto;
    white-space: pre-wrap;
    max-height: 200px;
  }

  .btn-gate-delete {
    margin-left: auto;
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .btn-gate-delete:hover:not(:disabled) {
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .btn-gate-delete:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-gate-delete:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .action-row-left { justify-content: flex-start; }

  /* ── Budget ───────────────────────────────────────────────────────── */
  .budget-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    padding: var(--space-5) var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .budget-stat-row {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-4);
  }

  .budget-stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .budget-stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .budget-stat-value {
    font-family: var(--font-mono);
    font-size: var(--text-lg);
    font-weight: 700;
    color: var(--color-text);
  }

  .budget-bar-wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .budget-bar-label {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .budget-bar-track {
    height: 8px;
    background: var(--color-border-strong);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .budget-bar-fill {
    height: 100%;
    border-radius: var(--radius-sm);
    transition: width var(--transition-normal);
  }

  .budget-bar-fill.bar-ok { background: var(--color-success); }
  .budget-bar-fill.bar-warn { background: var(--color-warning); }
  .budget-bar-fill.bar-danger { background: var(--color-danger); }

  /* ── Audit ────────────────────────────────────────────────────────── */
  .audit-filter-bar {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    flex-wrap: wrap;
  }

  .filter-select {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    min-width: 180px;
    transition: border-color var(--transition-fast);
  }

  .filter-select:focus:not(:focus-visible) { outline: none; }
  .filter-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .audit-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .audit-row {
    display: grid;
    grid-template-columns: 160px 100px 1fr auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }

  .audit-header {
    background: var(--color-surface-elevated);
    border-radius: var(--radius) var(--radius) 0 0;
    padding: var(--space-2) var(--space-4);
  }

  .audit-sort-btn {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    cursor: pointer;
    padding: 0;
    transition: color var(--transition-fast);
  }

  .audit-sort-btn:hover { color: var(--color-text); }

  .audit-sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .audit-type {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-info);
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .audit-actor {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .audit-detail {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .audit-refs {
    display: inline-flex;
    gap: 4px;
    margin-left: 6px;
  }

  .audit-ref-link {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px 6px;
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-primary);
    cursor: pointer;
    white-space: nowrap;
  }

  .audit-ref-link:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .audit-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    font-family: var(--font-mono);
  }

  /* ── Danger Zone ──────────────────────────────────────────────────── */
  .danger-title { color: var(--color-danger); }

  /* Release tab */
  .release-result {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .release-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-4);
    font-size: var(--text-sm);
  }

  .release-meta dt {
    color: var(--color-text-muted);
    font-weight: 600;
  }

  .release-version {
    font-size: var(--text-lg);
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-primary);
  }

  .release-section-title {
    font-size: var(--text-sm);
    font-weight: 600;
    margin: 0 0 var(--space-2);
  }

  .release-entry {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .release-entry-sha {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .release-entry-scope {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .release-breaking-badge {
    font-size: 10px;
    font-weight: 700;
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius-sm);
    padding: 0 4px;
  }

  .release-entry-agent {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .release-changelog-pre {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    max-height: 300px;
    overflow-y: auto;
  }

  .danger-card {
    background: var(--color-surface);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .danger-card-content {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-5) var(--space-6);
    flex-wrap: wrap;
  }

  .danger-card-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex: 1;
    min-width: 200px;
  }

  .danger-card-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .danger-card-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.6;
  }

  .danger-card-prereq {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: var(--space-2) 0 0;
    font-style: italic;
  }

  .confirm-box {
    padding: var(--space-5) var(--space-6);
    background: color-mix(in srgb, var(--color-danger) 5%, var(--color-surface));
    border-top: 1px solid var(--color-danger);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .confirm-msg {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: 0;
    line-height: 1.6;
  }

  .confirm-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    width: 100%;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .confirm-input:focus:not(:focus-visible) { outline: none; }
  .confirm-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .confirm-actions {
    display: flex;
    gap: var(--space-3);
    justify-content: flex-end;
  }

  /* ── Action row ───────────────────────────────────────────────────── */
  .action-row {
    display: flex;
    justify-content: flex-end;
  }

  /* ── Buttons ──────────────────────────────────────────────────────── */
  .btn-primary {
    padding: var(--space-2) var(--space-5);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .btn-primary:hover:not(:disabled) { background: var(--color-primary-hover); }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-primary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .btn-secondary:hover:not(:disabled) { border-color: var(--color-text-muted); }
  .btn-secondary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-secondary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .btn-danger {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .btn-danger:hover:not(:disabled) {
    background: var(--color-danger);
    color: var(--color-text-inverse);
  }

  .btn-danger:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-danger:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* ── State text ───────────────────────────────────────────────────── */
  .loading-text {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  .empty-text {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
    padding: var(--space-6) 0;
    text-align: center;
  }

  .error-text {
    font-size: var(--text-sm);
    color: var(--color-danger);
    margin: 0;
  }

  /* ── Responsive ───────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .settings-panel { padding: var(--space-4); }
    .budget-stat-row { grid-template-columns: 1fr; }
    .audit-row { grid-template-columns: 1fr 1fr; grid-template-rows: auto auto; }
    .danger-card-content { flex-direction: column; }
  }

  @media (prefers-reduced-motion: reduce) {
    .inner-tab-btn,
    .btn-primary,
    .btn-secondary,
    .btn-danger,
    .budget-bar-fill,
    .field-input,
    .field-textarea,
    .filter-select,
    .confirm-input { transition: none; }
  }

  /* ── Dependencies tab ─────────────────────────────────────────────── */
  .deps-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-6);
  }

  .deps-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .deps-section-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .deps-section-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .deps-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .dep-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated, var(--color-bg));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .dep-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .dep-type-tag, .dep-method-tag, .dep-depth-tag {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-border);
    color: var(--color-text-secondary);
  }

  .dep-notes {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .blast-item {
    border-left: 3px solid var(--color-warning);
  }
</style>
