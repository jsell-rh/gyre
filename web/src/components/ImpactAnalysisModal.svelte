<script>
  /**
   * ImpactAnalysisModal — pre-merge blast radius analysis for breaking changes.
   *
   * Shows direct & transitive dependents, per-repo health (version drift,
   * test status), cascade test results, and an acknowledgment flow for
   * `block` policy workspaces.
   */
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import Modal from '../lib/Modal.svelte';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';

  let {
    open = $bindable(false),
    repoId = null,
    repoName = '',
    workspaceId = null,
  } = $props();

  // ── Data state ──────────────────────────────────────────────────────
  let blastRadius = $state(null);
  let breakingChanges = $state([]);
  let dependencyPolicy = $state(null);
  let dependentEdges = $state([]);
  let cascadeTestResults = $state(null);
  let loading = $state(false);
  let error = $state(null);
  let acknowledging = $state({});
  let triggeringCascade = $state(false);
  let resolvedWorkspaceId = $state(null);

  // ── Derived ─────────────────────────────────────────────────────────
  const directCount = $derived(blastRadius?.direct_dependents?.length ?? 0);
  const transitiveCount = $derived(blastRadius?.transitive_dependents?.length ?? 0);
  const totalCount = $derived(blastRadius?.total ?? 0);
  const isBlockPolicy = $derived(dependencyPolicy?.breaking_change_behavior === 'block');
  const repoBreakingChanges = $derived(
    breakingChanges.filter(bc => bc.source_repo_id === repoId)
  );
  const allAcknowledged = $derived(
    repoBreakingChanges.length > 0 && repoBreakingChanges.every(bc => bc.acknowledged)
  );

  // ── Map dependency_edge_id → dependent repo (source_repo_id) ────────
  const edgeToDependent = $derived.by(() => {
    const map = new Map();
    for (const edge of dependentEdges) {
      map.set(edge.id, edge.source_repo_id);
    }
    return map;
  });

  // ── Build per-repo health table data ────────────────────────────────
  const healthData = $derived.by(() => {
    if (!blastRadius) return [];
    const allDeps = [
      ...(blastRadius.direct_dependents ?? []).map(id => ({ id, direct: true })),
      ...(blastRadius.transitive_dependents ?? []).map(id => ({ id, direct: false })),
    ];
    // Build a map from dependent repo ID → edge data for version info
    const edgeByDependent = new Map();
    for (const edge of dependentEdges) {
      edgeByDependent.set(edge.source_repo_id, edge);
    }
    return allDeps.map(dep => {
      // Find breaking change that affects this specific dependent via its edge
      const bc = repoBreakingChanges.find(b => {
        if (!b.dependency_edge_id) return false;
        const depRepoId = edgeToDependent.get(b.dependency_edge_id);
        return depRepoId === dep.id;
      });
      const edge = edgeByDependent.get(dep.id);
      // Cascade test result for this dependent, if available
      const cascadeResult = cascadeTestResults
        ? cascadeTestResults.find(r => r.repo_id === dep.id)
        : null;
      return {
        repoId: dep.id,
        name: entityName('repo', dep.id),
        direct: dep.direct,
        versionPinned: edge?.version_pinned ?? '--',
        versionCurrent: edge?.target_version_current ?? '--',
        drift: edge?.version_drift ?? null,
        testStatus: '--',
        cascadeResult: cascadeResult?.status ?? '--',
        breakingChange: bc ?? null,
      };
    });
  });

  // ── Load data when modal opens ──────────────────────────────────────
  $effect(() => {
    if (open && repoId) {
      loadData();
    }
    if (!open) {
      blastRadius = null;
      breakingChanges = [];
      dependencyPolicy = null;
      dependentEdges = [];
      cascadeTestResults = null;
      resolvedWorkspaceId = null;
      error = null;
    }
  });

  async function loadData() {
    loading = true;
    error = null;
    try {
      // Resolve workspace ID: prefer prop, fall back to repo lookup
      let wsId = workspaceId;
      if (!wsId && repoId) {
        try {
          const repoData = await api.repo(repoId);
          wsId = repoData?.workspace_id ?? null;
        } catch {
          // Non-critical — policy fetch will gracefully degrade
        }
      }
      resolvedWorkspaceId = wsId;

      const [blast, bcs, edges, policy, cascade] = await Promise.allSettled([
        api.repoBlastRadius(repoId),
        api.breakingChanges(),
        api.repoDependents(repoId),
        wsId ? api.workspaceDependencyPolicy(wsId) : Promise.resolve(null),
        api.cascadeTestResults?.(repoId) ?? Promise.resolve(null),
      ]);
      blastRadius = blast.status === 'fulfilled' ? blast.value : null;
      breakingChanges = bcs.status === 'fulfilled' ? (Array.isArray(bcs.value) ? bcs.value : []) : [];
      dependentEdges = edges.status === 'fulfilled' ? (Array.isArray(edges.value) ? edges.value : []) : [];
      dependencyPolicy = policy.status === 'fulfilled' ? policy.value : null;
      cascadeTestResults = cascade.status === 'fulfilled' && Array.isArray(cascade.value) ? cascade.value : null;
      if (blast.status === 'rejected') {
        error = 'Failed to load blast radius data';
      }
    } catch (e) {
      error = e?.message ?? 'Failed to load impact analysis';
    } finally {
      loading = false;
    }
  }

  // ── Acknowledge a breaking change ───────────────────────────────────
  async function acknowledge(bcId) {
    acknowledging = { ...acknowledging, [bcId]: true };
    try {
      await api.acknowledgeBreakingChange(bcId);
      breakingChanges = breakingChanges.map(bc =>
        bc.id === bcId ? { ...bc, acknowledged: true } : bc
      );
    } catch (e) {
      // Silently handle — user can retry
    } finally {
      acknowledging = { ...acknowledging, [bcId]: false };
    }
  }

  // ── Trigger cascade tests ───────────────────────────────────────────
  async function triggerCascadeTests() {
    triggeringCascade = true;
    try {
      await api.triggerCascadeTests?.(repoId);
      // Reload data to pick up new cascade test status
      await loadData();
    } catch {
      // Graceful degradation — cascade tests may not be configured
    } finally {
      triggeringCascade = false;
    }
  }

  function driftLabel(drift) {
    if (drift == null) return '--';
    if (drift === 0) return 'current';
    return `${drift} behind`;
  }

  function driftClass(drift) {
    if (drift == null) return '';
    if (drift === 0) return 'drift-ok';
    if (drift <= 2) return 'drift-warn';
    return 'drift-high';
  }
