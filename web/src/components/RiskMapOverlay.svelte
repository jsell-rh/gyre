<script>
  import { RISK_MAP_METRICS, riskHeatColor, riskMetricDef } from '../lib/risk-map.js';

  // Risk Map overlay controls (system-explorer.md §7): shown while the
  // Evaluative lens is active with the Risk metric selected. Owns the heat
  // map metric selector (churn / coupling / complexity / spec coverage /
  // test coverage) and the blue → yellow → red color scale legend.
  let {
    metric = 'churn',
    heat = null, // buildRiskHeatMap() result — drives the legend data range
    onMetricChange = () => {},
  } = $props();

  let metricDef = $derived(riskMetricDef(metric) ?? RISK_MAP_METRICS[0]);
  let gradient = $derived(
    `linear-gradient(to right, ${riskHeatColor(0)}, ${riskHeatColor(0.5)}, ${riskHeatColor(1)})`
  );
</script>

<div class="risk-map-controls" role="group" aria-label="Risk Map metric" data-testid="risk-map-controls">
  <span class="risk-map-label">Risk Map</span>
  <select
    class="risk-map-select"
    data-testid="risk-map-metric-select"
    aria-label="Risk Map heat metric"
    title={metricDef.description}
    value={metric}
    onchange={(e) => { onMetricChange(e.currentTarget.value); }}
  >
    {#each RISK_MAP_METRICS as m}
      <option value={m.key}>{m.label}</option>
    {/each}
  </select>
</div>

<!-- Color scale legend — corner of the canvas (blue → yellow → red).
     The parent canvas container is position:relative, so the absolutely
     positioned legend anchors to the canvas area, not the toolbar row. -->
<div class="risk-map-legend" data-testid="risk-map-legend" role="img" aria-label={`Risk Map color scale: ${metricDef.description}`}>
  <span class="risk-map-legend-metric">{metricDef.label}</span>
  <span class="risk-map-legend-bar" style="background: {gradient}"></span>
  <span class="risk-map-legend-labels">
    <span>{metricDef.lowLabel}</span>
    {#if heat?.max != null && heat.max > 0 && !metricDef.invert}
      <span class="risk-map-legend-max">{heat.max}</span>
    {/if}
    <span>{metricDef.highLabel}</span>
  </span>
</div>

<style>
  .risk-map-controls {
    display: inline-flex; align-items: center; gap: 5px;
  }
  .risk-map-label {
    font-size: 11px; font-weight: 600; color: #94a3b8;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .risk-map-select {
    background: #1e293b; color: #e2e8f0; border: 1px solid #334155;
    border-radius: 6px; font-size: 12px; padding: 3px 6px;
    font-family: system-ui, -apple-system, sans-serif; cursor: pointer;
  }
  .risk-map-select:hover { border-color: #475569; }
  .risk-map-select:focus-visible { outline: 1px solid #60a5fa; }

  .risk-map-legend {
    position: absolute; bottom: 12px; left: 12px; z-index: 30;
    display: flex; flex-direction: column; gap: 4px;
    background: rgba(15, 15, 26, 0.92); border: 1px solid #334155;
    border-radius: 8px; padding: 8px 10px; width: 150px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    font-family: system-ui, -apple-system, sans-serif;
    pointer-events: none;
  }
  .risk-map-legend-metric {
    font-size: 10px; font-weight: 700; color: #e2e8f0; letter-spacing: 0.02em;
  }
  .risk-map-legend-bar {
    display: block; height: 8px; border-radius: 4px;
    border: 1px solid rgba(0, 0, 0, 0.4);
  }
  .risk-map-legend-labels {
    display: flex; justify-content: space-between;
    font-size: 9px; color: #94a3b8;
  }
  .risk-map-legend-max { color: #64748b; }
</style>
