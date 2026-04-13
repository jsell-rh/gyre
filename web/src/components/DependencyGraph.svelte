<script>
  /**
   * DependencyGraph — Interactive DAG of cross-repo dependencies
   *
   * Spec ref: dependency-graph.md §UI — "interactive DAG at workspace and tenant
   * level. Nodes = repos, edges = dependencies colored by type. Stale edges in
   * yellow, breaking in red. Click a node to see impact analysis."
   *
   * Props:
   *   nodes     — GraphNode[] ({ repo_id, name })
   *   edges     — GraphEdgeResponse[] ({ id, source, target, type, status })
   *   scope     — 'workspace' | 'tenant'
   *   onScopeChange — (scope) => void
   *   onNodeClick   — (node) => void — navigate to repo detail / impact analysis
   */

  import { elkLayout } from '../lib/layout-engines.js';

  let {
    nodes = [],
    edges = [],
    scope = 'workspace',
    onScopeChange = () => {},
    onNodeClick = () => {},
  } = $props();

  // ── Layout state ──────────────────────────────────────────────────────────
  let positions = $state({});
  let layoutReady = $state(false);
  let svgEl = $state(null);

  // ── Pan / Zoom ────────────────────────────────────────────────────────────
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });

  // ── Hover state ───────────────────────────────────────────────────────────
  let hoveredNodeId = $state(null);

  // ── Node dimensions ───────────────────────────────────────────────────────
  const NODE_W = 160;
  const NODE_H = 52;
  const NODE_RX = 8;
  const ARROW_SIZE = 8;

  // ── Adjacency for hover highlight ─────────────────────────────────────────
  let adjacency = $derived.by(() => {
    const deps = new Map();   // repo_id → Set of repo_ids this node depends on
    const rdeps = new Map();  // repo_id → Set of repo_ids that depend on this node
    for (const e of edges) {
      if (!deps.has(e.source)) deps.set(e.source, new Set());
      deps.get(e.source).add(e.target);
      if (!rdeps.has(e.target)) rdeps.set(e.target, new Set());
      rdeps.get(e.target).add(e.source);
    }
    return { deps, rdeps };
  });

  /** Is a given repo_id highlighted by hover? (self, dependency, or dependent) */
  function isHighlighted(repoId) {
    if (!hoveredNodeId) return false;
    if (repoId === hoveredNodeId) return true;
    return (adjacency.deps.get(hoveredNodeId)?.has(repoId) ?? false)
        || (adjacency.rdeps.get(hoveredNodeId)?.has(repoId) ?? false);
  }

  /** Is a given edge highlighted by hover? */
  function isEdgeHighlighted(edge) {
    if (!hoveredNodeId) return false;
    return edge.source === hoveredNodeId || edge.target === hoveredNodeId;
  }

  // ── Node health indicator ─────────────────────────────────────────────────
  function nodeHealth(repoId) {
    const repoEdges = edges.filter(e => e.source === repoId || e.target === repoId);
    const hasBreaking = repoEdges.some(e => e.status === 'breaking');
    const hasStale = repoEdges.some(e => e.status === 'stale');
    if (hasBreaking) return 'breaking';
    if (hasStale) return 'stale';
    return 'healthy';
  }

  function nodeColors(health) {
    switch (health) {
      case 'breaking': return { fill: '#450a0a', stroke: '#ef4444', text: '#fecaca', badge: '#ef4444' };
      case 'stale':    return { fill: '#422006', stroke: '#eab308', text: '#fef9c3', badge: '#eab308' };
      default:         return { fill: '#1e293b', stroke: '#60a5fa', text: '#e2e8f0', badge: '#22c55e' };
    }
  }

  // ── Edge styling by dependency type ───────────────────────────────────────
  function edgeStyle(edge) {
    const isStale = edge.status === 'stale';
    const isBreaking = edge.status === 'breaking';
    const depType = edge.type ?? '';

    if (isBreaking) {
      return { stroke: '#ef4444', dasharray: '', width: 2.5, label: 'breaking' };
    }
    if (isStale) {
      return { stroke: '#eab308', dasharray: '', width: 2, label: depType };
    }

    switch (depType) {
      case 'code':
        return { stroke: '#60a5fa', dasharray: '', width: 2, label: 'code' };
      case 'spec':
        return { stroke: '#a78bfa', dasharray: '6 3', width: 2, label: 'spec' };
      case 'api':
        return { stroke: '#34d399', dasharray: '3 3', width: 2, label: 'api' };
      case 'schema':
        return { stroke: '#fb923c', dasharray: '8 2 2 2', width: 2, label: 'schema' };
      case 'manual':
        return { stroke: '#9ca3af', dasharray: '4 4', width: 1.5, label: 'manual' };
      default:
        return { stroke: '#9ca3af', dasharray: '', width: 1.5, label: depType || 'dep' };
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
      id: n.repo_id,
      node_type: 'repo',
      width: NODE_W,
      height: NODE_H,
    }));
    const elkEdges = edges.map((e, i) => ({
      id: e.id ?? `e${i}`,
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
    if (e.target.closest('.dep-node')) return;
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

    const sx = sp.x;
    const sy = sp.y + NODE_H / 2;
    const tx = tp.x;
    const ty = tp.y - NODE_H / 2;

    const midY = (sy + ty) / 2;
    return `M ${sx} ${sy} C ${sx} ${midY}, ${tx} ${midY}, ${tx} ${ty}`;
  }

  // ── Health badge icon ─────────────────────────────────────────────────────
  function healthBadge(health) {
    switch (health) {
      case 'breaking': return '!';
      case 'stale':    return '~';
      default:         return '✓';
    }
  }
</script>

<div class="dep-graph-container" data-testid="dep-graph">
  <!-- Scope toggle -->
  <div class="dep-scope-toggle" data-testid="dep-scope-toggle">
    <button
      class="dep-scope-btn"
      class:active={scope === 'workspace'}
      onclick={() => onScopeChange('workspace')}
      data-testid="dep-scope-workspace"
    >Workspace</button>
    <button
      class="dep-scope-btn"
      class:active={scope === 'tenant'}
      onclick={() => onScopeChange('tenant')}
      data-testid="dep-scope-tenant"
    >Tenant</button>
  </div>

  <!-- Legend -->
  <div class="dep-legend" data-testid="dep-legend">
    <span class="dep-legend-item"><span class="dep-legend-swatch" style="background: #60a5fa"></span>Code</span>
    <span class="dep-legend-item"><span class="dep-legend-swatch dep-legend-dashed" style="border-color: #a78bfa"></span>Spec</span>
    <span class="dep-legend-item"><span class="dep-legend-swatch dep-legend-dotted" style="border-color: #34d399"></span>API</span>
    <span class="dep-legend-item"><span class="dep-legend-swatch" style="background: #eab308"></span>Stale</span>
    <span class="dep-legend-item"><span class="dep-legend-swatch" style="background: #ef4444"></span>Breaking</span>
  </div>

  {#if !layoutReady}
    <div class="dep-loading" data-testid="dep-loading">Computing layout...</div>
  {:else if nodes.length === 0}
    <div class="dep-empty" data-testid="dep-empty">No cross-repo dependencies to display.</div>
  {:else}
    <svg
      bind:this={svgEl}
      class="dep-svg"
      data-testid="dep-svg"
      viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
      onwheel={handleWheel}
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={handlePointerUp}
      role="img"
      aria-label="Cross-repo dependency graph"
    >
      <defs>
        <!-- Arrow markers by dependency type -->
        {#each [
          { id: 'code', color: '#60a5fa' },
          { id: 'spec', color: '#a78bfa' },
          { id: 'api', color: '#34d399' },
          { id: 'schema', color: '#fb923c' },
          { id: 'manual', color: '#9ca3af' },
          { id: 'stale', color: '#eab308' },
          { id: 'breaking', color: '#ef4444' },
          { id: 'dep', color: '#9ca3af' },
        ] as marker}
          <marker
            id="dep-arrow-{marker.id}"
            viewBox="0 0 10 10"
            refX="10" refY="5"
            markerWidth={ARROW_SIZE} markerHeight={ARROW_SIZE}
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" fill={marker.color} />
          </marker>
        {/each}
      </defs>

      <!-- Edges -->
      {#each edges as edge, i}
        {@const style = edgeStyle(edge)}
        {@const path = edgePath(edge)}
        {@const highlighted = isEdgeHighlighted(edge)}
        {@const dimmed = hoveredNodeId && !highlighted}
        {@const markerId = edge.status === 'breaking' ? 'dep-arrow-breaking'
                         : edge.status === 'stale' ? 'dep-arrow-stale'
                         : `dep-arrow-${style.label}`}
        {#if path}
          <g
            class="dep-edge"
            data-testid="dep-edge-{i}"
            data-type={edge.type}
            data-status={edge.status}
            opacity={dimmed ? 0.15 : 1}
          >
            <path d={path} fill="none" stroke="transparent" stroke-width="12" />
            <path
              d={path}
              fill="none"
              stroke={style.stroke}
              stroke-width={highlighted ? style.width + 1 : style.width}
              stroke-dasharray={style.dasharray}
              marker-end="url(#{markerId})"
            />
            <!-- Edge type label -->
            {#if positions[edge.source] && positions[edge.target]}
              <text
                class="dep-edge-label"
                x={(positions[edge.source].x + positions[edge.target].x) / 2}
                y={(positions[edge.source].y + positions[edge.target].y) / 2 - 6}
                text-anchor="middle"
                fill={style.stroke}
                font-size="9"
                opacity={dimmed ? 0.3 : 0.8}
              >
                {style.label}
              </text>
            {/if}
            <!-- Breaking badge on edge -->
            {#if edge.status === 'breaking' && positions[edge.source] && positions[edge.target]}
              <text
                x={(positions[edge.source].x + positions[edge.target].x) / 2 + 18}
                y={(positions[edge.source].y + positions[edge.target].y) / 2 - 5}
                fill="#ef4444"
                font-size="14"
                font-weight="bold"
              >⚠</text>
            {/if}
          </g>
        {/if}
      {/each}

      <!-- Nodes -->
      {#each nodes as node}
        {@const pos = positions[node.repo_id]}
        {@const health = nodeHealth(node.repo_id)}
        {@const colors = nodeColors(health)}
        {@const highlighted = isHighlighted(node.repo_id)}
        {@const dimmed = hoveredNodeId && !highlighted}
        {#if pos}
          <g
            class="dep-node"
            data-testid="dep-node-{node.repo_id}"
            data-health={health}
            transform="translate({pos.x - NODE_W / 2}, {pos.y - NODE_H / 2})"
            tabindex="0"
            role="button"
            aria-label="{node.name} — {health}"
            opacity={dimmed ? 0.25 : 1}
            onclick={() => onNodeClick(node)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onNodeClick(node); } }}
            onpointerenter={() => { hoveredNodeId = node.repo_id; }}
            onpointerleave={() => { hoveredNodeId = null; }}
          >
            <rect
              width={NODE_W}
              height={NODE_H}
              rx={NODE_RX}
              fill={colors.fill}
              stroke={highlighted ? '#f8fafc' : colors.stroke}
              stroke-width={highlighted ? 2.5 : 1.5}
            />
            <!-- Repo name -->
            <text
              class="dep-node-label"
              x={NODE_W / 2}
              y={NODE_H / 2 - 6}
              text-anchor="middle"
              dominant-baseline="central"
              fill={colors.text}
              font-size="13"
              font-weight="600"
            >
              {node.name}
            </text>
            <!-- Health indicator -->
            <text
              class="dep-node-health"
              x={NODE_W / 2}
              y={NODE_H / 2 + 12}
              text-anchor="middle"
              dominant-baseline="central"
              fill={colors.badge}
              font-size="10"
              opacity="0.8"
            >
              {healthBadge(health)} {health}
            </text>
          </g>
        {/if}
      {/each}
    </svg>
  {/if}
</div>

<style>
  .dep-graph-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
  }

  .dep-scope-toggle {
    display: flex;
    gap: 4px;
    align-self: flex-start;
  }

  .dep-scope-btn {
    padding: 4px 12px;
    font-size: var(--text-xs, 0.75rem);
    font-family: var(--font-body, system-ui);
    color: var(--color-text-muted, #888);
    background: transparent;
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
    cursor: pointer;
    transition: all 0.15s;
  }

  .dep-scope-btn:hover {
    color: var(--color-text, #e5e7eb);
    border-color: var(--color-border-hover, #444);
  }

  .dep-scope-btn.active {
    color: var(--color-text, #e5e7eb);
    background: var(--color-surface-raised, #1e293b);
    border-color: #60a5fa;
  }

  .dep-legend {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text-muted, #888);
  }

  .dep-legend-item {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .dep-legend-swatch {
    display: inline-block;
    width: 16px;
    height: 3px;
    border-radius: 1px;
  }

  .dep-legend-dashed {
    background: none;
    border-top: 2px dashed;
    height: 0;
  }

  .dep-legend-dotted {
    background: none;
    border-top: 2px dotted;
    height: 0;
  }

  .dep-svg {
    width: 100%;
    flex: 1;
    min-height: 400px;
    cursor: grab;
    user-select: none;
    background: var(--color-surface, #0a0a0f);
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
  }

  .dep-svg:active {
    cursor: grabbing;
  }

  .dep-node {
    cursor: pointer;
    outline: none;
    transition: opacity 0.15s;
  }

  .dep-node:hover rect {
    filter: brightness(1.3);
  }

  .dep-node:focus-visible rect {
    stroke-width: 3;
    filter: brightness(1.3);
  }

  .dep-node-label {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
  }

  .dep-node-health {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
    text-transform: capitalize;
  }

  .dep-edge {
    transition: opacity 0.15s;
  }

  .dep-edge-label {
    pointer-events: none;
    font-family: var(--font-mono, monospace);
  }

  .dep-loading,
  .dep-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 300px;
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 0.875rem);
  }

  @media (prefers-reduced-motion: reduce) {
    .dep-node:hover rect,
    .dep-node:focus-visible rect {
      filter: none;
    }
    .dep-node,
    .dep-edge {
      transition: none;
    }
  }
</style>
