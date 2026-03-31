<script>
  /**
   * RepoSettings — repo settings tab content (§3 Tab: ⚙ Settings of ui-navigation.md)
   *
   * Tabs: General | Gates | Policies | Budget | Audit | Danger Zone
   * Rendered inside RepoMode when the ⚙ tab is active.
   */
  import { untrack } from 'svelte';
  import { api } from '../lib/api.js';

  let {
    workspace = null,
    repo = null,
  } = $props();

  const TABS = [
    { id: 'general',     label: 'General' },
    { id: 'gates',       label: 'Gates' },
    { id: 'policies',    label: 'Policies' },
    { id: 'budget',      label: 'Budget' },
    { id: 'audit',       label: 'Audit' },
    { id: 'danger-zone', label: 'Danger Zone' },
  ];

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
    `This will archive ${repo?.name ?? 'this repo'}. Agents will be stopped and new agents cannot be spawned.`
  );
  const deleteConfirmRequired = $derived(repo?.name ?? '');
  const deleteReady = $derived(deleteConfirmName === deleteConfirmRequired && deleteConfirmRequired !== '');

  // ── Data loading driven by tab ─────────────────────────────────────────
  $effect(() => {
    const repoId = repo?.id;
    if (!repoId) return;

    if (activeTab === 'gates') {
      if (untrack(() => gates.length === 0 && !gatesLoading)) loadGates(repoId);
    }
    if (activeTab === 'policies') {
      if (untrack(() => !specPolicy && !specPolicyLoading)) loadSpecPolicy(repoId);
    }
    if (activeTab === 'budget') {
      if (untrack(() => !repoBudget && !repoBudgetLoading)) loadRepoBudget(repoId);
    }
    if (activeTab === 'audit') {
      loadAudit(repoId);
    }
  });

  async function loadGates(repoId) {
    gatesLoading = true;
    gatesError = null;
    try {
      gates = await api.repoGates(repoId) ?? [];
    } catch (e) {
      gatesError = e.message;
      gates = [];
    }
    finally { gatesLoading = false; }
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
    } catch { /* ignore */ }
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
  <div class="inner-tab-bar" role="tablist" aria-label="Repo settings sections" data-testid="repo-settings-tabs" onkeydown={handleTabKeydown}>
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
        <h2 class="tab-title">General</h2>

        <div class="field-card">
          <div class="field">
            <span class="field-label">Repository Name</span>
            <div class="field-value" data-testid="repo-name-display">{repo?.name ?? '—'}</div>
            <p class="field-hint">Repository names cannot be changed after creation.</p>
          </div>

          <div class="field">
            <label class="field-label" for="repo-desc-input">Description</label>
            <textarea
              id="repo-desc-input"
              class="field-textarea"
              rows="3"
              placeholder="Describe this repository…"
              bind:value={repoDescription}
              data-testid="repo-desc-input"
            ></textarea>
          </div>

          <div class="field">
            <label class="field-label" for="repo-branch-input">Default Branch</label>
            <input
              id="repo-branch-input"
              class="field-input"
              type="text"
              placeholder="main"
              bind:value={repoDefaultBranch}
              data-testid="repo-branch-input"
            />
          </div>

          <div class="field">
            <label class="field-label" for="repo-max-agents-input">Max Concurrent Agents</label>
            <input
              id="repo-max-agents-input"
              class="field-input field-input-sm"
              type="number"
              min="1"
              max="50"
              bind:value={repoMaxConcurrent}
              data-testid="repo-max-agents-input"
            />
            <p class="field-hint">Maximum number of agents allowed to work on this repo simultaneously.</p>
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
            {#if generalSaving}Saving…{:else if generalSaved}Saved ✓{:else}Save Changes{/if}
          </button>
        </div>
      </div>

    <!-- Gates tab -->
    {:else if activeTab === 'gates'}
      <div class="tab-body" data-testid="repo-gates-tab">
        <h2 class="tab-title">Gates</h2>
        <p class="tab-desc">Gate chain configuration — the checks agents must pass before merging.</p>

        {#if gatesLoading}
          <p class="loading-text">Loading gates…</p>
        {:else if gatesError}
          <p class="error-text" role="alert">{gatesError}</p>
        {:else if gates.length === 0}
          <p class="empty-text">No gates configured for this repository.</p>
        {:else}
          <div class="gates-list" data-testid="gates-list">
            {#each gates as gate}
              <div class="gate-card" data-testid="gate-card">
                <div class="gate-header">
                  <span class="gate-name">{gate.name ?? gate.id}</span>
                  {#if gate.kind}
                    <span class="gate-kind">{gate.kind}</span>
                  {/if}
                  {#if gate.required !== undefined}
                    <span class="gate-required" class:required={gate.required}>
                      {gate.required ? 'Required' : 'Optional'}
                    </span>
                  {/if}
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
      </div>

    <!-- Policies tab -->
    {:else if activeTab === 'policies'}
      <div class="tab-body" data-testid="repo-policies-tab">
        <h2 class="tab-title">Policies</h2>
        <p class="tab-desc">Spec enforcement and merge policies for this repository.</p>

        {#if specPolicyLoading}
          <p class="loading-text">Loading policies…</p>
        {:else if specPolicyError}
          <p class="error-text" role="alert">{specPolicyError}</p>
        {:else if !specPolicy}
          <p class="empty-text">No spec policy configured for this repository.</p>
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
                  <span class="toggle-name">Require spec reference</span>
                  <span class="toggle-hint">MRs must reference a spec before merging.</span>
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
                  <span class="toggle-name">Require spec approval</span>
                  <span class="toggle-hint">Specs must be approved before agents can implement them.</span>
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
                  <span class="toggle-name">Stale spec warning</span>
                  <span class="toggle-hint">Warn when a spec has not been updated in 30+ days.</span>
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
              {#if specPolicySaving}Saving…{:else if specPolicySaved}Saved ✓{:else}Save Policies{/if}
            </button>
          </div>
        {/if}
      </div>

    <!-- Budget tab -->
    {:else if activeTab === 'budget'}
      <div class="tab-body" data-testid="repo-budget-tab">
        <h2 class="tab-title">Budget</h2>
        <p class="tab-desc">Credit allocation for agents in this repository. Cannot exceed the workspace budget.</p>

        {#if repoBudgetLoading}
          <p class="loading-text">Loading budget…</p>
        {:else if repoBudgetError}
          <p class="empty-text" data-testid="budget-unavailable">Budget data unavailable — this feature may not be configured.</p>
        {:else if !repoBudget}
          <p class="empty-text">No budget allocation configured for this repository.</p>
        {:else}
          <div class="budget-card" data-testid="repo-budget-card">
            <div class="budget-stat-row">
              <div class="budget-stat">
                <span class="budget-stat-label">Allocated</span>
                <span class="budget-stat-value">{repoBudget.total_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">Used</span>
                <span class="budget-stat-value">{repoBudget.used_credits ?? '—'}</span>
              </div>
              <div class="budget-stat">
                <span class="budget-stat-label">Remaining</span>
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
                  <span>Usage</span>
                  <span>{repoBudgetPct}%</span>
                </div>
                <div
                  class="budget-bar-track"
                  role="progressbar"
                  aria-valuenow={repoBudgetPct}
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-label="Budget {repoBudgetPct}% used"
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

    <!-- Audit tab -->
    {:else if activeTab === 'audit'}
      <div class="tab-body" data-testid="repo-audit-tab">
        <h2 class="tab-title">Audit Log</h2>

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
            onclick={() => loadAudit(repo?.id)}
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
          <p class="empty-text">No audit events found for this repository.</p>
        {:else}
          <div class="audit-list" data-testid="repo-audit-list">
            <div class="audit-row audit-header">
              <button class="audit-sort-btn" aria-sort={auditSortCol === 'event_type' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'} onclick={() => toggleAuditSort('event_type')}>Type{auditSortCol === 'event_type' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-sort={auditSortCol === 'actor' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'} onclick={() => toggleAuditSort('actor')}>Actor{auditSortCol === 'actor' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-sort={auditSortCol === 'details' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'} onclick={() => toggleAuditSort('details')}>Detail{auditSortCol === 'details' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
              <button class="audit-sort-btn" aria-sort={auditSortCol === 'timestamp' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'} onclick={() => toggleAuditSort('timestamp')}>Time{auditSortCol === 'timestamp' ? (auditSortDir === 1 ? ' ↑' : ' ↓') : ''}</button>
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

    <!-- Danger Zone tab -->
    {:else if activeTab === 'danger-zone'}
      <div class="tab-body" data-testid="repo-danger-tab">
        <h2 class="tab-title danger-title">Danger Zone</h2>

        <!-- Archive -->
        <div class="danger-card" data-testid="archive-section">
          <div class="danger-card-content">
            <div class="danger-card-info">
              <h3 class="danger-card-title">Archive Repository</h3>
              <p class="danger-card-desc">
                Archiving stops all active agents and prevents new agent spawns.
                The repository remains readable but no further development work can proceed.
                You can unarchive at any time.
              </p>
            </div>
            <button
              class="btn-danger"
              onclick={() => { archiveConfirm = !archiveConfirm; deleteConfirmName = ''; }}
              data-testid="archive-btn"
            >
              Archive
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
                  Cancel
                </button>
                <button
                  class="btn-danger"
                  onclick={archiveRepo}
                  disabled={archiving}
                  data-testid="archive-confirm-btn"
                >
                  {archiving ? 'Archiving…' : 'Confirm Archive'}
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
                <h3 class="danger-card-title">Delete Repository</h3>
                <p class="danger-card-desc">
                  Permanently deletes this repository and all associated data including specs,
                  agents, merge requests, and history.
                  <strong>This action cannot be undone.</strong>
                </p>
                <p class="danger-card-prereq" data-testid="delete-archive-required">
                  Archive this repository first before deleting it.
                </p>
              </div>
              <button
                class="btn-danger"
                disabled
                data-testid="delete-btn"
                title="Archive this repository first"
              >
                Delete
              </button>
            </div>
          {:else}
            <!-- Repo is archived — deletion is allowed -->
            <div class="danger-card-content">
              <div class="danger-card-info">
                <h3 class="danger-card-title">Delete Repository</h3>
                <p class="danger-card-desc">
                  Permanently deletes this repository and all associated data including specs,
                  agents, merge requests, and history.
                  <strong>This action cannot be undone.</strong>
                </p>
              </div>
              <button
                class="btn-danger"
                onclick={() => { deleteConfirmName = ''; archiveConfirm = false; deleteError = null; }}
                data-testid="delete-btn"
              >
                Delete
              </button>
            </div>

            {#if deleteConfirmName !== undefined && !archiveConfirm}
              <div class="confirm-box" data-testid="delete-confirm-box">
                <p class="confirm-msg">
                  To confirm deletion, type the repository name:
                  <strong>{deleteConfirmRequired}</strong>
                </p>
                <input
                  class="confirm-input"
                  type="text"
                  placeholder={deleteConfirmRequired}
                  bind:value={deleteConfirmName}
                  aria-label="Type repository name to confirm deletion"
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
                    Cancel
                  </button>
                  <button
                    class="btn-danger"
                    onclick={deleteRepo}
                    disabled={!deleteReady || deleting}
                    data-testid="delete-confirm-btn"
                  >
                    {deleting ? 'Deleting…' : 'Delete Repository'}
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

  .audit-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    font-family: var(--font-mono);
  }

  /* ── Danger Zone ──────────────────────────────────────────────────── */
  .danger-title { color: var(--color-danger); }

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
</style>
