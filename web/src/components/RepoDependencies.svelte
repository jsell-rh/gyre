<script>
  /**
   * RepoDependencies — cross-repo dependency section for repo detail page.
   *
   * Shows outgoing dependencies (what this repo depends on) and incoming
   * dependents (what depends on this repo). Includes version drift indicators,
   * stale/breaking change badges, and a blast radius tree.
   */
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import Badge from '../lib/Badge.svelte';

  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  import { getContext } from 'svelte';

  let {
    repoId = null,
    workspace = null,
  } = $props();

  // ── Data state ──────────────────────────────────────────────────────
  let dependencies = $state([]);
  let dependents = $state([]);
  let loading = $state(false);
  let error = $state(null);

  // Blast radius
  let blastRadius = $state(null);
  let blastLoading = $state(false);
  let blastOpen = $state(false);

  // ── Fetch dependencies + dependents ─────────────────────────────────
  async function loadDeps(id) {
    if (!id) return;
    loading = true;
    error = null;
    dependencies = [];
    dependents = [];
    blastRadius = null;
    blastOpen = false;
    try {
      const [deps, depts] = await Promise.all([
        api.repoDependencies(id),
        api.repoDependents(id),
      ]);
      dependencies = Array.isArray(deps) ? deps : [];
      dependents = Array.isArray(depts) ? depts : [];
    } catch (err) {
      error = err?.message ?? 'Failed to load dependencies';
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    const id = repoId;
    if (id) loadDeps(id);
  });

  // ── Blast radius fetch ──────────────────────────────────────────────
  async function loadBlastRadius() {
    if (!repoId || blastLoading) return;
    blastLoading = true;
    try {
      const result = await api.repoBlastRadius(repoId);
      blastRadius = result;
      blastOpen = true;
    } catch (err) {
      blastRadius = null;
    } finally {
      blastLoading = false;
    }
  }

  // ── Derived counts ──────────────────────────────────────────────────
  const staleDeps = $derived(dependencies.filter(d => d.status === 'Stale' || d.status === 'stale'));
  const breakingDeps = $derived(dependencies.filter(d => d.status === 'Breaking' || d.status === 'breaking'));

  const summaryText = $derived.by(() => {
    const parts = [];
    if (dependencies.length > 0) parts.push(`${dependencies.length} dependenc${dependencies.length === 1 ? 'y' : 'ies'}`);
    if (dependents.length > 0) parts.push(`${dependents.length} dependent${dependents.length === 1 ? '' : 's'}`);
    if (staleDeps.length > 0) parts.push(`${staleDeps.length} stale`);
    if (breakingDeps.length > 0) parts.push(`${breakingDeps.length} breaking`);
    return parts.join(', ');
  });

  // ── Helpers ─────────────────────────────────────────────────────────
  function depTypeVariant(type) {
    const t = (type ?? '').toLowerCase();
    if (t === 'code') return 'info';
    if (t === 'spec') return 'purple';
    if (t === 'api') return 'warning';
    if (t === 'schema') return 'success';
    if (t === 'manual') return 'muted';
    return 'muted';
  }

  function statusVariant(status) {
    const s = (status ?? '').toLowerCase();
    if (s === 'active') return 'success';
    if (s === 'stale') return 'warning';
    if (s === 'breaking') return 'danger';
    if (s === 'orphaned') return 'muted';
    return 'muted';
  }

  function driftColor(drift) {
    if (drift == null) return '';
    if (drift === 0) return 'drift-ok';
    if (drift <= 2) return 'drift-warn';
    return 'drift-high';
  }

  function navigateToRepo(targetRepoId) {
    if (!targetRepoId || !goToEntityDetail) return;
    goToEntityDetail('repo', targetRepoId, { id: targetRepoId });
  }
</script>

