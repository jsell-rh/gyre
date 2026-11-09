<script>
  let {
    evaluativeMetric = 'span_duration',
    traceData = null,
    hasRiskData = false,
    onMetricChange = () => {},
  } = $props();
</script>

<div class="eval-metric-group" role="group" aria-label="Evaluative metric">
  {#if traceData?.spans?.length}
    <!-- Trace-based metrics (behavioral data from test execution per spec) -->
    {#each [['span_duration', 'Duration'], ['span_count', 'Spans'], ['error_rate', 'Errors']] as [key, label]}
      <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => { onMetricChange(key); }} type="button">{label}</button>
    {/each}
  {/if}
  {#if hasRiskData}
    <!-- Risk overlay (task-178): heat from GET /repos/:id/graph/risks —
         churn × complexity per node, fetched when the Evaluative lens is on. -->
    <button
      class="tb-btn tb-btn-sm"
      class:active={evaluativeMetric === 'risk'}
      onclick={() => { onMetricChange('risk'); }}
      title="Risk heat from graph/risks (churn × complexity)"
      data-testid="evaluative-metric-risk"
      type="button"
    >Risk</button>
  {/if}
  {#if !traceData?.spans?.length && !hasRiskData}
    <span class="eval-no-trace">No trace data — connect OpenTelemetry to see evaluative metrics</span>
  {/if}
</div>

<style>
  .eval-metric-group { display: flex; gap: 2px; align-items: center; }
  .eval-no-trace { font-size: 11px; color: #64748b; font-style: italic; margin-left: 4px; }

  .tb-btn {
    padding: 5px 14px; border: none; border-radius: 7px; font-size: 13px; font-weight: 500;
    cursor: pointer; background: transparent; color: #94a3b8; transition: all 0.15s;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .tb-btn:hover:not(:disabled) { background: rgba(51,65,85,0.5); color: #e2e8f0; }
  .tb-btn.active { background: #1e293b; color: #e2e8f0; box-shadow: 0 1px 4px rgba(0,0,0,0.3); }
  .tb-btn-sm { font-size: 11px; padding: 4px 8px; }

  @media (prefers-reduced-motion: reduce) {
    .tb-btn { transition: none; }
  }
</style>
