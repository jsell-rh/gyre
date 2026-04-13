<script>
  let {
    evalPlaying = false,
    evalScrubber = 0,
    evalSpeed = 1,
    evalParticles = [],
    traceData = null,
    traceElapsedDisplay = '',
    traceTotalDisplay = '',
    onPlayToggle = () => {},
    onScrubberChange = () => {},
    onSpeedChange = () => {},
  } = $props();
</script>

{#if traceData?.spans?.length}
  <div class="trace-playback-bar" role="toolbar" aria-label="Trace playback controls">
    <button class="trace-pb-btn" onclick={onPlayToggle} type="button" title={evalPlaying ? 'Pause trace playback' : 'Play trace playback'}>
      {#if evalPlaying}
        <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><polygon points="5,3 19,12 5,21"/></svg>
      {/if}
    </button>
    <input type="range" min="0" max="1000" value={Math.round(evalScrubber * 1000)}
      oninput={(e) => { onScrubberChange(parseInt(e.target.value) / 1000); }}
      class="trace-pb-scrubber" title="Trace timeline position" />
    <span class="trace-pb-time">{traceElapsedDisplay} / {traceTotalDisplay}</span>
    <div class="trace-pb-sep"></div>
    <select class="trace-pb-speed" value={evalSpeed}
      onchange={(e) => { onSpeedChange(parseFloat(e.target.value)); }}>
      <option value="0.25">0.25x</option>
      <option value="0.5">0.5x</option>
      <option value="1">1x</option>
      <option value="2">2x</option>
      <option value="5">5x</option>
    </select>
    <span class="trace-pb-particles">{evalParticles.length} active spans</span>
  </div>
{/if}

<style>
  .trace-playback-bar {
    position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); z-index: 40;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px; background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
  }
  .trace-pb-btn {
    display: flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border: none; border-radius: 6px;
    background: #1e293b; color: #60a5fa; cursor: pointer;
    transition: all 0.15s;
  }
  .trace-pb-btn:hover { background: #334155; color: #93c5fd; }
  .trace-pb-scrubber { width: 160px; accent-color: #60a5fa; height: 4px; cursor: pointer; }
  .trace-pb-time {
    font-size: 11px; color: #e2e8f0; font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap; min-width: 100px;
  }
  .trace-pb-sep { width: 1px; height: 20px; background: #334155; }
  .trace-pb-speed {
    background: rgba(15, 15, 26, 0.9); color: #94a3b8; border: 1px solid #334155;
    border-radius: 4px; padding: 4px 6px; font-size: 11px;
    font-family: 'SF Mono', Menlo, monospace; cursor: pointer;
  }
  .trace-pb-particles { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; white-space: nowrap; }

  @media (prefers-reduced-motion: reduce) {
    .trace-pb-btn { transition: none; }
  }
</style>
