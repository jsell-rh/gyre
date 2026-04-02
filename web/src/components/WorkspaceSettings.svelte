<script>
  /**
   * WorkspaceSettings — full-page workspace settings (§2 Workspace Settings of ui-navigation.md)
   *
   * Tabs: General | Trust & Policies | Teams | Budget | Compute | Audit
   * Accessed via gear icon ⚙ in workspace header or /workspaces/:slug/settings URL.
   */
  import { untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { toastError } from '../lib/toast.svelte.js';
  import { shortId } from '../lib/entityNames.svelte.js';

  let {
    workspace = null,
    onBack = undefined,
  } = $props();

  const TAB_IDS = ['general', 'trust', 'teams', 'budget', 'compute', 'llm', 'audit'];
  let TABS = $derived(TAB_IDS.map(id => ({ id, label: id === 'llm' ? 'LLM Config' : $t(`workspace_settings.tabs.${id}`) })));

  let activeTab = $state('general');

  // ── General ──────────────────────────────────────────────────────────
  // (display-only for name/description; compute target selector)
  let computeTargets = $state([]);
  let computeLoading = $state(false);
  let defaultComputeTarget = $state(workspace?.default_compute_target ?? '');
  let generalSaving = $state(false);
  let generalSaved = $state(false);

  // ── Trust & Policies ─────────────────────────────────────────────────
  const TRUST_LEVEL_IDS = ['Supervised', 'Guided', 'Autonomous', 'Custom'];
  let TRUST_LEVELS = $derived(TRUST_LEVEL_IDS.map(id => ({
    id,
    label: $t(`workspace_settings.trust.${id.toLowerCase()}`),
    desc: $t(`workspace_settings.trust.${id.toLowerCase()}_desc`),
  })));
  let trustLevel = $state(workspace?.trust_level ?? 'Autonomous');
  let trustSaving = $state(false);
  let trustSaved = $state(false);

  // ABAC policies
  let abacPolicies = $state([]);
  let policiesLoading = $state(false);

  // MetaSpec drift policy
  let warnOnDrift = $state(workspace?.meta_spec_policy?.warn_on_drift ?? true);
  let blockOnDrift = $state(workspace?.meta_spec_policy?.block_on_drift ?? false);
  let driftTolerance = $state(workspace?.meta_spec_policy?.drift_tolerance ?? 0);

  // Sync form values when workspace prop changes
  $effect(() => {
    if (workspace) {
      defaultComputeTarget = workspace.default_compute_target ?? '';
      trustLevel = workspace.trust_level ?? 'Autonomous';
      warnOnDrift = workspace.meta_spec_policy?.warn_on_drift ?? true;
      blockOnDrift = workspace.meta_spec_policy?.block_on_drift ?? false;
      driftTolerance = workspace.meta_spec_policy?.drift_tolerance ?? 0;
    }
  });
  let policyDriftSaving = $state(false);
  let policyDriftSaved = $state(false);

  // ── Teams ─────────────────────────────────────────────────────────────
  let members = $state([]);
  let membersLoading = $state(false);
  let membersError = $state(null);
  let membersSortCol = $state('name');
  let membersSortDir = $state('asc');

  function toggleMembersSort(col) {
    if (membersSortCol === col) {
      membersSortDir = membersSortDir === 'asc' ? 'desc' : 'asc';
    } else {
      membersSortCol = col;
      membersSortDir = 'asc';
    }
  }

  function membersSortArrow(col) {
    if (membersSortCol !== col) return '↕';
    return membersSortDir === 'asc' ? '↑' : '↓';
  }

  let sortedMembers = $derived.by(() => {
    return [...members].sort((a, b) => {
      let av, bv;
      if (membersSortCol === 'name') {
        av = a.name ?? a.username ?? '';
        bv = b.name ?? b.username ?? '';
      } else {
        av = String(a[membersSortCol] ?? '');
        bv = String(b[membersSortCol] ?? '');
      }
      const cmp = av.localeCompare(bv);
      return membersSortDir === 'asc' ? cmp : -cmp;
    });
  });

  // ── Budget ────────────────────────────────────────────────────────────
  let budget = $state(null);
  let budgetLoading = $state(false);
  let budgetError = $state(null);
  let budgetEditCredits = $state('');
  let budgetSaving = $state(false);
  let budgetSaved = $state(false);
  let budgetSaveError = $state(null);

  // ── Compute ───────────────────────────────────────────────────────────
  let allCompute = $state([]);
  let allComputeLoading = $state(false);
  let allComputeError = $state(null);

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
    'spec_approved', 'spec_revoked', 'gate_override', 'trust_changed',
    'agent_spawned', 'agent_stopped', 'policy_evaluated', 'member_added', 'member_removed',
  ];

  // ── LLM Config ────────────────────────────────────────────────────────
  const LLM_FEATURES = ['briefing-ask', 'spec-assist', 'explorer-generate', 'graph-predict'];
  let llmConfigs = $state({});
  let llmPrompts = $state({});
  let llmLoading = $state(false);
  let llmError = $state(null);
  let llmEditFeature = $state(null);
  let llmEditModel = $state('');
  let llmEditMaxTokens = $state('');
  let llmEditPrompt = $state('');
  let llmSaving = $state(false);
  let llmSaved = $state(false);

  async function loadLlmConfigs(wsId) {
    llmLoading = true;
    llmError = null;
    try {
      const [configList, ...prompts] = await Promise.all([
        api.llmConfigList(wsId).catch(() => []),
        ...LLM_FEATURES.map(f => api.llmPromptGet(wsId, f).catch(() => null)),
      ]);
      const cfgMap = {};
      if (Array.isArray(configList)) {
        for (const c of configList) {
          cfgMap[c.feature ?? c.name ?? 'unknown'] = c;
        }
      }
      // Also try to fetch each feature's config individually
      for (const f of LLM_FEATURES) {
        if (!cfgMap[f]) {
          try {
            const cfg = await api.llmConfigGet(wsId, f);
            if (cfg && cfg.model_name) cfgMap[f] = cfg;
          } catch { /* not configured */ }
        }
      }
      llmConfigs = cfgMap;
      const promptMap = {};
      LLM_FEATURES.forEach((f, i) => {
        if (prompts[i]) promptMap[f] = prompts[i];
      });
      llmPrompts = promptMap;
    } catch (e) {
      llmError = e?.message ?? 'Failed to load LLM config';
    } finally {
      llmLoading = false;
    }
  }

  function editLlmFeature(feature) {
    llmEditFeature = feature;
    const cfg = llmConfigs[feature];
    llmEditModel = cfg?.model_name ?? '';
    llmEditMaxTokens = cfg?.max_tokens ? String(cfg.max_tokens) : '';
    llmEditPrompt = llmPrompts[feature]?.content ?? '';
    llmSaved = false;
  }

  async function saveLlmConfig() {
    const wsId = workspace?.id;
    if (!wsId || !llmEditFeature) return;
    llmSaving = true;
    llmSaved = false;
    try {
      if (llmEditModel.trim()) {
        const data = { model_name: llmEditModel.trim() };
        if (llmEditMaxTokens.trim()) data.max_tokens = parseInt(llmEditMaxTokens);
        await api.llmConfigSet(wsId, llmEditFeature, data);
      } else {
        await api.llmConfigDelete(wsId, llmEditFeature).catch(() => {});
      }
      if (llmEditPrompt.trim()) {
        await api.llmPromptSet(wsId, llmEditFeature, { content: llmEditPrompt.trim() });
      } else {
        await api.llmPromptDelete(wsId, llmEditFeature).catch(() => {});
      }
      llmSaved = true;
      setTimeout(() => { llmSaved = false; }, 2000);
      await loadLlmConfigs(wsId);
    } catch (e) {
      toastError('Failed to save: ' + (e?.message ?? e));
    } finally {
      llmSaving = false;
    }
  }

  async function resetLlmFeature(feature) {
    const wsId = workspace?.id;
    if (!wsId) return;
    try {
      await Promise.all([
        api.llmConfigDelete(wsId, feature).catch(() => {}),
        api.llmPromptDelete(wsId, feature).catch(() => {}),
      ]);
      await loadLlmConfigs(wsId);
      if (llmEditFeature === feature) llmEditFeature = null;
    } catch (e) {
      toastError('Failed to reset: ' + (e?.message ?? e));
    }
  }

  // ── Data loading driven by tab ─────────────────────────────────────────
  $effect(() => {
    const wsId = workspace?.id;
    if (!wsId) return;

    if (activeTab === 'general' || activeTab === 'compute') {
      if (untrack(() => computeTargets.length === 0 && !computeLoading)) loadComputeTargets();
    }
    if (activeTab === 'trust') {
      if (untrack(() => abacPolicies.length === 0 && !policiesLoading)) loadAbacPolicies(wsId);
    }
    if (activeTab === 'teams') {
      if (untrack(() => members.length === 0 && !membersLoading)) loadMembers(wsId);
    }
    if (activeTab === 'budget') {
      if (untrack(() => !budget && !budgetLoading)) loadBudget(wsId);
    }
    if (activeTab === 'compute') {
      if (untrack(() => allCompute.length === 0 && !allComputeLoading)) loadAllCompute();
    }
    if (activeTab === 'llm') {
      if (untrack(() => Object.keys(llmConfigs).length === 0 && !llmLoading)) loadLlmConfigs(wsId);
    }
    if (activeTab === 'audit') {
      loadAudit(wsId);
    }
  });

  async function loadComputeTargets() {
    computeLoading = true;
    try {
      computeTargets = await api.computeList() ?? [];
    } catch { computeTargets = []; }
    finally { computeLoading = false; }
  }

  async function loadAbacPolicies(wsId) {
    policiesLoading = true;
    try {
      abacPolicies = await api.workspaceAbacPolicies(wsId) ?? [];
    } catch { abacPolicies = []; }
    finally { policiesLoading = false; }
  }

  let deletingPolicyId = $state(null);

  async function deleteAbacPolicy(policyId) {
    const wsId = workspace?.id;
    if (!wsId) return;
    deletingPolicyId = policyId;
    try {
      await api.deleteWorkspaceAbacPolicy(wsId, policyId);
      await loadAbacPolicies(wsId);
    } catch (e) {
      toastError($t('workspace_settings.trust.abac_delete_failed', { values: { error: e?.message ?? 'unknown' } }));
    } finally { deletingPolicyId = null; }
  }

  async function loadMembers(wsId) {
    membersLoading = true;
    membersError = null;
    try {
      members = await api.workspaceMembers(wsId) ?? [];
    } catch (e) {
      membersError = e.message;
      members = [];
    }
    finally { membersLoading = false; }
  }

  async function loadBudget(wsId) {
    budgetLoading = true;
    budgetError = null;
    try {
      budget = await api.workspaceBudget(wsId);
      budgetEditCredits = String(budget?.config?.max_tokens_per_day ?? '');
    } catch (e) {
      budgetError = e.message;
      budget = null;
    }
    finally { budgetLoading = false; }
  }

  async function saveBudget() {
    const wsId = workspace?.id;
    if (!wsId) return;
    const total = Number(budgetEditCredits);
    if (!Number.isFinite(total) || total < 0) {
      budgetSaveError = $t('workspace_settings.budget.invalid_number');
      return;
    }
    budgetSaving = true;
    budgetSaved = false;
    budgetSaveError = null;
    try {
      budget = await api.setWorkspaceBudget(wsId, { max_tokens_per_day: total });
      budgetEditCredits = String(budget?.config?.max_tokens_per_day ?? total);
      budgetSaved = true;
      setTimeout(() => { budgetSaved = false; }, 2000);
    } catch (e) {
      budgetSaveError = e?.message ?? $t('workspace_settings.budget.save_failed');
    } finally {
      budgetSaving = false;
    }
  }

  async function loadAllCompute() {
    allComputeLoading = true;
    allComputeError = null;
    try {
      allCompute = await api.computeList() ?? [];
    } catch (e) {
      allComputeError = e.message;
      allCompute = [];
    }
    finally { allComputeLoading = false; }
  }

  async function loadAudit(wsId) {
    auditLoading = true;
    auditError = null;
    try {
      const params = { workspace_id: wsId };
      if (auditFilterType) params.event_type = auditFilterType;
      const raw = await api.auditEvents(params);
      auditEvents = Array.isArray(raw) ? raw : (raw?.events ?? []);
    } catch (e) {
      auditError = e.message;
    }
    finally { auditLoading = false; }
  }

  async function saveGeneralSettings() {
    if (!workspace?.id) return;
    generalSaving = true;
    try {
      await api.updateWorkspace(workspace.id, { default_compute_target: defaultComputeTarget });
      generalSaved = true;
      setTimeout(() => { generalSaved = false; }, 2000);
    } catch (e) { toastError(e?.message ?? $t('workspace_settings.save_failed_settings')); }
    finally { generalSaving = false; }
  }

  async function saveTrustLevel() {
    if (!workspace?.id) return;
    trustSaving = true;
    try {
      await api.updateWorkspace(workspace.id, { trust_level: trustLevel });
      trustSaved = true;
      setTimeout(() => { trustSaved = false; }, 2000);
    } catch (e) { toastError(e?.message ?? $t('workspace_settings.save_failed_trust')); }
    finally { trustSaving = false; }
  }

  async function saveDriftPolicy() {
    if (!workspace?.id) return;
    policyDriftSaving = true;
    try {
      await api.updateWorkspace(workspace.id, {
        meta_spec_policy: {
          warn_on_drift: warnOnDrift,
          block_on_drift: blockOnDrift,
          drift_tolerance: Number(driftTolerance),
        },
      });
      policyDriftSaved = true;
      setTimeout(() => { policyDriftSaved = false; }, 2000);
    } catch (e) { toastError(e?.message ?? $t('workspace_settings.save_failed_drift')); }
    finally { policyDriftSaving = false; }
  }

  function handleTabKeydown(e) {
    const idx = TABS.findIndex(tab => tab.id === activeTab);
    if (idx < 0) return;
    let next = -1;
    if (e.key === 'ArrowRight') next = (idx + 1) % TABS.length;
    else if (e.key === 'ArrowLeft') next = (idx - 1 + TABS.length) % TABS.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = TABS.length - 1;
    if (next >= 0) {
      e.preventDefault();
      activeTab = TABS[next].id;
      const btn = e.currentTarget?.querySelector(`#ws-tab-${TABS[next].id}`);
      btn?.focus();
    }
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(typeof ts === 'number' ? ts * 1000 : ts).toLocaleString();
  }

  const budgetPct = $derived.by(() => {
    if (!budget) return null;
    const used = budget.usage?.tokens_used_today ?? 0;
    const total = budget.config?.max_tokens_per_day ?? 0;
    if (!total) return null;
    return Math.round((used / total) * 100);
  });
