<script>
  /**
   * EffectivePermissions — "select a user/agent, see what they can do
   * across all resource types" (abac-policy-engine.md §UI).
   *
   * GET /api/v1/policies/effective?subject={id}&resource_type={type}
   * (global endpoint; the server loads the subject's REAL attributes —
   * roles, memberships, teams — from the stores). Queried once per known
   * resource type to build the resource-types × actions view.
   */
  import { api } from '../../lib/api.js';
  import { KNOWN_RESOURCE_TYPES } from '../../lib/abac.js';

  let { defaultSubjectId = '' } = $props();

  const QUERY_RESOURCE_TYPES = KNOWN_RESOURCE_TYPES.filter((r) => r !== '*');

  let subjectId = $state(defaultSubjectId ?? '');
  let loading = $state(false);
  let error = $state(null);
  let rows = $state([]);
  let queried = $state(false);

  let canQuery = $derived(subjectId.trim() !== '' && !loading);

  async function query() {
    if (!canQuery) return;
    loading = true;
    error = null;
    try {
      // One request per resource type: the endpoint answers for a single
      // resource_type, sampling each known action against the live policy
      // set with the subject's real store-loaded attributes.
      const results = await Promise.all(
        QUERY_RESOURCE_TYPES.map(async (resourceType) => {
          const perms = await api.effectivePermissions({
            subject: subjectId.trim(),
            resource_type: resourceType,
          });
          return (Array.isArray(perms) ? perms : []).map((p) => ({
            resource_type: p.resource_type ?? resourceType,
            action: p.action ?? '—',
            decision: (p.decision ?? 'deny').toLowerCase(),
            matched_policy: p.matched_policy ?? null,
          }));
        })
      );
      rows = results.flat();
      queried = true;
    } catch (e) {
      error = e?.message ?? String(e);
      rows = [];
    } finally {
      loading = false;
    }
  }

  let allowedCount = $derived(rows.filter((r) => r.decision === 'allow').length);
</script>

<div class="effective-permissions" data-testid="effective-permissions">
  <p class="hint-text">
    Enter a user or agent id — the server resolves the subject's real attributes (roles, memberships, teams)
    and evaluates every sampled action against the live policy set for each resource type.
  </p>

  <div class="query-row">
    <label class="field">
      <span class="field-label">Subject ID <span class="field-hint">(user or agent)</span></span>
      <input
        class="field-input"
        bind:value={subjectId}
        placeholder="user id / agent id"
        onkeydown={(e) => { if (e.key === 'Enter') query(); }}
        data-testid="effective-subject-input"
      />
    </label>
    <button type="button" class="btn-small btn-primary" onclick={query} disabled={!canQuery} data-testid="effective-query-btn">
      {loading ? 'Checking…' : 'Check permissions'}
    </button>
  </div>

  {#if error}
    <p class="error-text" role="alert" data-testid="effective-error">{error}</p>
  {:else if loading}
    <p class="hint-text" data-testid="effective-loading">Evaluating effective permissions…</p>
  {:else if queried && rows.length === 0}
    <p class="hint-text" data-testid="effective-empty">No results.</p>
  {:else if rows.length > 0}
    <p class="summary" data-testid="effective-summary">
      {allowedCount} of {rows.length} sampled actions allowed.
    </p>
    <table class="perms-table" data-testid="effective-permissions-table">
      <thead>
        <tr>
          <th scope="col">Resource type</th>
          <th scope="col">Action</th>
          <th scope="col">Decision</th>
          <th scope="col">Matched policy</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.resource_type + ':' + row.action)}
          <tr data-testid="effective-permission-row">
            <td class="mono">{row.resource_type}</td>
            <td class="mono">{row.action}</td>
            <td>
              <span
                class="decision-badge"
                class:decision-allow={row.decision === 'allow'}
                class:decision-deny={row.decision === 'deny'}
              >
                {row.decision}
              </span>
            </td>
            <td class="mono">{row.matched_policy ?? '— (default deny)'}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .effective-permissions {
    display: flex;
    flex-direction: column;
    gap: var(--space-3, 12px);
  }

  .hint-text {
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 13px);
    margin: 0;
  }

  .query-row {
    display: flex;
    align-items: flex-end;
    gap: var(--space-3, 12px);
    flex-wrap: wrap;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1, 4px);
  }

  .field-label {
    font-size: var(--text-sm, 13px);
    font-weight: 600;
  }

  .field-hint {
    font-weight: 400;
    color: var(--color-text-muted, #888);
  }

  .field-input {
    padding: 6px 10px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface, #fff);
    color: inherit;
    font: inherit;
    min-width: 260px;
  }

  .btn-small {
    padding: 6px 14px;
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

  .summary {
    margin: 0;
    font-size: var(--text-sm, 13px);
    color: var(--color-text-muted, #666);
  }

  .perms-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm, 13px);
  }

  .perms-table th {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 2px solid var(--color-border, #ddd);
    font-weight: 600;
  }

  .perms-table td {
    padding: 6px 10px;
    border-bottom: 1px solid var(--color-border, #eee);
  }

  .mono {
    font-family: var(--font-mono, monospace);
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

  .error-text {
    color: var(--color-danger, #c0392b);
    margin: 0;
  }
</style>
