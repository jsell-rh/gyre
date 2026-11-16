<script>
  /**
   * SvgGraphCanvas — SVG-based graph renderer
   * (ui-layout.md §10 Canvas Rendering + Layout Engines).
   *
   * SVG is used for graph/hierarchical/layered layouts because DOM elements
   * are individually addressable (click events, tooltips, accessibility),
   * Svelte's reactive model works naturally with SVG elements, and CSS
   * transitions apply to visual state changes.
   *
   *   engine 'graph'        → d3-force (Svelte wrapper, sync ticks)
   *   engine 'hierarchical' → ELK layered, top-down
   *   engine 'layered'      → ELK layered, left-to-right
   *
   * Interactions:
   *   - pan by dragging the background, zoom with the wheel (around the
   *     cursor) or the +/- buttons
   *   - every node and edge is an interactive element that emits a
   *     standardized ViewEvent (click / dblclick / hover / context-menu)
   *     and click additionally calls `onSelectNode`
   *
   * Density policy (§10): >500 nodes auto-filter to public-only with a
   * banner; >1000 nodes even after filtering switch to the virtualized
   * list with a warning.
   */
  import { computeLayout } from './layout-engines.js';
  import { applyDensityPolicy } from './canvas-density.js';
  import { dispatchViewEvent, viewEventFromDom } from './viewEvents.js';
  import VirtualNodeList from './VirtualNodeList.svelte';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    engine = 'graph',
    onSelectNode = undefined,
  } = $props();

  const MIN_ZOOM = 0.1;
  const MAX_ZOOM = 8;
  const NODE_W = 132;
  const NODE_H = 34;

  // ── Density policy (§10 auto-filter thresholds) ──────────────────────────
  let showAll = $state(false);
  let density = $derived(applyDensityPolicy(nodes, { showAll }));
  let renderNodes = $derived(density.nodes);

  // ── Layout (ELK / d3-force via layout-engines.js) ────────────────────────
  let positions = $state({});
  // Plain (non-$state) invalidation token: bumped on each effect run so a
  // stale async layout result (from a previous nodes/engine change) is
  // discarded. Never read in the template — writing $state that the same
  // effect also reads would loop (effect_update_depth_exceeded).
  let layoutVersion = 0;
  let layoutError = $state(null);

  $effect(() => {
    // In list-fallback mode the SVG (and therefore the positions) is never
    // rendered — computing a 300-tick force/ELK layout for >1000 nodes
    // would burn CPU for a result nothing consumes.
    if (density.mode === 'list') {
      positions = {};
      return;
    }
    const ns = renderNodes;
    const es = edges;
    const eng = engine;
    const version = ++layoutVersion;
    if (!ns.length) {
      positions = {};
      return;
    }
    computeLayout(eng, ns, es, 900, 600)
      .then((pos) => {
        if (version === layoutVersion) {
          positions = pos;
          layoutError = null;
        }
      })
      .catch((err) => {
        if (version === layoutVersion) {
          layoutError = err?.message ?? String(err);
          positions = {};
        }
      });
  });

  // Node/edge geometry derived from layout positions. Nodes without a
  // position yet (layout pending) stack in a pending column so they are
  // still visible and addressable.
  let nodeGeoms = $derived.by(() => {
    const geoms = [];
    let pending = 0;
    for (const n of renderNodes) {
      const p = positions[n.id];
      if (p) {
        geoms.push({ node: n, x: p.x, y: p.y });
      } else {
        geoms.push({ node: n, x: 40, y: 40 + pending * (NODE_H + 8) });
        pending++;
      }
    }
    return geoms;
  });

  let edgeGeoms = $derived.by(() => {
    const byId = new Map(nodeGeoms.map((g) => [g.node.id, g]));
    const geoms = [];
    for (const e of edges) {
      const sid = e.source_id ?? e.from_node_id ?? e.from;
      const tid = e.target_id ?? e.to_node_id ?? e.to;
      const s = byId.get(sid);
      const t = byId.get(tid);
      if (s && t) {
        geoms.push({ edge: e, id: e.id ?? `${sid}->${tid}`, x1: s.x, y1: s.y, x2: t.x, y2: t.y });
      }
    }
    return geoms;
  });

  // ── Pan / zoom camera ────────────────────────────────────────────────────
  let view = $state({ x: 0, y: 0, k: 1 });
  let svgEl = $state(null);
  let panning = false;
  let panStart = null; // { sx, sy, vx, vy } screen + view at drag start
  let dragMoved = false;
  let selectedNodeId = $state(null);
  let hoveredNodeId = $state(null);

  function onBackgroundPointerDown(e) {
    if (e.button !== 0) return;
    panning = true;
    dragMoved = false;
    panStart = { sx: e.clientX, sy: e.clientY, vx: view.x, vy: view.y };
    window.addEventListener('pointermove', onWindowPointerMove);
    window.addEventListener('pointerup', onWindowPointerUp);
  }

  // Window-level drag tracking: pan continues even when the pointer leaves
  // the svg, and no pointer capture is used so node click targets are
  // unaffected (capture would retarget synthesized mouse events).
  function onWindowPointerMove(e) {
    if (!panning || !panStart) return;
    const dx = e.clientX - panStart.sx;
    const dy = e.clientY - panStart.sy;
    if (Math.abs(dx) + Math.abs(dy) > 3) dragMoved = true;
    view = { ...view, x: panStart.vx + dx, y: panStart.vy + dy };
  }

  function onWindowPointerUp() {
    panning = false;
    panStart = null;
    window.removeEventListener('pointermove', onWindowPointerMove);
    window.removeEventListener('pointerup', onWindowPointerUp);
  }

  // Unmount safety: drop the window drag listeners if the component is
  // destroyed mid-pan.
  $effect(() => {
    return () => {
      window.removeEventListener('pointermove', onWindowPointerMove);
      window.removeEventListener('pointerup', onWindowPointerUp);
    };
  });

  function onWheel(e) {
    e.preventDefault();
    const rect = svgEl?.getBoundingClientRect();
    if (!rect) return;
    // Pointer position relative to the svg center (view transform origin).
    const px = e.clientX - rect.left - rect.width / 2;
    const py = e.clientY - rect.top - rect.height / 2;
    zoomAt(px, py, e.deltaY < 0 ? 1.2 : 1 / 1.2);
  }

  function zoomAt(px, py, factor) {
    const k = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, view.k * factor));
    const applied = k / view.k;
    view = {
      k,
      x: view.x - (px * applied - px),
      y: view.y - (py * applied - py),
    };
  }

  function zoomIn() { zoomAt(0, 0, 1.3); }
  function zoomOut() { zoomAt(0, 0, 1 / 1.3); }

  function fitView() {
    if (!nodeGeoms.length) {
      view = { x: 0, y: 0, k: 1 };
      return;
    }
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const g of nodeGeoms) {
      minX = Math.min(minX, g.x - NODE_W / 2);
      maxX = Math.max(maxX, g.x + NODE_W / 2);
      minY = Math.min(minY, g.y - NODE_H / 2);
      maxY = Math.max(maxY, g.y + NODE_H / 2);
    }
    const rect = svgEl?.getBoundingClientRect();
    const w = rect?.width || 900;
    const h = rect?.height || 600;
    const k = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.min(w / (maxX - minX || 1), h / (maxY - minY || 1)) * 0.9));
    view = { x: -(minX + maxX) / 2, y: -(minY + maxY) / 2, k };
  }

  // ── ViewEvent emission (§10 Interaction Events) ──────────────────────────
  function emitNode(domEvent, node) {
    dispatchViewEvent(domEvent.currentTarget, viewEventFromDom(domEvent, 'node', node.id, node));
  }

  function emitEdge(domEvent, edge) {
    const sid = edge.source_id ?? edge.from_node_id ?? edge.from;
    const tid = edge.target_id ?? edge.to_node_id ?? edge.to;
    dispatchViewEvent(
      domEvent.currentTarget,
      viewEventFromDom(domEvent, 'edge', edge.id ?? `${sid}->${tid}`, {
        ...edge,
        source_id: sid,
        target_id: tid,
      })
    );
  }

  function nodeClick(domEvent, node) {
    if (dragMoved) return; // click that ended a pan drag is not a selection
    selectedNodeId = node.id;
    emitNode(domEvent, node);
    onSelectNode?.(node);
  }

  function nodeKeydown(e, node) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      selectedNodeId = node.id;
      emitNode(e, node);
      onSelectNode?.(node);
    }
  }

  // ── Encoding: node_type → fill color (categorical scale) ─────────────────
  const TYPE_COLORS = {
    package: '#1d4ed8',
    module: '#1e40af',
    crate: '#3730a3',
    type: '#047857',
    interface: '#b45309',
    trait: '#b45309',
    struct: '#047857',
    enum: '#047857',
    function: '#6d28d9',
    method: '#6d28d9',
    endpoint: '#be123c',
    component: '#0e7490',
    table: '#4d7c0f',
    constant: '#a16207',
    spec: '#9333ea',
    test: '#65a30d',
  };

  function nodeColor(node) {
    return TYPE_COLORS[(node.node_type ?? '').toLowerCase()] ?? '#475569';
  }

  function label(node) {
    const name = node.name ?? node.id;
    return name.length > 18 ? name.slice(0, 17) + '…' : name;
  }

  const ENGINE_LABELS = {
    graph: 'Force-directed',
    hierarchical: 'Hierarchical (ELK)',
    layered: 'Layered (ELK)',
  };
