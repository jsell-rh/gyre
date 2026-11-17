<script>
  /**
   * PolicySimulator — dry-run "what would happen if user X tried to do Y on
   * resource Z?" (abac-policy-engine.md §UI).
   *
   * POST /api/v1/policies/evaluate (the GLOBAL endpoint — there is no
   * workspace-scoped simulate route; workspace-prefixed URLs return 405).
   */
  import { api } from '../../lib/api.js';
  import { KNOWN_ACTIONS, KNOWN_RESOURCE_TYPES } from '../../lib/abac.js';

  let { defaultWorkspaceId = '', defaultRepoId = '' } = $props();

  let subjectType = $state('user');
  let subjectId = $state('');
  let workspaceRole = $state('');
  let action = $state('read');
  let resourceType = $state('repo');
  let resourceId = $state('');
  let resourceWorkspaceId = $state(defaultWorkspaceId ?? '');
  let resourceRepoId = $state(defaultRepoId ?? '');
  let resourceTenantId = $state('');
  let running = $state(false);
  let result = $state(null);
  let error = $state(null);

  let canRun = $derived(subjectType !== '' && action !== '' && resourceType !== '' && !running);

  async function run() {
    if (!canRun) return;
    running = true;
    error = null;
    result = null;
    try {
      const subject = { type: subjectType };
      if (subjectId.trim()) subject.id = subjectId.trim();
      if (workspaceRole.trim()) subject.workspace_role = workspaceRole.trim();
      const resource = { type: resourceType };
      if (resourceId.trim()) resource.id = resourceId.trim();
      if (resourceWorkspaceId.trim()) resource.workspace_id = resourceWorkspaceId.trim();
      if (resourceRepoId.trim()) resource.repo_id = resourceRepoId.trim();
      if (resourceTenantId.trim()) resource.tenant_id = resourceTenantId.trim();
      result = await api.evaluatePolicy({ subject, action, resource });
    } catch (e) {
      error = e?.message ?? String(e);
    } finally {
      running = false;
    }
  }
</script>

<div class="simulator" data-testid="policy-simulator">
  <p class="hint-text">
    Dry-run: what would happen if a subject tried to do an action on a resource?
    Runs the real policy engine against the live policy set — no request is authorized by this.
  </p>

  <div class="form-grid">
    <label class="field">
      <span class="field-label">Subject type</span>
      <select class="field-input" bind:value={subjectType} data-testid="simulator-subject-type">
        <option value="user">user</option>
        <option value="agent">agent</option>
        <option value="system">system</option>
      </select>
    </label>
    <label class="field">
      <span class="field-label">Subject ID</span>
      <input class="field-input" bind:value={subjectId} placeholder="user:jsell / agent id" data-testid="simulator-subject-id" />
    </label>
    <label class="field">
      <span class="field-label">Workspace role <span class="field-hint">(optional)</span></span>
      <input class="field-input" bind:value={workspaceRole} placeholder="Developer" />
    </label>
  </div>

  <div class="form-grid">
    <label class="field">
      <span class="field-label">Action</span>
      <select class="field-input" bind:value={action} data-testid="simulator-action">
        {#each KNOWN_ACTIONS.filter((a) => a !== '*') as a (a)}
          <option value={a}>{a}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      <span class="field-label">Resource type</span>
      <select class="field-input" bind:value={resourceType} data-testid="simulator-resource-type">
        {#each KNOWN_RESOURCE_TYPES.filter((r) => r !== '*') as r (r)}
          <option value={r}>{r}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      <span class="field-label">Resource ID <span class="field-hint">(optional)</span></span>
      <input class="field-input" bind:value={resourceId} placeholder="resource id" data-testid="simulator-resource-id" />
    </label>
  </div>

  <div class="form-grid">
    <label class="field">
      <span class="field-label">Resource workspace <span class="field-hint">(scope chain)</span></span>
      <input class="field-input" bind:value={resourceWorkspaceId} placeholder="ws-…" data-testid="simulator-resource-workspace-id" />
    </label>
    <label class="field">
      <span class="field-label">Resource repo <span class="field-hint">(scope chain)</span></span>
      <input class="field-input" bind:value={resourceRepoId} placeholder="repo-…" data-testid="simulator-resource-repo-id" />
    </label>
    <label class="field">
      <span class="field-label">Resource tenant <span class="field-hint">(scope chain)</span></span>
      <input class="field-input" bind:value={resourceTenantId} placeholder="tenant-…" data-testid="simulator-resource-tenant-id" />
    </label>
  </div>

  <div class="action-row">
    <button type="button" class="btn-small btn-primary" onclick={run} disabled={!canRun} data-testid="simulator-run-btn">
      {running ? 'Evaluating…' : 'Simulate'}
    </button>
  </div>

  {#if error}
    <p class="error-text" role="alert" data-testid="simulator-error">{error}</p>
  {:else if result}
    <div class="result-card" data-testid="simulator-result">
      <div class="result-row">
        <span class="result-label">Decision</span>
        <span
          class="decision-badge"
          class:decision-allow={result.decision === 'allow'}
          class:decision-deny={result.decision === 'deny'}
          data-testid="simulator-decision"
        >
          {result.decision === 'allow' ? 'Allow' : 'Deny'}
        </span>
      </div>
      <div class="result-row">
        <span class="result-label">Matched policy</span>
        <span class="mono" data-testid="simulator-matched-policy">{result.matched_policy ?? '— (default deny)'}</span>
      </div>
      <div class="result-row">
        <span class="result-label">Policies evaluated</span>
        <span class="mono" data-testid="simulator-evaluated-count">{result.evaluated_policies ?? '—'}</span>
      </div>
      <div class="result-row">
        <span class="result-label">Evaluation time</span>
        <span class="mono" data-testid="simulator-evaluation-ms">{result.evaluation_ms ?? '—'} ms</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .simulator {
    display: flex;
    flex-direction: column;
    gap: var(--space-4, 16px);
  }

  .hint-text {
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 13px);
    margin: 0;
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(170px, 1fr));
    gap: var(--space-3, 12px);
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
    min-width: 0;
  }

  .action-row {
    display: flex;
    gap: var(--space-2, 8px);
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

  .result-card {
    border: 1px solid var(--color-border, #ddd);
    border-radius: var(--radius, 6px);
    padding: var(--space-3, 12px) var(--space-4, 16px);
    display: flex;
    flex-direction: column;
    gap: var(--space-2, 8px);
    background: var(--color-surface-elevated, #fafafa);
  }

  .result-row {
    display: flex;
    align-items: center;
    gap: var(--space-3, 12px);
  }

  .result-label {
    min-width: 140px;
    font-size: var(--text-sm, 13px);
    font-weight: 600;
    color: var(--color-text-muted, #666);
  }

  .decision-badge {
    padding: 2px 10px;
    border-radius: 999px;
    font-weight: 600;
    font-size: var(--text-sm, 13px);
  }

  .decision-allow {
    background: color-mix(in srgb, #16a34a 15%, transparent);
    color: #16a34a;
  }

  .decision-deny {
    background: color-mix(in srgb, #dc2626 15%, transparent);
    color: #dc2626;
  }

  .mono {
    font-family: var(--font-mono, monospace);
    font-size: var(--text-sm, 13px);
  }

  .error-text {
    color: var(--color-danger, #c0392b);
    margin: 0;
  }
</style>
