<script>
  /**
   * Constraint editing panel for spec approval (authorization-provenance.md §7.6).
   *
   * Shows strategy-implied constraints (read-only) derived from workspace config,
   * allows explicit constraint entry (CEL expression), scope definition (glob patterns),
   * and a dry-run button to evaluate constraints server-side via the CEL parser.
   */
  import { api } from './api.js';

  let {
    specPath = '',
    specSha = '',
    workspaceId = '',
    repoId = '',
    onApprove = () => {},
    onCancel = () => {},
    approving = false,
  } = $props();

  // Explicit output constraints (CEL expressions).
  let constraints = $state([]);
  // Scope constraints (glob patterns).
  let allowedPaths = $state('');
  let forbiddenPaths = $state('');
  // Dry-run state.
  let dryRunResult = $state(null);
  let dryRunning = $state(false);

  // Strategy-implied constraints fetched from the server (read-only, shown for context).
  let strategyConstraints = $state([]);
  let strategyLoading = $state(false);

  // Fetch strategy-implied constraints from the server on mount (§7.6).
  $effect(() => {
    strategyLoading = true;
    const params = new URLSearchParams();
    if (workspaceId) params.set('workspace_id', workspaceId);
    api.getStrategyConstraints(params.toString())
      .then(data => {
        strategyConstraints = data.constraints || [];
      })
      .catch(() => {
        strategyConstraints = [];
      })
      .finally(() => {
        strategyLoading = false;
      });
  });

  function addConstraint() {
    constraints = [...constraints, { name: '', expression: '' }];
  }

  function removeConstraint(index) {
    constraints = constraints.filter((_, i) => i !== index);
  }

  function handleApprove() {
    const output_constraints = constraints
      .filter(c => c.name.trim() && c.expression.trim())
      .map(c => ({ name: c.name.trim(), expression: c.expression.trim() }));

    const scope = buildScope();

    onApprove({ output_constraints, scope });
  }

  function buildScope() {
    const allowed = allowedPaths.split(',').map(p => p.trim()).filter(Boolean);
    const forbidden = forbiddenPaths.split(',').map(p => p.trim()).filter(Boolean);
    if (allowed.length === 0 && forbidden.length === 0) return null;
    return { allowed_paths: allowed, forbidden_paths: forbidden };
  }

  async function dryRun() {
    dryRunning = true;
    dryRunResult = null;
    try {
      const output_constraints = constraints
        .filter(c => c.name.trim() && c.expression.trim())
        .map(c => ({ name: c.name.trim(), expression: c.expression.trim() }));
      const scope = buildScope();

      // Validate constraints server-side using the real CEL parser (§7.6).
      const body = { constraints: output_constraints };
      if (scope) {
        body.scope = scope;
      }

      const result = await api.validateConstraints(body);
      const issues = [];
      for (const r of result.results) {
        if (!r.valid) {
          issues.push(`"${r.name}": ${r.error || 'invalid'}`);
        }
      }
      dryRunResult = issues.length
        ? { valid: false, issues }
        : { valid: true, issues: [] };
    } catch (e) {
      dryRunResult = { valid: false, issues: [`Validation request failed: ${e.message}`] };
    } finally {
      dryRunning = false;
    }
  }
</script>

