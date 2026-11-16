<script>
  /**
   * PolicyManager — ABAC policy management surface (abac-policy-engine.md
   * §UI): scope-filtered list with create/edit/delete, conflict detection,
   * dry-run simulator, decision audit log, and effective permissions view.
   *
   * URL pattern (spec §UI): ALL calls go to the GLOBAL policy endpoints —
   * `GET /api/v1/policies?scope=Workspace&scope_id=<id>` — workspace (or
   * repo/tenant) scoping is a query-parameter filter on the global CRUD,
   * never a workspace-prefixed resource URL (those return 405).
   */
  import { api } from '../../lib/api.js';
  import { detectPolicyConflicts } from '../../lib/abac.js';
  import PolicyEditor from './PolicyEditor.svelte';
  import PolicySimulator from './PolicySimulator.svelte';
  import PolicyDecisionLog from './PolicyDecisionLog.svelte';
  import EffectivePermissions from './EffectivePermissions.svelte';

  let {
    initialScope = 'tenant',   // 'tenant' | 'workspace' | 'repo'
    initialScopeId = null,
    lockScope = false,         // embedding settings page fixes the scope
    heading = 'ABAC Policies',
  } = $props();

  const VIEWS = [
    { id: 'policies', label: 'Policies' },
    { id: 'simulator', label: 'Dry-Run Simulator' },
    { id: 'decisions', label: 'Decision Log' },
    { id: 'effective', label: 'Effective Permissions' },
  ];

  let scope = $state((initialScope ?? 'tenant').toLowerCase());
  let scopeIdInput = $state(initialScopeId ?? '');
  // The scope filter actually applied (committed by Apply / scope change).
  let appliedScope = $state((initialScope ?? 'tenant').toLowerCase());
  let appliedScopeId = $state(initialScopeId ?? '');

  let policies = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let view = $state('policies');

  let editorOpen = $state(false);
  let editingPolicy = $state(null);
  let deletingPolicyId = $state(null);
  let deleteError = $state(null);

  // Follow the embedding context's scope id until the user edits it
  // (TenantSettings passes the admin's tenant once me() resolves).
  let scopeIdTouched = $state(false);
  $effect(() => {
    if (!scopeIdTouched && initialScopeId != null && initialScopeId !== '') {
      scopeIdInput = initialScopeId;
      appliedScopeId = initialScopeId;
    }
  });

  async function load() {
    loading = true;
    error = null;
    try {
      // Spec §UI URL pattern: `?scope=Workspace&scope_id=<id>` — the
      // capitalized scope spelling on the GLOBAL /policies endpoint
      // (the server accepts either case; task-132 made parse_scope
      // case-insensitive).
      const params = { scope: appliedScope.charAt(0).toUpperCase() + appliedScope.slice(1) };
      if (appliedScopeId && String(appliedScopeId).trim() !== '') {
        params.scope_id = String(appliedScopeId).trim();
      }
      const rows = await api.policies(params);
      policies = Array.isArray(rows) ? rows : [];
    } catch (e) {
      error = e?.message ?? String(e);
      policies = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Load on mount and whenever the committed scope filter changes.
    void appliedScope;
    void appliedScopeId;
    load();
  });

  function commitScopeFilter() {
    appliedScope = scope;
    appliedScopeId = scopeIdInput.trim();
    scopeIdTouched = true;
  }

  function onScopeChange() {
    commitScopeFilter();
  }

  function onApplyFilter() {
    commitScopeFilter();
  }

  // ── Conflict detection (client-side, §UI) ────────────────────────────
  let conflicts = $derived(detectPolicyConflicts(policies));
  let conflictingIds = $derived(new Set(conflicts.flatMap((c) => c.policyIds)));

  // ── CRUD guards mirroring the server's (api/policies.rs) ─────────────
  function isServerManaged(p) {
    const name = p?.name ?? '';
    return p?.built_in === true || p?.immutable === true ||
      name.startsWith('trust:') || name.startsWith('builtin:');
  }

  function managementTooltip(p) {
    const name = p?.name ?? '';
    if (name.startsWith('trust:') || name.startsWith('builtin:')) {
      return ` "${name}" is server-managed (reserved prefix) — use trust-level transitions, not direct edits.`;
    }
    if (p?.built_in) return `"${name}" is a built-in policy and cannot be modified or deleted.`;
    if (p?.immutable) return `"${name}" is immutable and cannot be modified.`;
    return '';
  }

  function openCreate() {
    editingPolicy = null;
    editorOpen = true;
  }

  function openEdit(policy) {
    editingPolicy = policy;
    editorOpen = true;
  }

  async function onEditorSaved() {
    await load();
  }

  async function removePolicy(policy) {
    if (isServerManaged(policy) || deletingPolicyId) return;
    deletingPolicyId = policy.id;
    deleteError = null;
    try {
      await api.deletePolicy(policy.id);
      await load();
    } catch (e) {
      deleteError = e?.message ?? String(e);
    } finally {
      deletingPolicyId = null;
    }
  }

  function fmtScope(p) {
    const s = (p?.scope ?? '').toLowerCase();
    return p?.scope_id ? `${s}:${p.scope_id}` : s;
  }
