<script>
  /**
   * SpecGraphDAG — Interactive DAG visualization for spec relationships
   *
   * Spec ref: spec-links.md §UI — "interactive DAG at workspace or tenant level.
   * Nodes colored by approval status. Stale links highlighted in yellow.
   * Conflicts highlighted in red. Click to navigate."
   *
   * Props:
   *   nodes    — SpecGraphNode[] from GET /api/v1/specs/graph
   *   edges    — SpecGraphEdge[] from GET /api/v1/specs/graph
   *   onNodeClick — (node) => void — called when a node is clicked
   *   impactPath  — string | null — when set, highlights transitive dependents
   *   onImpactSelect — (node) => void — called in impact mode when node selected
   *   impactMode — boolean — when true, clicking selects for impact analysis
   */

  import { onMount } from 'svelte';
  import { elkLayout } from '../lib/layout-engines.js';

  let { nodes = [], edges = [], onNodeClick = () => {}, impactPath = null, onImpactSelect = () => {}, impactMode = false } = $props();

  // ── Layout state ──────────────────────────────────────────────────────────
  let positions = $state({});
  let layoutReady = $state(false);
  let svgEl = $state(null);

  // ── Pan / Zoom ────────────────────────────────────────────────────────────
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });

  // ── Node dimensions ───────────────────────────────────────────────────────
  const NODE_W = 160;
  const NODE_H = 44;
  const NODE_RX = 6;
  const ARROW_SIZE = 8;

  // ── Colors by approval status ─────────────────────────────────────────────
  function nodeColor(status) {
    switch (status) {
      case 'approved':   return { fill: '#166534', stroke: '#22c55e', text: '#dcfce7' };
      case 'pending':    return { fill: '#854d0e', stroke: '#eab308', text: '#fef9c3' };
      case 'rejected':   return { fill: '#991b1b', stroke: '#ef4444', text: '#fee2e2' };
      case 'deprecated': return { fill: '#374151', stroke: '#6b7280', text: '#d1d5db' };
      default:           return { fill: '#374151', stroke: '#9ca3af', text: '#e5e7eb' };
    }
  }

  // ── Edge styling by link type ─────────────────────────────────────────────
  function edgeStyle(edge) {
    const isStale = edge.status === 'stale';
    const linkType = (edge.link_type ?? '').toLowerCase();

    switch (linkType) {
      case 'dependson':
      case 'depends_on':
        return {
          stroke: isStale ? '#eab308' : '#60a5fa',
          dasharray: '',
          width: 2,
          label: 'depends_on',
        };
      case 'implements':
        return {
          stroke: isStale ? '#eab308' : '#34d399',
          dasharray: '6 3',
          width: 2,
          label: 'implements',
        };
      case 'supersedes':
        return {
          stroke: isStale ? '#eab308' : '#a78bfa',
          dasharray: '3 3',
          width: 2,
          label: 'supersedes',
        };
      case 'conflictswith':
      case 'conflicts_with':
        return {
          stroke: '#ef4444',
          dasharray: '',
          width: 2.5,
          label: 'conflicts_with',
        };
      case 'extends':
        return {
          stroke: isStale ? '#eab308' : '#fb923c',
          dasharray: '8 2 2 2',
          width: 2,
          label: 'extends',
        };
      default:
        return {
          stroke: isStale ? '#eab308' : '#9ca3af',
          dasharray: '',
          width: 1.5,
          label: linkType || 'related',
        };
    }
  }

  // ── Compute layout whenever nodes/edges change ────────────────────────────
  $effect(() => {
    if (nodes.length === 0) {
      positions = {};
      layoutReady = true;
      return;
    }
    layoutReady = false;
    const elkNodes = nodes.map(n => ({
      id: n.path,
      node_type: 'spec',
      width: NODE_W,
      height: NODE_H,
    }));
    const elkEdges = edges.map((e, i) => ({
      id: `e${i}`,
      source_id: e.source,
      target_id: e.target,
    }));
    elkLayout(elkNodes, elkEdges, 'DOWN').then(pos => {
      positions = pos;
      layoutReady = true;
      fitToContent();
    });
  });

  // ── Fit viewBox to content ────────────────────────────────────────────────
  function fitToContent() {
    const keys = Object.keys(positions);
    if (keys.length === 0) return;

    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const k of keys) {
      const p = positions[k];
      minX = Math.min(minX, p.x - NODE_W / 2);
      minY = Math.min(minY, p.y - NODE_H / 2);
      maxX = Math.max(maxX, p.x + NODE_W / 2);
      maxY = Math.max(maxY, p.y + NODE_H / 2);
    }

    const pad = 60;
    viewBox = {
      x: minX - pad,
      y: minY - pad,
      w: maxX - minX + pad * 2,
      h: maxY - minY + pad * 2,
    };
  }

  // ── Zoom handler ──────────────────────────────────────────────────────────
  function handleWheel(e) {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 1.1 : 0.9;
    const rect = svgEl.getBoundingClientRect();
    // Where in the viewBox did the cursor point?
    const cx = viewBox.x + (e.clientX - rect.left) / rect.width * viewBox.w;
    const cy = viewBox.y + (e.clientY - rect.top) / rect.height * viewBox.h;

    const newW = viewBox.w * factor;
    const newH = viewBox.h * factor;
    viewBox = {
      x: cx - (cx - viewBox.x) * factor,
      y: cy - (cy - viewBox.y) * factor,
      w: newW,
      h: newH,
    };
  }

  // ── Pan handlers ──────────────────────────────────────────────────────────
  function handlePointerDown(e) {
    if (e.target.closest('.dag-node')) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    svgEl.setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e) {
    if (!isPanning) return;
    const rect = svgEl.getBoundingClientRect();
    const dx = (e.clientX - panStart.x) / rect.width * viewBox.w;
    const dy = (e.clientY - panStart.y) / rect.height * viewBox.h;
    viewBox = { ...viewBox, x: viewBox.x - dx, y: viewBox.y - dy };
    panStart = { x: e.clientX, y: e.clientY };
  }

  function handlePointerUp() {
    isPanning = false;
  }

  // ── Edge path computation ─────────────────────────────────────────────────
  function edgePath(edge) {
    const sp = positions[edge.source];
    const tp = positions[edge.target];
    if (!sp || !tp) return '';

    // Source exits bottom center, target enters top center
    const sx = sp.x;
    const sy = sp.y + NODE_H / 2;
    const tx = tp.x;
    const ty = tp.y - NODE_H / 2;

    // Smooth bezier curve
    const midY = (sy + ty) / 2;
    return `M ${sx} ${sy} C ${sx} ${midY}, ${tx} ${midY}, ${tx} ${ty}`;
  }

  // ── Arrowhead end point (for marker) ──────────────────────────────────────
  function edgeEndPoint(edge) {
    const tp = positions[edge.target];
    if (!tp) return { x: 0, y: 0 };
    return { x: tp.x, y: tp.y - NODE_H / 2 };
  }

  // ── Node label (short filename) ───────────────────────────────────────────
  function nodeLabel(path) {
    const parts = (path ?? '').split('/');
    const name = parts[parts.length - 1] || path;
    return name.replace(/\.md$/, '');
  }

  // ── Is a conflicts_with edge? ─────────────────────────────────────────────
  function isConflict(edge) {
    const lt = (edge.link_type ?? '').toLowerCase();
    return lt === 'conflictswith' || lt === 'conflicts_with';
  }

  // ── Is this node a target of a supersedes edge? ─────────────────────────
  function isSupersededTarget(nodePath) {
    return edges.some(e => {
      const lt = (e.link_type ?? '').toLowerCase();
      return (lt === 'supersedes') && e.target === nodePath;
    });
  }

  // ── Impact analysis: compute transitive dependents ────────────────────
  // Edges: source depends_on/implements/extends target.
  // Dependents of X = all specs that (transitively) have X as a target.
  // We follow reverse edges: edge.target === X → edge.source is a dependent.
  const IMPACT_LINK_TYPES = new Set(['dependson', 'depends_on', 'implements', 'extends']);

  const impactDependents = $derived.by(() => {
    if (!impactPath) return new Set();

    // Build reverse adjacency: for each target, list all sources
    const reverseAdj = new Map();
    for (const e of edges) {
      const lt = (e.link_type ?? '').toLowerCase();
      if (!IMPACT_LINK_TYPES.has(lt)) continue;
      if (!reverseAdj.has(e.target)) reverseAdj.set(e.target, []);
      reverseAdj.get(e.target).push(e.source);
    }

    // BFS from impactPath following reverse edges
    const visited = new Set();
    const queue = [impactPath];
    while (queue.length > 0) {
      const current = queue.shift();
      const sources = reverseAdj.get(current) ?? [];
      for (const src of sources) {
        if (!visited.has(src)) {
          visited.add(src);
          queue.push(src);
        }
      }
    }
    return visited;
  });

  function isImpactHighlighted(nodePath) {
    if (!impactPath) return false;
    return nodePath === impactPath || impactDependents.has(nodePath);
  }

  function isImpactEdgeHighlighted(edge) {
    if (!impactPath) return false;
    // An edge is highlighted if both endpoints are in the impact set
    return isImpactHighlighted(edge.source) && isImpactHighlighted(edge.target);
  }

  function handleNodeClick(node) {
    if (impactMode) {
      onImpactSelect(node);
    } else {
      onNodeClick(node);
    }
  }
