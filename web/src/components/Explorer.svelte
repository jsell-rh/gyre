<script>
  import { onMount, onDestroy, getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import {
    forceSimulation,
    forceLink,
    forceManyBody,
    forceCenter,
    forceCollide,
  } from 'd3';

  const navigate = getContext('navigate');

  // State
  let repos = $state([]);
  let selectedRepoId = $state('');
  let nodes = $state([]);
  let edges = $state([]);
  let loading = $state(false);
  let selectedNode = $state(null);
  let detailPanelOpen = $state(false);

  // Controls
  let nameFilter = $state('');
  let riskMapOn = $state(false);
  let visibleTypes = $state(new Set(['Package', 'Module', 'Function', 'Type', 'Interface', 'Trait', 'Endpoint']));

  // SVG canvas
  let svgEl = $state(null);
  let canvasW = $state(900);
  let canvasH = $state(600);
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });

  // Simulation positions (mutable, updated by D3 on each tick)
  let positions = $state({});
  let simulation = null;

  // Arrowhead marker id
  const ARROW_ID = 'explorer-arrow';

  onMount(async () => {
    try {
      repos = await api.allRepos();
      if (repos.length > 0) {
        selectedRepoId = repos[0].id;
        await loadGraph();
      }
    } catch (e) {
      showToast('Failed to load repos: ' + e.message, { type: 'error' });
    }
  });

  onDestroy(() => {
    simulation?.stop();
  });

  async function loadGraph() {
    if (!selectedRepoId) return;
    loading = true;
    simulation?.stop();
    try {
      const [rawNodes, rawEdges] = await Promise.all([
        api.graphNodes(selectedRepoId),
        api.graphEdges(selectedRepoId),
      ]);
      nodes = rawNodes ?? [];
      edges = rawEdges ?? [];
      runSimulation();
    } catch (e) {
      showToast('Failed to load knowledge graph: ' + e.message, { type: 'error' });
      nodes = [];
      edges = [];
    } finally {
      loading = false;
    }
  }

  function runSimulation() {
    simulation?.stop();
    if (!nodes.length) return;

    // Build mutable copies for D3 (D3 mutates x/y on nodes)
    const simNodes = nodes.map((n) => ({
      id: n.id,
      x: Math.random() * canvasW,
      y: Math.random() * canvasH,
    }));

    const idIndex = Object.fromEntries(simNodes.map((n, i) => [n.id, i]));

    const simLinks = edges
      .filter((e) => e.source_id in idIndex && e.target_id in idIndex)
      .map((e) => ({
        source: idIndex[e.source_id],
        target: idIndex[e.target_id],
        edge_type: e.edge_type,
      }));

    simulation = forceSimulation(simNodes)
      .force('link', forceLink(simLinks).id((d) => d.id).distance(90).strength(0.3))
      .force('charge', forceManyBody().strength(-180))
      .force('center', forceCenter(canvasW / 2, canvasH / 2))
      .force('collide', forceCollide(32))
      .alphaDecay(0.03)
      .on('tick', () => {
        // Update reactive positions map from D3's mutable array
        const pos = {};
        simNodes.forEach((n) => {
          pos[n.id] = { x: n.x, y: n.y };
        });
        positions = pos;
      });
  }

  // Derived: nodes that pass filters
  let visibleNodes = $derived(
    nodes.filter((n) => {
      const typeMatch = visibleTypes.has(n.node_type ?? 'Module');
      const nameMatch = !nameFilter || (n.qualified_name ?? n.name ?? '').toLowerCase().includes(nameFilter.toLowerCase());
      return typeMatch && nameMatch;
    })
  );

  let visibleEdges = $derived(() => {
    const visibleIds = new Set(visibleNodes.map((n) => n.id));
    return edges.filter((e) => visibleIds.has(e.source_id) && visibleIds.has(e.target_id));
  });

  // Node visual properties
  function nodeColor(node) {
    if (riskMapOn) return churnColor(node.churn_count_30d ?? 0);
    if (selectedNode?.id === node.id) return '#1e1e1e';
    return '#1e1e1e';
  }

  function nodeStroke(node) {
    if (selectedNode?.id === node.id) return 'var(--color-primary)';
    if (node.spec_path) return '#22c55e';
    return '#444';
  }

  function nodeStrokeWidth(node) {
    return selectedNode?.id === node.id ? 2 : 1;
  }

  function churnColor(churn) {
    // Green (low) → red (high). Max reasonable churn = 30.
    const t = Math.min(churn / 30, 1);
    const r = Math.round(34 + t * (238 - 34));
    const g = Math.round(197 - t * (197 - 34));
    const b = 34;
    return `rgb(${r},${g},${b})`;
  }

  function edgeColor(edgeType) {
    const t = (edgeType ?? '').toLowerCase();
    if (t === 'calls') return '#4a9eff';
    if (t === 'imports') return '#a855f7';
    if (t === 'implements') return '#22c55e';
    if (t === 'extends') return '#f59e0b';
    if (t === 'uses') return '#6b7280';
    return '#555';
  }

  // Node shapes by type
  function nodeShape(node, pos) {
    if (!pos) return null;
    const { x, y } = pos;
    const t = (node.node_type ?? 'Module').toLowerCase();

    if (t === 'package') {
      return { type: 'circle', cx: x, cy: y, r: 20 };
    }
    if (t === 'function') {
      // Diamond
      const s = 14;
      return { type: 'polygon', points: `${x},${y - s} ${x + s},${y} ${x},${y + s} ${x - s},${y}` };
    }
    if (t === 'type') {
      return { type: 'ellipse', cx: x, cy: y, rx: 22, ry: 13 };
    }
    if (t === 'interface' || t === 'trait') {
      // Hexagon
      const r = 16;
      const pts = Array.from({ length: 6 }, (_, i) => {
        const a = (Math.PI / 3) * i - Math.PI / 6;
        return `${x + r * Math.cos(a)},${y + r * Math.sin(a)}`;
      }).join(' ');
      return { type: 'polygon', points: pts };
    }
    if (t === 'endpoint') {
      return { type: 'rect', x: x - 16, y: y - 8, width: 32, height: 16, rx: 4 };
    }
    // Module (default): rect
    return { type: 'rect', x: x - 12, y: y - 8, width: 24, height: 16, rx: 2 };
  }

  // Node label position
  function labelY(node, pos) {
    if (!pos) return 0;
    const t = (node.node_type ?? 'Module').toLowerCase();
    if (t === 'package') return pos.y + 28;
    if (t === 'function') return pos.y + 20;
    return pos.y + 22;
  }

  // Pan / zoom
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0, vbx: 0, vby: 0 });

  function onSvgMouseDown(e) {
    if (e.button !== 0) return;
    if (e.target.closest('.graph-node')) return; // don't pan on node click
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY, vbx: viewBox.x, vby: viewBox.y };
  }

  function onSvgMouseMove(e) {
    if (!isPanning) return;
    const scaleX = viewBox.w / canvasW;
    const scaleY = viewBox.h / canvasH;
    viewBox = {
      ...viewBox,
      x: panStart.vbx - (e.clientX - panStart.x) * scaleX,
      y: panStart.vby - (e.clientY - panStart.y) * scaleY,
    };
  }

  function onSvgMouseUp() { isPanning = false; }

  function onSvgWheel(e) {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 1.1 : 0.9;
    const cx = viewBox.x + viewBox.w / 2;
    const cy = viewBox.y + viewBox.h / 2;
    const nw = Math.max(200, Math.min(3000, viewBox.w * factor));
    const nh = Math.max(150, Math.min(2000, viewBox.h * factor));
    viewBox = { x: cx - nw / 2, y: cy - nh / 2, w: nw, h: nh };
  }

  function selectNode(node) {
    selectedNode = node;
    detailPanelOpen = true;
  }

  function closeDetail() {
    detailPanelOpen = false;
    selectedNode = null;
  }

  function toggleType(type) {
    const next = new Set(visibleTypes);
    if (next.has(type)) {
      next.delete(type);
    } else {
      next.add(type);
    }
    visibleTypes = next;
  }

  const ALL_NODE_TYPES = ['Package', 'Module', 'Function', 'Type', 'Interface', 'Trait', 'Endpoint'];

  function specConfidenceBadge(score) {
    if (!score && score !== 0) return { label: 'unknown', variant: 'default' };
    if (score >= 0.8) return { label: 'high', variant: 'success' };
    if (score >= 0.5) return { label: 'medium', variant: 'warning' };
    return { label: 'low', variant: 'danger' };
  }

  function shortName(qualifiedName) {
    if (!qualifiedName) return '';
    const parts = qualifiedName.split('::');
    return parts[parts.length - 1] ?? qualifiedName;
  }

  function navigateToSpecs() {
    navigate?.('specs');
  }