</script>

<div class="policy-manager" data-testid="policy-manager">
  <div class="manager-header">
    <h3 class="manager-heading" data-testid="policy-manager-heading">{heading}</h3>
    {#if view === 'policies'}
      <button type="button" class="btn-small btn-primary" onclick={openCreate} data-testid="create-policy-btn">
        + Create Policy
      </button>
    {/if}
  </div>

  {#if view === 'policies'}
    <!-- Scope filter — a query filter on the GLOBAL /api/v1/policies CRUD -->
    <div class="scope-bar" data-testid="policy-scope-filter">
      <label class="scope-field">
        <span class="field-label">Scope</span>
        <select
          class="field-input"
          bind:value={scope}
          onchange={onScopeChange}
          disabled={lockScope}
          data-testid="policy-scope-select"
        >
          <option value="tenant">Tenant</option>
          <option value="workspace">Workspace</option>
          <option value="repo">Repo</option>
        </select>
      </label>
      <label class="scope-field">
        <span class="field-label">Scope ID <span class="field-hint">(blank = all at this scope)</span></span>
        <input
          class="field-input"
          bind:value={scopeIdInput}
          disabled={lockScope}
          placeholder="tenant / workspace / repo id"
          onkeydown={(e) => { if (e.key === 'Enter') onApplyFilter(); }}
          data-testid="policy-scope-id-input"
        />
      </label>
      <button type="button" class="btn-small" onclick={onApplyFilter} data-testid="policy-apply-scope-btn">Apply</button>
      <button type="button" class="btn-small" onclick={load} disabled={loading} data-testid="policy-refresh-btn">Refresh</button>
    </div>
  {/if}

  <!-- View switcher -->
  <div class="view-tabs" role="tablist" aria-label="Policy management views" data-testid="policy-view-tabs">
    {#each VIEWS as v (v.id)}
      <button
        type="button"
        role="tab"
        aria-selected={view === v.id}
        class="view-tab"
        class:view-tab-active={view === v.id}
        onclick={() => (view = v.id)}
        data-testid="policy-view-tab-{v.id}"
      >
        {v.label}
      </button>
    {/each}
  </div>

  {#if view === 'policies'}
    {#if deleteError}
      <p class="error-text" role="alert" data-testid="policy-delete-error">{deleteError}</p>
    {/if}
    {#if loading}
      <p class="hint-text" data-testid="policy-list-loading">Loading policies…</p>
    {:else if error}
      <p class="error-text" role="alert" data-testid="policy-list-error">{error}</p>
    {:else if policies.length === 0}
      <p class="hint-text" data-testid="policy-list-empty">No policies at this scope.</p>
    {:else}
      <!-- Conflict detector (§UI): highlight overlapping policies with
           conflicting effects before the raw list. -->
      {#if conflicts.length > 0}
        <div class="conflict-panel" data-testid="policy-conflicts">
          <h4 class="conflict-title">Conflicts detected ({conflicts.length})</h4>
          {#each conflicts as conflict, i (i)}
            <p class="conflict-item" data-testid="policy-conflict-item">{conflict.explanation}</p>
          {/each}
        </div>
      {/if}

      <table class="policies-table" data-testid="policy-table">
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Scope</th>
            <th scope="col">Effect</th>
            <th scope="col">Priority</th>
            <th scope="col">Enabled</th>
            <th scope="col">Actions</th>
          </tr>
        </thead>
        <tbody>
          {#each policies as policy (policy.id ?? policy.name)}
            <tr
              class:conflict-row={conflictingIds.has(policy.id)}
              title={conflictingIds.has(policy.id) ? 'This policy is involved in a detected conflict — see the conflicts panel.' : managementTooltip(policy)}
              data-testid="policy-row"
            >
              <td class="td-name">
                {policy.name ?? policy.id}
                {#if conflictingIds.has(policy.id)}
                  <span class="conflict-badge" title="Involved in a policy conflict">⚠</span>
                {/if}
              </td>
              <td class="mono">{fmtScope(policy)}</td>
              <td>
                <span
                  class="decision-badge"
                  class:decision-allow={(policy.effect ?? '').toLowerCase() === 'allow'}
                  class:decision-deny={(policy.effect ?? '').toLowerCase() === 'deny'}
                >
                  {policy.effect ?? '—'}
                </span>
              </td>
              <td class="mono">{policy.priority ?? '—'}</td>
              <td>{policy.enabled === false ? 'No' : 'Yes'}</td>
              <td class="td-actions">
                <button
                  type="button"
                  class="btn-small"
                  onclick={() => openEdit(policy)}
                  disabled={isServerManaged(policy)}
                  title={managementTooltip(policy)}
                  data-testid="edit-policy-btn"
                >
                  Edit
                </button>
                <button
                  type="button"
                  class="btn-small btn-danger"
                  onclick={() => removePolicy(policy)}
                  disabled={isServerManaged(policy) || deletingPolicyId === policy.id}
                  title={managementTooltip(policy)}
                  data-testid="delete-policy-btn"
                >
                  {deletingPolicyId === policy.id ? '…' : 'Delete'}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if view === 'simulator'}
    <PolicySimulator
      defaultWorkspaceId={appliedScope === 'workspace' ? appliedScopeId : ''}
      defaultRepoId={appliedScope === 'repo' ? appliedScopeId : ''}
    />
  {:else if view === 'decisions'}
    <PolicyDecisionLog />
  {:else if view === 'effective'}
    <EffectivePermissions />
  {/if}

  <PolicyEditor
    bind:open={editorOpen}
    policy={editingPolicy}
    defaultScope={appliedScope}
    defaultScopeId={appliedScopeId}
    lockScope={lockScope}
    onsaved={onEditorSaved}
  />
</div>

<style>
  .policy-manager {
    display: flex;
    flex-direction: column;
    gap: var(--space-3, 12px);
  }

  .manager-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3, 12px);
  }

  .manager-heading {
    font-size: var(--text-md, 15px);
    font-weight: 600;
    margin: 0;
  }

  .scope-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2, 8px);
  }

  .scope-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1, 4px);
  }

  .field-label {
    font-size: var(--text-xs, 12px);
    font-weight: 600;
    color: var(--color-text-muted, #666);
  }

  .field-hint {
    font-weight: 400;
  }

  .field-input {
    padding: 5px 9px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface, #fff);
    color: inherit;
    font: inherit;
    font-size: var(--text-sm, 13px);
    min-width: 0;
  }

  .field-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .view-tabs {
    display: flex;
    gap: var(--space-1, 4px);
    border-bottom: 1px solid var(--color-border, #ddd);
  }

  .view-tab {
    padding: 6px 12px;
    border: none;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--color-text-muted, #666);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-sm, 13px);
  }

  .view-tab-active {
    color: var(--color-text, inherit);
    border-bottom-color: var(--color-primary, #2563eb);
    font-weight: 600;
  }

  .policies-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm, 13px);
  }

  .policies-table th {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 2px solid var(--color-border, #ddd);
    font-weight: 600;
  }

  .policies-table td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--color-border, #eee);
  }

  .td-name {
    font-weight: 500;
  }

  .td-actions {
    display: flex;
    gap: var(--space-1, 4px);
  }

  .conflict-row {
    background: color-mix(in srgb, #f59e0b 8%, transparent);
  }

  .conflict-badge {
    margin-left: 4px;
    color: #d97706;
  }

  .conflict-panel {
    border: 1px solid color-mix(in srgb, #f59e0b 45%, transparent);
    border-radius: var(--radius, 6px);
    padding: var(--space-2, 8px) var(--space-4, 16px);
    background: color-mix(in srgb, #f59e0b 6%, transparent);
  }

  .conflict-title {
    margin: 0 0 var(--space-1, 4px);
    font-size: var(--text-sm, 13px);
    color: #b45309;
  }

  .conflict-item {
    margin: 0 0 var(--space-1, 4px);
    font-size: var(--text-sm, 13px);
  }

  .decision-badge {
    padding: 1px 9px;
    border-radius: 999px;
    font-weight: 600;
    text-transform: capitalize;
  }

  .decision-allow {
    background: color-mix(in srgb, #16a34a 15%, transparent);
    color: #16a34a;
  }

  .decision-deny {
    background: color-mix(in srgb, #dc2626 15%, transparent);
    color: #dc2626;
  }

  .btn-small {
    padding: 5px 12px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface-elevated, #f5f5f5);
    color: inherit;
    cursor: pointer;
    font: inherit;
    font-size: var(--text-sm, 13px);
  }

  .btn-small:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: var(--color-primary, #2563eb);
    border-color: var(--color-primary, #2563eb);
    color: #fff;
  }

  .btn-danger {
    color: var(--color-danger, #c0392b);
  }

  .mono {
    font-family: var(--font-mono, monospace);
  }

  .hint-text {
    color: var(--color-text-muted, #888);
    margin: 0;
  }

  .error-text {
    color: var(--color-danger, #c0392b);
    margin: 0;
  }
</style>