</script>

<div class="ws-settings" data-testid="workspace-settings">
  <!-- ── Page header ─────────────────────────────────────────────────── -->
  <div class="page-header">
    <button
      class="back-btn"
      onclick={() => onBack?.()}
      aria-label={$t('workspace_settings.back_label')}
      data-testid="ws-settings-back"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 5l-7 7 7 7"/>
      </svg>
    </button>
    <div class="page-title-group">
      <h1 class="page-title" data-testid="ws-settings-title">{workspace?.name ? $t('workspace_settings.page_title', { values: { name: workspace.name } }) : $t('workspace_settings.page_title_fallback')}</h1>
    </div>
  </div>

  <!-- ── Tab bar ────────────────────────────────────────────────────── -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="settings-tab-bar" role="tablist" aria-label={$t('workspace_settings.tab_bar_label')} data-testid="ws-settings-tabs" onkeydown={handleTabKeydown}>
    {#each TABS as tab}
      <button
        class="settings-tab-btn"
        class:active={activeTab === tab.id}
        role="tab"
        id="ws-tab-{tab.id}"
        aria-selected={activeTab === tab.id}
        aria-controls="ws-panel-{tab.id}"
        tabindex={activeTab === tab.id ? 0 : -1}
        onclick={() => { activeTab = tab.id; }}
      >
        {tab.label}
      </button>
    {/each}
  </div>

  <!-- ── Tab content ────────────────────────────────────────────────── -->
  <div class="settings-content" role="tabpanel" id="ws-panel-{activeTab}" aria-labelledby="ws-tab-{activeTab}" tabindex="0">

    <!-- General tab -->
    {#if activeTab === 'general'}
      <div class="settings-section" data-testid="general-tab">
        <h2 class="section-title">{$t('workspace_settings.general.title')}</h2>

        <div class="field-group">
          <div class="field">
            <span class="field-label">{$t('workspace_settings.general.workspace_name')}</span>
            <div class="field-value" data-testid="ws-name">{workspace?.name ?? '—'}</div>
            <p class="field-hint">{$t('workspace_settings.general.rename_hint')}</p>
          </div>

          <div class="field">
            <span class="field-label">{$t('workspace_settings.general.description')}</span>
            <div class="field-value">{workspace?.description ?? '—'}</div>
          </div>

          <div class="field">
            <label class="field-label" for="compute-target-select">{$t('workspace_settings.general.default_compute_target')}</label>
            {#if computeLoading}
              <div class="field-loading">{$t('workspace_settings.general.loading_targets')}</div>
            {:else}
              <select
                id="compute-target-select"
                class="field-select"
                bind:value={defaultComputeTarget}
                data-testid="compute-target-select"
              >
                <option value="">{$t('workspace_settings.general.none_tenant_default')}</option>
                {#each computeTargets as ct}
                  <option value={ct.id}>{ct.name ?? shortId(ct.id)}</option>
                {/each}
              </select>
            {/if}
          </div>
        </div>

        <div class="action-row">
          <button
            class="btn-primary"
            onclick={saveGeneralSettings}
            disabled={generalSaving}
            data-testid="save-general-btn"
          >
            {#if generalSaving}{$t('workspace_settings.general.saving')}{:else if generalSaved}{$t('workspace_settings.general.saved')}{:else}{$t('workspace_settings.general.save')}{/if}
          </button>
        </div>
      </div>

    <!-- Trust & Policies tab -->
    {:else if activeTab === 'trust'}
      <div class="settings-section" data-testid="trust-tab">
        <h2 class="section-title">{$t('workspace_settings.trust.title')}</h2>

        <!-- Trust level -->
        <div class="sub-section">
          <h3 class="sub-title">{$t('workspace_settings.trust.trust_level_title')}</h3>
          <p class="sub-desc">{$t('workspace_settings.trust.trust_level_desc')}</p>

          <div class="trust-grid" role="radiogroup" aria-label={$t('workspace_settings.trust.trust_level_label')}>
            {#each TRUST_LEVELS as tl}
              <label
                class="trust-card"
                class:selected={trustLevel === tl.id}
                data-testid="trust-card-{tl.id.toLowerCase()}"
              >
                <input
                  type="radio"
                  name="trust-level"
                  value={tl.id}
                  bind:group={trustLevel}
                  class="sr-only"
                />
                <span class="trust-label">{tl.label}</span>
                <span class="trust-desc">{tl.desc}</span>
              </label>
            {/each}
          </div>

          <div class="action-row">
            <button
              class="btn-primary"
              onclick={saveTrustLevel}
              disabled={trustSaving}
              data-testid="save-trust-btn"
            >
              {#if trustSaving}{$t('workspace_settings.trust.saving')}{:else if trustSaved}{$t('workspace_settings.trust.saved')}{:else}{$t('workspace_settings.trust.save_trust_level')}{/if}
            </button>
          </div>
        </div>

        <!-- ABAC Policies -->
        <div class="sub-section">
          <h3 class="sub-title">{$t('workspace_settings.trust.abac_title')}</h3>
          {#if policiesLoading}
            <p class="loading-text">{$t('workspace_settings.trust.abac_loading')}</p>
          {:else if abacPolicies.length === 0}
            <p class="empty-text">{$t('workspace_settings.trust.abac_empty')}</p>
          {:else}
            <div class="policy-list" data-testid="abac-policy-list">
              {#each abacPolicies as policy}
                <div class="policy-row">
                  <span class="policy-name">{policy.name ?? shortId(policy.id)}</span>
                  <span class="policy-effect policy-effect-{(policy.effect ?? 'allow').toLowerCase()}">
                    {policy.effect ?? 'allow'}
                  </span>
                  {#if policy.description}
                    <span class="policy-desc">{policy.description}</span>
                  {/if}
                  <button
                    class="policy-delete-btn"
                    onclick={() => deleteAbacPolicy(policy.id)}
                    disabled={deletingPolicyId === policy.id}
                    aria-label="{$t('common.delete')} {policy.name ?? shortId(policy.id)}"
                    data-testid="delete-abac-policy-btn"
                  >
                    {deletingPolicyId === policy.id ? '…' : $t('common.delete')}
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <!-- MetaSpec Drift Policy -->
        <div class="sub-section">
          <h3 class="sub-title">{$t('workspace_settings.trust.drift_title')}</h3>
          <p class="sub-desc">{$t('workspace_settings.trust.drift_desc')}</p>

          <div class="toggle-group" data-testid="drift-policy-toggles">
            <label class="toggle-row">
              <input type="checkbox" bind:checked={warnOnDrift} data-testid="toggle-warn-on-drift" />
              <span class="toggle-label">
                <span class="toggle-name">{$t('workspace_settings.trust.warn_on_drift')}</span>
                <span class="toggle-hint">{$t('workspace_settings.trust.warn_on_drift_hint')}</span>
              </span>
            </label>

            <label class="toggle-row">
              <input type="checkbox" bind:checked={blockOnDrift} data-testid="toggle-block-on-drift" />
              <span class="toggle-label">
                <span class="toggle-name">{$t('workspace_settings.trust.block_on_drift')}</span>
                <span class="toggle-hint">{$t('workspace_settings.trust.block_on_drift_hint')}</span>
              </span>
            </label>

            <div class="field">
              <label class="field-label" for="drift-tolerance-input">{$t('workspace_settings.trust.drift_tolerance_label')}</label>
              <input
                id="drift-tolerance-input"
                class="field-input"
                type="number"
                min="0"
                max="10"
                bind:value={driftTolerance}
                data-testid="drift-tolerance-input"
              />
              <p class="field-hint">{$t('workspace_settings.trust.drift_tolerance_hint')}</p>
            </div>
          </div>

          <div class="action-row">
            <button
              class="btn-primary"
              onclick={saveDriftPolicy}
              disabled={policyDriftSaving}
              data-testid="save-drift-policy-btn"
            >
              {#if policyDriftSaving}{$t('workspace_settings.trust.saving')}{:else if policyDriftSaved}{$t('workspace_settings.trust.saved')}{:else}{$t('workspace_settings.trust.save_drift_policy')}{/if}
            </button>
          </div>
        </div>
      </div>

    <!-- Teams tab -->
    {:else if activeTab === 'teams'}
      <div class="settings-section" data-testid="teams-tab">
        <h2 class="section-title">{$t('workspace_settings.teams.title')}</h2>

        {#if membersLoading}
          <p class="loading-text">{$t('workspace_settings.teams.loading')}</p>
        {:else if membersError}
          <p class="error-text" role="alert">{membersError}</p>
        {:else if members.length === 0}
          <p class="empty-text">{$t('workspace_settings.teams.empty')}</p>
        {:else}
          <table class="members-table" data-testid="members-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={membersSortCol === 'name' ? (membersSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                  <button class="sort-btn" onclick={() => toggleMembersSort('name')}>{$t('workspace_settings.teams.col_name')} <span class="sort-arrow" aria-hidden="true">{membersSortArrow('name')}</span></button>
                </th>
                <th scope="col" aria-sort={membersSortCol === 'email' ? (membersSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                  <button class="sort-btn" onclick={() => toggleMembersSort('email')}>{$t('workspace_settings.teams.col_email')} <span class="sort-arrow" aria-hidden="true">{membersSortArrow('email')}</span></button>
                </th>
                <th scope="col" aria-sort={membersSortCol === 'role' ? (membersSortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                  <button class="sort-btn" onclick={() => toggleMembersSort('role')}>{$t('workspace_settings.teams.col_role')} <span class="sort-arrow" aria-hidden="true">{membersSortArrow('role')}</span></button>
                </th>
              </tr>
            </thead>
            <tbody>
              {#each sortedMembers as member}
                <tr data-testid="member-row">
                  <td class="member-name">{member.name ?? member.username ?? '—'}</td>
                  <td class="member-email">{member.email ?? '—'}</td>
                  <td class="member-role">{member.role ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- Budget tab -->
    {:else if activeTab === 'budget'}
      <div class="settings-section" data-testid="budget-tab">
        <h2 class="section-title">{$t('workspace_settings.budget.title')}</h2>

        {#if budgetLoading}
          <p class="loading-text">{$t('workspace_settings.budget.loading')}</p>
        {:else if budgetError}
          <p class="error-text" role="alert">{budgetError}</p>
        {:else if !budget}
          <p class="empty-text">{$t('workspace_settings.budget.empty')}</p>
        {:else}
          <div class="budget-overview" data-testid="budget-overview">
            <div class="budget-stat-row">
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('workspace_settings.budget.token_limit')}</span>
                <span class="budget-stat-value">{budget.config?.max_tokens_per_day ?? $t('workspace_settings.budget.unlimited')}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('workspace_settings.budget.tokens_used')}</span>
                <span class="budget-stat-value">{budget.usage?.tokens_used_today ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">{$t('workspace_settings.budget.cost_today')}</span>
                <span class="budget-stat-value">
                  {budget.usage?.cost_today != null ? `$${budget.usage.cost_today.toFixed(4)}` : '—'}
                </span>
              </div>
            </div>

            {#if budgetPct !== null}
              <div class="budget-bar-wrap">
                <div class="budget-bar-label">
                  <span>{$t('workspace_settings.budget.usage')}</span>
                  <span>{budgetPct}%</span>
                </div>
                <div
                  class="budget-bar-track"
                  role="progressbar"
                  aria-valuenow={budgetPct}
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-label={$t('workspace_settings.budget_used_label', { values: { pct: budgetPct } })}
                  data-testid="budget-bar"
                >
                  <div
                    class="budget-bar-fill"
                    class:bar-danger={budgetPct > 90}
                    class:bar-warn={budgetPct > 70 && budgetPct <= 90}
                    class:bar-ok={budgetPct <= 70}
                    style="width: {budgetPct}%"
                  ></div>
                </div>
              </div>
            {/if}

            {#if budget.usage?.period_start}
              <p class="budget-reset">{$t('workspace_settings.budget.period_started', { values: { date: fmtDate(budget.usage.period_start) } })}</p>
            {/if}

            <div class="budget-edit" data-testid="budget-edit">
              <h3 class="budget-edit-title">{$t('workspace_settings.budget.set_daily_limit')}</h3>
              <div class="budget-edit-row">
                <label for="budget-credits-input" class="budget-edit-label">{$t('workspace_settings.budget.max_tokens_label')}</label>
                <input
                  id="budget-credits-input"
                  class="budget-edit-input"
                  type="number"
                  min="0"
                  step="1"
                  bind:value={budgetEditCredits}
                  placeholder={$t('workspace_settings.budget.placeholder_tokens')}
                  data-testid="budget-credits-input"
                  disabled={budgetSaving}
                />
                <button
                  class="btn-primary budget-save-btn"
                  onclick={saveBudget}
                  disabled={budgetSaving}
                  data-testid="budget-save-btn"
                >
                  {budgetSaving ? $t('workspace_settings.budget.saving') : budgetSaved ? $t('workspace_settings.budget.saved') : $t('workspace_settings.budget.save')}
                </button>
              </div>
              {#if budgetSaveError}
                <p class="error-text" role="alert" data-testid="budget-save-error">{budgetSaveError}</p>
              {/if}
            </div>
          </div>
        {/if}
      </div>

    <!-- Compute tab -->
    {:else if activeTab === 'compute'}
      <div class="settings-section" data-testid="compute-tab">
        <h2 class="section-title">{$t('workspace_settings.compute.title')}</h2>
        <p class="section-desc">{$t('workspace_settings.compute.desc')}</p>

        {#if allComputeLoading}
          <p class="loading-text">{$t('workspace_settings.compute.loading')}</p>
        {:else if allComputeError}
          <p class="error-text" role="alert">{allComputeError}</p>
        {:else if allCompute.length === 0}
          <p class="empty-text">{$t('workspace_settings.compute.empty')}</p>
        {:else}
          <div class="compute-list" data-testid="compute-list">
            {#each allCompute as ct}
              <div class="compute-card" data-testid="compute-card">
                <div class="compute-card-header">
                  <span class="compute-name">{ct.name ?? shortId(ct.id)}</span>
                  {#if ct.kind}
                    <span class="compute-kind">{ct.kind}</span>
                  {/if}
                </div>
                {#if ct.description}
                  <p class="compute-desc">{ct.description}</p>
                {/if}
                {#if ct.id === (defaultComputeTarget || workspace?.default_compute_target)}
                  <span class="compute-default-badge">{$t('workspace_settings.compute.default_badge')}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

    <!-- LLM Config tab -->
    {:else if activeTab === 'llm'}
      <div class="settings-section" data-testid="llm-config-tab">
        <h2 class="section-title">LLM Model Configuration</h2>
        <p class="section-desc">Override the default LLM model and prompt templates for each feature in this workspace. Leave blank to use tenant defaults.</p>

        {#if llmLoading}
          <p class="settings-loading">Loading LLM configuration...</p>
        {:else if llmError}
          <p class="settings-error">{llmError}</p>
        {:else}
          <table class="settings-table">
            <thead>
              <tr>
                <th>Feature</th>
                <th>Model Override</th>
                <th>Custom Prompt</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each LLM_FEATURES as feature}
                {@const cfg = llmConfigs[feature]}
                {@const prompt = llmPrompts[feature]}
                <tr class="settings-row">
                  <td class="cell-feature">
                    <span class="feature-name">{feature.replace(/-/g, ' ')}</span>
                    <span class="feature-desc">
                      {feature === 'briefing-ask' ? 'Workspace Q&A and briefing' :
                       feature === 'spec-assist' ? 'LLM-assisted spec editing' :
                       feature === 'explorer-generate' ? 'Auto-generate graph views' :
                       feature === 'graph-predict' ? 'Structural code predictions' : ''}
                    </span>
                  </td>
                  <td>
                    {#if cfg?.model_name}
                      <code class="mono">{cfg.model_name}</code>
                      {#if cfg.max_tokens}
                        <span class="meta-hint">max {cfg.max_tokens} tokens</span>
                      {/if}
                    {:else}
                      <span class="meta-hint">default</span>
                    {/if}
                  </td>
                  <td>
                    {#if prompt?.content}
                      <span class="prompt-preview" title={prompt.content}>{prompt.content.slice(0, 40)}...</span>
                    {:else}
                      <span class="meta-hint">default</span>
                    {/if}
                  </td>
                  <td class="cell-actions">
                    <button class="action-btn" onclick={() => editLlmFeature(feature)}>Edit</button>
                    {#if cfg?.model_name || prompt?.content}
                      <button class="action-btn action-btn-danger" onclick={() => resetLlmFeature(feature)}>Reset</button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>

          {#if llmEditFeature}
            <div class="llm-edit-form">
              <h3 class="form-heading">Edit: {llmEditFeature.replace(/-/g, ' ')}</h3>
              <div class="field">
                <label class="field-label" for="llm-model">Model name</label>
                <input id="llm-model" class="field-input" bind:value={llmEditModel} placeholder="e.g. claude-sonnet-4-20250514" />
                <p class="field-hint">Leave empty to use tenant default</p>
              </div>
              <div class="field">
                <label class="field-label" for="llm-tokens">Max tokens</label>
                <input id="llm-tokens" class="field-input" type="number" bind:value={llmEditMaxTokens} placeholder="e.g. 4096" />
              </div>
              <div class="field">
                <label class="field-label" for="llm-prompt">Custom prompt template</label>
                <textarea id="llm-prompt" class="field-textarea" rows="5" bind:value={llmEditPrompt} placeholder="Leave empty to use default prompt. Use {{context}} and {{question}} as placeholders."></textarea>
              </div>
              <div class="form-actions">
                <button class="action-btn" onclick={() => { llmEditFeature = null; }}>Cancel</button>
                <button class="action-btn action-btn-primary" onclick={saveLlmConfig} disabled={llmSaving}>
                  {llmSaving ? 'Saving...' : llmSaved ? 'Saved!' : 'Save'}
                </button>
              </div>
            </div>
          {/if}
        {/if}
      </div>

    <!-- Audit tab -->
    {:else if activeTab === 'audit'}
      <div class="settings-section" data-testid="audit-tab">
        <h2 class="section-title">{$t('workspace_settings.audit.title')}</h2>

        <div class="audit-filter-bar">
          <select
            class="filter-select"
            bind:value={auditFilterType}
            onchange={() => loadAudit(workspace?.id)}
            aria-label={$t('workspace_settings.audit.filter_label')}
            data-testid="audit-filter-select"
          >
            <option value="">{$t('workspace_settings.audit.all_event_types')}</option>
            {#each AUDIT_EVENT_TYPES as et}
              <option value={et}>{et}</option>
            {/each}
          </select>
          <button
            class="btn-secondary"
            onclick={() => loadAudit(workspace?.id)}
            disabled={auditLoading}
            data-testid="audit-refresh-btn"
          >
            {auditLoading ? $t('workspace_settings.audit.loading_btn') : $t('workspace_settings.audit.refresh')}
          </button>
        </div>

        {#if auditLoading}
          <p class="loading-text">{$t('workspace_settings.audit.loading')}</p>
        {:else if auditError}
          <p class="error-text" role="alert">{auditError}</p>
        {:else if auditEvents.length === 0}
          <p class="empty-text">{$t('workspace_settings.audit.empty')}</p>
        {:else}
          <div class="audit-list" data-testid="audit-list">
            <div class="audit-row audit-header">
              <button class="audit-sort-btn" onclick={() => toggleAuditSort('event_type')}>{$t('workspace_settings.audit.col_type')}{auditSortCol === 'event_type' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" onclick={() => toggleAuditSort('actor')}>{$t('workspace_settings.audit.col_actor')}{auditSortCol === 'actor' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" onclick={() => toggleAuditSort('details')}>{$t('workspace_settings.audit.col_detail')}{auditSortCol === 'details' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" onclick={() => toggleAuditSort('timestamp')}>{$t('workspace_settings.audit.col_time')}{auditSortCol === 'timestamp' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
            </div>
            {#each sortedAuditEvents as evt}
              <div class="audit-row" data-testid="audit-row">
                <span class="audit-type">{evt.event_type ?? evt.type ?? '—'}</span>
                <span class="audit-actor">{evt.actor ?? evt.user_id ?? '—'}</span>
                <span class="audit-detail">{evt.details ?? evt.message ?? ''}</span>
                <span class="audit-time">{fmtDate(evt.timestamp ?? evt.created_at)}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .ws-settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--color-bg);
  }

  /* ── Page header ──────────────────────────────────────────────────── */
  .page-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    border-radius: var(--radius);
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

  .page-title-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  /* ── Tab bar ──────────────────────────────────────────────────────── */
  .settings-tab-bar {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .settings-tab-btn {
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

  .settings-tab-btn:hover { color: var(--color-text); }

  .settings-tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .settings-tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Content area ─────────────────────────────────────────────────── */
  .settings-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
  }

  .settings-content:focus { outline: none; }
  .settings-content:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .settings-section {
    max-width: 640px;
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .section-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    margin-top: calc(-1 * var(--space-4));
  }

  /* ── Sub-sections ─────────────────────────────────────────────────── */
  .sub-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5) var(--space-6);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
  }

  .sub-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .sub-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    margin-top: calc(-1 * var(--space-2));
  }

  /* ── Fields ───────────────────────────────────────────────────────── */
  .field-group {
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
    padding: var(--space-2) 0;
  }

  .field-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    font-style: italic;
  }

  .field-select {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .field-select:focus:not(:focus-visible) { outline: none; }
  .field-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .field-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    width: 80px;
  }

  .field-input:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .field-loading {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  /* ── Trust grid ───────────────────────────────────────────────────── */
  .trust-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--space-3);
  }

  .trust-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: 2px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .trust-card:hover { border-color: var(--color-text-muted); }

  .trust-card.selected {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, var(--color-surface-elevated));
  }

  .trust-label {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .trust-desc {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  /* ── Policy list ──────────────────────────────────────────────────── */
  .policy-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .policy-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    flex-wrap: wrap;
  }

  .policy-name {
    font-weight: 500;
    color: var(--color-text);
    flex: 1;
    min-width: 120px;
  }

  .policy-effect {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
  }

  .policy-effect-allow {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .policy-effect-deny {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  .policy-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    width: 100%;
  }

  .policy-delete-btn {
    margin-left: auto;
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    flex-shrink: 0;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .policy-delete-btn:hover:not(:disabled) {
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .policy-delete-btn:disabled { opacity: 0.6; cursor: not-allowed; }
  .policy-delete-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* ── Toggle group ─────────────────────────────────────────────────── */
  .toggle-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

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

  /* ── Members table ────────────────────────────────────────────────── */
  .members-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .members-table th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0;
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
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

  .members-table tbody tr {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }

  .members-table tbody tr:last-child { border-bottom: none; }
  .members-table tbody tr:hover { background: var(--color-surface-elevated); }

  .members-table td {
    padding: var(--space-3) var(--space-4);
    vertical-align: middle;
  }

  .member-name { color: var(--color-text); font-weight: 500; }
  .member-email { color: var(--color-text-secondary); }
  .member-role {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    text-transform: capitalize;
  }

  /* ── Budget ───────────────────────────────────────────────────────── */
  .budget-overview {
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

  .budget-reset {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .budget-edit {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-4);
    margin-top: var(--space-4);
  }

  .budget-edit-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3) 0;
  }

  .budget-edit-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .budget-edit-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .budget-edit-input {
    flex: 1;
    min-width: 120px;
    max-width: 220px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }

  .budget-edit-input:focus {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .budget-save-btn {
    padding: var(--space-2) var(--space-4);
    font-size: var(--text-sm);
    white-space: nowrap;
  }

  /* ── Compute list ─────────────────────────────────────────────────── */
  .compute-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .compute-card {
    padding: var(--space-4) var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .compute-card-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .compute-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .compute-kind {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-family: var(--font-mono);
  }

  .compute-desc {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .compute-default-badge {
    font-size: var(--text-xs);
    color: var(--color-primary);
    font-weight: 500;
  }

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
    transition: border-color var(--transition-fast);
    min-width: 200px;
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
    grid-template-columns: 160px 120px 1fr auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
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
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .audit-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    font-family: var(--font-mono);
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

  /* ── Accessibility ────────────────────────────────────────────────── */
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  /* ── Responsive ───────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .settings-tab-bar { padding: 0 var(--space-3); }
    .settings-content { padding: var(--space-4); }
    .trust-grid { grid-template-columns: 1fr; }
    .audit-row { grid-template-columns: 1fr 1fr; grid-template-rows: auto auto; }
    .budget-stat-row { grid-template-columns: 1fr; }
  }

  /* ── LLM Config ─────────────────────────────────────────────────────── */
  .cell-feature { display: flex; flex-direction: column; gap: 2px; }
  .feature-name { font-weight: 600; text-transform: capitalize; }
  .feature-desc { font-size: var(--text-xs); color: var(--color-text-muted); }
  .prompt-preview { font-size: var(--text-xs); color: var(--color-text-secondary); font-family: var(--font-mono); }
  .meta-hint { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; }
  .cell-actions { display: flex; gap: var(--space-2); white-space: nowrap; }
  .action-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
  }
  .action-btn:hover { background: var(--color-surface-elevated); }
  .action-btn-primary { background: var(--color-primary); color: white; border-color: var(--color-primary); }
  .action-btn-primary:hover { opacity: 0.9; }
  .action-btn-danger { color: var(--color-danger); border-color: var(--color-danger); background: transparent; }
  .action-btn-danger:hover { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .llm-edit-form {
    margin-top: var(--space-4);
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
  }
  .form-heading { font-size: var(--text-sm); font-weight: 600; margin: 0 0 var(--space-3); text-transform: capitalize; }
  .form-actions { display: flex; gap: var(--space-2); justify-content: flex-end; margin-top: var(--space-3); }

  @media (prefers-reduced-motion: reduce) {
    .settings-tab-btn,
    .back-btn,
    .trust-card,
    .btn-primary,
    .btn-secondary,
    .budget-bar-fill,
    .field-select,
    .filter-select { transition: none; }
  }
</style>
