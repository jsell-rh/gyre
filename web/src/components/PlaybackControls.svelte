<script>
  let {
    evalPlaying = false,
    evalScrubber = 0,
    evalSpeed = 1,
    evalParticles = [],
    traceData = null,
    traceElapsedDisplay = '',
    traceTotalDisplay = '',
    // Test cases (root spans) for the selector + legend. Each carries the
    // hue-shifted swatch its particles render in (ui-layout.md §4 "Multiple
    // test cases" + "A legend shows which test case each hue represents").
    rootCases = [],
    testFilter = null, // root span id, or null = all test cases
    onPlayToggle = () => {},
    onScrubberChange = () => {},
    onSpeedChange = () => {},
    // HSI §3a step-through: advance to the next span, pausing playback.
    onStep = () => {},
    // ui-layout.md §4 Test selector: filter which test case(s) to animate.
    onTestFilterChange = () => {},
  } = $props();
</script>

{#if traceData?.spans?.length}
  <div class="trace-playback-wrap">
    <div class="trace-playback-bar" role="toolbar" aria-label="Trace playback controls">
      <button class="trace-pb-btn" onclick={onPlayToggle} type="button" title={evalPlaying ? 'Pause trace playback' : 'Play trace playback'}>
        {#if evalPlaying}
          <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><polygon points="5,3 19,12 5,21"/></svg>
        {/if}
      </button>
      <button class="trace-pb-btn trace-pb-step" onclick={onStep} type="button" title="Step to next span" aria-label="Step to next span">
        <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><polygon points="5,4 15,12 5,20"/><rect x="16" y="4" width="3" height="16"/></svg>
      </button>
      <input type="range" min="0" max="1000" value={Math.round(evalScrubber * 1000)}
        oninput={(e) => { onScrubberChange(parseInt(e.target.value) / 1000); }}
        class="trace-pb-scrubber" title="Trace timeline position" />
      <span class="trace-pb-time">{traceElapsedDisplay} / {traceTotalDisplay}</span>
      <div class="trace-pb-sep"></div>
      <select class="trace-pb-speed" value={String(evalSpeed)}
        onchange={(e) => { onSpeedChange(parseFloat(e.target.value)); }}
        title="Playback speed" aria-label="Playback speed">
        <option value="0.25">0.25x</option>
        <option value="0.5">0.5x</option>
        <option value="1">1x</option>
        <option value="2">2x</option>
        <option value="5">5x</option>
        <option value="10">10x</option>
      </select>
      {#if rootCases.length > 1}
        <select class="trace-pb-test" value={testFilter ?? 'all'}
          onchange={(e) => { onTestFilterChange(e.target.value === 'all' ? null : e.target.value); }}
          title="Filter test cases" aria-label="Test case filter">
          <option value="all">Test: all</option>
          {#each rootCases as c (c.spanId)}
            <option value={c.spanId}>{c.operationName}</option>
          {/each}
        </select>
      {/if}
      <span class="trace-pb-particles">{evalParticles.length} active spans</span>
    </div>
    {#if rootCases.length > 1}
      <div class="trace-pb-legend" role="list" aria-label="Test case legend">
        {#each rootCases as c (c.spanId)}
          <button
            class="trace-pb-legend-item"
            class:trace-pb-legend-active={testFilter === c.spanId}
            onclick={() => { onTestFilterChange(testFilter === c.spanId ? null : c.spanId); }}
            type="button"
            title={testFilter === c.spanId ? `Showing only: ${c.operationName} (click to show all)` : `Show only: ${c.operationName}`}
          >
            <span class="trace-pb-legend-dot" style="background: {c.color}"></span>
            <span class="trace-pb-legend-name">{c.operationName}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .trace-playback-wrap {
    position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); z-index: 40;
    display: flex; flex-direction: column; align-items: center; gap: 6px;
  }
  .trace-playback-bar {
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
  .trace-pb-speed, .trace-pb-test {
    background: rgba(15, 15, 26, 0.9); color: #94a3b8; border: 1px solid #334155;
    border-radius: 4px; padding: 4px 6px; font-size: 11px;
    font-family: 'SF Mono', Menlo, monospace; cursor: pointer;
  }
  .trace-pb-particles { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; white-space: nowrap; }
  .trace-pb-legend {
    display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: 4px;
    padding: 4px 10px; background: rgba(15, 15, 26, 0.9); border: 1px solid #1e293b;
    border-radius: 6px; max-width: 480px;
  }
  .trace-pb-legend-item {
    display: flex; align-items: center; gap: 5px;
    background: none; border: 1px solid transparent; border-radius: 4px;
    padding: 2px 6px; cursor: pointer; color: #94a3b8;
    font-size: 10px; font-family: 'SF Mono', Menlo, monospace;
  }
  .trace-pb-legend-item:hover { background: #1e293b; color: #e2e8f0; }
  .trace-pb-legend-active { border-color: #60a5fa; color: #e2e8f0; background: #1e293b; }
  .trace-pb-legend-dot { width: 8px; height: 8px; border-radius: 50%; flex: none; }
  .trace-pb-legend-name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 140px; }

  @media (prefers-reduced-motion: reduce) {
    .trace-pb-btn { transition: none; }
  }
</style>