</script>

<div class="explorer-view">
  <!-- Controls panel -->
  <div class="controls-bar">
    <div class="controls-left">
      <label class="ctrl-label" for="repo-select">Repo</label>
      <select
        id="repo-select"
        class="ctrl-select"
        bind:value={selectedRepoId}
        onchange={loadGraph}
      >
        <option value="">— select repo —</option>
        {#each repos as repo}
          <option value={repo.id}>{repo.name}</option>
        {/each}
      </select>

      <span class="ctrl-sep" aria-hidden="true">|</span>

      <label class="ctrl-label" for="name-filter">Search</label>
      <input
        id="name-filter"
        class="ctrl-input"
        type="text"
        placeholder="filter by name…"
        bind:value={nameFilter}
      />

      <span class="ctrl-sep" aria-hidden="true">|</span>

      <fieldset class="type-filters" aria-label="Node type filters">
        <legend class="ctrl-label">Types</legend>
        {#each ALL_NODE_TYPES as t}
          <label class="type-pill" class:active={visibleTypes.has(t)}>
            <input
              type="checkbox"
              class="sr-only"
              checked={visibleTypes.has(t)}
              onchange={() => toggleType(t)}
            />
            {t}
          </label>
        {/each}
      </fieldset>
    </div>

    <div class="controls-right">
      <label class="risk-toggle">
        <input type="checkbox" bind:checked={riskMapOn} />
        <span class="risk-label">Risk Map</span>
      </label>

      <div class="node-count" aria-live="polite">
        {visibleNodes.length} / {nodes.length} nodes
      </div>
    </div>
  </div>

  <!-- Main canvas + detail panel -->
  <div class="canvas-area">
    <!-- SVG Graph -->
    <div class="svg-wrap" bind:clientWidth={canvasW} bind:clientHeight={canvasH}>
      {#if loading}
        <div class="loading-overlay">
          <Skeleton lines={4} />
        </div>
      {:else if !selectedRepoId}
        <EmptyState
          title="Select a repository"
          message="Choose a repo from the dropdown to visualize its knowledge graph."
        />
      {:else if nodes.length === 0}
        <EmptyState
          title="No graph data"
          message="This repository has no extracted nodes yet. Push code to populate the knowledge graph."
        />
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <svg
          class="graph-svg"
          role="img"
          aria-label="Knowledge graph canvas"
          viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
          onmousedown={onSvgMouseDown}
          onmousemove={onSvgMouseMove}
          onmouseup={onSvgMouseUp}
          onmouseleave={onSvgMouseUp}
          onwheel={onSvgWheel}
          style="cursor: {isPanning ? 'grabbing' : 'grab'}"
          bind:this={svgEl}
        >
          <defs>
            <marker
              id={ARROW_ID}
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill="#555" />
            </marker>
          </defs>

          <!-- Edges -->
          <g class="edges-layer" aria-hidden="true">
            {#each visibleEdges() as edge (edge.id ?? `${edge.source_id}-${edge.target_id}`)}
              {#if positions[edge.source_id] && positions[edge.target_id]}
                {@const src = positions[edge.source_id]}
                {@const tgt = positions[edge.target_id]}
                <line
                  class="graph-edge"
                  x1={src.x}
                  y1={src.y}
                  x2={tgt.x}
                  y2={tgt.y}
                  stroke={edgeColor(edge.edge_type)}
                  stroke-width="1"
                  stroke-opacity="0.5"
                  marker-end="url(#{ARROW_ID})"
                />
              {/if}
            {/each}
          </g>

          <!-- Nodes -->
          <g class="nodes-layer">
            {#each visibleNodes as node (node.id)}
              {@const pos = positions[node.id]}
              {@const shape = nodeShape(node, pos)}
              {#if shape && pos}
                <!-- svelte-ignore a11y_interactive_supports_focus -->
                <g
                  class="graph-node"
                  role="button"
                  aria-label={node.qualified_name ?? node.name ?? node.id}
                  aria-pressed={selectedNode?.id === node.id}
                  onclick={() => selectNode(node)}
                  onkeydown={(e) => e.key === 'Enter' && selectNode(node)}
                  style="cursor:pointer"
                >
                  {#if shape.type === 'circle'}
                    <circle
                      cx={shape.cx}
                      cy={shape.cy}
                      r={shape.r}
                      fill={nodeColor(node)}
                      stroke={nodeStroke(node)}
                      stroke-width={nodeStrokeWidth(node)}
                    />
                  {:else if shape.type === 'ellipse'}
                    <ellipse
                      cx={shape.cx}
                      cy={shape.cy}
                      rx={shape.rx}
                      ry={shape.ry}
                      fill={nodeColor(node)}
                      stroke={nodeStroke(node)}
                      stroke-width={nodeStrokeWidth(node)}
                    />
                  {:else if shape.type === 'polygon'}
                    <polygon
                      points={shape.points}
                      fill={nodeColor(node)}
                      stroke={nodeStroke(node)}
                      stroke-width={nodeStrokeWidth(node)}
                    />
                  {:else}
                    <rect
                      x={shape.x}
                      y={shape.y}
                      width={shape.width}
                      height={shape.height}
                      rx={shape.rx ?? 2}
                      fill={nodeColor(node)}
                      stroke={nodeStroke(node)}
                      stroke-width={nodeStrokeWidth(node)}
                    />
                  {/if}

                  <!-- Node label -->
                  <text
                    class="node-label"
                    x={pos.x}
                    y={labelY(node, pos)}
                    text-anchor="middle"
                    font-size="9"
                    fill="#aaa"
                  >{shortName(node.qualified_name ?? node.name ?? node.id)}</text>
                </g>
              {/if}
            {/each}
          </g>
        </svg>

        <!-- Risk map legend -->
        {#if riskMapOn}
          <div class="risk-legend" aria-label="Risk heat map legend">
            <span class="risk-low">stable</span>
            <div class="risk-gradient" aria-hidden="true"></div>
            <span class="risk-high">high churn</span>
          </div>
        {/if}
      {/if}
    </div>

    <!-- Detail panel -->
    {#if detailPanelOpen && selectedNode}
      {@const outgoing = edges.filter((e) => e.source_id === selectedNode.id)}
      {@const incoming = edges.filter((e) => e.target_id === selectedNode.id)}
      <div class="detail-panel" role="complementary" aria-label="Node detail">
        <div class="detail-header">
          <div class="detail-title-row">
            <span class="detail-type-badge">{selectedNode.node_type ?? 'Node'}</span>
            <button class="detail-close" onclick={closeDetail} aria-label="Close detail panel">✕</button>
          </div>
          <h3 class="detail-name">{selectedNode.qualified_name ?? selectedNode.name ?? selectedNode.id}</h3>
        </div>

        <div class="detail-body">
          {#if selectedNode.file_path}
            <div class="detail-row">
              <span class="detail-label">File</span>
              <span class="detail-val mono">{selectedNode.file_path}</span>
            </div>
          {/if}

          {#if selectedNode.visibility}
            <div class="detail-row">
              <span class="detail-label">Visibility</span>
              <span class="detail-val">{selectedNode.visibility}</span>
            </div>
          {/if}

          {#if selectedNode.spec_path}
            <div class="detail-row">
              <span class="detail-label">Spec</span>
              <button class="spec-link" onclick={navigateToSpecs} title="Open spec registry">
                {selectedNode.spec_path}
              </button>
            </div>
          {/if}

          {#if selectedNode.spec_confidence !== undefined && selectedNode.spec_confidence !== null}
            {@const badge = specConfidenceBadge(selectedNode.spec_confidence)}
            <div class="detail-row">
              <span class="detail-label">Spec confidence</span>
              <Badge value={badge.label} variant={badge.variant} />
            </div>
          {/if}

          {#if selectedNode.complexity !== undefined && selectedNode.complexity !== null}
            <div class="detail-row">
              <span class="detail-label">Complexity</span>
              <span class="detail-val">{selectedNode.complexity}</span>
            </div>
          {/if}

          {#if selectedNode.churn_count_30d !== undefined && selectedNode.churn_count_30d !== null}
            <div class="detail-row">
              <span class="detail-label">Churn (30d)</span>
              <span class="detail-val">{selectedNode.churn_count_30d}</span>
            </div>
          {/if}

          {#if selectedNode.doc_comment}
            <div class="detail-section">
              <span class="detail-section-label">Documentation</span>
              <p class="detail-doc">{selectedNode.doc_comment}</p>
            </div>
          {/if}

          {#if outgoing.length > 0}
            <div class="detail-section">
              <span class="detail-section-label">Outgoing ({outgoing.length})</span>
              <ul class="edge-list">
                {#each outgoing.slice(0, 8) as e}
                  {@const tgt = nodes.find((n) => n.id === e.target_id)}
                  <li class="edge-item">
                    <span class="edge-type" style="color:{edgeColor(e.edge_type)}">{e.edge_type ?? '→'}</span>
                    <button class="edge-link" onclick={() => { const n = nodes.find(x => x.id === e.target_id); if (n) selectNode(n); }}>
                      {shortName(tgt?.qualified_name ?? tgt?.name ?? e.target_id)}
                    </button>
                  </li>
                {/each}
                {#if outgoing.length > 8}
                  <li class="edge-more">+{outgoing.length - 8} more</li>
                {/if}
              </ul>
            </div>
          {/if}

          {#if incoming.length > 0}
            <div class="detail-section">
              <span class="detail-section-label">Incoming ({incoming.length})</span>
              <ul class="edge-list">
                {#each incoming.slice(0, 8) as e}
                  {@const src = nodes.find((n) => n.id === e.source_id)}
                  <li class="edge-item">
                    <span class="edge-type" style="color:{edgeColor(e.edge_type)}">{e.edge_type ?? '←'}</span>
                    <button class="edge-link" onclick={() => { const n = nodes.find(x => x.id === e.source_id); if (n) selectNode(n); }}>
                      {shortName(src?.qualified_name ?? src?.name ?? e.source_id)}
                    </button>
                  </li>
                {/each}
                {#if incoming.length > 8}
                  <li class="edge-more">+{incoming.length - 8} more</li>
                {/if}
              </ul>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .explorer-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--gray-95, #151515);
  }

  /* Controls bar */
  .controls-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .controls-left {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .controls-right {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .ctrl-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    white-space: nowrap;
  }

  .ctrl-select {
    height: 28px;
    padding: 0 var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    max-width: 180px;
  }

  .ctrl-input {
    height: 28px;
    padding: 0 var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    width: 150px;
  }

  .ctrl-input:focus {
    outline: none;
    border-color: var(--color-primary);
  }

  .ctrl-sep {
    color: var(--color-border-strong);
    padding: 0 var(--space-1);
  }

  .type-filters {
    border: none;
    padding: 0;
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .type-filters legend {
    float: left;
    margin-right: var(--space-2);
  }

  .type-pill {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--space-2);
    border-radius: var(--radius);
    font-size: 0.68rem;
    font-weight: 500;
    cursor: pointer;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    user-select: none;
    transition: background var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast);
  }

  .type-pill.active {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    color: var(--color-text);
  }

  .risk-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .risk-label {
    user-select: none;
  }

  .node-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  /* Canvas + panel layout */
  .canvas-area {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .svg-wrap {
    flex: 1;
    position: relative;
    overflow: hidden;
  }

  .loading-overlay {
    padding: var(--space-8);
  }

  .graph-svg {
    width: 100%;
    height: 100%;
    display: block;
    user-select: none;
  }

  .graph-edge {
    pointer-events: none;
  }

  .node-label {
    pointer-events: none;
    font-family: var(--font-mono);
  }

  /* Risk map legend */
  .risk-legend {
    position: absolute;
    bottom: var(--space-4);
    right: var(--space-4);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: rgba(0,0,0,0.7);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
  }

  .risk-gradient {
    width: 80px;
    height: 8px;
    border-radius: 4px;
    background: linear-gradient(to right, #22c55e, #f59e0b, #ef4444);
  }

  .risk-low { color: #22c55e; font-weight: 600; }
  .risk-high { color: #ef4444; font-weight: 600; }

  /* Detail panel */
  .detail-panel {
    width: 380px;
    flex-shrink: 0;
    background: #1a1a1a;
    border-left: 1px solid #333;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .detail-header {
    padding: var(--space-4);
    border-bottom: 1px solid #333;
    flex-shrink: 0;
  }

  .detail-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }

  .detail-type-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-primary);
  }

  .detail-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .detail-close:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .detail-name {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-text);
    word-break: break-all;
    font-weight: 600;
  }

  .detail-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .detail-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .detail-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    width: 90px;
    flex-shrink: 0;
    font-weight: 500;
  }

  .detail-val {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .detail-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .spec-link {
    background: transparent;
    border: none;
    color: #4a9eff;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-align: left;
    padding: 0;
    text-decoration: underline;
    word-break: break-all;
  }

  .spec-link:hover { color: #7cb9ff; }

  .detail-section {
    border-top: 1px solid #2a2a2a;
    padding-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .detail-section-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
  }

  .detail-doc {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .edge-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .edge-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .edge-type {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    min-width: 60px;
    flex-shrink: 0;
  }

  .edge-link {
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    text-align: left;
    padding: 0;
    word-break: break-all;
  }

  .edge-link:hover {
    color: var(--color-text);
    text-decoration: underline;
  }

  .edge-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding-left: calc(60px + var(--space-2));
  }

  /* Accessibility */
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    border: 0;
  }
</style>
