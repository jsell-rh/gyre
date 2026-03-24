<script>
  import { getContext } from 'svelte';
  import { api } from './api.js';
  import Badge from './Badge.svelte';
  import EmptyState from './EmptyState.svelte';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    onSelectNode = undefined,
    showSpecLinkage = false,
  } = $props();

  const navigate = getContext('navigate');

  // Pan/zoom state
  let svgEl = $state(null);
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });

  // Selected node
  let selectedNode = $state(null);

  // Context menu state
  let contextMenu = $state(null); // { x, y, node }

  // Find Usages highlight state
  let highlightedNodeIds = $state(new Set());

  // Drill-in state: when set, only show this node + immediate neighbors
  let drillNode = $state(null);

  // Spec-linkage overlay state
  let specLinkageOn = $state(showSpecLinkage);
  let showUnspeccedOnly = $state(false);

  // Spec linkage statistics
  let specCounts = $derived(() => {
    const specced = nodes.filter(n => !!n.spec_path).length;
    return { specced, unspecced: nodes.length - specced };
  });

  // Derived: visible nodes/edges (drill-in + unspecced filter)
  let visibleNodes = $derived(() => {
    let result = nodes;
    if (drillNode) {
      const neighborIds = new Set([drillNode.id]);
      for (const e of edges) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        if (src === drillNode.id) neighborIds.add(tgt);
        if (tgt === drillNode.id) neighborIds.add(src);
      }
      result = result.filter(n => neighborIds.has(n.id));
    }
    if (showUnspeccedOnly) result = result.filter(n => !n.spec_path);
    return result;
  });

  let visibleEdges = $derived(() => {
    if (!drillNode) return edges;
    const visibleIds = new Set(visibleNodes().map(n => n.id));
    return edges.filter(e => {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      return visibleIds.has(src) && visibleIds.has(tgt);
    });
  });

  // Layout: position nodes
  let nodePositions = $derived(() => {
    const ns = visibleNodes();
    if (!ns.length) return {};
    const byType = {};
    for (const n of ns) {
      const t = n.node_type ?? 'Unknown';
      byType[t] = (byType[t] ?? []);
      byType[t].push(n);
    }

    const typeOrder = ['package', 'module', 'type', 'interface', 'function', 'endpoint', 'component', 'table', 'constant'];
    const cols = Object.keys(byType).sort((a, b) => {
      const ai = typeOrder.indexOf(a);
      const bi = typeOrder.indexOf(b);
      return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
    });

    const positions = {};
    const colW = 160;
    const rowH = 60;
    const startX = 80;
    const startY = 60;

    cols.forEach((col, ci) => {
      const group = byType[col];
      group.forEach((n, ri) => {
        positions[n.id] = {
          x: startX + ci * colW,
          y: startY + ri * rowH,
        };
      });
    });

    return positions;
  });

  // Node type → color mapping (node_type values are snake_case from API)
  function nodeColor(type) {
    switch (type) {
      case 'package':   return { fill: '#3b1fa8', stroke: '#7c5ff5' };
      case 'module':    return { fill: '#1a3a6b', stroke: '#4a9eff' };
      case 'type':      return { fill: '#14532d', stroke: '#22c55e' };
      case 'interface': return { fill: '#78350f', stroke: '#f59e0b' };
      case 'function':  return { fill: '#134e4a', stroke: '#14b8a6' };
      case 'endpoint':  return { fill: '#7f1d1d', stroke: '#ef4444' };
      case 'component': return { fill: '#4a1d96', stroke: '#a78bfa' };
      case 'table':     return { fill: '#374151', stroke: '#9ca3af' };
      case 'constant':  return { fill: '#713f12', stroke: '#fbbf24' };
      default:          return { fill: '#1e293b', stroke: '#64748b' };
    }
  }

  function nodeShape(type) {
    if (type === 'interface') return 'diamond';
    if (type === 'function') return 'ellipse';
    if (type === 'endpoint') return 'hexagon';
    return 'rect';
  }

  // Spec-linkage ring color for a node
  function specRingColor(node) {
    if (!node.spec_path) return { color: '#ef4444', dashed: true };
    switch (node.spec_confidence) {
      case 'High':   return { color: '#22c55e', dashed: false };
      case 'Medium': return { color: '#eab308', dashed: false };
      case 'Low':    return { color: '#f97316', dashed: false };
      default:       return { color: '#ef4444', dashed: true };
    }
  }

  // Compute SVG bounds based on node positions
  let canvasBounds = $derived(() => {
    const pos = nodePositions();
    const xs = Object.values(pos).map(p => p.x);
    const ys = Object.values(pos).map(p => p.y);
    if (!xs.length) return { w: 900, h: 600 };
    return {
      w: Math.max(900, Math.max(...xs) + 200),
      h: Math.max(600, Math.max(...ys) + 120),
    };
  });

  function getPos(id) {
    const p = nodePositions()[id];
    return p ?? { x: 400, y: 300 };
  }

  // Pan/zoom handlers
  function onMouseDown(e) {
    if (e.button !== 0) return;
    // Only pan if not clicking a node
    if (e.target.closest('.graph-node')) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    e.preventDefault();
  }

  function onMouseMove(e) {
    if (!isPanning) return;
    const dx = e.clientX - panStart.x;
    const dy = e.clientY - panStart.y;
    const scaleX = viewBox.w / (svgEl?.clientWidth ?? 900);
    const scaleY = viewBox.h / (svgEl?.clientHeight ?? 600);
    viewBox = {
      ...viewBox,
      x: viewBox.x - dx * scaleX,
      y: viewBox.y - dy * scaleY,
    };
    panStart = { x: e.clientX, y: e.clientY };
  }

  function onMouseUp() {
    isPanning = false;
  }

  function onWheel(e) {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 1.15 : 0.87;
    const rect = svgEl?.getBoundingClientRect();
    const mx = rect ? (e.clientX - rect.left) / rect.width * viewBox.w + viewBox.x : viewBox.x + viewBox.w / 2;
    const my = rect ? (e.clientY - rect.top) / rect.height * viewBox.h + viewBox.y : viewBox.y + viewBox.h / 2;
    viewBox = {
      x: mx - (mx - viewBox.x) * factor,
      y: my - (my - viewBox.y) * factor,
      w: viewBox.w * factor,
      h: viewBox.h * factor,
    };
  }

  function resetView() {
    const b = canvasBounds();
    viewBox = { x: 0, y: 0, w: b.w, h: b.h };
  }

  function selectNode(node) {
    selectedNode = node;
    onSelectNode?.(node);
  }

  function closeDetail() {
    selectedNode = null;
  }

  // Right-click context menu
  function onContextMenu(e) {
    e.preventDefault();
    const nodeEl = e.target.closest('.graph-node');
    if (!nodeEl) {
      contextMenu = null;
      return;
    }
    // Find which node was right-clicked by matching data-node-id attribute
    const nodeId = nodeEl.dataset.nodeId;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) { contextMenu = null; return; }
    contextMenu = { x: e.clientX, y: e.clientY, node };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      contextMenu = null;
    }
  }

  // Context menu actions
  function ctxViewDetails(node) {
    closeContextMenu();
    selectNode(node);
  }

  async function ctxFindUsages(node) {
    closeContextMenu();
    if (!repoId) return;
    try {
      const result = await api.repoGraphNode(repoId, node.id);
      // result has .node and .edges; collect connected node IDs
      const connectedIds = new Set([node.id]);
      for (const e of (result.edges ?? [])) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        if (src) connectedIds.add(src);
        if (tgt) connectedIds.add(tgt);
      }
      highlightedNodeIds = connectedIds;
    } catch {
      // silently ignore fetch errors
    }
  }

  function ctxGoToSpec(node) {
    closeContextMenu();
    if (node.spec_path && navigate) {
      navigate('specs');
    }
  }

  function ctxCopyName(node) {
    closeContextMenu();
    navigator.clipboard?.writeText(node.qualified_name ?? node.name ?? '');
  }

  // Double-click drill-in
  function onDblClick(e) {
    const nodeEl = e.target.closest('.graph-node');
    if (!nodeEl) return;
    const nodeId = nodeEl.dataset.nodeId;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) return;
    drillNode = node;
    highlightedNodeIds = new Set();
    // Reset viewbox after drill-in
    setTimeout(resetView, 0);
  }

  function exitDrillIn() {
    drillNode = null;
    highlightedNodeIds = new Set();
    setTimeout(resetView, 0);
  }

  // Node shape renderers
  function rectPath(cx, cy, w, h) {
    const x = cx - w / 2, y = cy - h / 2;
    return `M${x},${y + 3} Q${x},${y} ${x + 3},${y} L${x + w - 3},${y} Q${x + w},${y} ${x + w},${y + 3} L${x + w},${y + h - 3} Q${x + w},${y + h} ${x + w - 3},${y + h} L${x + 3},${y + h} Q${x},${y + h} ${x},${y + h - 3} Z`;
  }

  function diamondPath(cx, cy, s) {
    return `M${cx},${cy - s} L${cx + s},${cy} L${cx},${cy + s} L${cx - s},${cy} Z`;
  }

  function hexPath(cx, cy, r) {
    const pts = [];
    for (let i = 0; i < 6; i++) {
      const a = (Math.PI / 180) * (60 * i - 30);
      pts.push(`${cx + r * Math.cos(a)},${cy + r * Math.sin(a)}`);
    }
    return `M${pts[0]} L${pts.slice(1).join(' L')} Z`;
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() / 1000 - ts;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="canvas-wrap" onclick={closeContextMenu}>
  {#if !nodes.length}
    <EmptyState
      title="No graph data"
      message="Select a repository to view its knowledge graph. Graph nodes are extracted on push."
    />
  {:else}
    <div class="canvas-toolbar">
      <button class="tool-btn" onclick={resetView} title="Reset view">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
          <path d="M3 12a9 9 0 109-9M3 12V7m0 5H8"/>
        </svg>
        Reset
      </button>
      {#if drillNode}
        <button class="tool-btn drill-back" onclick={exitDrillIn}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <path d="M19 12H5M12 5l-7 7 7 7"/>
          </svg>
          Full Graph
        </button>
        <span class="drill-label">Drill-in: <strong>{drillNode.name}</strong></span>
      {/if}
      <button
        class="tool-btn"
        class:active={specLinkageOn}
        onclick={() => (specLinkageOn = !specLinkageOn)}
        title="Toggle spec linkage overlay"
        aria-pressed={specLinkageOn}
      >
        Spec Linkage
      </button>
      {#if specLinkageOn}
        <button
          class="tool-btn"
          class:active={showUnspeccedOnly}
          onclick={() => (showUnspeccedOnly = !showUnspeccedOnly)}
          title="Show only unspecced nodes"
          aria-pressed={showUnspeccedOnly}
        >
          Unspecced only ({specCounts().unspecced})
        </button>
      {/if}
      <span class="node-count">{visibleNodes().length} nodes · {visibleEdges().length} edges</span>
      <div class="legend">
        {#each [['Package','#7c5ff5'],['Module','#4a9eff'],['Type','#22c55e'],['Interface','#f59e0b'],['Function','#14b8a6'],['Endpoint','#ef4444'],['Component','#a78bfa'],['Table','#9ca3af'],['Constant','#fbbf24']] as [label, color]}
          <span class="legend-item">
            <span class="legend-dot" style="background:{color}"></span>
            {label}
          </span>
        {/each}
      </div>
    </div>

    <div class="graph-area" class:has-panel={!!selectedNode}>
      <!-- SVG Canvas -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <svg
        bind:this={svgEl}
        class="graph-svg"
        class:panning={isPanning}
        viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
        role="application"
        aria-label="Architecture graph canvas — pan with drag, zoom with scroll, right-click for options, double-click to drill in"
        onmousedown={onMouseDown}
        onmousemove={onMouseMove}
        onmouseup={onMouseUp}
        onmouseleave={onMouseUp}
        onwheel={onWheel}
        oncontextmenu={onContextMenu}
        ondblclick={onDblClick}
      >
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
            <path d="M0,0 L0,6 L8,3 z" fill="#475569" />
          </marker>
          <marker id="arrow-hover" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
            <path d="M0,0 L0,6 L8,3 z" fill="#94a3b8" />
          </marker>
        </defs>

        <!-- Edges -->
        {#each visibleEdges() as edge}
          {@const from = getPos(edge.source_id ?? edge.from_node_id ?? edge.from)}
          {@const to = getPos(edge.target_id ?? edge.to_node_id ?? edge.to)}
          <line
            class="graph-edge"
            x1={from.x} y1={from.y}
            x2={to.x} y2={to.y}
            marker-end="url(#arrow)"
          />
        {/each}

        <!-- Nodes -->
        {#each visibleNodes() as node}
          {@const pos = getPos(node.id)}
          {@const colors = nodeColor(node.node_type)}
          {@const shape = nodeShape(node.node_type)}
          {@const isSelected = selectedNode?.id === node.id}
          {@const isHighlighted = highlightedNodeIds.size > 0 && highlightedNodeIds.has(node.id)}
          {@const isDimmed = highlightedNodeIds.size > 0 && !highlightedNodeIds.has(node.id)}
          {@const ring = specLinkageOn ? specRingColor(node) : null}
          <g
            class="graph-node"
            class:selected={isSelected}
            class:highlighted={isHighlighted}
            class:dimmed={isDimmed}
            data-node-id={node.id}
            transform="translate({pos.x},{pos.y})"
            role="button"
            tabindex="0"
            aria-label="{node.node_type}: {node.name}"
            aria-pressed={isSelected}
            onclick={() => selectNode(node)}
            onkeydown={(e) => e.key === 'Enter' && selectNode(node)}
          >
            {#if shape === 'diamond'}
              <path
                d={diamondPath(0, 0, 22)}
                fill={colors.fill}
                stroke={isSelected ? '#fff' : isHighlighted ? '#facc15' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2 : 1.5}
                opacity="0.9"
              />
            {:else if shape === 'ellipse'}
              <ellipse
                rx="28" ry="14"
                fill={colors.fill}
                stroke={isSelected ? '#fff' : isHighlighted ? '#facc15' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2 : 1.5}
                opacity="0.9"
              />
            {:else if shape === 'hexagon'}
              <path
                d={hexPath(0, 0, 22)}
                fill={colors.fill}
                stroke={isSelected ? '#fff' : isHighlighted ? '#facc15' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2 : 1.5}
                opacity="0.9"
              />
            {:else}
              <!-- rect (Package, Module, Struct, Table, Spec, default) -->
              <path
                d={rectPath(0, 0, 64, 28)}
                fill={colors.fill}
                stroke={isSelected ? '#fff' : isHighlighted ? '#facc15' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2 : 1.5}
                opacity="0.9"
              />
            {/if}
            <!-- Spec-linkage ring overlay -->
            {#if ring}
              <circle
                class="spec-ring"
                r="36"
                fill="none"
                stroke={ring.color}
                stroke-width="2.5"
                stroke-dasharray={ring.dashed ? '4 3' : 'none'}
                opacity="0.85"
                pointer-events="none"
              />
            {/if}
            <text
              text-anchor="middle"
              dominant-baseline="middle"
              font-size="9"
              fill="#f1f5f9"
              font-family="var(--font-mono)"
              pointer-events="none"
              style="user-select:none"
            >
              {(node.name ?? '').substring(0, 12)}
            </text>
            {#if isSelected}
              <circle r="4" cx="26" cy="-12" fill="var(--color-primary)" />
            {/if}
          </g>
        {/each}
      </svg>

      <!-- Spec-linkage legend overlay -->
      {#if specLinkageOn}
        <div class="spec-legend" aria-label="Spec linkage legend">
          <div class="spec-legend-title">Spec Coverage</div>
          {#each [
            { label: 'High confidence', color: '#22c55e', dashed: false },
            { label: 'Medium confidence', color: '#eab308', dashed: false },
            { label: 'Low confidence', color: '#f97316', dashed: false },
            { label: 'Unspecced', color: '#ef4444', dashed: true },
          ] as entry}
            <div class="spec-legend-item">
              <svg width="20" height="12" aria-hidden="true">
                <circle
                  cx="6" cy="6" r="5"
                  fill="none"
                  stroke={entry.color}
                  stroke-width="2"
                  stroke-dasharray={entry.dashed ? '3 2' : 'none'}
                />
              </svg>
              <span>{entry.label}</span>
            </div>
          {/each}
          <div class="spec-legend-counts">
            <span class="spec-count specced">{specCounts().specced} specced</span>
            <span class="spec-count unspecced">{specCounts().unspecced} unspecced</span>
          </div>
        </div>
      {/if}

      <!-- Detail side panel -->
      {#if selectedNode}
        {@const colors = nodeColor(selectedNode.node_type)}
        <div class="detail-panel" role="complementary" aria-label="Node details">
          <div class="panel-header" style="border-left: 3px solid {colors.stroke}">
            <div class="panel-title-row">
              <span class="panel-type">{selectedNode.node_type}</span>
              <button class="close-btn" onclick={closeDetail} aria-label="Close detail panel">×</button>
            </div>
            <span class="panel-name">{selectedNode.name}</span>
            {#if selectedNode.qualified_name && selectedNode.qualified_name !== selectedNode.name}
              <span class="panel-qualified">{selectedNode.qualified_name}</span>
            {/if}
          </div>

          <div class="panel-body">
            {#if selectedNode.file_path}
              <div class="panel-row">
                <span class="panel-label">File</span>
                <span class="panel-val mono">{selectedNode.file_path}:{selectedNode.line_start ?? ''}</span>
              </div>
            {/if}

            {#if selectedNode.visibility}
              <div class="panel-row">
                <span class="panel-label">Visibility</span>
                <Badge variant="default" value={selectedNode.visibility} />
              </div>
            {/if}

            {#if selectedNode.spec_path}
              <div class="panel-row">
                <span class="panel-label">Spec</span>
                <span class="panel-val mono spec-link">{selectedNode.spec_path}</span>
              </div>
            {/if}

            {#if selectedNode.spec_confidence}
              <div class="panel-row">
                <span class="panel-label">Confidence</span>
                <Badge
                  variant={selectedNode.spec_confidence === 'High' ? 'success' : selectedNode.spec_confidence === 'Medium' ? 'warning' : 'default'}
                  value={selectedNode.spec_confidence}
                />
              </div>
            {/if}

            {#if selectedNode.doc_comment}
              <div class="panel-section">
                <div class="panel-label">Doc</div>
                <p class="panel-doc">{selectedNode.doc_comment}</p>
              </div>
            {/if}

            <div class="panel-metrics">
              {#if selectedNode.complexity != null}
                <div class="metric">
                  <span class="metric-val">{selectedNode.complexity}</span>
                  <span class="metric-label">complexity</span>
                </div>
              {/if}
              {#if selectedNode.churn_count_30d != null}
                <div class="metric">
                  <span class="metric-val">{selectedNode.churn_count_30d}</span>
                  <span class="metric-label">churn/30d</span>
                </div>
              {/if}
            </div>

            {#if selectedNode.last_modified_at}
              <div class="panel-row">
                <span class="panel-label">Modified</span>
                <span class="panel-val">{relativeTime(selectedNode.last_modified_at)}</span>
              </div>
            {/if}

            {#if selectedNode.last_modified_by}
              <div class="panel-row">
                <span class="panel-label">By agent</span>
                <span class="panel-val mono">{selectedNode.last_modified_by}</span>
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- Context menu (rendered outside SVG, positioned at cursor) -->
{#if contextMenu}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="ctx-menu"
    style="left:{contextMenu.x}px; top:{contextMenu.y}px"
    onclick={(e) => e.stopPropagation()}
    role="menu"
    tabindex="-1"
    aria-label="Node context menu"
  >
    <button class="ctx-item" role="menuitem" onclick={() => ctxViewDetails(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true">
        <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
      </svg>
      View Details
    </button>
    <button class="ctx-item" role="menuitem" onclick={() => ctxFindUsages(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true">
        <path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/>
      </svg>
      Find Usages
    </button>
    <button
      class="ctx-item"
      class:disabled={!contextMenu.node.spec_path}
      role="menuitem"
      onclick={() => ctxGoToSpec(contextMenu.node)}
      disabled={!contextMenu.node.spec_path}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true">
        <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>
      </svg>
      Go to Spec
    </button>
    <div class="ctx-separator"></div>
    <button class="ctx-item" role="menuitem" onclick={() => ctxCopyName(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/>
      </svg>
      Copy Name
    </button>
  </div>
{/if}

<style>
  .canvas-wrap {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .canvas-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .tool-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .tool-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }

  .tool-btn.active {
    background: rgba(34, 197, 94, 0.12);
    border-color: #22c55e;
    color: #22c55e;
  }

  .drill-back {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .drill-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .drill-label strong {
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .node-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .legend {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    flex-wrap: wrap;
    margin-left: auto;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .legend-dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .graph-area {
    flex: 1;
    display: flex;
    overflow: hidden;
    position: relative;
  }

  .graph-svg {
    flex: 1;
    width: 100%;
    height: 100%;
    background: var(--color-surface);
    cursor: grab;
    display: block;
  }

  .graph-svg.panning {
    cursor: grabbing;
  }

  .graph-edge {
    stroke: #334155;
    stroke-width: 1.5;
    stroke-opacity: 0.7;
    transition: stroke var(--transition-fast);
  }

  .graph-node {
    cursor: pointer;
  }

  .graph-node:hover path,
  .graph-node:hover ellipse {
    filter: brightness(1.3);
  }

  .graph-node.selected path,
  .graph-node.selected ellipse {
    filter: brightness(1.4);
  }

  .graph-node.highlighted path,
  .graph-node.highlighted ellipse {
    filter: brightness(1.5) drop-shadow(0 0 6px #facc15);
  }

  .graph-node.dimmed {
    opacity: 0.3;
  }

  /* Context menu */
  .ctx-menu {
    position: fixed;
    z-index: 1000;
    background: var(--color-surface-elevated, #1e293b);
    border: 1px solid var(--color-border-strong, #334155);
    border-radius: var(--radius, 4px);
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
    min-width: 160px;
    padding: 4px 0;
    font-size: var(--text-sm, 13px);
    font-family: var(--font-body);
  }

  .ctx-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 14px;
    background: transparent;
    border: none;
    color: var(--color-text, #f1f5f9);
    cursor: pointer;
    text-align: left;
    font-size: var(--text-sm, 13px);
    font-family: var(--font-body);
    transition: background var(--transition-fast, 0.1s);
  }

  .ctx-item:hover:not(.disabled) {
    background: var(--color-surface, #0f172a);
    color: var(--color-primary, #ee0000);
  }

  .ctx-item.disabled,
  .ctx-item:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .ctx-separator {
    height: 1px;
    background: var(--color-border, #1e293b);
    margin: 4px 0;
  }

  /* Spec-linkage legend overlay */
  .spec-legend {
    position: absolute;
    bottom: var(--space-4);
    left: var(--space-4);
    background: rgba(15, 23, 42, 0.9);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 160px;
    backdrop-filter: blur(4px);
    pointer-events: none;
  }

  .spec-legend-title {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    margin-bottom: var(--space-1);
  }

  .spec-legend-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .spec-legend-counts {
    display: flex;
    gap: var(--space-3);
    padding-top: var(--space-1);
    border-top: 1px solid var(--color-border);
    margin-top: var(--space-1);
  }

  .spec-count {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 600;
  }

  .spec-count.specced { color: #22c55e; }
  .spec-count.unspecced { color: #ef4444; }

  /* Detail panel */
  .detail-panel {
    width: 280px;
    flex-shrink: 0;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .panel-header {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .panel-title-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-1);
  }

  .panel-type {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 18px;
    line-height: 1;
    padding: 0;
  }

  .close-btn:hover { color: var(--color-text); }

  .panel-name {
    display: block;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    font-family: var(--font-mono);
    word-break: break-all;
  }

  .panel-qualified {
    display: block;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    margin-top: 2px;
    word-break: break-all;
  }

  .panel-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .panel-row {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .panel-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .panel-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex-shrink: 0;
    min-width: 64px;
  }

  .panel-val {
    font-size: var(--text-sm);
    color: var(--color-text);
    word-break: break-all;
  }

  .panel-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .spec-link {
    color: var(--color-primary);
  }

  .panel-doc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    padding: var(--space-2);
    font-style: italic;
  }

  .panel-metrics {
    display: flex;
    gap: var(--space-4);
  }

  .metric {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .metric-val {
    font-size: var(--text-lg);
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text);
    line-height: 1;
  }

  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }
</style>
