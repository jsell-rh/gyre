<script>
  /**
   * MergeQueueGraph — Interactive DAG of queued MRs with dependency edges
   *
   * Spec ref: merge-dependencies.md §Merge Queue Integration > Visualization —
   * "Returns a DAG of queued MRs with dependency edges, gate status per node,
   * and atomic group boundaries. The dashboard renders this as a visual pipeline
   * showing what's blocked on what."
   *
   * Props:
   *   nodes       — GraphNode[] from GET /api/v1/merge-queue/graph
   *   onNodeClick — (node) => void — navigate to MR detail
   */

  import { elkLayout } from '../lib/layout-engines.js';

  let { nodes = [], onNodeClick = () => {} } = $props();

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
  const NODE_W = 180;
  const NODE_H = 56;
  const NODE_RX = 8;
  const ARROW_SIZE = 8;
  const GROUP_PAD = 16;

  // ── Derived: build edges from node depends_on ─────────────────────────────
  let graphEdges = $derived.by(() => {
    const edgeList = [];
    const nodeIds = new Set(nodes.map(n => n.mr_id));
    for (const node of nodes) {
      for (const dep of (node.depends_on ?? [])) {
        if (nodeIds.has(dep.mr_id)) {
          edgeList.push({
            id: `${dep.mr_id}->${node.mr_id}`,
            source: dep.mr_id,
            target: node.mr_id,
            depSource: dep.source,
          });
        }
      }
    }
    return edgeList;
  });

  // ── Derived: atomic groups ────────────────────────────────────────────────
  let atomicGroups = $derived.by(() => {
    const groups = new Map();
    for (const node of nodes) {
      if (node.atomic_group) {
        if (!groups.has(node.atomic_group)) {
          groups.set(node.atomic_group, []);
        }
        groups.get(node.atomic_group).push(node.mr_id);
      }
    }
    return groups;
  });

  // ── Derived: adjacency for hover highlight ────────────────────────────────
  let adjacency = $derived.by(() => {
    const deps = new Map();
    const rdeps = new Map();
    for (const e of graphEdges) {
      if (!deps.has(e.target)) deps.set(e.target, new Set());
      deps.get(e.target).add(e.source);
      if (!rdeps.has(e.source)) rdeps.set(e.source, new Set());
      rdeps.get(e.source).add(e.target);
    }
    return { deps, rdeps };
  });

  // ── Node status helpers ───────────────────────────────────────────────────

  /** Check if an MR is blocked (has unmerged dependencies) */
  function isBlocked(node) {
    if (!node.depends_on?.length) return false;
    const nodeMap = new Map(nodes.map(n => [n.mr_id, n]));
    return node.depends_on.some(dep => {
      const depNode = nodeMap.get(dep.mr_id);
      return depNode && depNode.status !== 'merged';
    });
  }

  /** Node fill/stroke/text colors based on status */
  function nodeColors(node) {
    const status = node.status ?? '';
    if (status === 'merged') {
      return { fill: '#14532d', stroke: '#22c55e', text: '#dcfce7', badge: '#22c55e' };
    }
    if (status === 'closed' || status === 'reverted') {
      return { fill: '#450a0a', stroke: '#ef4444', text: '#fecaca', badge: '#ef4444' };
    }
    if (isBlocked(node)) {
      return { fill: '#1c1917', stroke: '#78716c', text: '#a8a29e', badge: '#78716c' };
    }
    if (status === 'approved') {
      return { fill: '#1e3a5f', stroke: '#60a5fa', text: '#dbeafe', badge: '#60a5fa' };
    }
    // open or other
    return { fill: '#1e293b', stroke: '#94a3b8', text: '#e2e8f0', badge: '#94a3b8' };
  }

  /** Status badge text */
  function statusBadge(node) {
    if (node.status === 'merged') return 'merged';
    if (node.status === 'closed') return 'closed';
    if (node.status === 'reverted') return 'reverted';
    if (isBlocked(node)) return 'blocked';
    if (node.status === 'approved') return 'ready';
    return 'open';
  }

  /** Edge color based on dependency status */
  function edgeColor(edge) {
    const nodeMap = new Map(nodes.map(n => [n.mr_id, n]));
    const depNode = nodeMap.get(edge.source);
    if (!depNode) return '#9ca3af';
    if (depNode.status === 'merged') return '#22c55e';
    if (depNode.status === 'closed' || depNode.status === 'reverted') return '#ef4444';
    return '#eab308';
  }

  /** Is node highlighted by hover? */
  function isHighlighted(mrId) {
    if (!hoveredNodeId) return false;
    if (mrId === hoveredNodeId) return true;
    return (adjacency.deps.get(hoveredNodeId)?.has(mrId) ?? false)
        || (adjacency.rdeps.get(hoveredNodeId)?.has(mrId) ?? false);
  }

  /** Is edge highlighted by hover? */
  function isEdgeHighlighted(edge) {
    if (!hoveredNodeId) return false;
    return edge.source === hoveredNodeId || edge.target === hoveredNodeId;
  }

  /** Truncate text to max length */
  function truncate(text, max = 22) {
    if (!text || text.length <= max) return text ?? '';
    return text.slice(0, max - 1) + '…';
  }

  // ── Tooltip ───────────────────────────────────────────────────────────────
  let tooltip = $state(null);

  function showTooltip(node, e) {
    const rect = svgEl.getBoundingClientRect();
    const nodeMap = new Map(nodes.map(n => [n.mr_id, n]));
    const blockingDeps = (node.depends_on ?? [])
      .filter(dep => {
        const depNode = nodeMap.get(dep.mr_id);
        return depNode && depNode.status !== 'merged';
      })
      .map(dep => {
        const depNode = nodeMap.get(dep.mr_id);
        return depNode?.title ?? dep.mr_id;
      });
    tooltip = {
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
      title: node.title ?? node.mr_id,
      status: statusBadge(node),
      priority: node.priority,
      group: node.atomic_group,
      blockingDeps,
    };
  }

  function hideTooltip() {
    tooltip = null;
  }

  // ── Compute layout whenever nodes change ──────────────────────────────────
  $effect(() => {
    if (nodes.length === 0) {
      positions = {};
      layoutReady = true;
      return;
    }
    layoutReady = false;
    const elkNodes = nodes.map(n => ({
      id: n.mr_id,
      node_type: 'mr',
      width: NODE_W,
      height: NODE_H,
    }));
    const elkEdges = graphEdges.map(e => ({
      id: e.id,
      source_id: e.source,
      target_id: e.target,
    }));
    elkLayout(elkNodes, elkEdges, 'RIGHT').then(pos => {
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
    if (e.target.closest('.mq-node')) return;
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

  // ── Edge path computation (left-to-right) ─────────────────────────────────
  function edgePath(edge) {
    const sp = positions[edge.source];
    const tp = positions[edge.target];
    if (!sp || !tp) return '';

    const sx = sp.x + NODE_W / 2;
    const sy = sp.y;
    const tx = tp.x - NODE_W / 2;
    const ty = tp.y;

    const midX = (sx + tx) / 2;
    return `M ${sx} ${sy} C ${midX} ${sy}, ${midX} ${ty}, ${tx} ${ty}`;
  }

  // ── Atomic group boundary ─────────────────────────────────────────────────
  function groupBounds(memberIds) {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const id of memberIds) {
      const p = positions[id];
      if (!p) continue;
      minX = Math.min(minX, p.x - NODE_W / 2);
      minY = Math.min(minY, p.y - NODE_H / 2);
      maxX = Math.max(maxX, p.x + NODE_W / 2);
      maxY = Math.max(maxY, p.y + NODE_H / 2);
    }
    if (minX === Infinity) return null;
    return {
      x: minX - GROUP_PAD,
      y: minY - GROUP_PAD,
      w: maxX - minX + GROUP_PAD * 2,
      h: maxY - minY + GROUP_PAD * 2,
    };
  }
</script>

{#if !layoutReady}
  <div class="mq-loading" data-testid="mq-loading">Computing layout...</div>
{:else if nodes.length === 0}
  <div class="mq-empty" data-testid="mq-empty">No MRs in the merge queue.</div>
{:else}
  <div class="mq-graph-container" data-testid="mq-graph">
    <!-- Legend -->
    <div class="mq-legend" data-testid="mq-legend">
      <span class="mq-legend-item"><span class="mq-legend-swatch" style="background: #22c55e"></span>Satisfied</span>
      <span class="mq-legend-item"><span class="mq-legend-swatch" style="background: #eab308"></span>Pending</span>
      <span class="mq-legend-item"><span class="mq-legend-swatch" style="background: #ef4444"></span>Failed</span>
      <span class="mq-legend-item"><span class="mq-legend-swatch mq-legend-dashed" style="border-color: #a78bfa"></span>Atomic group</span>
    </div>

    <svg
      bind:this={svgEl}
      class="mq-svg"
      data-testid="mq-svg"
      viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
      onwheel={handleWheel}
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={handlePointerUp}
      role="img"
      aria-label="Merge queue dependency graph"
    >
      <defs>
        <!-- Arrow markers -->
        {#each [
          { id: 'satisfied', color: '#22c55e' },
          { id: 'pending', color: '#eab308' },
          { id: 'failed', color: '#ef4444' },
          { id: 'default', color: '#9ca3af' },
        ] as marker}
          <marker
            id="mq-arrow-{marker.id}"
            viewBox="0 0 10 10"
            refX="10" refY="5"
            markerWidth={ARROW_SIZE} markerHeight={ARROW_SIZE}
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" fill={marker.color} />
          </marker>
        {/each}
      </defs>

      <!-- Atomic group boundaries -->
      {#each [...atomicGroups.entries()] as [groupName, memberIds]}
        {@const bounds = groupBounds(memberIds)}
        {#if bounds}
          <g class="mq-group" data-testid="mq-group-{groupName}">
            <rect
              x={bounds.x}
              y={bounds.y}
              width={bounds.w}
              height={bounds.h}
              rx="12"
              fill="rgba(167, 139, 250, 0.06)"
              stroke="#a78bfa"
              stroke-width="1.5"
              stroke-dasharray="6 4"
            />
            <text
              x={bounds.x + 8}
              y={bounds.y + 14}
              fill="#a78bfa"
              font-size="10"
              font-weight="600"
              opacity="0.8"
            >
              {groupName}
            </text>
          </g>
        {/if}
      {/each}

      <!-- Edges -->
      {#each graphEdges as edge}
        {@const color = edgeColor(edge)}
        {@const path = edgePath(edge)}
        {@const highlighted = isEdgeHighlighted(edge)}
        {@const dimmed = hoveredNodeId && !highlighted}
        {@const markerId = color === '#22c55e' ? 'mq-arrow-satisfied'
                         : color === '#ef4444' ? 'mq-arrow-failed'
                         : color === '#eab308' ? 'mq-arrow-pending'
                         : 'mq-arrow-default'}
        {#if path}
          <g
            class="mq-edge"
            data-testid="mq-edge-{edge.id}"
            data-source={edge.depSource}
            opacity={dimmed ? 0.15 : 1}
          >
            <path d={path} fill="none" stroke="transparent" stroke-width="12" />
            <path
              d={path}
              fill="none"
              stroke={color}
              stroke-width={highlighted ? 3 : 2}
              marker-end="url(#{markerId})"
            />
          </g>
        {/if}
      {/each}

      <!-- Nodes -->
      {#each nodes as node}
        {@const pos = positions[node.mr_id]}
        {@const colors = nodeColors(node)}
        {@const badge = statusBadge(node)}
        {@const blocked = isBlocked(node)}
        {@const highlighted = isHighlighted(node.mr_id)}
        {@const dimmed = hoveredNodeId && !highlighted}
        {#if pos}
          <g
            class="mq-node"
            data-testid="mq-node-{node.mr_id}"
            data-status={badge}
            transform="translate({pos.x - NODE_W / 2}, {pos.y - NODE_H / 2})"
            tabindex="0"
            role="button"
            aria-label="{node.title ?? node.mr_id} — {badge}"
            opacity={dimmed ? 0.25 : 1}
            onclick={() => onNodeClick(node)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onNodeClick(node); } }}
            onpointerenter={(e) => { hoveredNodeId = node.mr_id; showTooltip(node, e); }}
            onpointermove={(e) => { showTooltip(node, e); }}
            onpointerleave={() => { hoveredNodeId = null; hideTooltip(); }}
          >
            <rect
              width={NODE_W}
              height={NODE_H}
              rx={NODE_RX}
              fill={colors.fill}
              stroke={highlighted ? '#f8fafc' : colors.stroke}
              stroke-width={highlighted ? 2.5 : 1.5}
            />
            <!-- MR title (truncated) -->
            <text
              class="mq-node-label"
              x={NODE_W / 2}
              y={NODE_H / 2 - 10}
              text-anchor="middle"
              dominant-baseline="central"
              fill={colors.text}
              font-size="12"
              font-weight="600"
            >
              {truncate(node.title ?? node.mr_id)}
            </text>
            <!-- Status + priority row -->
            <text
              class="mq-node-status"
              x={NODE_W / 2}
              y={NODE_H / 2 + 8}
              text-anchor="middle"
              dominant-baseline="central"
              fill={colors.badge}
              font-size="10"
            >
              {badge}{blocked ? ' 🔒' : ''}{node.priority != null ? ` · p${node.priority}` : ''}
            </text>
          </g>
        {/if}
      {/each}
    </svg>

    <!-- Tooltip overlay -->
    {#if tooltip}
      <div
        class="mq-tooltip"
        data-testid="mq-tooltip"
        style="left: {tooltip.x + 12}px; top: {tooltip.y - 8}px"
      >
        <div class="mq-tooltip-title">{tooltip.title}</div>
        <div class="mq-tooltip-row">Status: <strong>{tooltip.status}</strong></div>
        {#if tooltip.priority != null}
          <div class="mq-tooltip-row">Priority: {tooltip.priority}</div>
        {/if}
        {#if tooltip.group}
          <div class="mq-tooltip-row">Group: {tooltip.group}</div>
        {/if}
        {#if tooltip.blockingDeps.length > 0}
          <div class="mq-tooltip-section">Blocked by:</div>
          {#each tooltip.blockingDeps as dep}
            <div class="mq-tooltip-dep">· {truncate(dep, 40)}</div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .mq-graph-container {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
  }

  .mq-legend {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text-muted, #888);
  }

  .mq-legend-item {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .mq-legend-swatch {
    display: inline-block;
    width: 16px;
    height: 3px;
    border-radius: 1px;
  }

  .mq-legend-dashed {
    background: none;
    border-top: 2px dashed;
    height: 0;
  }

  .mq-svg {
    width: 100%;
    flex: 1;
    min-height: 300px;
    cursor: grab;
    user-select: none;
    background: var(--color-surface, #0a0a0f);
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
  }

  .mq-svg:active {
    cursor: grabbing;
  }

  .mq-node {
    cursor: pointer;
    outline: none;
    transition: opacity 0.15s;
  }

  .mq-node:hover rect {
    filter: brightness(1.3);
  }

  .mq-node:focus-visible rect {
    stroke-width: 3;
    filter: brightness(1.3);
  }

  .mq-node-label {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
  }

  .mq-node-status {
    pointer-events: none;
    font-family: var(--font-body, system-ui);
    text-transform: capitalize;
  }

  .mq-edge {
    transition: opacity 0.15s;
  }

  .mq-tooltip {
    position: absolute;
    pointer-events: none;
    background: var(--color-surface-elevated, #1e293b);
    border: 1px solid var(--color-border, #334155);
    border-radius: var(--radius, 6px);
    padding: 8px 12px;
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text, #e2e8f0);
    max-width: 280px;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }

  .mq-tooltip-title {
    font-weight: 600;
    margin-bottom: 4px;
    word-break: break-word;
  }

  .mq-tooltip-row {
    color: var(--color-text-muted, #94a3b8);
    line-height: 1.4;
  }

  .mq-tooltip-section {
    margin-top: 4px;
    font-weight: 600;
    color: var(--color-warning, #eab308);
  }

  .mq-tooltip-dep {
    color: var(--color-text-muted, #94a3b8);
    padding-left: 4px;
    font-size: 11px;
  }

  .mq-loading,
  .mq-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 300px;
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 0.875rem);
  }

  @media (prefers-reduced-motion: reduce) {
    .mq-node:hover rect,
    .mq-node:focus-visible rect {
      filter: none;
    }
    .mq-node,
    .mq-edge {
      transition: none;
    }
  }
</style>
