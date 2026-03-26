<script>
  /**
   * FlowRenderer — Integration wrapper for Explorer flow layout.
   * Combines SVG graph layer (ExplorerCanvas) with Canvas 2D particle overlay (FlowCanvas)
   * and SVG node badge overlay (NodeBadge).
   *
   * Spec ref: ui-layout.md §4 (flow layout detail)
   */

  import ExplorerCanvas from './ExplorerCanvas.svelte';
  import FlowCanvas from './FlowCanvas.svelte';
  import NodeBadge from './NodeBadge.svelte';

  let {
    nodes = [],         // graph nodes from API
    edges = [],         // graph edges from API
    spans = [],         // trace spans [{id, parent_id, node_id, start_time, duration_us, status}]
    repoId = '',
    workspaceId = '',
    scope = null,
    viewSpec = null,
  } = $props();

  // Playback controls state
  let playing = $state(false);
  let currentTime = $state(0);
  let speed = $state(1);
  let selectedTests = $state([]);

  // Canvas dimensions (bound to wrapper div size)
  let wrapperEl = $state(null);
  let canvasWidth = $state(800);
  let canvasHeight = $state(600);

  // Node positions derived from the simple column layout (mirroring ExplorerCanvas)
  // This will be replaced by bind:positionedNodes from ExplorerCanvas once it exposes them.
  let positionedNodes = $derived(() => {
    if (!nodes.length) return [];
    const byType = {};
    for (const n of nodes) {
      const t = n.node_type ?? 'Unknown';
      (byType[t] = byType[t] ?? []).push(n);
    }
    const typeOrder = ['package', 'module', 'type', 'interface', 'function', 'endpoint', 'component', 'table', 'constant'];
    const cols = Object.keys(byType).sort((a, b) => {
      const ai = typeOrder.indexOf(a);
      const bi = typeOrder.indexOf(b);
      return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
    });
    const colW = 160;
    const rowH = 60;
    const startX = 80;
    const startY = 60;
    const result = [];
    cols.forEach((col, ci) => {
      byType[col].forEach((n, ri) => {
        result.push({
          ...n,
          x: startX + ci * colW,
          y: startY + ri * rowH,
          width: 64,
          height: 28,
        });
      });
    });
    return result;
  });

  // Compute per-node metrics from spans
  let nodeMetrics = $derived(() => {
    const m = {};
    const byNode = {};
    for (const s of spans) {
      if (!s.node_id) continue;
      (byNode[s.node_id] = byNode[s.node_id] ?? []).push(s);
    }
    for (const [nodeId, nodeSpans] of Object.entries(byNode)) {
      const errors = nodeSpans.filter(s => s.status === 'error').length;
      const durations = nodeSpans.map(s => s.duration_us ?? 0);
      m[nodeId] = {
        span_count: nodeSpans.length,
        error_rate: nodeSpans.length ? errors / nodeSpans.length : 0,
        mean_duration_us: durations.length ? durations.reduce((a, b) => a + b, 0) / durations.length : 0,
      };
    }
    return m;
  });

  let nodesWithMetrics = $derived(() => {
    return positionedNodes().filter(n => nodeMetrics()[n.id]);
  });

  // Max time for scrubber
  let maxTime = $derived(() => {
    if (!spans.length) return 10000;
    return Math.max(...spans.map(s => s.start_time + (s.duration_us ?? 0)));
  });

  function togglePlay() {
    playing = !playing;
  }

  function onScrub(e) {
    currentTime = Number(e.target.value);
    playing = false;
  }

  function setSpeed(s) {
    speed = s;
  }

  $effect(() => {
    if (!wrapperEl) return;
    const ro = new ResizeObserver(entries => {
      for (const entry of entries) {
        canvasWidth = entry.contentRect.width;
        canvasHeight = entry.contentRect.height;
      }
    });
    ro.observe(wrapperEl);
    return () => ro.disconnect();
  });
</script>

<div class="flow-renderer" data-testid="flow-renderer">
  <!-- Playback Controls -->
  <div class="flow-controls" role="toolbar" aria-label="Flow animation controls">
    <button
      class="ctrl-btn play-btn"
      class:playing
      onclick={togglePlay}
      aria-label={playing ? 'Pause animation' : 'Play animation'}
      aria-pressed={playing}
    >
      {#if playing}
        <!-- Pause icon -->
        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true">
          <rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>
        </svg>
        Pause
      {:else}
        <!-- Play icon -->
        <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true">
          <polygon points="5,3 19,12 5,21"/>
        </svg>
        Play
      {/if}
    </button>

    <label class="scrub-label" for="flow-scrubber">
      <span class="sr-only">Scrub time</span>
      <input
        id="flow-scrubber"
        class="scrubber-input"
        type="range"
        min="0"
        max={maxTime()}
        step="1000"
        value={currentTime}
        oninput={onScrub}
        aria-label="Animation scrubber"
      />
    </label>

    <span class="time-label" aria-live="polite">
      {(currentTime / 1000).toFixed(1)}ms
    </span>

    <div class="speed-controls" role="group" aria-label="Playback speed">
      {#each [0.25, 0.5, 1, 2, 5] as s}
        <button
          class="speed-btn"
          class:active={speed === s}
          onclick={() => setSpeed(s)}
          aria-label="{s}× speed"
          aria-pressed={speed === s}
        >{s}×</button>
      {/each}
    </div>
  </div>

  <!-- Canvas + Overlay wrapper -->
  <div
    class="flow-wrapper"
    bind:this={wrapperEl}
    style="position:relative"
  >
    <!-- SVG graph layer (ExplorerCanvas) -->
    <ExplorerCanvas
      {nodes}
      {edges}
      {repoId}
    />

    <!-- Canvas 2D particle overlay -->
    <FlowCanvas
      nodes={positionedNodes()}
      {edges}
      {spans}
      bind:currentTime
      {playing}
      {speed}
      {selectedTests}
      width={canvasWidth}
      height={canvasHeight}
      style="position:absolute;top:0;left:0"
    />

    <!-- Node badge SVG overlay -->
    <svg
      class="badge-overlay"
      style="position:absolute;top:0;left:0;pointer-events:none;overflow:visible"
      width={canvasWidth}
      height={canvasHeight}
      aria-hidden="true"
    >
      {#each nodesWithMetrics() as node (node.id)}
        <NodeBadge {node} metrics={nodeMetrics()[node.id]} />
      {/each}
    </svg>
  </div>
</div>

<style>
  .flow-renderer {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .flow-controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .ctrl-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .ctrl-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }

  .play-btn.playing {
    border-color: var(--color-primary);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
  }

  .scrub-label {
    flex: 1;
    min-width: 120px;
    max-width: 400px;
  }

  .scrubber-input {
    width: 100%;
    accent-color: var(--color-primary);
    cursor: pointer;
  }

  .time-label {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    min-width: 60px;
    text-align: right;
  }

  .speed-controls {
    display: flex;
    gap: 2px;
  }

  .speed-btn {
    padding: 2px 6px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .speed-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }

  .speed-btn.active {
    border-color: var(--color-primary);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
  }

  .flow-wrapper {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .badge-overlay {
    display: block;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    white-space: nowrap;
    border: 0;
  }
</style>
