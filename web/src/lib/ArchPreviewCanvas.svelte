<script>
  import { columnLayout } from './layout-engines.js';
  import { t } from 'svelte-i18n';
  import EmptyState from './EmptyState.svelte';

  /**
   * ArchPreviewCanvas — reusable embeddable graph canvas with ghost overlay support.
   *
   * Used by spec detail panels, bidirectional navigation previews, and
   * any surface that needs to render an architecture graph with LLM predictions.
   *
   * Spec: specs/system/ui-navigation.md — TASK-358
   *
   * Props:
   *   nodes          — array of { id, label?, name?, kind?, node_type?, spec_path? }
   *   edges          — array of { source, target, label? } or { source_id, target_id, ... }
   *   ghostOverlays  — array of { nodeId, type: 'new'|'modified'|'removed' }
   *   highlightNodeIds — array of node IDs to highlight (cross-navigation)
   *   size           — 'mini' (~300px height, no pan/zoom) | 'full' (full canvas, pan/zoom)
   *   onNodeClick    — callback(nodeId) when a node is clicked
   */
  let {
    nodes = [],
    edges = [],
    ghostOverlays = [],
    highlightNodeIds = [],
    size = 'full',
    onNodeClick = undefined,
  } = $props();

  // ── Layout ─────────────────────────────────────────────────────────────────
  let nodePositions = $derived.by(() => {
    if (!nodes.length) return {};
    return columnLayout(nodes);
  });

  function getPos(id) {
    return nodePositions[id] ?? { x: 100, y: 100 };
  }

  // ── Ghost overlay lookup ───────────────────────────────────────────────────
  let ghostByNodeId = $derived.by(() => {
    const m = new Map();
    for (const g of ghostOverlays) m.set(g.nodeId, g.type);
    return m;
  });

  // ── Highlight set ──────────────────────────────────────────────────────────
  let highlightSet = $derived.by(() => new Set(highlightNodeIds));

  // ── Canvas bounds ──────────────────────────────────────────────────────────
  let canvasBounds = $derived.by(() => {
    const pos = nodePositions;
    const xs = Object.values(pos).map(p => p.x);
    const ys = Object.values(pos).map(p => p.y);
    if (!xs.length) return { w: 600, h: 300 };
    const w = Math.max(size === 'mini' ? 400 : 600, Math.max(...xs) + 120);
    const h = Math.max(size === 'mini' ? 200 : 400, Math.max(...ys) + 80);
    return { w, h };
  });

  // ── Pan/zoom (full mode only) ──────────────────────────────────────────────
  let svgEl = $state(null);
  let viewBox = $state({ x: 0, y: 0, w: 600, h: 400 });
  let isPanning = $state(false);
  let panStart = { x: 0, y: 0 };

  $effect(() => {
    const b = canvasBounds;
    viewBox = { x: 0, y: 0, w: b.w, h: b.h };
  });

  function onMouseDown(e) {
    if (size !== 'full') return;
    if (e.button !== 0) return;
    if (e.target.closest('.arch-node')) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    e.preventDefault();
  }

  function onMouseMove(e) {
    if (!isPanning || size !== 'full') return;
    const dx = e.clientX - panStart.x;
    const dy = e.clientY - panStart.y;
    const scaleX = viewBox.w / (svgEl?.clientWidth  ?? 600);
    const scaleY = viewBox.h / (svgEl?.clientHeight ?? 400);
    viewBox = { ...viewBox, x: viewBox.x - dx * scaleX, y: viewBox.y - dy * scaleY };
    panStart = { x: e.clientX, y: e.clientY };
  }

  function onMouseUp() { isPanning = false; }

  function onWheel(e) {
    if (size !== 'full') return;
    e.preventDefault();
    const factor = e.deltaY > 0 ? 1.15 : 0.87;
    const rect = svgEl?.getBoundingClientRect();
    const mx = rect ? (e.clientX - rect.left) / rect.width  * viewBox.w + viewBox.x : viewBox.x + viewBox.w / 2;
    const my = rect ? (e.clientY - rect.top)  / rect.height * viewBox.h + viewBox.y : viewBox.y + viewBox.h / 2;
    const newW = Math.max(viewBox.w / 5, Math.min(viewBox.w * 5, viewBox.w * factor));
    const scale = newW / viewBox.w;
    viewBox = { x: mx - (mx - viewBox.x) * scale, y: my - (my - viewBox.y) * scale, w: newW, h: viewBox.h * scale };
  }

  function resetView() {
    const b = canvasBounds;
    viewBox = { x: 0, y: 0, w: b.w, h: b.h };
  }

  // ── Node interaction ───────────────────────────────────────────────────────
  function handleNodeClick(node) {
    onNodeClick?.(node.id);
  }

  // ── Node rendering helpers ─────────────────────────────────────────────────
  function nodeLabel(node) {
    const raw = node.label ?? node.name ?? node.id ?? '';
    return raw.substring(0, size === 'mini' ? 10 : 14);
  }

  function nodeTypeColor(type) {
    switch (type) {
      case 'package':   return { fill: '#3b1fa8', stroke: '#7c5ff5' };
      case 'module':    return { fill: '#1a3a6b', stroke: '#4a9eff' };
      case 'type':      return { fill: '#14532d', stroke: '#22c55e' };
      case 'interface': return { fill: '#78350f', stroke: '#f59e0b' };
      case 'function':  return { fill: '#134e4a', stroke: '#14b8a6' };
      case 'endpoint':  return { fill: '#7f1d1d', stroke: '#ef4444' };
      case 'component': return { fill: '#4a1d96', stroke: '#a78bfa' };
      case 'table':     return { fill: '#374151', stroke: '#9ca3af' };
      default:          return { fill: '#1e293b', stroke: '#64748b' };
    }
  }

  function ghostOverlayStyle(ghostType) {
    switch (ghostType) {
      case 'new':      return { stroke: '#22c55e', dasharray: '5 3' };
      case 'modified': return { stroke: '#eab308', dasharray: '5 3' };
      case 'removed':  return { stroke: '#ef4444', dasharray: '5 3' };
      default:         return null;
    }
  }

  function rectPath(cx, cy, w, h) {
    const r = 3;
    const x = cx - w / 2, y = cy - h / 2;
    return `M${x},${y + r} Q${x},${y} ${x + r},${y} L${x + w - r},${y} Q${x + w},${y} ${x + w},${y + r} L${x + w},${y + h - r} Q${x + w},${y + h} ${x + w - r},${y + h} L${x + r},${y + h} Q${x},${y + h} ${x},${y + h - r} Z`;
  }

  // edge source/target normalisation
  function edgeSrc(e) { return e.source_id ?? e.source ?? e.from_node_id ?? e.from; }
  function edgeTgt(e) { return e.target_id ?? e.target ?? e.to_node_id ?? e.to; }

  const nodeW = 64;
  const nodeH = 28;
