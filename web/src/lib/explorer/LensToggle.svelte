<script>
  let {
    lens = 'structural',
    onLensChange = () => {},
    traceData = null,
    evalPlaying = false,
    evalScrubber = 0,
    evalSpeed = 1,
    evalParticleCount = 0,
    evaluativeMetric = 'complexity',
    onPlayToggle = () => {},
    onScrubberChange = () => {},
    onSpeedChange = () => {},
    onMetricChange = () => {},
  } = $props();
</script>

<div class="lens-group" role="group" aria-label="Lens toggle">
  <button class="tb-btn" class:active={lens === 'structural'} onclick={() => onLensChange('structural')} aria-pressed={lens === 'structural'} type="button">Structural</button>
  <button class="tb-btn" class:active={lens === 'evaluative'} onclick={() => onLensChange('evaluative')} aria-pressed={lens === 'evaluative'} title="Overlay test/trace data on the structural topology" type="button">Evaluative</button>
  <button class="tb-btn tb-btn-disabled" disabled type="button" title="Requires production telemetry integration">Observable <span class="tb-coming-soon">(coming soon)</span></button>
</div>

{#if lens === 'evaluative'}
  <div class="eval-metric-group" role="group" aria-label="Evaluative metric">
    {#each [['complexity', 'Complexity'], ['churn', 'Churn'], ['incoming_calls', 'Call Count'], ['test_coverage', 'Test Coverage']] as [key, label]}
      <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => onMetricChange(key)} type="button">{label}</button>
    {/each}
  </div>
  {#if traceData?.spans?.length}
    <div class="eval-playback" role="group" aria-label="Trace playback">
      <button class="tb-btn tb-btn-sm" onclick={onPlayToggle} type="button" title={evalPlaying ? 'Pause' : 'Play'}>
        {evalPlaying ? '\u23F8' : '\u25B6'}
      </button>
      <input type="range" min="0" max="100" value={Math.round(evalScrubber * 100)}
        oninput={(e) => onScrubberChange(parseInt(e.target.value) / 100)}
        class="eval-scrubber" title="Trace timeline position" />
      <select class="eval-speed" value={evalSpeed}
        onchange={(e) => onSpeedChange(parseFloat(e.target.value))}>
        <option value="0.25">0.25x</option>
        <option value="0.5">0.5x</option>
        <option value="1">1x</option>
        <option value="2">2x</option>
        <option value="5">5x</option>
      </select>
      <span class="eval-particle-count">{evalParticleCount} spans</span>
    </div>
  {:else}
    <span class="eval-no-trace">No trace data</span>
  {/if}
  <div class="tb-sep"></div>
{/if}

<style>
  .lens-group, .eval-metric-group, .eval-playback {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .tb-btn {
    padding: 4px 10px; font-size: 12px; cursor: pointer; border-radius: 4px;
    border: 1px solid transparent; color: #94a3b8; background: transparent;
    white-space: nowrap;
  }
  .tb-btn:hover:not(:disabled) { background: rgba(51,65,85,0.5); color: #e2e8f0; }
  .tb-btn.active { background: #1e293b; color: #e2e8f0; box-shadow: 0 1px 4px rgba(0,0,0,0.3); }
  .tb-btn:disabled { opacity: 0.35; cursor: not-allowed; }
  .tb-btn-sm { font-size: 11px; padding: 4px 8px; }
  .tb-btn-disabled { opacity: 0.35; cursor: not-allowed; }
  .tb-coming-soon { font-size: 9px; opacity: 0.6; font-style: italic; }
  .eval-scrubber { width: 80px; cursor: pointer; }
  .eval-speed { font-size: 11px; background: #1e293b; color: #e2e8f0; border: 1px solid #334155; border-radius: 4px; padding: 2px; }
  .eval-particle-count { font-size: 10px; color: #64748b; white-space: nowrap; }
  .eval-no-trace { font-size: 11px; color: #64748b; font-style: italic; }
  .tb-sep { width: 1px; height: 18px; background: #334155; margin: 0 4px; flex-shrink: 0; }
</style>