</script>

{#if density.mode === 'list'}
  <!-- §10: >1000 nodes even after the public filter → list layout with
       virtual scrolling and a warning. -->
  <div class="svg-canvas">
    <div class="density-banner density-warn" role="status">
      Graph too large for canvas rendering ({density.total} nodes) — switched to
      list layout with virtual scrolling.
    </div>
    <VirtualNodeList nodes={renderNodes} {onSelectNode} viewportHeight={520} />
  </div>
{:else}
  <div class="svg-canvas">
    {#if density.mode === 'public-only'}
      <div class="density-banner" role="status">
        Showing public API only — {density.hiddenCount} private nodes hidden.
        <button type="button" class="density-show-all" onclick={() => (showAll = true)}>Show All</button>
      </div>
    {/if}
    {#if layoutError}
      <div class="density-banner density-warn" role="alert">Layout error: {layoutError}</div>
    {/if}
    <div class="svg-toolbar">
      <span class="svg-engine-label">{ENGINE_LABELS[engine] ?? engine} · {renderNodes.length} nodes</span>
      <span class="svg-zoom-controls">
        <button type="button" onclick={zoomOut} aria-label="Zoom out" title="Zoom out">−</button>
        <span class="svg-zoom-label">{Math.round(view.k * 100)}%</span>
        <button type="button" onclick={zoomIn} aria-label="Zoom in" title="Zoom in">+</button>
        <button type="button" onclick={fitView} aria-label="Fit view" title="Fit view">Fit</button>
      </span>
    </div>
    <svg
      class="graph-svg"
      bind:this={svgEl}
      viewBox="-450 -300 900 600"
      preserveAspectRatio="xMidYMid slice"
      role="application"
      aria-label="Architecture graph (SVG)"
      onpointerdown={onBackgroundPointerDown}
      onwheel={onWheel}
    >
      <g class="viewport" transform="translate({view.x}, {view.y}) scale({view.k})">
        <!-- Edges: addressable, hoverable, clickable SVG lines -->
        {#each edgeGeoms as eg (eg.id)}
          <line
            class="svg-edge"
            class:hovered={hoveredNodeId === eg.id}
            x1={eg.x1}
            y1={eg.y1}
            x2={eg.x2}
            y2={eg.y2}
            tabindex="0"
            role="button"
            aria-label="Edge {eg.edge.edge_type ?? ''} {eg.id}"
            onclick={(e) => emitEdge(e, eg.edge)}
            ondblclick={(e) => emitEdge(e, eg.edge)}
            onmouseenter={(e) => { hoveredNodeId = eg.id; emitEdge(e, eg.edge); }}
            onmouseleave={() => { if (hoveredNodeId === eg.id) hoveredNodeId = null; }}
            oncontextmenu={(e) => { e.preventDefault(); emitEdge(e, eg.edge); }}
            onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); emitEdge(e, eg.edge); } }}
          ><title>{eg.edge.edge_type ?? 'edge'} {eg.id}</title></line>
        {/each}

        <!-- Nodes: addressable SVG groups with CSS-transitioned state -->
        {#each nodeGeoms as g (g.node.id)}
          <g
            class="svg-node"
            class:selected={selectedNodeId === g.node.id}
            class:hovered={hoveredNodeId === g.node.id}
            style="transform: translate({g.x - NODE_W / 2}px, {g.y - NODE_H / 2}px)"
            tabindex="0"
            role="button"
            aria-label="{g.node.node_type ?? 'node'} {g.node.name ?? g.node.id}"
            onclick={(e) => nodeClick(e, g.node)}
            ondblclick={(e) => emitNode(e, g.node)}
            onmouseenter={(e) => { hoveredNodeId = g.node.id; emitNode(e, g.node); }}
            onmouseleave={() => { if (hoveredNodeId === g.node.id) hoveredNodeId = null; }}
            oncontextmenu={(e) => { e.preventDefault(); emitNode(e, g.node); }}
            onkeydown={(e) => nodeKeydown(e, g.node)}
          >
            <rect width={NODE_W} height={NODE_H} rx="7" fill={nodeColor(g.node)}></rect>
            <text x={NODE_W / 2} y={NODE_H / 2 + 4} text-anchor="middle">{label(g.node)}</text>
            <title>{g.node.qualified_name ?? g.node.name ?? g.node.id}</title>
          </g>
        {/each}
      </g>
    </svg>
  </div>
{/if}

<style>
  .svg-canvas {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 320px;
    background: var(--color-bg, #0b1220);
    border: 1px solid var(--color-border, #1e293b);
    border-radius: 8px;
    overflow: hidden;
  }
  .graph-svg {
    flex: 1 1 auto;
    width: 100%;
    min-height: 0;
    cursor: grab;
    touch-action: none;
  }
  .graph-svg:active {
    cursor: grabbing;
  }

  .density-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 12px;
    font-size: 12.5px;
    background: #1e293b;
    color: #fbbf24;
    border-bottom: 1px solid #334155;
  }
  .density-banner.density-warn {
    color: #f87171;
  }
  .density-show-all {
    margin-left: auto;
    background: transparent;
    border: 1px solid #fbbf24;
    color: #fbbf24;
    border-radius: 6px;
    padding: 2px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .density-show-all:hover {
    background: rgba(251, 191, 36, 0.12);
  }

  .svg-toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    font-size: 12px;
    color: #94a3b8;
    border-bottom: 1px solid #1e293b;
    background: #0f172a;
  }
  .svg-zoom-controls {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .svg-zoom-controls button {
    background: #1e293b;
    color: #e2e8f0;
    border: 1px solid #334155;
    border-radius: 5px;
    width: 26px;
    height: 22px;
    cursor: pointer;
    font-size: 13px;
    line-height: 1;
  }
  .svg-zoom-label {
    min-width: 42px;
    text-align: center;
    color: #cbd5e1;
  }

  /* CSS transitions for visual state changes (§10 rationale for SVG) */
  .svg-node {
    transition: opacity 150ms ease;
  }
  .svg-node rect {
    stroke: #0b1220;
    stroke-width: 1.5;
    transition: stroke 150ms ease, stroke-width 150ms ease, filter 150ms ease;
  }
  .svg-node text {
    fill: #f1f5f9;
    font-size: 12px;
    font-family: system-ui, sans-serif;
    pointer-events: none;
    user-select: none;
  }
  .svg-node:hover rect,
  .svg-node:focus-visible rect {
    stroke: #fbbf24;
    stroke-width: 2.5;
  }
  .svg-node.selected rect {
    stroke: #22d3ee;
    stroke-width: 3;
    filter: drop-shadow(0 0 4px rgba(34, 211, 238, 0.6));
  }
  .svg-node:focus-visible,
  .svg-edge:focus-visible {
    outline: none;
  }
  .svg-edge {
    stroke: #475569;
    stroke-width: 1.5;
    transition: stroke 150ms ease, stroke-width 150ms ease;
  }
  .svg-edge:hover,
  .svg-edge:focus-visible,
  .svg-edge.hovered {
    stroke: #fbbf24;
    stroke-width: 2.5;
  }
</style>