</script>

<div class="arch-canvas-wrap" class:mini={size === 'mini'} class:full={size === 'full'}>
  {#if !nodes.length}
    <EmptyState title={$t('explorer_canvas.no_graph')} description={$t('explorer_canvas.no_graph_desc')} />
  {:else}
    {#if size === 'full'}
      <div class="arch-toolbar">
        <button class="arch-tool-btn" onclick={resetView} aria-label={$t('explorer_canvas.reset')}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><path d="M3 12a9 9 0 109-9M3 12V7m0 5H8"/></svg>
          {$t('explorer_canvas.reset')}
        </button>
        <span class="arch-node-count">{nodes.length} {nodes.length === 1 ? $t('explorer_canvas.node') : $t('explorer_canvas.nodes')} · {edges.length} {edges.length === 1 ? $t('explorer_canvas.edge') : $t('explorer_canvas.edges')}</span>
        {#if ghostOverlays.length}
          <span class="arch-ghost-legend">
            <span class="ghost-chip new">{$t('explorer_canvas.ghost_new')}</span>
            <span class="ghost-chip modified">{$t('explorer_canvas.ghost_modified')}</span>
            <span class="ghost-chip removed">{$t('explorer_canvas.ghost_removed')}</span>
          </span>
        {/if}
      </div>
    {/if}

    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <svg
      bind:this={svgEl}
      class="arch-svg"
      class:panning={isPanning}
      viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
      role="application"
      aria-label={$t('explorer_canvas.arch_preview')}
      onmousedown={onMouseDown}
      onmousemove={onMouseMove}
      onmouseup={onMouseUp}
      onmouseleave={onMouseUp}
      onwheel={size === 'full' ? onWheel : undefined}
      data-testid="arch-preview-svg">
      <defs>
        <marker id="arch-arrow" markerWidth="7" markerHeight="7" refX="5" refY="3" orient="auto">
          <path d="M0,0 L0,6 L7,3 z" fill="#475569" />
        </marker>
      </defs>

      <!-- Edges -->
      {#each edges as edge}
        {@const from = getPos(edgeSrc(edge))}
        {@const to = getPos(edgeTgt(edge))}
        <line
          class="arch-edge"
          x1={from.x} y1={from.y}
          x2={to.x} y2={to.y}
          stroke="#475569"
          stroke-width="1.2"
          stroke-opacity="0.6"
          marker-end="url(#arch-arrow)" />
      {/each}

      <!-- Nodes -->
      {#each nodes as node}
        {@const pos = getPos(node.id)}
        {@const colors = nodeTypeColor(node.node_type ?? node.kind)}
        {@const isHighlighted = highlightSet.has(node.id)}
        {@const ghostType = ghostByNodeId.get(node.id)}
        {@const ghost = ghostType ? ghostOverlayStyle(ghostType) : null}
        {@const isGhost = !!ghost}
        <g
          class="arch-node"
          class:highlighted={isHighlighted}
          class:ghost-node={isGhost}
          data-node-id={node.id}
          data-ghost-type={ghostType ?? ''}
          transform="translate({pos.x},{pos.y})"
          role="button"
          tabindex="0"
          aria-label="{node.node_type ?? node.kind ?? 'node'}: {node.label ?? node.name ?? node.id}"
          onclick={() => handleNodeClick(node)}
          onkeydown={(e) => e.key === 'Enter' && handleNodeClick(node)}>
          <!-- Ghost border overlay (dashed ring) -->
          {#if ghost}
            <rect
              class="ghost-border"
              x={-nodeW / 2 - 4}
              y={-nodeH / 2 - 4}
              width={nodeW + 8}
              height={nodeH + 8}
              rx="5"
              fill="none"
              stroke={ghost.stroke}
              stroke-width="2"
              stroke-dasharray={ghost.dasharray}
              pointer-events="none"
              aria-hidden="true" />
          {/if}
          <!-- Node body -->
          <path
            d={rectPath(0, 0, nodeW, nodeH)}
            fill={isGhost ? 'transparent' : colors.fill}
            stroke={isHighlighted ? '#facc15' : colors.stroke}
            stroke-width={isHighlighted ? 2.5 : isGhost ? 1 : 1.5}
            stroke-dasharray={isGhost ? '4 2' : 'none'}
            opacity={isGhost ? 0.55 : 0.9} />
          <!-- Highlight ring -->
          {#if isHighlighted}
            <rect
              class="highlight-ring"
              x={-nodeW / 2 - 5}
              y={-nodeH / 2 - 5}
              width={nodeW + 10}
              height={nodeH + 10}
              rx="6"
              fill="none"
              stroke="#facc15"
              stroke-width="1.5"
              stroke-dasharray="none"
              opacity="0.7"
              pointer-events="none"
              aria-hidden="true" />
          {/if}
          <!-- Label -->
          <text
            text-anchor="middle"
            dominant-baseline="middle"
            font-size={size === 'mini' ? 8 : 9}
            fill={isGhost ? '#94a3b8' : '#f1f5f9'}
            pointer-events="none"
            style="font-family: var(--font-mono, monospace); user-select: none">
            {nodeLabel(node)}
          </text>
        </g>
      {/each}
    </svg>
  {/if}
</div>

<style>
  .arch-canvas-wrap {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-surface, #0f172a);
  }

  .arch-canvas-wrap.mini {
    height: 300px;
  }

  .arch-canvas-wrap.full {
    height: 100%;
    min-height: 400px;
  }

  .arch-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-3, 8px);
    padding: var(--space-2, 4px) var(--space-3, 8px);
    border-bottom: 1px solid var(--color-border, #1e293b);
    background: var(--color-surface, #0f172a);
    flex-shrink: 0;
  }

  .arch-tool-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1, 2px);
    padding: 2px var(--space-2, 4px);
    background: var(--color-surface-elevated, #1e293b);
    border: 1px solid var(--color-border-strong, #334155);
    border-radius: var(--radius, 4px);
    color: var(--color-text-secondary, #94a3b8);
    cursor: pointer;
    font-size: var(--text-xs, 11px);
    font-family: var(--font-body, sans-serif);
    transition: border-color 0.15s, color 0.15s;
  }

  .arch-tool-btn:hover {
    border-color: var(--color-focus, #60a5fa);
    color: var(--color-text, #f1f5f9);
  }

  .arch-tool-btn:focus-visible {
    outline: 2px solid var(--color-focus, #60a5fa);
    outline-offset: 2px;
  }

  .arch-node-count {
    font-size: var(--text-xs, 11px);
    color: var(--color-text-muted, #64748b);
    font-family: var(--font-mono, monospace);
  }

  .arch-ghost-legend {
    display: flex;
    align-items: center;
    gap: var(--space-2, 4px);
    margin-left: auto;
  }

  .ghost-chip {
    font-size: 10px;
    font-family: var(--font-mono, monospace);
    padding: 1px 5px;
    border-radius: 3px;
    border: 1px dashed;
  }

  .ghost-chip.new {
    color: #22c55e;
    border-color: #22c55e;
    background: color-mix(in srgb, #22c55e 8%, transparent);
  }

  .ghost-chip.modified {
    color: #eab308;
    border-color: #eab308;
    background: color-mix(in srgb, #eab308 8%, transparent);
  }

  .ghost-chip.removed {
    color: #ef4444;
    border-color: #ef4444;
    background: color-mix(in srgb, #ef4444 8%, transparent);
  }

  .arch-svg {
    flex: 1;
    width: 100%;
    height: 100%;
    background: var(--color-surface, #0f172a);
    display: block;
    cursor: default;
  }

  .arch-canvas-wrap.full .arch-svg {
    cursor: grab;
  }

  .arch-svg.panning {
    cursor: grabbing;
  }

  .arch-node {
    cursor: pointer;
  }

  .arch-node:hover path {
    filter: brightness(1.3);
  }

  .arch-node.highlighted path {
    filter: brightness(1.4);
  }

  .arch-node.ghost-node {
    cursor: default;
    pointer-events: none;
  }

  .arch-node:focus-visible {
    outline: 2px solid var(--color-focus, #60a5fa);
    outline-offset: 3px;
  }

  .arch-edge {
    stroke: #475569;
  }

  @media (prefers-reduced-motion: reduce) {
    .arch-node,
    .arch-node:hover path,
    .arch-node.highlighted path {
      transition: none;
      filter: none;
    }
  }
</style>