<div class="constraint-editor" data-testid="constraint-editor">
  <div class="constraint-section">
    <h4>Spec Approval: {specPath.split('/').pop()}</h4>
    <p class="sha-label">SHA: <code>{specSha.slice(0, 8)}</code></p>
  </div>

  <!-- Strategy-Implied Constraints (read-only) -->
  <div class="constraint-section">
    <h5>Strategy-Implied Constraints</h5>
    <p class="hint">These are automatically derived from workspace config, trust level, and attestation policy. They cannot be edited.</p>
    {#if strategyLoading}
      <p class="hint">Loading strategy constraints...</p>
    {:else if strategyConstraints.length === 0}
      <p class="hint">No strategy-implied constraints for this context.</p>
    {:else}
      {#each strategyConstraints as sc}
        <div class="constraint-row readonly">
          <span class="constraint-name">{sc.name}</span>
          <code class="constraint-expr">{sc.expression}</code>
        </div>
      {/each}
    {/if}
  </div>

  <!-- Explicit Constraints -->
  <div class="constraint-section">
    <h5>Explicit Constraints</h5>
    <p class="hint">Add CEL expressions that the agent's output must satisfy.</p>
    {#each constraints as c, i}
      <div class="constraint-row editable">
        <input
          type="text"
          placeholder="Constraint name"
          bind:value={c.name}
          class="constraint-input name"
        />
        <input
          type="text"
          placeholder='CEL expression, e.g. output.changed_files.all(f, f.startsWith("src/"))'
          bind:value={c.expression}
          class="constraint-input expr"
        />
        <button class="remove-btn" onclick={() => removeConstraint(i)} title="Remove">&times;</button>
      </div>
    {/each}
    <button class="add-constraint-btn" onclick={addConstraint}>+ Add Constraint</button>
  </div>

  <!-- Scope Constraints -->
  <div class="constraint-section">
    <h5>Scope Constraints</h5>
    <p class="hint">Restrict which files the agent may modify (comma-separated glob patterns).</p>
    <label class="scope-label">
      Allowed paths
      <input
        type="text"
        placeholder='e.g. src/payments/**, tests/**'
        bind:value={allowedPaths}
        class="scope-input"
      />
    </label>
    <label class="scope-label">
      Forbidden paths
      <input
        type="text"
        placeholder='e.g. src/auth/**, migrations/**'
        bind:value={forbiddenPaths}
        class="scope-input"
      />
    </label>
  </div>

  <!-- Dry-Run -->
  <div class="constraint-section">
    <button class="dry-run-btn" onclick={dryRun} disabled={dryRunning}>
      {dryRunning ? 'Evaluating...' : 'Dry Run'}
    </button>
    {#if dryRunResult}
      <div class="dry-run-result" class:valid={dryRunResult.valid} class:invalid={!dryRunResult.valid}>
        {#if dryRunResult.valid}
          <span class="check">All constraints look valid</span>
        {:else}
          <ul class="issues">
            {#each dryRunResult.issues as issue}
              <li>{issue}</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Actions -->
  <div class="constraint-actions">
    <button class="cancel-btn" onclick={onCancel}>Cancel</button>
    <button class="approve-btn" onclick={handleApprove} disabled={approving}>
      {approving ? 'Approving...' : 'Approve with Constraints'}
    </button>
  </div>
</div>

<style>
  .constraint-editor {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 0.5rem 0;
  }
  .constraint-section {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .constraint-section h4 {
    margin: 0;
    font-size: 0.95rem;
  }
  .constraint-section h5 {
    margin: 0;
    font-size: 0.85rem;
    color: var(--muted-foreground, #888);
  }
  .hint {
    font-size: 0.75rem;
    color: var(--muted-foreground, #888);
    margin: 0;
  }
  .sha-label {
    font-size: 0.8rem;
    margin: 0;
    color: var(--muted-foreground, #888);
  }
  .constraint-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    font-size: 0.8rem;
  }
  .constraint-row.readonly {
    background: var(--muted, #1a1a2e);
    opacity: 0.8;
  }
  .constraint-name {
    min-width: 120px;
    font-weight: 500;
  }
  .constraint-expr {
    flex: 1;
    font-size: 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .constraint-input {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    background: var(--input, #0d0d1a);
    color: inherit;
    font-size: 0.8rem;
  }
  .constraint-input.name {
    width: 150px;
    flex-shrink: 0;
  }
  .constraint-input.expr {
    flex: 1;
    font-family: monospace;
  }
  .remove-btn {
    background: none;
    border: none;
    color: var(--destructive, #e33);
    cursor: pointer;
    font-size: 1.1rem;
    padding: 0 0.3rem;
  }
  .add-constraint-btn {
    align-self: flex-start;
    background: none;
    border: 1px dashed var(--border, #333);
    color: var(--muted-foreground, #888);
    padding: 0.3rem 0.8rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.8rem;
  }
  .add-constraint-btn:hover {
    color: var(--foreground, #eee);
    border-color: var(--foreground, #eee);
  }
  .scope-label {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: 0.8rem;
  }
  .scope-input {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    background: var(--input, #0d0d1a);
    color: inherit;
    font-family: monospace;
    font-size: 0.8rem;
  }
  .dry-run-btn {
    align-self: flex-start;
    padding: 0.4rem 1rem;
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    background: var(--secondary, #1a1a2e);
    color: inherit;
    cursor: pointer;
    font-size: 0.8rem;
  }
  .dry-run-result {
    padding: 0.5rem;
    border-radius: 4px;
    font-size: 0.8rem;
  }
  .dry-run-result.valid {
    background: rgba(34, 197, 94, 0.1);
    color: #22c55e;
  }
  .dry-run-result.invalid {
    background: rgba(239, 68, 68, 0.1);
    color: #ef4444;
  }
  .issues {
    margin: 0;
    padding-left: 1.2rem;
  }
  .constraint-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border, #333);
  }
  .cancel-btn {
    padding: 0.4rem 1rem;
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .approve-btn {
    padding: 0.4rem 1rem;
    border: none;
    border-radius: 4px;
    background: #22c55e;
    color: #000;
    cursor: pointer;
    font-weight: 500;
  }
  .approve-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