</script>

{#if !layoutReady}
  <div class="dag-loading" data-testid="dag-loading">Computing layout...</div>
{:else if nodes.length === 0}
  <div class="dag-empty" data-testid="dag-empty">No spec relationships to display.</div>
{:else}
  <svg
    bind:this={svgEl}
    class="dag-svg"
    data-testid="dag-svg"
    viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
    onwheel={handleWheel}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    role="img"
    aria-label="Spec relationship graph"
  >
    <defs>
      <!-- Arrow marker for directed edges -->
      <marker
        id="arrow-default"
        viewBox="0 0 10 10"
        refX="10" refY="5"
        markerWidth={ARROW_SIZE} markerHeight={ARROW_SIZE}
        orient="auto-start-reverse"
      >
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#9ca3af" />
      </marker>
      <!-- Colored arrow markers per link type -->
      {#each ['depends_on', 'implements', 'supersedes', 'extends', 'conflicts_with'] as lt}
        {@const style = edgeStyle({ link_type: lt, status: 'active' })}
        <marker
          id="arrow-{lt}"
          viewBox="0 0 10 10"
          refX="10" refY="5"
          markerWidth={ARROW_SIZE} markerHeight={ARROW_SIZE}
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill={style.stroke} />
        </marker>
      {/each}
      <!-- Stale arrow marker (yellow) -->
      <marker
        id="arrow-stale"
        viewBox="0 0 10 10"
        refX="10" refY="5"
        markerWidth={ARROW_SIZE} markerHeight={ARROW_SIZE}
        orient="auto-start-reverse"
      >
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#eab308" />
      </marker>
    </defs>

    <!-- Edges -->
    {#each edges as edge, i}
      {@const style = edgeStyle(edge)}
      {@const path = edgePath(edge)}
      {@const conflict = isConflict(edge)}
      {@const stale = edge.status === 'stale'}
      {@const markerId = stale ? 'arrow-stale' : `arrow-${style.label}`}
      {@const edgeDimmed = impactPath ? !isImpactEdgeHighlighted(edge) : false}
      {#if path}
        <g class="dag-edge" data-testid="dag-edge-{i}" data-link-type={style.label} data-status={edge.status ?? 'active'} opacity={edgeDimmed ? 0.15 : 1}>
          <!-- Hit area (invisible wider stroke for hover) -->
          <path d={path} fill="none" stroke="transparent" stroke-width="12" />
          <!-- Visible edge -->
          <path
            d={path}
            fill="none"
            stroke={style.stroke}
            stroke-width={style.width}
            stroke-dasharray={style.dasharray}
            marker-end={conflict ? '' : `url(#${markerId})`}
          />
          {#if conflict}
            <!-- Bidirectional: add marker on both ends -->
            <path
              d={path}
              fill="none"
              stroke="transparent"
              marker-start="url(#arrow-{style.label})"
              marker-end="url(#arrow-{style.label})"
            />
          {/if}
          <!-- Edge label -->
          {#if positions[edge.source] && positions[edge.target]}
            <text
              class="dag-edge-label"
              x={(positions[edge.source].x + positions[edge.target].x) / 2}
              y={(positions[edge.source].y + positions[edge.target].y) / 2 - 6}
              text-anchor="middle"
              fill={stale ? '#eab308' : '#9ca3af'}
              font-size="9"
            >
              {style.label}{stale ? ' (stale)' : ''}
            </text>
          {/if}
        </g>
      {/if}
    {/each}

    <!-- Nodes -->
    {#each nodes as node}
      {@const pos = positions[node.path]}
      {@const colors = nodeColor(node.approval_status)}
      {@const superseded = isSupersededTarget(node.path)}
      {@const nodeDimmed = impactPath ? !isImpactHighlighted(node.path) : false}
      {@const isImpactRoot = impactPath === node.path}
      {#if pos}
        <g
          class="dag-node"
          class:dag-node-superseded={superseded}
          class:dag-node-impact-root={isImpactRoot}
          data-testid="dag-node-{node.path}"
          data-status={node.approval_status}
          data-superseded={superseded ? 'true' : undefined}
          data-impact={impactPath ? (isImpactRoot ? 'root' : impactDependents.has(node.path) ? 'dependent' : 'dimmed') : undefined}
          opacity={nodeDimmed ? 0.2 : 1}
          transform="translate({pos.x - NODE_W / 2}, {pos.y - NODE_H / 2})"
          tabindex="0"
          role="button"
          aria-label="{nodeLabel(node.path)} — {node.approval_status}{superseded ? ' (superseded)' : ''}{isImpactRoot ? ' (impact analysis root)' : impactDependents.has(node.path) ? ' (dependent)' : ''}"
          onclick={() => handleNodeClick(node)}
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleNodeClick(node); } }}
        >
          {#if isImpactRoot}
            <!-- Highlight ring for impact analysis root -->
            <rect
              width={NODE_W + 6}
              height={NODE_H + 6}
              x="-3"
              y="-3"
              rx={NODE_RX + 2}
              fill="none"
              stroke="#f59e0b"
              stroke-width="2"
              class="dag-impact-ring"
              data-testid="impact-ring"
            />
          {/if}
          <rect
            width={NODE_W}
            height={NODE_H}
            rx={NODE_RX}
            fill={colors.fill}
            stroke={isImpactRoot ? '#f59e0b' : colors.stroke}
            stroke-width={isImpactRoot ? 3 : 2}
          />
          <text
            class="dag-node-label"
            x={NODE_W / 2}
            y={NODE_H / 2 - 4}
            text-anchor="middle"
            dominant-baseline="central"
            fill={colors.text}
            font-size="12"
            font-weight="600"
          >
            {nodeLabel(node.path)}
          </text>
          {#if superseded}
            <!-- Strikethrough line across label text for superseded targets -->
            <line
              class="dag-strikethrough"
              x1={NODE_W * 0.15}
              y1={NODE_H / 2 - 4}
              x2={NODE_W * 0.85}
              y2={NODE_H / 2 - 4}
              stroke={colors.text}
              stroke-width="1.5"
              opacity="0.8"
            />
          {/if}
          <text
            class="dag-node-status"
            x={NODE_W / 2}
            y={NODE_H / 2 + 10}
            text-anchor="middle"
            dominant-baseline="central"
            fill={colors.text}
            font-size="9"
            opacity="0.7"
          >
            {node.approval_status ?? 'unknown'}
          </text>
        </g>
      {/if}
    {/each}
  </svg>
{/if}

<style>
  .dag-svg {
    width: 100%;
    height: 100%;
    min-height: 400px;
    cursor: grab;
    user-select: none;
    background: var(--color-surface, #0a0a0f);
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
  }

  .dag-svg:active {
    cursor: grabbing;
  }

  .dag-node {
    cursor: pointer;
    outline: none;
  }

  .dag-node:hover rect {
    filter: brightness(1.3);
  }

  .dag-node:focus-visible rect {
    stroke-width: 3;
    filter: brightness(1.3);
  }

  .dag-node-label {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
  }

  .dag-node-status {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
    text-transform: capitalize;
  }

  .dag-node-superseded {
    opacity: 0.7;
  }

  .dag-strikethrough {
    pointer-events: none;
  }

  .dag-impact-ring {
    pointer-events: none;
    animation: impact-pulse 2s ease-in-out infinite;
  }

  @keyframes impact-pulse {
    0%, 100% { opacity: 0.6; }
    50% { opacity: 1; }
  }

  .dag-node-impact-root rect {
    filter: brightness(1.2);
  }

  .dag-edge-label {
    pointer-events: none;
    font-family: var(--font-mono, monospace);
  }

  .dag-loading,
  .dag-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 300px;
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 0.875rem);
  }

  @media (prefers-reduced-motion: reduce) {
    .dag-node:hover rect,
    .dag-node:focus-visible rect {
      filter: none;
    }
  }
</style>