<div class="repo-deps" data-testid="repo-dependencies">
  <!-- Breaking change alert -->
  {#if breakingDeps.length > 0}
    <div class="breaking-alert" data-testid="breaking-alert">
      <span class="breaking-icon">⚠</span>
      <span class="breaking-text">
        {breakingDeps.length} breaking change{breakingDeps.length !== 1 ? 's' : ''} detected
      </span>
      <span class="breaking-detail">
        {#each breakingDeps as dep, i}
          <button class="breaking-link" onclick={() => navigateToRepo(dep.target_repo_id)}>
            {entityName('repo', dep.target_repo_id)}
          </button>{#if i < breakingDeps.length - 1}, {/if}
        {/each}
      </span>
    </div>
  {/if}

  {#if loading}
    <p class="deps-loading">Loading dependencies...</p>
  {:else if error}
    <div class="deps-error">
      <p>{error}</p>
      <button class="retry-btn" onclick={() => loadDeps(repoId)}>Retry</button>
    </div>
  {:else if dependencies.length === 0 && dependents.length === 0}
    <div class="deps-empty" data-testid="deps-empty">
      <p>No cross-repo dependencies detected</p>
      <p class="deps-empty-hint">Dependencies are auto-detected from Cargo.toml, package.json, go.mod, and other manifest files when code is pushed.</p>
    </div>
  {:else}
    <!-- Summary header -->
    {#if summaryText}
      <div class="deps-summary" data-testid="deps-summary">{summaryText}</div>
    {/if}

    <div class="deps-columns">
      <!-- Outgoing dependencies -->
      <div class="deps-column" data-testid="deps-outgoing">
        <h4 class="deps-col-heading">
          <span class="deps-arrow deps-arrow-out">→</span>
          Dependencies ({dependencies.length})
        </h4>
        {#if dependencies.length === 0}
          <p class="deps-col-empty">No outgoing dependencies</p>
        {:else}
          <div class="deps-list">
            {#each dependencies as dep}
              <button
                class="dep-row"
                class:dep-row-stale={dep.status === 'Stale' || dep.status === 'stale'}
                class:dep-row-breaking={dep.status === 'Breaking' || dep.status === 'breaking'}
                onclick={() => navigateToRepo(dep.target_repo_id)}
                data-testid="dep-row"
                title="Depends on {entityName('repo', dep.target_repo_id)} via {dep.source_artifact ?? ''} → {dep.target_artifact ?? ''}"
              >
                <div class="dep-row-main">
                  <span class="dep-repo-name">{entityName('repo', dep.target_repo_id)}</span>
                  <Badge value={dep.dependency_type ?? 'code'} variant={depTypeVariant(dep.dependency_type)} />
                  {#if dep.status && dep.status !== 'Active' && dep.status !== 'active'}
                    <Badge value={dep.status} variant={statusVariant(dep.status)} />
                  {/if}
                </div>
                <div class="dep-row-detail">
                  {#if dep.version_pinned}
                    <span class="dep-version" title="Pinned version">{dep.version_pinned}</span>
                  {/if}
                  {#if dep.version_drift != null}
                    <span class="dep-drift {driftColor(dep.version_drift)}" title="{dep.version_drift} version{dep.version_drift !== 1 ? 's' : ''} behind">
                      {#if dep.version_drift === 0}
                        ✓ current
                      {:else}
                        {dep.version_drift} behind
                      {/if}
                    </span>
                  {:else}
                    <span class="dep-drift dep-drift-na" title="Version drift not available">--</span>
                  {/if}
                  <span class="dep-artifact" title="{dep.source_artifact ?? ''} → {dep.target_artifact ?? ''}">{dep.target_artifact ?? ''}</span>
                </div>
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Incoming dependents -->
      <div class="deps-column" data-testid="deps-incoming">
        <h4 class="deps-col-heading">
          <span class="deps-arrow deps-arrow-in">←</span>
          Dependents ({dependents.length})
        </h4>
        {#if dependents.length === 0}
          <p class="deps-col-empty">No repos depend on this one</p>
        {:else}
          <div class="deps-list">
            {#each dependents as dep}
              <button
                class="dep-row"
                class:dep-row-stale={dep.status === 'Stale' || dep.status === 'stale'}
                class:dep-row-breaking={dep.status === 'Breaking' || dep.status === 'breaking'}
                onclick={() => navigateToRepo(dep.source_repo_id)}
                data-testid="dependent-row"
                title="{entityName('repo', dep.source_repo_id)} depends on this repo via {dep.source_artifact ?? ''} → {dep.target_artifact ?? ''}"
              >
                <div class="dep-row-main">
                  <span class="dep-repo-name">{entityName('repo', dep.source_repo_id)}</span>
                  <Badge value={dep.dependency_type ?? 'code'} variant={depTypeVariant(dep.dependency_type)} />
                  {#if dep.status && dep.status !== 'Active' && dep.status !== 'active'}
                    <Badge value={dep.status} variant={statusVariant(dep.status)} />
                  {/if}
                </div>
                <div class="dep-row-detail">
                  {#if dep.version_pinned}
                    <span class="dep-version" title="Pinned version">{dep.version_pinned}</span>
                  {/if}
                  {#if dep.version_drift != null}
                    <span class="dep-drift {driftColor(dep.version_drift)}" title="{dep.version_drift} version{dep.version_drift !== 1 ? 's' : ''} behind">
                      {#if dep.version_drift === 0}
                        ✓ current
                      {:else}
                        {dep.version_drift} behind
                      {/if}
                    </span>
                  {:else}
                    <span class="dep-drift dep-drift-na" title="Version drift not available">--</span>
                  {/if}
                  <span class="dep-artifact" title="{dep.source_artifact ?? ''} → {dep.target_artifact ?? ''}">{dep.source_artifact ?? ''}</span>
                </div>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <!-- Blast radius -->
    <div class="blast-section" data-testid="blast-section">
      <button
        class="blast-btn"
        onclick={loadBlastRadius}
        disabled={blastLoading}
        data-testid="blast-btn"
      >
        {#if blastLoading}
          Loading impact...
        {:else if blastOpen && blastRadius}
          Refresh Impact
        {:else}
          Show Impact
        {/if}
      </button>

      {#if blastOpen && blastRadius}
        <div class="blast-tree" data-testid="blast-tree">
          <div class="blast-header">
            <span class="blast-total">Total blast radius: {blastRadius.total ?? 0} repo{(blastRadius.total ?? 0) !== 1 ? 's' : ''}</span>
          </div>

          {#if (blastRadius.direct_dependents?.length ?? 0) > 0}
            <div class="blast-group">
              <span class="blast-group-label">Direct ({blastRadius.direct_dependents.length})</span>
              <div class="blast-items">
                {#each blastRadius.direct_dependents as depId}
                  <button class="blast-item" onclick={() => navigateToRepo(depId)} data-testid="blast-direct">
                    <span class="blast-connector">├─</span>
                    <span class="blast-item-name">{entityName('repo', depId)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if (blastRadius.transitive_dependents?.length ?? 0) > 0}
            <div class="blast-group">
              <span class="blast-group-label">Transitive ({blastRadius.transitive_dependents.length})</span>
              <div class="blast-items">
                {#each blastRadius.transitive_dependents as depId}
                  <button class="blast-item blast-item-transitive" onclick={() => navigateToRepo(depId)} data-testid="blast-transitive">
                    <span class="blast-connector">│ └─</span>
                    <span class="blast-item-name">{entityName('repo', depId)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if (blastRadius.direct_dependents?.length ?? 0) === 0 && (blastRadius.transitive_dependents?.length ?? 0) === 0}
            <p class="blast-empty">No downstream impact — no repos depend on this one.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .repo-deps {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-6);
    overflow-y: auto;
    flex: 1;
  }

  /* ── Breaking change alert ──────────────────────────────────────────── */
  .breaking-alert {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-danger) 8%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    border-radius: var(--radius);
    flex-wrap: wrap;
  }

  .breaking-icon {
    font-size: var(--text-base);
    flex-shrink: 0;
  }

  .breaking-text {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-danger);
  }

  .breaking-detail {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .breaking-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .breaking-link:hover {
    text-decoration: underline;
  }

  /* ── Loading / Error / Empty ─────────────────────────────────────────── */
  .deps-loading {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
    padding: var(--space-8) 0;
    text-align: center;
  }

  .deps-error {
    text-align: center;
    padding: var(--space-6) 0;
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .retry-btn {
    margin-top: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }

  .retry-btn:hover { border-color: var(--color-link); }

  .deps-empty {
    text-align: center;
    padding: var(--space-8) var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .deps-empty-hint {
    font-size: var(--text-xs);
    margin-top: var(--space-2);
    opacity: 0.7;
  }

  /* ── Summary ─────────────────────────────────────────────────────────── */
  .deps-summary {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    padding: 0 0 var(--space-1) 0;
  }

  /* ── Two-column layout ───────────────────────────────────────────────── */
  .deps-columns {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
  }

  @media (max-width: 768px) {
    .deps-columns {
      grid-template-columns: 1fr;
    }
  }

  .deps-column {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .deps-col-heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .deps-arrow {
    font-weight: 700;
    font-size: var(--text-sm);
    flex-shrink: 0;
    width: 16px;
    text-align: center;
  }

  .deps-arrow-out { color: var(--color-info, #1e90ff); }
  .deps-arrow-in { color: var(--color-warning); }

  .deps-col-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-2) 0;
    margin: 0;
  }

  /* ── Dependency rows ─────────────────────────────────────────────────── */
  .deps-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .dep-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: border-color var(--transition-fast);
  }

  .dep-row:hover {
    border-color: var(--color-border-strong);
  }

  .dep-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .dep-row-stale {
    border-color: color-mix(in srgb, var(--color-warning) 40%, var(--color-border));
    background: color-mix(in srgb, var(--color-warning) 3%, var(--color-surface));
  }

  .dep-row-breaking {
    border-color: color-mix(in srgb, var(--color-danger) 40%, var(--color-border));
    background: color-mix(in srgb, var(--color-danger) 3%, var(--color-surface));
  }

  .dep-row-main {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .dep-repo-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-link);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .dep-row-detail {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-wrap: wrap;
  }

  .dep-version {
    font-family: var(--font-mono);
    padding: 0 var(--space-1);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
  }

  .dep-drift {
    font-family: var(--font-mono);
    font-weight: 500;
  }

  .drift-ok { color: var(--color-success); }
  .drift-warn { color: var(--color-warning); }
  .drift-high { color: var(--color-danger); font-weight: 600; }
  .dep-drift-na { opacity: 0.5; }

  .dep-artifact {
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 150px;
  }

  /* ── Blast radius ────────────────────────────────────────────────────── */
  .blast-section {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-4);
  }

  .blast-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-link);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .blast-btn:hover { border-color: var(--color-link); }
  .blast-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .blast-tree {
    margin-top: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .blast-header {
    margin-bottom: var(--space-2);
  }

  .blast-total {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .blast-group {
    margin-top: var(--space-2);
  }

  .blast-group-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .blast-items {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin-top: var(--space-1);
  }

  .blast-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast);
    width: 100%;
  }

  .blast-item:hover {
    background: var(--color-surface-elevated);
  }

  .blast-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .blast-connector {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
    width: 28px;
  }

  .blast-item-name {
    font-size: var(--text-sm);
    color: var(--color-link);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .blast-item-transitive .blast-item-name {
    opacity: 0.8;
  }

  .blast-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    margin: var(--space-2) 0 0 0;
  }
</style>