</script>

<Modal bind:open title="Impact Analysis: {repoName || entityName('repo', repoId)}" size="lg" onclose={() => { open = false; }}>
  {#if loading}
    <div class="impact-loading" data-testid="impact-loading">
      <p>Analyzing blast radius...</p>
    </div>
  {:else if error}
    <div class="impact-error" data-testid="impact-error">
      <p>{error}</p>
      <Button variant="secondary" size="sm" onclick={loadData}>Retry</Button>
    </div>
  {:else if blastRadius}
    <!-- Blast Radius Summary -->
    <div class="impact-summary" data-testid="impact-summary">
      <div class="summary-stat summary-total">
        <span class="stat-value">{totalCount}</span>
        <span class="stat-label">repos affected</span>
      </div>
      <div class="summary-stat">
        <span class="stat-value">{directCount}</span>
        <span class="stat-label">direct</span>
      </div>
      <div class="summary-stat">
        <span class="stat-value">{transitiveCount}</span>
        <span class="stat-label">transitive</span>
      </div>
      {#if repoBreakingChanges.length > 0}
        <div class="summary-breaking" data-testid="breaking-badge">
          <Badge value="Breaking" variant="danger" />
        </div>
      {/if}
    </div>

    <!-- Block policy notice -->
    {#if isBlockPolicy && repoBreakingChanges.length > 0}
      <div class="block-notice" data-testid="block-notice">
        {#if allAcknowledged}
          <span class="block-notice-text block-notice-resolved">All dependents acknowledged — merge unblocked</span>
        {:else}
          <span class="block-notice-text">Merge blocked until all dependents acknowledge this breaking change</span>
        {/if}
      </div>
    {/if}

    <!-- Dependency Tree -->
    <div class="impact-tree-section" data-testid="dependency-tree">
      <h4 class="section-heading">Dependency Tree</h4>
      {#if directCount > 0}
        <div class="tree-group">
          <span class="tree-group-label">Direct ({directCount})</span>
          <div class="tree-items">
            {#each blastRadius.direct_dependents as depId}
              <div class="tree-item" data-testid="direct-dep">
                <span class="tree-connector">├─</span>
                <span class="tree-name">{entityName('repo', depId)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      {#if transitiveCount > 0}
        <div class="tree-group">
          <span class="tree-group-label">Transitive ({transitiveCount})</span>
          <div class="tree-items">
            {#each blastRadius.transitive_dependents as depId}
              <div class="tree-item tree-item-transitive" data-testid="transitive-dep">
                <span class="tree-connector">│ └─</span>
                <span class="tree-name">{entityName('repo', depId)}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      {#if totalCount === 0}
        <p class="no-impact">No downstream impact — no repos depend on this one.</p>
      {/if}
    </div>

    <!-- Per-Repo Health Table -->
    {#if healthData.length > 0}
      <div class="health-section" data-testid="health-table">
        <h4 class="section-heading">Dependent Repo Health</h4>
        <div class="health-table-wrapper">
          <table class="health-table">
            <thead>
              <tr>
                <th>Repo</th>
                <th>Pinned</th>
                <th>Current</th>
                <th>Drift</th>
                <th>Tests</th>
                {#if repoBreakingChanges.length > 0 && isBlockPolicy}
                  <th>Acknowledge</th>
                {/if}
              </tr>
            </thead>
            <tbody>
              {#each healthData as row}
                <tr data-testid="health-row">
                  <td class="health-repo">
                    <span class="health-repo-name">{row.name}</span>
                    {#if row.direct}
                      <Badge value="direct" variant="info" />
                    {:else}
                      <Badge value="transitive" variant="muted" />
                    {/if}
                  </td>
                  <td class="health-version mono">{row.versionPinned}</td>
                  <td class="health-version mono">{row.versionCurrent}</td>
                  <td class="health-drift">
                    <span class={driftClass(row.drift)}>{driftLabel(row.drift)}</span>
                  </td>
                  <td class="health-test">{row.testStatus}</td>
                  {#if repoBreakingChanges.length > 0 && isBlockPolicy}
                    <td class="health-ack">
                      {#if row.breakingChange}
                        {#if row.breakingChange.acknowledged}
                          <Badge value="Acknowledged" variant="success" />
                        {:else}
                          <button
                            class="ack-btn"
                            disabled={acknowledging[row.breakingChange.id]}
                            onclick={() => acknowledge(row.breakingChange.id)}
                            data-testid="acknowledge-btn"
                          >
                            {acknowledging[row.breakingChange.id] ? 'Acknowledging...' : 'Acknowledge'}
                          </button>
                        {/if}
                      {:else}
                        <span class="health-na">—</span>
                      {/if}
                    </td>
                  {/if}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}

    <!-- Cascade Test Status -->
    <div class="cascade-section" data-testid="cascade-section">
      <h4 class="section-heading">Cascade Tests</h4>
      {#if cascadeTestResults && cascadeTestResults.length > 0}
        <div class="cascade-results" data-testid="cascade-results">
          {#each cascadeTestResults as result}
            <div class="cascade-result-row" data-testid="cascade-result">
              <span class="cascade-repo">{entityName('repo', result.repo_id)}</span>
              <Badge
                value={result.status === 'passed' ? 'Pass' : result.status === 'failed' ? 'Fail' : result.status}
                variant={result.status === 'passed' ? 'success' : result.status === 'failed' ? 'danger' : 'muted'}
              />
            </div>
          {/each}
        </div>
      {:else}
        <p class="cascade-status" data-testid="cascade-status">
          {cascadeTestResults ? 'No cascade test results yet' : 'Cascade tests: not configured'}
        </p>
      {/if}
      <button
        class="cascade-trigger-btn"
        disabled={triggeringCascade}
        onclick={triggerCascadeTests}
        data-testid="trigger-cascade-btn"
      >
        {triggeringCascade ? 'Triggering...' : 'Trigger Cascade Tests'}
      </button>
    </div>
  {:else}
    <div class="impact-empty" data-testid="impact-empty">
      <p>No blast radius data available.</p>
    </div>
  {/if}
</Modal>

<style>
  .impact-loading, .impact-error, .impact-empty {
    text-align: center;
    padding: var(--space-8) var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .impact-error {
    color: var(--color-danger);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
  }

  /* ── Summary ────────────────────────────────────────────────────────── */
  .impact-summary {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    margin-bottom: var(--space-4);
  }

  .summary-stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
  }

  .summary-total .stat-value {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
  }

  .stat-value {
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    line-height: 1;
  }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .summary-breaking {
    margin-left: auto;
  }

  /* ── Block notice ───────────────────────────────────────────────────── */
  .block-notice {
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-danger) 8%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    border-radius: var(--radius);
    margin-bottom: var(--space-4);
  }

  .block-notice-text {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-danger);
  }

  .block-notice-resolved {
    color: var(--color-success);
  }

  /* ── Section headings ───────────────────────────────────────────────── */
  .section-heading {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3) 0;
  }

  /* ── Dependency tree ────────────────────────────────────────────────── */
  .impact-tree-section {
    margin-bottom: var(--space-4);
  }

  .tree-group {
    margin-bottom: var(--space-2);
  }

  .tree-group-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .tree-items {
    display: flex;
    flex-direction: column;
    margin-top: var(--space-1);
  }

  .tree-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
  }

  .tree-connector {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
    width: 28px;
  }

  .tree-name {
    font-size: var(--text-sm);
    color: var(--color-link);
  }

  .tree-item-transitive .tree-name {
    opacity: 0.8;
  }

  .no-impact {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  /* ── Health table ───────────────────────────────────────────────────── */
  .health-section {
    margin-bottom: var(--space-4);
  }

  .health-table-wrapper {
    overflow-x: auto;
  }

  .health-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .health-table th {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    border-bottom: 1px solid var(--color-border);
  }

  .health-table td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .health-repo {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .health-repo-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .health-version {
    color: var(--color-text-secondary);
  }

  .mono {
    font-family: var(--font-mono);
  }

  .health-drift .drift-ok { color: var(--color-success); }
  .health-drift .drift-warn { color: var(--color-warning); }
  .health-drift .drift-high { color: var(--color-danger); font-weight: 600; }

  .health-test {
    color: var(--color-text-muted);
  }

  .health-na {
    color: var(--color-text-muted);
  }

  /* ── Cascade tests ──────────────────────────────────────────────────── */
  .cascade-section {
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }

  .cascade-status {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3) 0;
  }

  .cascade-results {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .cascade-result-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .cascade-repo {
    color: var(--color-text);
    font-weight: 500;
  }

  .cascade-trigger-btn, .ack-btn {
    padding: var(--space-1) var(--space-3);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .cascade-trigger-btn:hover:not(:disabled), .ack-btn:hover:not(:disabled) {
    background: var(--color-border);
    border-color: var(--color-text-muted);
  }

  .cascade-trigger-btn:disabled, .ack-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .cascade-trigger-btn:focus-visible, .ack-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }
</style>
