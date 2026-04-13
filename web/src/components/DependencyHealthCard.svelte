<script>
  /**
   * DependencyHealthCard — Compact dependency health summary for workspace dashboard
   *
   * Spec ref: dependency-graph.md §UI — "Workspace dashboard: aggregate dependency
   * health. '3 repos have stale dependencies. 1 breaking change unacknowledged.'"
   *
   * Props:
   *   totalWithDeps     — number of repos that have at least one dependency
   *   staleCount        — number of repos with stale dependencies
   *   breakingCount     — number of unacknowledged breaking changes
   *   onViewGraph       — () => void — navigate to full dependency graph
   *   loading           — boolean — show skeleton while data loads
   */

  let {
    totalWithDeps = 0,
    staleCount = 0,
    breakingCount = 0,
    onViewGraph = () => {},
    loading = false,
  } = $props();
</script>

<div class="dep-health-card" data-testid="dep-health-card">
  <div class="dep-health-header">
    <h3 class="dep-health-title">Dependency Health</h3>
    <button class="dep-health-link" onclick={onViewGraph} data-testid="dep-health-view-graph">View graph</button>
  </div>

  {#if loading}
    <div class="dep-health-skeleton" data-testid="dep-health-loading">
      <div class="skeleton-bar"></div>
      <div class="skeleton-bar short"></div>
    </div>
  {:else if totalWithDeps === 0}
    <p class="dep-health-empty" data-testid="dep-health-empty">No dependencies detected.</p>
  {:else}
    <div class="dep-health-stats" data-testid="dep-health-stats">
      <div class="dep-health-stat">
        <span class="dep-health-count">{totalWithDeps}</span>
        <span class="dep-health-label">repo{totalWithDeps !== 1 ? 's' : ''} with dependencies</span>
      </div>

      {#if staleCount > 0}
        <div class="dep-health-stat stale" data-testid="dep-health-stale">
          <span class="dep-health-count">{staleCount}</span>
          <span class="dep-health-label">repo{staleCount !== 1 ? 's' : ''} with stale dependencies</span>
        </div>
      {/if}

      {#if breakingCount > 0}
        <div class="dep-health-stat breaking" data-testid="dep-health-breaking">
          <span class="dep-health-count">{breakingCount}</span>
          <span class="dep-health-label">breaking change{breakingCount !== 1 ? 's' : ''} unacknowledged</span>
        </div>
      {/if}

      {#if staleCount === 0 && breakingCount === 0}
        <div class="dep-health-stat healthy" data-testid="dep-health-healthy">
          <span class="dep-health-label">All dependencies healthy</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .dep-health-card {
    padding: 12px 14px;
  }

  .dep-health-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .dep-health-title {
    font-size: var(--text-sm, 0.875rem);
    font-weight: 600;
    color: var(--color-text, #e5e7eb);
    margin: 0;
  }

  .dep-health-link {
    font-size: var(--text-xs, 0.75rem);
    color: #60a5fa;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .dep-health-link:hover {
    text-decoration: underline;
  }

  .dep-health-empty {
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text-muted, #888);
    margin: 0;
  }

  .dep-health-stats {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .dep-health-stat {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text-muted, #888);
  }

  .dep-health-count {
    font-weight: 700;
    font-size: var(--text-sm, 0.875rem);
    color: var(--color-text, #e5e7eb);
  }

  .dep-health-stat.stale .dep-health-count {
    color: #eab308;
  }

  .dep-health-stat.breaking .dep-health-count {
    color: #ef4444;
  }

  .dep-health-stat.healthy {
    color: #22c55e;
  }

  .dep-health-skeleton {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .skeleton-bar {
    height: 12px;
    border-radius: 4px;
    background: var(--color-border, #2a2a3a);
    animation: pulse-skeleton 1.5s ease-in-out infinite;
  }

  .skeleton-bar.short {
    width: 60%;
  }

  @keyframes pulse-skeleton {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }
</style>
