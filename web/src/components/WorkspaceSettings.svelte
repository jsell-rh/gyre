<script>
  /**
   * WorkspaceSettings — full-page workspace settings (§2 Workspace Settings of ui-navigation.md)
   *
   * Tabs: General | Trust & Policies | Teams | Budget | Compute | Audit
   * Accessed via gear icon ⚙ in workspace header or /workspaces/:slug/settings URL.
   */
  import { untrack } from 'svelte';
  import { api } from '../lib/api.js';

  let {
    workspace = null,
    onBack = undefined,
  } = $props();

  const TABS = [
    { id: 'general',  label: 'General' },
    { id: 'trust',    label: 'Trust & Policies' },
    { id: 'teams',    label: 'Teams' },
    { id: 'budget',   label: 'Budget' },
    { id: 'compute',  label: 'Compute' },
    { id: 'audit',    label: 'Audit' },
  ];

  let activeTab = $state('general');

  // ── General ──────────────────────────────────────────────────────────
  // (display-only for name/description; compute target selector)
  let computeTargets = $state([]);
  let computeLoading = $state(false);
  let defaultComputeTarget = $state(workspace?.default_compute_target ?? '');
  let generalSaving = $state(false);
  let generalSaved = $state(false);

  // ── Trust & Policies ─────────────────────────────────────────────────
  const TRUST_LEVELS = [
    { id: 'Supervised', label: 'Supervised', desc: 'I review everything before it merges' },
    { id: 'Guided',     label: 'Guided',     desc: 'Agents merge if gates pass, alert me on failures' },
    { id: 'Autonomous', label: 'Autonomous', desc: 'Only interrupt me for exceptions' },
    { id: 'Custom',     label: 'Custom',     desc: 'Configure policies manually' },
  ];
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
  let policyDriftSaving = $state(false);
  let policyDriftSaved = $state(false);

  // ── Teams ─────────────────────────────────────────────────────────────
  let members = $state([]);
  let membersLoading = $state(false);
  let membersError = $state(null);

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

  const AUDIT_EVENT_TYPES = [
    'spec_approved', 'spec_revoked', 'gate_override', 'trust_changed',
    'agent_spawned', 'agent_stopped', 'policy_evaluated', 'member_added', 'member_removed',
  ];

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
      budgetEditCredits = String(budget?.total_credits ?? '');
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
      budgetSaveError = 'Enter a valid non-negative number.';
      return;
    }
    budgetSaving = true;
    budgetSaved = false;
    budgetSaveError = null;
    try {
      budget = await api.setWorkspaceBudget(wsId, { total_credits: total });
      budgetEditCredits = String(budget?.total_credits ?? total);
      budgetSaved = true;
      setTimeout(() => { budgetSaved = false; }, 2000);
    } catch (e) {
      budgetSaveError = e?.message ?? 'Failed to save budget.';
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
    } catch { /* ignore */ }
    finally { generalSaving = false; }
  }

  async function saveTrustLevel() {
    if (!workspace?.id) return;
    trustSaving = true;
    try {
      await api.updateWorkspace(workspace.id, { trust_level: trustLevel });
      trustSaved = true;
      setTimeout(() => { trustSaved = false; }, 2000);
    } catch { /* ignore */ }
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
    } catch { /* ignore */ }
    finally { policyDriftSaving = false; }
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
    const used = budget.used_credits ?? 0;
    const total = budget.total_credits ?? 0;
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
      aria-label="Back to workspace home"
      data-testid="ws-settings-back"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 5l-7 7 7 7"/>
      </svg>
    </button>
    <div class="page-title-group">
      <h1 class="page-title" data-testid="ws-settings-title">{workspace?.name ?? 'Workspace'} Settings</h1>
    </div>
  </div>

  <!-- ── Tab bar ────────────────────────────────────────────────────── -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="settings-tab-bar" role="tablist" aria-label="Workspace settings sections" data-testid="ws-settings-tabs" onkeydown={handleTabKeydown}>
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
        <h2 class="section-title">General</h2>

        <div class="field-group">
          <div class="field">
            <label class="field-label">Workspace Name</label>
            <div class="field-value" data-testid="ws-name">{workspace?.name ?? '—'}</div>
            <p class="field-hint">Contact your administrator to rename the workspace.</p>
          </div>

          <div class="field">
            <label class="field-label">Description</label>
            <div class="field-value">{workspace?.description ?? '—'}</div>
          </div>

          <div class="field">
            <label class="field-label" for="compute-target-select">Default Compute Target</label>
            {#if computeLoading}
              <div class="field-loading">Loading targets…</div>
            {:else}
              <select
                id="compute-target-select"
                class="field-select"
                bind:value={defaultComputeTarget}
                data-testid="compute-target-select"
              >
                <option value="">— None (use tenant default) —</option>
                {#each computeTargets as ct}
                  <option value={ct.id}>{ct.name ?? ct.id}</option>
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
            {#if generalSaving}Saving…{:else if generalSaved}Saved ✓{:else}Save{/if}
          </button>
        </div>
      </div>

    <!-- Trust & Policies tab -->
    {:else if activeTab === 'trust'}
      <div class="settings-section" data-testid="trust-tab">
        <h2 class="section-title">Trust & Policies</h2>

        <!-- Trust level -->
        <div class="sub-section">
          <h3 class="sub-title">Trust Level</h3>
          <p class="sub-desc">Controls how autonomously agents operate in this workspace.</p>

          <div class="trust-grid" role="radiogroup" aria-label="Trust level">
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
              {#if trustSaving}Saving…{:else if trustSaved}Saved ✓{:else}Save Trust Level{/if}
            </button>
          </div>
        </div>

        <!-- ABAC Policies -->
        <div class="sub-section">
          <h3 class="sub-title">ABAC Policies</h3>
          {#if policiesLoading}
            <p class="loading-text">Loading policies…</p>
          {:else if abacPolicies.length === 0}
            <p class="empty-text">No ABAC policies configured for this workspace.</p>
          {:else}
            <div class="policy-list" data-testid="abac-policy-list">
              {#each abacPolicies as policy}
                <div class="policy-row">
                  <span class="policy-name">{policy.name ?? policy.id}</span>
                  <span class="policy-effect policy-effect-{(policy.effect ?? 'allow').toLowerCase()}">
                    {policy.effect ?? 'allow'}
                  </span>
                  {#if policy.description}
                    <span class="policy-desc">{policy.description}</span>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <!-- MetaSpec Drift Policy -->
        <div class="sub-section">
          <h3 class="sub-title">Meta-Spec Drift Policy</h3>
          <p class="sub-desc">Controls how the workspace responds when agents produce code under outdated meta-spec versions.</p>

          <div class="toggle-group" data-testid="drift-policy-toggles">
            <label class="toggle-row">
              <input type="checkbox" bind:checked={warnOnDrift} data-testid="toggle-warn-on-drift" />
              <span class="toggle-label">
                <span class="toggle-name">Warn on drift</span>
                <span class="toggle-hint">Show a warning when code is produced under an older meta-spec version.</span>
              </span>
            </label>

            <label class="toggle-row">
              <input type="checkbox" bind:checked={blockOnDrift} data-testid="toggle-block-on-drift" />
              <span class="toggle-label">
                <span class="toggle-name">Block on drift</span>
                <span class="toggle-hint">Block merges when the code was produced under an older meta-spec version.</span>
              </span>
            </label>

            <div class="field">
              <label class="field-label" for="drift-tolerance-input">Drift tolerance (versions behind)</label>
              <input
                id="drift-tolerance-input"
                class="field-input"
                type="number"
                min="0"
                max="10"
                bind:value={driftTolerance}
                data-testid="drift-tolerance-input"
              />
              <p class="field-hint">Allow agents to be this many versions behind before triggering warn/block.</p>
            </div>
          </div>

          <div class="action-row">
            <button
              class="btn-primary"
              onclick={saveDriftPolicy}
              disabled={policyDriftSaving}
              data-testid="save-drift-policy-btn"
            >
              {#if policyDriftSaving}Saving…{:else if policyDriftSaved}Saved ✓{:else}Save Drift Policy{/if}
            </button>
          </div>
        </div>
      </div>

    <!-- Teams tab -->
    {:else if activeTab === 'teams'}
      <div class="settings-section" data-testid="teams-tab">
        <h2 class="section-title">Teams</h2>

        {#if membersLoading}
          <p class="loading-text">Loading members…</p>
        {:else if membersError}
          <p class="error-text" role="alert">{membersError}</p>
        {:else if members.length === 0}
          <p class="empty-text">No members found in this workspace.</p>
        {:else}
          <table class="members-table" data-testid="members-table">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Email</th>
                <th scope="col">Role</th>
              </tr>
            </thead>
            <tbody>
              {#each members as member}
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
        <h2 class="section-title">Budget</h2>

        {#if budgetLoading}
          <p class="loading-text">Loading budget…</p>
        {:else if budgetError}
          <p class="error-text" role="alert">{budgetError}</p>
        {:else if !budget}
          <p class="empty-text">No budget data available for this workspace.</p>
        {:else}
          <div class="budget-overview" data-testid="budget-overview">
            <div class="budget-stat-row">
              <div class="budget-stat">
                <span class="budget-stat-label">Total Credits</span>
                <span class="budget-stat-value">{budget.total_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">Used Credits</span>
                <span class="budget-stat-value">{budget.used_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">Remaining</span>
                <span class="budget-stat-value">
                  {budget.total_credits != null && budget.used_credits != null
                    ? budget.total_credits - budget.used_credits
                    : '—'}
                </span>
              </div>
            </div>

            {#if budgetPct !== null}
              <div class="budget-bar-wrap">
                <div class="budget-bar-label">
                  <span>Usage</span>
                  <span>{budgetPct}%</span>
                </div>
                <div
                  class="budget-bar-track"
                  role="progressbar"
                  aria-valuenow={budgetPct}
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-label="Budget {budgetPct}% used"
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

            {#if budget.reset_at}
              <p class="budget-reset">Resets: {fmtDate(budget.reset_at)}</p>
            {/if}

            <div class="budget-edit" data-testid="budget-edit">
              <h3 class="budget-edit-title">Set Total Credits</h3>
              <div class="budget-edit-row">
                <label for="budget-credits-input" class="budget-edit-label">Total Credits</label>
                <input
                  id="budget-credits-input"
                  class="budget-edit-input"
                  type="number"
                  min="0"
                  step="1"
                  bind:value={budgetEditCredits}
                  placeholder="e.g. 10000"
                  data-testid="budget-credits-input"
                  disabled={budgetSaving}
                />
                <button
                  class="btn-primary budget-save-btn"
                  onclick={saveBudget}
                  disabled={budgetSaving}
                  data-testid="budget-save-btn"
                >
                  {budgetSaving ? 'Saving…' : budgetSaved ? 'Saved ✓' : 'Save'}
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
        <h2 class="section-title">Compute Targets</h2>
        <p class="section-desc">Compute targets available for agents in this workspace, configured by your tenant.</p>

        {#if allComputeLoading}
          <p class="loading-text">Loading compute targets…</p>
        {:else if allComputeError}
          <p class="error-text" role="alert">{allComputeError}</p>
        {:else if allCompute.length === 0}
          <p class="empty-text">No compute targets configured. Contact your tenant administrator.</p>
        {:else}
          <div class="compute-list" data-testid="compute-list">
            {#each allCompute as ct}
              <div class="compute-card" data-testid="compute-card">
                <div class="compute-card-header">
                  <span class="compute-name">{ct.name ?? ct.id}</span>
                  {#if ct.kind}
                    <span class="compute-kind">{ct.kind}</span>
                  {/if}
                </div>
                {#if ct.description}
                  <p class="compute-desc">{ct.description}</p>
                {/if}
                {#if ct.id === (defaultComputeTarget || workspace?.default_compute_target)}
                  <span class="compute-default-badge">Default for this workspace</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>

    <!-- Audit tab -->
    {:else if activeTab === 'audit'}
      <div class="settings-section" data-testid="audit-tab">
        <h2 class="section-title">Audit Log</h2>

        <div class="audit-filter-bar">
          <select
            class="filter-select"
            bind:value={auditFilterType}
            aria-label="Filter by event type"
            data-testid="audit-filter-select"
          >
            <option value="">All event types</option>
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
            {auditLoading ? 'Loading…' : 'Refresh'}
          </button>
        </div>

        {#if auditLoading}
          <p class="loading-text">Loading audit events…</p>
        {:else if auditError}
          <p class="error-text" role="alert">{auditError}</p>
        {:else if auditEvents.length === 0}
          <p class="empty-text">No audit events found for this workspace.</p>
        {:else}
          <div class="audit-list" data-testid="audit-list">
            {#each auditEvents as evt}
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
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
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
