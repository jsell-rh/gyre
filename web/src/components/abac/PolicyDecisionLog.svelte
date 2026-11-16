<script>
  /**
   * PolicyDecisionLog — filterable table of all policy evaluations with
   * Allow/Deny outcome (abac-policy-engine.md §UI "Decision audit log").
   *
   * GET /api/v1/policies/decisions (global endpoint) with server-side
   * subject_id / resource_type filters; decision (allow/deny) and date
   * range are applied client-side over the returned audit records.
   */
  import { api } from '../../lib/api.js';
  import { toEpochSec } from '../../lib/timeFormat.js';

  let decisions = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let loaded = $state(false);

  // Server-side filters
  let filterSubjectId = $state('');
  let filterResourceType = $state('');

  // Client-side filters
  let filterDecision = $state('');
  let filterFrom = $state(''); // YYYY-MM-DD
  let filterTo = $state('');

  function fmtTime(ts) {
    const sec = toEpochSec(ts);
    if (sec == null) return '—';
    return new Date(sec * 1000).toLocaleString();
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const params = { limit: 200 };
      if (filterSubjectId.trim()) params.subject_id = filterSubjectId.trim();
      if (filterResourceType.trim()) params.resource_type = filterResourceType.trim();
      const rows = await api.policyDecisions(params);
      decisions = Array.isArray(rows) ? rows : [];
      loaded = true;
    } catch (e) {
      error = e?.message ?? String(e);
      decisions = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  function fromSec(dateStr) {
    if (!dateStr) return null;
    const ms = Date.parse(dateStr + 'T00:00:00');
    return isNaN(ms) ? null : Math.floor(ms / 1000);
  }

  function toSec(dateStr) {
    if (!dateStr) return null;
    const ms = Date.parse(dateStr + 'T23:59:59');
    return isNaN(ms) ? null : Math.floor(ms / 1000);
  }

  let filtered = $derived.by(() => {
    let rows = decisions;
    if (filterDecision) {
      rows = rows.filter((d) => (d.decision ?? '').toLowerCase() === filterDecision);
    }
    const from = fromSec(filterFrom);
    const to = toSec(filterTo);
    if (from != null) rows = rows.filter((d) => (toEpochSec(d.evaluated_at) ?? 0) >= from);
    if (to != null) rows = rows.filter((d) => (toEpochSec(d.evaluated_at) ?? Infinity) <= to);
    return rows;
  });
</script>

<div class="decision-log" data-testid="policy-decision-log">
  <div class="filter-bar" data-testid="decision-filters">
    <label class="filter-field">
      <span class="field-label">Subject ID</span>
      <input class="field-input" bind:value={filterSubjectId} placeholder="subject id" data-testid="decision-filter-subject-id" />
    </label>
    <label class="filter-field">
      <span class="field-label">Resource type</span>
      <input class="field-input" bind:value={filterResourceType} placeholder="repo" data-testid="decision-filter-resource-type" />
    </label>
    <label class="filter-field">
      <span class="field-label">Decision</span>
      <select class="field-input" bind:value={filterDecision} data-testid="decision-filter-decision">
        <option value="">all</option>
        <option value="allow">allow</option>
        <option value="deny">deny</option>
      </select>
    </label>
    <label class="filter-field">
      <span class="field-label">From</span>
      <input class="field-input" type="date" bind:value={filterFrom} data-testid="decision-filter-from" />
    </label>
    <label class="filter-field">
      <span class="field-label">To</span>
      <input class="field-input" type="date" bind:value={filterTo} data-testid="decision-filter-to" />
    </label>
    <button type="button" class="btn-small" onclick={load} data-testid="decision-apply-filters">Apply</button>
  </div>

  {#if loading}
    <p class="hint-text" data-testid="decision-log-loading">Loading decisions…</p>
  {:else if error}
    <p class="error-text" role="alert" data-testid="decision-log-error">{error}</p>
  {:else if filtered.length === 0}
    <p class="hint-text" data-testid="decision-log-empty">
      {loaded ? 'No policy evaluations match the current filters.' : ''}
    </p>
  {:else}
    <table class="decisions-table" data-testid="decision-log-table">
      <thead>
        <tr>
          <th scope="col">Time</th>
          <th scope="col">Subject</th>
          <th scope="col">Action</th>
          <th scope="col">Resource</th>
          <th scope="col">Decision</th>
          <th scope="col">Matched policy</th>
        </tr>
      </thead>
      <tbody>
        {#each filtered as dec (dec.request_id ?? JSON.stringify(dec))}
          <tr data-testid="decision-row">
            <td class="mono">{fmtTime(dec.evaluated_at)}</td>
            <td class="mono">{dec.subject_type ? dec.subject_type + ':' : ''}{dec.subject_id ?? '—'}</td>
            <td class="mono">{dec.action ?? '—'}</td>
            <td class="mono">{dec.resource_type ?? '—'}{dec.resource_id ? ':' + dec.resource_id : ''}</td>
            <td>
              <span
                class="decision-badge"
                class:decision-allow={(dec.decision ?? '').toLowerCase() === 'allow'}
                class:decision-deny={(dec.decision ?? '').toLowerCase() === 'deny'}
              >
                {dec.decision ?? '—'}
              </span>
            </td>
            <td class="mono">{dec.matched_policy ?? '—'}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .decision-log {
    display: flex;
    flex-direction: column;
    gap: var(--space-3, 12px);
  }

  .filter-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2, 8px);
  }

  .filter-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1, 4px);
  }

  .field-label {
    font-size: var(--text-xs, 12px);
    font-weight: 600;
    color: var(--color-text-muted, #666);
  }

  .field-input {
    padding: 5px 9px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface, #fff);
    color: inherit;
    font: inherit;
    font-size: var(--text-sm, 13px);
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

  .decisions-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm, 13px);
  }

  .decisions-table th {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 2px solid var(--color-border, #ddd);
    font-weight: 600;
  }

  .decisions-table td {
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

  .hint-text {
    color: var(--color-text-muted, #888);
    margin: 0;
  }

  .error-text {
    color: var(--color-danger, #c0392b);
    margin: 0;
  }
</style>
