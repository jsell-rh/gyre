<script>
  let {
    evaluativeMetric = 'complexity',
    traceData = null,
    onMetricChange = () => {},
  } = $props();
</script>

<div class="eval-metric-group" role="group" aria-label="Evaluative metric">
  {#if traceData?.spans?.length}
    <!-- Trace-based metrics (primary evaluative data per spec) -->
    {#each [['span_duration', 'Duration'], ['span_count', 'Spans'], ['error_rate', 'Errors']] as [key, label]}
      <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => { onMetricChange(key); }} type="button">{label}</button>
    {/each}
    <span class="tb-sep-v"></span>
    <span class="eval-label">Static:</span>
  {/if}
  <!-- Static analysis metrics (structural overlay for repos without trace data) -->
  {#each [['complexity', 'Complexity'], ['churn', 'Churn'], ['incoming_calls', 'Call Count'], ['test_coverage', 'Test Coverage']] as [key, label]}
    <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => { onMetricChange(key); }} type="button">{label}</button>
  {/each}
</div>

{#if !traceData?.spans?.length}
  <span class="eval-no-trace">No trace data</span>
{/if}

<style>
  .eval-metric-group { display: flex; gap: 2px; align-items: center; }
  .eval-label { font-size: 10px; color: #64748b; margin: 0 2px; white-space: nowrap; }
  .eval-no-trace { font-size: 11px; color: #64748b; font-style: italic; margin-left: 4px; }

  .tb-btn {
    padding: 5px 14px; border: none; border-radius: 7px; font-size: 13px; font-weight: 500;
    cursor: pointer; background: transparent; color: #94a3b8; transition: all 0.15s;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .tb-btn:hover:not(:disabled) { background: rgba(51,65,85,0.5); color: #e2e8f0; }
  .tb-btn.active { background: #1e293b; color: #e2e8f0; box-shadow: 0 1px 4px rgba(0,0,0,0.3); }
  .tb-btn-sm { font-size: 11px; padding: 4px 8px; }
  .tb-sep-v { width: 1px; height: 16px; background: #475569; margin: 0 2px; display: inline-block; vertical-align: middle; }

  @media (prefers-reduced-motion: reduce) {
    .tb-btn { transition: none; }
  }
</style>
