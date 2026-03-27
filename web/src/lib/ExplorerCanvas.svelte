<script>
  import { getContext } from 'svelte';
  import { api } from './api.js';
  import Badge from './Badge.svelte';
  import EmptyState from './EmptyState.svelte';
  import { dispatchViewEvent, viewEventFromDom } from './viewEvents.js';
  import { computeLayout, columnLayout } from './layout-engines.js';

  /**
   * ExplorerCanvas — SVG graph canvas with pluggable layout engines.
   *
   * Spec: ui-layout.md §4 (View Spec Grammar), §10 (Rendering Technology)
   *
   * Layout engines:
   *   column       — type-grouped columns (default, sync)
   *   graph        — d3-force physics simulation
   *   hierarchical — ELK top-down layered
   *   layered      — ELK left-to-right layered
   */
  let {
    // View spec grammar (ui-layout.md §4)
    viewSpec = null,
    workspaceId = null,
    repoId = '',
    scope = 'workspace',
    onViewEvent = null,

    // Legacy direct-pass props (backward compat with MoldableView)
    nodes = [],
    edges = [],
    onSelectNode = undefined,
    showSpecLinkage = false,
  } = $props();

  // Shell context API (S4.1)
  const navigate = getContext('navigate');
  const openDetailPanel = getContext('openDetailPanel');

  // ── Layout engine ──────────────────────────────────────────────────────────
  let layoutEngine = $state('column');
  $effect(() => { layoutEngine = viewSpec?.layout ?? 'column'; });

  let nodePositionsMap = $state({});
  let layoutPending = $state(false);

  // ── Pan/zoom ───────────────────────────────────────────────────────────────
  let svgEl = $state(null);
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });

  // ── Node selection ─────────────────────────────────────────────────────────
  let selectedNode = $state(null);

  // ── Risk heat-map ──────────────────────────────────────────────────────────
  let showRiskHeatmap = $state(false);
  let riskData = $state([]);
  let highlightedNodeId = $state(null);
  let riskSortBy = $state('score');

  $effect(() => {
    if (showRiskHeatmap && repoId) {
      api.repoGraphRisks(repoId).then(data => {
        riskData = Array.isArray(data) ? data : [];
      }).catch(() => { riskData = []; });
    } else if (!showRiskHeatmap) {
      riskData = [];
      highlightedNodeId = null;
    }
  });

  let riskByNodeId = $derived.by(() => {
    const m = new Map();
    for (const r of riskData) m.set(r.node_id, r);
    return m;
  });

  let riskScores = $derived.by(() => {
    const data = riskData;
    if (!data.length) return new Map();
    const maxFanOut = Math.max(...data.map(r => r.fan_out ?? 0), 1);
    const maxComplexity = Math.max(...data.map(r => r.complexity ?? 0), 1);
    const scores = new Map();
    for (const r of data) {
      const churnNorm = Math.min(1, r.churn_rate ?? 0);
      const fanOutNorm = (r.fan_out ?? 0) / maxFanOut;
      const complexityNorm = (r.complexity ?? 0) / maxComplexity;
      const specPenalty = r.spec_covered ? 0 : 0.1;
      scores.set(r.node_id, Math.min(1, churnNorm * 0.4 + fanOutNorm * 0.3 + complexityNorm * 0.2 + specPenalty));
    }
    return scores;
  });

  let topRiskNodes = $derived.by(() => {
    const scores = riskScores;
    const byId = riskByNodeId;
    const entries = [...scores.entries()].sort((a, b) => {
      if (riskSortBy === 'score') return b[1] - a[1];
      const ra = byId.get(a[0]) ?? {};
      const rb = byId.get(b[0]) ?? {};
      if (riskSortBy === 'name')       return (ra.name ?? '').localeCompare(rb.name ?? '');
      if (riskSortBy === 'churn_rate') return (rb.churn_rate ?? 0) - (ra.churn_rate ?? 0);
      if (riskSortBy === 'fan_out')    return (rb.fan_out ?? 0) - (ra.fan_out ?? 0);
      return b[1] - a[1];
    });
    return entries.slice(0, 10).map(([id, score]) => {
      const risk = byId.get(id) ?? {};
      const node = nodes.find(n => n.id === id);
      return { id, score, name: risk.name ?? node?.name ?? id, churn_rate: risk.churn_rate ?? 0, fan_out: risk.fan_out ?? 0 };
    });
  });

  // ── Context menu + overlays ────────────────────────────────────────────────
  let contextMenu = $state(null);
  let highlightedNodeIds = $state(new Set());
  let drillNode = $state(null);
  let specLinkageOn = $state(false);
  $effect.pre(() => { specLinkageOn = showSpecLinkage; });
  let showUnspeccedOnly = $state(false);

  let specCounts = $derived.by(() => {
    const specced = nodes.filter(n => !!n.spec_path).length;
    return { specced, unspecced: nodes.length - specced };
  });

  // ── Performance thresholds ─────────────────────────────────────────────────
  const THRESHOLD_FILTER = 500;
  const THRESHOLD_LIST   = 1000;
  let showAllPrivate = $state(false);

  let showPublicOnlyBanner = $derived.by(() => nodes.length > THRESHOLD_FILTER && nodes.length <= THRESHOLD_LIST);
  let showListFallback     = $derived.by(() => nodes.length > THRESHOLD_LIST);
  let privateNodeCount     = $derived.by(() => nodes.filter(n => n.visibility === 'private').length);

  // ── Visible nodes/edges ────────────────────────────────────────────────────
  let visibleNodes = $derived.by(() => {
    let result = nodes;

    if (nodes.length > THRESHOLD_FILTER && !showAllPrivate) {
      result = result.filter(n => n.visibility !== 'private');
    }

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

    // viewSpec data filters (client-side)
    if (viewSpec?.data) {
      const d = viewSpec.data;
      if (d.node_types?.length)     result = result.filter(n => d.node_types.includes(n.node_type));
      if (d.filter?.visibility)     result = result.filter(n => n.visibility === d.filter.visibility);
      if (d.filter?.min_churn != null) result = result.filter(n => (n.churn_count_30d ?? 0) >= d.filter.min_churn);
      if (d.filter?.spec_path)      result = result.filter(n => n.spec_path === d.filter.spec_path);
    }

    return result;
  });

  let visibleEdges = $derived.by(() => {
    const visibleIds = new Set(visibleNodes.map(n => n.id));
    let result = edges.filter(e => {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      return visibleIds.has(src) && visibleIds.has(tgt);
    });
    if (viewSpec?.data?.edge_types?.length) {
      result = result.filter(e => viewSpec.data.edge_types.includes(e.edge_type));
    }
    return result;
  });

  // ── Highlight from viewSpec ────────────────────────────────────────────────
  let specHighlightIds = $derived.by(() => {
    const h = viewSpec?.highlight;
    if (!h) return null;
    if (h.node_ids?.length) return new Set(h.node_ids);
    if (h.spec_path) return new Set(nodes.filter(n => n.spec_path === h.spec_path).map(n => n.id));
    return null;
  });

  // ── Encoding helpers ───────────────────────────────────────────────────────
  function encodedNodeColor(node) {
    const enc = viewSpec?.encoding?.color;
    if (!enc) return nodeTypeColor(node.node_type);
    if (enc.field === 'node_type') return nodeTypeColor(node.node_type);
    if (enc.field === 'spec_confidence') {
      const map = { High: '#22c55e', Medium: '#eab308', Low: '#f97316', None: '#475569' };
      const c = map[node.spec_confidence ?? 'None'] ?? '#475569';
      return { fill: c, stroke: c };
    }
    if (enc.field === 'visibility') {
      return node.visibility === 'public'
        ? { fill: '#1a3a6b', stroke: '#4a9eff' }
        : { fill: '#2d1f3d', stroke: '#a78bfa' };
    }
    return nodeTypeColor(node.node_type);
  }

  function encodedNodeSize(node) {
    const enc = viewSpec?.encoding?.size;
    if (!enc) return 1;
    const val = node[enc.field] ?? 0;
    const [minS, maxS] = enc.range ?? [1, 2.5];
    const maxVal = Math.max(1, ...visibleNodes.map(n => n[enc.field] ?? 0));
    return minS + (val / maxVal) * (maxS - minS);
  }

  function encodedNodeLabel(node) {
    const field = viewSpec?.encoding?.label ?? 'name';
    if (field === 'qualified_name') return (node.qualified_name ?? node.name ?? '').substring(0, 18);
    if (field === 'file_path')      return (node.file_path ?? '').split('/').pop()?.substring(0, 14) ?? '';
    return (node.name ?? '').substring(0, 12);
  }

  function encodedNodeOpacity(node) {
    const enc = viewSpec?.encoding?.opacity;
    if (!enc) return 0.9;
    if (enc.field === 'visibility') {
      const scale = enc.scale ?? { public: 1.0, private: 0.4 };
      return scale[node.visibility ?? 'private'] ?? 0.9;
    }
    return 0.9;
  }

  function encodedEdgeColor(edge) {
    const enc = viewSpec?.encoding?.edge_color;
    if (!enc) return '#475569';
    const map = { contains: '#3b82f6', implements: '#22c55e', depends_on: '#f59e0b', routes_to: '#ef4444', references: '#8b5cf6' };
    return map[edge.edge_type] ?? '#475569';
  }

  function encodedEdgeDash(edge) {
    const enc = viewSpec?.encoding?.edge_style;
    if (!enc) return 'none';
    if (enc.field === 'edge_type') {
      return ['references', 'depends_on'].includes(edge.edge_type) ? '4 2' : 'none';
    }
    return 'none';
  }

  // ── Layout computation ─────────────────────────────────────────────────────
  let layoutGeneration = 0;

  $effect(() => {
    const ns  = visibleNodes;
    const es  = visibleEdges;
    const eng = layoutEngine;

    if (!ns.length) { nodePositionsMap = {}; return; }

    if (eng === 'column') {
      layoutGeneration++;
      nodePositionsMap = columnLayout(ns);
      return;
    }

    layoutPending = true;
    const gen = ++layoutGeneration;
    const w = svgEl?.clientWidth  ?? 900;
    const h = svgEl?.clientHeight ?? 600;

    computeLayout(eng, ns, es, w, h).then(pos => {
      // Discard result if a newer layout was requested (prevents race on rapid switches)
      if (gen !== layoutGeneration) return;
      nodePositionsMap = pos;
      layoutPending = false;
    }).catch(() => {
      if (gen !== layoutGeneration) return;
      nodePositionsMap = columnLayout(ns);
      layoutPending = false;
    });
  });

  function getPos(id) { return nodePositionsMap[id] ?? { x: 400, y: 300 }; }

  let canvasBounds = $derived.by(() => {
    const pos = nodePositionsMap;
    const xs = Object.values(pos).map(p => p.x);
    const ys = Object.values(pos).map(p => p.y);
    if (!xs.length) return { w: 900, h: 600 };
    return { w: Math.max(900, Math.max(...xs) + 200), h: Math.max(600, Math.max(...ys) + 120) };
  });

  function resetView() {
    const b = canvasBounds;
    viewBox = { x: 0, y: 0, w: b.w, h: b.h };
  }

  // ── Pan/zoom handlers ──────────────────────────────────────────────────────
  function onMouseDown(e) {
    if (e.button !== 0) return;
    closeContextMenu();
    if (e.target.closest('.graph-node')) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    e.preventDefault();

    // Background click → ViewEvent
    const ve = { type: 'click', entity_type: 'canvas', entity_id: null, position: { x: e.clientX, y: e.clientY } };
    dispatchViewEvent(svgEl, ve);
    onViewEvent?.(ve);
  }

  function onMouseMove(e) {
    if (!isPanning) return;
    const dx = e.clientX - panStart.x;
    const dy = e.clientY - panStart.y;
    const scaleX = viewBox.w / (svgEl?.clientWidth  ?? 900);
    const scaleY = viewBox.h / (svgEl?.clientHeight ?? 600);
    viewBox = { ...viewBox, x: viewBox.x - dx * scaleX, y: viewBox.y - dy * scaleY };
    panStart = { x: e.clientX, y: e.clientY };
  }

  function onMouseUp() { isPanning = false; }

  function onWheel(e) {
    e.preventDefault();
    // Clamp zoom factor: 0.2–5x
    const factor = e.deltaY > 0 ? 1.15 : 0.87;
    const rect = svgEl?.getBoundingClientRect();
    const mx = rect ? (e.clientX - rect.left) / rect.width  * viewBox.w + viewBox.x : viewBox.x + viewBox.w / 2;
    const my = rect ? (e.clientY - rect.top)  / rect.height * viewBox.h + viewBox.y : viewBox.y + viewBox.h / 2;
    const newW = Math.max(viewBox.w / 5, Math.min(viewBox.w * 5, viewBox.w * factor));
    const scale = newW / viewBox.w;
    viewBox = { x: mx - (mx - viewBox.x) * scale, y: my - (my - viewBox.y) * scale, w: newW, h: viewBox.h * scale };
  }

  // ── Node interaction ───────────────────────────────────────────────────────
  function selectNode(node, domEvent) {
    selectedNode = node;
    onSelectNode?.(node);

    // Shell context: open detail panel
    openDetailPanel?.({ type: 'node', id: node.id, data: node });

    if (domEvent) {
      const ve = viewEventFromDom(domEvent, 'node', node.id, node);
      dispatchViewEvent(domEvent.target, ve);
      onViewEvent?.(ve);
    }
  }

  function closeDetail() { selectedNode = null; }

  function onDblClick(e) {
    const nodeEl = e.target.closest('.graph-node');
    if (!nodeEl) return;
    const nodeId = nodeEl.dataset.nodeId;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) return;

    drillNode = node;
    highlightedNodeIds = new Set();
    setTimeout(resetView, 0);

    // Shell context: scope drill-down
    navigate?.('explorer', { scope: 'repo', repoId: node.repo_id ?? repoId });

    const ve = viewEventFromDom(e, 'node', node.id, node);
    dispatchViewEvent(e.target, { ...ve, type: 'dblclick' });
    onViewEvent?.({ ...ve, type: 'dblclick' });
  }

  // ── Context menu ───────────────────────────────────────────────────────────
  function onContextMenu(e) {
    e.preventDefault();
    const nodeEl = e.target.closest('.graph-node');
    if (!nodeEl) { contextMenu = null; return; }
    const nodeId = nodeEl.dataset.nodeId;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) { contextMenu = null; return; }
    contextMenu = { x: e.clientX, y: e.clientY, node };
  }

  function closeContextMenu() { contextMenu = null; }

  function onKeydown(e) { if (e.key === 'Escape') contextMenu = null; }

  function ctxViewDetails(node) { closeContextMenu(); selectNode(node); }

  async function ctxFindUsages(node) {
    closeContextMenu();
    if (!repoId) return;
    try {
      const result = await api.repoGraphNode(repoId, node.id);
      const connectedIds = new Set([node.id]);
      for (const e of (result.edges ?? [])) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        if (src) connectedIds.add(src);
        if (tgt) connectedIds.add(tgt);
      }
      highlightedNodeIds = connectedIds;
    } catch { /* silently ignore */ }
  }

  function ctxGoToSpec(node) {
    closeContextMenu();
    if (node.spec_path && navigate) navigate('specs');
  }

  function ctxCopyName(node) {
    closeContextMenu();
    navigator.clipboard?.writeText(node.qualified_name ?? node.name ?? '');
  }

  function exitDrillIn() {
    drillNode = null;
    highlightedNodeIds = new Set();
    setTimeout(resetView, 0);
  }

  function panToNode(nodeId) {
    const pos = getPos(nodeId);
    viewBox = { ...viewBox, x: pos.x - viewBox.w / 2, y: pos.y - viewBox.h / 2 };
    highlightedNodeId = nodeId;
  }

  // ── Node rendering helpers ─────────────────────────────────────────────────
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
      case 'constant':  return { fill: '#713f12', stroke: '#fbbf24' };
      default:          return { fill: '#1e293b', stroke: '#64748b' };
    }
  }

  function nodeShape(type) {
    if (type === 'interface') return 'diamond';
    if (type === 'function')  return 'ellipse';
    if (type === 'endpoint')  return 'hexagon';
    return 'rect';
  }

  function specRingColor(node) {
    if (!node.spec_path) return { color: '#ef4444', dashed: true };
    switch (node.spec_confidence) {
      case 'High':   return { color: '#22c55e', dashed: false };
      case 'Medium': return { color: '#eab308', dashed: false };
      case 'Low':    return { color: '#f97316', dashed: false };
      default:       return { color: '#ef4444', dashed: true };
    }
  }

  function rectPath(cx, cy, w, h) {
    const x = cx - w / 2, y = cy - h / 2;
    return `M${x},${y + 3} Q${x},${y} ${x + 3},${y} L${x + w - 3},${y} Q${x + w},${y} ${x + w},${y + 3} L${x + w},${y + h - 3} Q${x + w},${y + h} ${x + w - 3},${y + h} L${x + 3},${y + h} Q${x},${y + h} ${x},${y + h - 3} Z`;
  }

  function diamondPath(cx, cy, s) { return `M${cx},${cy - s} L${cx + s},${cy} L${cx},${cy + s} L${cx - s},${cy} Z`; }

  function hexPath(cx, cy, r) {
    const pts = [];
    for (let i = 0; i < 6; i++) {
      const a = (Math.PI / 180) * (60 * i - 30);
      pts.push(`${cx + r * Math.cos(a)},${cy + r * Math.sin(a)}`);
    }
    return `M${pts[0]} L${pts.slice(1).join(' L')} Z`;
  }

  function getEffectiveColors(node) {
    if (viewSpec?.encoding?.color) return encodedNodeColor(node);
    if (showRiskHeatmap) {
      const score = riskScores.get(node.id);
      if (score != null) { const fill = riskFillColor(score); return { fill, stroke: fill }; }
    }
    return nodeTypeColor(node.node_type);
  }

  function getNodeScale(nodeId) {
    if (viewSpec?.encoding?.size) {
      const node = nodes.find(n => n.id === nodeId);
      if (node) return encodedNodeSize(node);
    }
    if (showRiskHeatmap) {
      const risk = riskByNodeId.get(nodeId);
      if (risk) return Math.min(3, 1 + (risk.fan_in ?? 0) * 0.5);
    }
    return 1;
  }

  function getNodeTooltip(node) {
    if (!showRiskHeatmap) return `${node.node_type}: ${node.name}`;
    const risk  = riskByNodeId.get(node.id);
    const score = riskScores.get(node.id);
    if (!risk) return `${node.node_type}: ${node.name} (no risk data)`;
    return [`${node.node_type}: ${node.name}`, `Risk: ${score?.toFixed(2)}`, `Churn: ${risk.churn_rate ?? 0}`, `Fan-out: ${risk.fan_out ?? 0}`, `Spec: ${risk.spec_covered ? 'yes' : 'no'}`].join('\n');
  }

  function lerpInt(a, b, t) { return Math.round(a + (b - a) * t); }
  function riskFillColor(score) {
    if (score <= 0.5) { const t = score * 2; return `rgb(${lerpInt(34, 234, t)},${lerpInt(197, 179, t)},${lerpInt(94, 8, t)})`; }
    const t = (score - 0.5) * 2;
    return `rgb(${lerpInt(234, 239, t)},${lerpInt(179, 68, t)},${lerpInt(8, 68, t)})`;
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() / 1000 - ts;
    if (diff < 3600)  return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function sortIcon(col) { return riskSortBy === col ? '▼' : '⇅'; }

  const LAYOUT_LABELS = { column: 'Column', graph: 'Force', hierarchical: 'Hierarchical', layered: 'Layered' };
</script>

<svelte:window onkeydown={onKeydown} />

<div class="canvas-wrap">
  {#if !nodes.length}
    <EmptyState title="No graph data" description="Select a repository to view its knowledge graph. Graph nodes are extracted on push." />
  {:else if showListFallback}
    <div class="threshold-banner warning">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
      Graph too large ({nodes.length} nodes) — showing list view. Drill into a module to view its graph.
    </div>
    <div class="list-fallback-wrap">
      <table class="list-table" aria-label="Node list (graph too large for canvas)">
        <thead><tr><th scope="col">Type</th><th scope="col">Name</th><th>File</th><th>Spec</th></tr></thead>
        <tbody>
          {#each nodes.slice(0, 1000) as node}
            <tr class="list-row" role="button" tabindex="0" aria-label="Select node {node.name}"
              onclick={() => selectNode(node)} onkeydown={(e) => e.key === 'Enter' && selectNode(node)}>
              <td><Badge variant="default" value={node.node_type ?? '?'} /></td>
              <td class="mono">{node.name}</td>
              <td class="mono muted">{node.file_path ?? ''}</td>
              <td>{node.spec_path ? node.spec_path.split('/').pop() : '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    {#if showPublicOnlyBanner && !showAllPrivate}
      <div class="threshold-banner info">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
        Showing public API only — {privateNodeCount} private nodes hidden.
        <button class="banner-action" onclick={() => (showAllPrivate = true)}>Show All</button>
      </div>
    {/if}

    {#if viewSpec?.explanation}
      <div class="explanation-banner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 015.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {viewSpec.explanation}
      </div>
    {/if}

    <div class="canvas-toolbar">
      <button class="tool-btn" onclick={resetView} title="Reset view" aria-label="Reset view">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M3 12a9 9 0 109-9M3 12V7m0 5H8"/></svg>
        Reset
      </button>
      {#if drillNode}
        <button class="tool-btn drill-back" onclick={exitDrillIn}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M19 12H5M12 5l-7 7 7 7"/></svg>
          Full Graph
        </button>
        <span class="drill-label">Drill-in: <strong>{drillNode.name}</strong></span>
      {/if}

      <!-- Layout engine switcher -->
      <div class="layout-switcher" role="group" aria-label="Layout engine">
        {#each Object.entries(LAYOUT_LABELS) as [eng, label]}
          <button class="layout-btn" class:active={layoutEngine === eng}
            onclick={() => (layoutEngine = eng)} aria-pressed={layoutEngine === eng} title="Switch to {label} layout" aria-label="Switch to {label} layout">
            {label}
          </button>
        {/each}
        {#if layoutPending}<span class="layout-spinner" aria-label="Computing layout…"></span>{/if}
      </div>

      <button class="tool-btn" class:active={specLinkageOn} onclick={() => (specLinkageOn = !specLinkageOn)}
        title="Toggle spec linkage overlay" aria-label="Toggle spec linkage overlay" aria-pressed={specLinkageOn}>Spec Linkage</button>
      {#if specLinkageOn}
        <button class="tool-btn" class:active={showUnspeccedOnly} onclick={() => (showUnspeccedOnly = !showUnspeccedOnly)}
          title="Show only unspecced nodes" aria-label="Show only unspecced nodes" aria-pressed={showUnspeccedOnly}>
          Unspecced only ({specCounts.unspecced})
        </button>
      {/if}

      <button class="tool-btn risk-toggle" class:active={showRiskHeatmap} onclick={() => (showRiskHeatmap = !showRiskHeatmap)}
        title={showRiskHeatmap ? 'Disable risk heat-map' : 'Enable risk heat-map'} aria-label={showRiskHeatmap ? 'Disable risk heat-map' : 'Enable risk heat-map'} aria-pressed={showRiskHeatmap}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
          <circle cx="12" cy="12" r="9"/><path d="M12 8v4l3 3"/><path d="M8 12h1M15 12h1M12 8v1M12 15v1"/>
        </svg>
        Risk Heat-map
      </button>
      {#if showRiskHeatmap}
        <span class="heatmap-legend">
          <span class="hm-dot" style="background:#22c55e"></span>low
          <span class="hm-dot" style="background:#eab308"></span>medium
          <span class="hm-dot" style="background:#ef4444"></span>high
        </span>
      {/if}

      <span class="node-count">{visibleNodes.length} nodes · {visibleEdges.length} edges</span>

      {#if !showRiskHeatmap}
        <div class="legend">
          {#each [['Package','#7c5ff5'],['Module','#4a9eff'],['Type','#22c55e'],['Interface','#f59e0b'],['Function','#14b8a6'],['Endpoint','#ef4444'],['Component','#a78bfa'],['Table','#9ca3af'],['Constant','#fbbf24']] as [lbl, color]}
            <span class="legend-item"><span class="legend-dot" style="background:{color}"></span>{lbl}</span>
          {/each}
        </div>
      {/if}
    </div>

    <div class="graph-area" class:has-panel={!!selectedNode} class:has-risk-panel={showRiskHeatmap}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <svg bind:this={svgEl} class="graph-svg" class:panning={isPanning}
        viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
        role="application"
        aria-label="Architecture graph canvas — pan with drag, zoom with scroll, right-click for options, double-click to drill in"
        onmousedown={onMouseDown} onmousemove={onMouseMove} onmouseup={onMouseUp}
        onmouseleave={onMouseUp} onwheel={onWheel} oncontextmenu={onContextMenu} ondblclick={onDblClick}>
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
            <path d="M0,0 L0,6 L8,3 z" fill="#475569" />
          </marker>
        </defs>

        <!-- Edges -->
        {#each visibleEdges as edge}
          {@const from = getPos(edge.source_id ?? edge.from_node_id ?? edge.from)}
          {@const to   = getPos(edge.target_id ?? edge.to_node_id ?? edge.to)}
          <line class="graph-edge" x1={from.x} y1={from.y} x2={to.x} y2={to.y}
            stroke={encodedEdgeColor(edge)} stroke-dasharray={encodedEdgeDash(edge)}
            marker-end="url(#arrow)" />
        {/each}

        <!-- Nodes -->
        {#each visibleNodes as node}
          {@const pos = getPos(node.id)}
          {@const colors = getEffectiveColors(node)}
          {@const shape = nodeShape(node.node_type)}
          {@const isSelected = selectedNode?.id === node.id}
          {@const isFindHighlighted = highlightedNodeIds.size > 0 && highlightedNodeIds.has(node.id)}
          {@const isRiskHighlighted = highlightedNodeId === node.id}
          {@const isSpecHighlighted = specHighlightIds?.has(node.id)}
          {@const isHighlighted = isFindHighlighted || isRiskHighlighted || isSpecHighlighted}
          {@const isDimmed = highlightedNodeIds.size > 0 && !highlightedNodeIds.has(node.id)}
          {@const ring = specLinkageOn ? specRingColor(node) : null}
          {@const scale = getNodeScale(node.id)}
          {@const opacity = encodedNodeOpacity(node)}
          <g class="graph-node" class:selected={isSelected} class:highlighted={isHighlighted}
            class:dimmed={isDimmed} class:spec-highlighted={isSpecHighlighted}
            data-node-id={node.id}
            transform="translate({pos.x},{pos.y}) scale({scale})"
            role="button" tabindex="0"
            aria-label="{node.node_type}: {node.name}{isSelected ? ' (selected)' : ''}"
            onclick={(e) => selectNode(node, e)}
            onkeydown={(e) => e.key === 'Enter' && selectNode(node)}>
            <title>{getNodeTooltip(node)}</title>
            {#if shape === 'diamond'}
              <path d={diamondPath(0, 0, 22)} fill={colors.fill}
                stroke={isSelected ? '#fff' : isFindHighlighted ? '#facc15' : isSpecHighlighted ? '#a78bfa' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2.5 : 1.5} opacity={opacity} />
            {:else if shape === 'ellipse'}
              <ellipse rx="28" ry="14" fill={colors.fill}
                stroke={isSelected ? '#fff' : isFindHighlighted ? '#facc15' : isSpecHighlighted ? '#a78bfa' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2.5 : 1.5} opacity={opacity} />
            {:else if shape === 'hexagon'}
              <path d={hexPath(0, 0, 22)} fill={colors.fill}
                stroke={isSelected ? '#fff' : isFindHighlighted ? '#facc15' : isSpecHighlighted ? '#a78bfa' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2.5 : 1.5} opacity={opacity} />
            {:else}
              <path d={rectPath(0, 0, 64, 28)} fill={colors.fill}
                stroke={isSelected ? '#fff' : isFindHighlighted ? '#facc15' : isSpecHighlighted ? '#a78bfa' : colors.stroke}
                stroke-width={isSelected || isHighlighted ? 2.5 : 1.5} opacity={opacity} />
            {/if}
            {#if ring}
              <circle class="spec-ring" r="36" fill="none" stroke={ring.color} stroke-width="2.5"
                stroke-dasharray={ring.dashed ? '4 3' : 'none'} opacity="0.85" pointer-events="none" />
            {/if}
            {#if viewSpec?.annotations}
              {@const ann = viewSpec.annotations.find(a => a.node_name === node.name)}
              {#if ann}
                <text text-anchor="middle" dominant-baseline="auto" y="-24" font-size="8"
                  fill="#94a3b8" pointer-events="none" style="font-family: var(--font-mono); user-select:none">
                  {ann.text.substring(0, 30)}
                </text>
              {/if}
            {/if}
            <text text-anchor="middle" dominant-baseline="middle" font-size="9" fill="#f1f5f9"
              pointer-events="none" style="font-family: var(--font-mono); user-select:none">
              {encodedNodeLabel(node)}
            </text>
            {#if isSelected}
              <circle r="4" cx="26" cy="-12" fill="var(--color-focus)" />
            {/if}
            {#if isRiskHighlighted && !isSelected}
              <circle r="4" cx="26" cy="-12" fill="#eab308" />
            {/if}
          </g>
        {/each}
      </svg>

      {#if specLinkageOn}
        <div class="spec-legend" aria-label="Spec linkage legend">
          <div class="spec-legend-title">Spec Coverage</div>
          {#each [{ label: 'High confidence', color: '#22c55e', dashed: false },{ label: 'Medium confidence', color: '#eab308', dashed: false },{ label: 'Low confidence', color: '#f97316', dashed: false },{ label: 'Unspecced', color: '#ef4444', dashed: true }] as entry}
            <div class="spec-legend-item">
              <svg width="20" height="12" aria-hidden="true">
                <circle cx="6" cy="6" r="5" fill="none" stroke={entry.color} stroke-width="2" stroke-dasharray={entry.dashed ? '3 2' : 'none'} />
              </svg>
              <span>{entry.label}</span>
            </div>
          {/each}
          <div class="spec-legend-counts">
            <span class="spec-count specced">{specCounts.specced} specced</span>
            <span class="spec-count unspecced">{specCounts.unspecced} unspecced</span>
          </div>
        </div>
      {/if}

      {#if showRiskHeatmap}
        <div class="risk-panel" role="complementary" aria-label="Risk heat-map — top 10 nodes">
          <div class="risk-panel-header">
            <span class="risk-panel-title">Risk Heat-map</span>
            <span class="risk-panel-sub">Top 10 · click to highlight</span>
          </div>
          {#if riskData.length === 0}
            <div class="risk-empty">{repoId ? 'Loading risk data…' : 'No risk data available'}</div>
          {:else}
            <div class="risk-table-wrap">
              <table class="risk-table" aria-label="Top 10 highest-risk nodes">
                <thead>
                  <tr>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'name')} aria-label="Sort by name">Name {sortIcon('name')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'score')} aria-label="Sort by risk score">Score {sortIcon('score')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'churn_rate')} aria-label="Sort by churn">Churn {sortIcon('churn_rate')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'fan_out')} aria-label="Sort by fan-out">Fan-out {sortIcon('fan_out')}</button></th>
                  </tr>
                </thead>
                <tbody>
                  {#each topRiskNodes as entry}
                    {@const fill = riskFillColor(entry.score)}
                    <tr class="risk-row" class:highlighted={highlightedNodeId === entry.id} role="button" tabindex="0"
                      aria-label="Highlight node {entry.name} on canvas"
                      onclick={() => panToNode(entry.id)} onkeydown={(e) => e.key === 'Enter' && panToNode(entry.id)}>
                      <td class="risk-name" title={entry.name}>{entry.name.substring(0, 14)}</td>
                      <td><span class="risk-score-chip" style="--chip-color: {fill}">{entry.score.toFixed(2)}</span></td>
                      <td class="risk-num">{typeof entry.churn_rate === 'number' ? entry.churn_rate.toFixed(2) : entry.churn_rate}</td>
                      <td class="risk-num">{entry.fan_out}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
            <div class="risk-panel-footer"><span class="risk-panel-hint">Node size ∝ fan-in · Color = risk score</span></div>
          {/if}
        </div>
      {/if}

      {#if selectedNode}
        {@const colors = nodeTypeColor(selectedNode.node_type)}
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
              <div class="panel-row"><span class="panel-label">File</span><span class="panel-val mono">{selectedNode.file_path}:{selectedNode.line_start ?? ''}</span></div>
            {/if}
            {#if selectedNode.visibility}
              <div class="panel-row"><span class="panel-label">Visibility</span><Badge variant="default" value={selectedNode.visibility} /></div>
            {/if}
            {#if selectedNode.spec_path}
              <div class="panel-row"><span class="panel-label">Spec</span>
                <button class="spec-link-btn" onclick={() => navigate?.('specs')} title="Navigate to spec">{selectedNode.spec_path}</button>
              </div>
            {/if}
            {#if selectedNode.spec_confidence}
              <div class="panel-row"><span class="panel-label">Confidence</span>
                <Badge variant={selectedNode.spec_confidence === 'High' ? 'success' : selectedNode.spec_confidence === 'Medium' ? 'warning' : 'default'} value={selectedNode.spec_confidence} />
              </div>
            {/if}
            {#if showRiskHeatmap && riskByNodeId.has(selectedNode.id)}
              {@const risk  = riskByNodeId.get(selectedNode.id)}
              {@const score = riskScores.get(selectedNode.id)}
              <div class="panel-section risk-detail-section">
                <div class="panel-label">Risk</div>
                <div class="risk-detail-grid">
                  <div class="risk-detail-item"><span class="risk-detail-val" style="color:{riskFillColor(score ?? 0)}">{score?.toFixed(2) ?? '?'}</span><span class="risk-detail-label">score</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{typeof risk.churn_rate === 'number' ? risk.churn_rate.toFixed(2) : risk.churn_rate ?? 0}</span><span class="risk-detail-label">churn</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{risk.fan_out ?? 0}</span><span class="risk-detail-label">fan-out</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{risk.fan_in ?? 0}</span><span class="risk-detail-label">fan-in</span></div>
                </div>
                <div class="panel-row" style="margin-top:4px"><span class="panel-label">Spec</span><Badge variant={risk.spec_covered ? 'success' : 'warning'} value={risk.spec_covered ? 'covered' : 'missing'} /></div>
              </div>
            {/if}
            {#if selectedNode.doc_comment}
              <div class="panel-section"><div class="panel-label">Doc</div><p class="panel-doc">{selectedNode.doc_comment}</p></div>
            {/if}
            <div class="panel-metrics">
              {#if selectedNode.complexity != null}<div class="metric"><span class="metric-val">{selectedNode.complexity}</span><span class="metric-label">complexity</span></div>{/if}
              {#if selectedNode.churn_count_30d != null}<div class="metric"><span class="metric-val">{selectedNode.churn_count_30d}</span><span class="metric-label">churn/30d</span></div>{/if}
            </div>
            {#if selectedNode.last_modified_at}<div class="panel-row"><span class="panel-label">Modified</span><span class="panel-val">{relativeTime(selectedNode.last_modified_at)}</span></div>{/if}
            {#if selectedNode.last_modified_by}<div class="panel-row"><span class="panel-label">By agent</span><span class="panel-val mono">{selectedNode.last_modified_by}</span></div>{/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

{#if contextMenu}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ctx-menu" style="left:{contextMenu.x}px; top:{contextMenu.y}px"
    onclick={(e) => e.stopPropagation()} role="menu" tabindex="-1" aria-label="Node context menu">
    <button class="ctx-item" role="menuitem" onclick={() => ctxViewDetails(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
      View Details
    </button>
    <button class="ctx-item" role="menuitem" onclick={() => ctxFindUsages(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/></svg>
      Find Usages
    </button>
    <button class="ctx-item" class:disabled={!contextMenu.node.spec_path} role="menuitem"
      onclick={() => ctxGoToSpec(contextMenu.node)} disabled={!contextMenu.node.spec_path}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
      Go to Spec
    </button>
    <div class="ctx-separator"></div>
    <button class="ctx-item" role="menuitem" onclick={() => ctxCopyName(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
      Copy Name
    </button>
  </div>
{/if}

<style>
  .canvas-wrap { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .threshold-banner {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) var(--space-4); font-size: var(--text-xs); flex-shrink: 0;
  }
  .threshold-banner.info { background: color-mix(in srgb, var(--color-info) 10%, transparent); border-bottom: 1px solid color-mix(in srgb, var(--color-info) 30%, transparent); color: #60a5fa; }
  .threshold-banner.warning { background: color-mix(in srgb, var(--color-warning) 10%, transparent); border-bottom: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent); color: #fbbf24; }
  .banner-action {
    margin-left: var(--space-2); background: transparent; border: 1px solid currentColor; color: inherit;
    border-radius: var(--radius-sm); padding: var(--space-1) var(--space-2); font-size: var(--text-xs); font-family: var(--font-body); cursor: pointer; opacity: 0.8;
  }
  .banner-action:hover { opacity: 1; }
  .explanation-banner {
    display: flex; align-items: flex-start; gap: var(--space-2);
    padding: var(--space-2) var(--space-4); background: color-mix(in srgb, var(--color-info) 6%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-info) 20%, transparent); font-size: var(--text-xs);
    color: var(--color-text-secondary); flex-shrink: 0; font-style: italic;
  }
  .list-fallback-wrap { flex: 1; overflow: auto; background: var(--color-surface); }
  .list-table { width: 100%; border-collapse: collapse; font-size: var(--text-sm); }
  .list-table th {
    position: sticky; top: 0; background: var(--color-surface-elevated);
    padding: var(--space-2) var(--space-3); text-align: left; font-size: var(--text-xs);
    font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em;
    color: var(--color-text-muted); border-bottom: 1px solid var(--color-border);
  }
  .list-row { cursor: pointer; border-bottom: 1px solid var(--color-border); transition: background var(--transition-fast); }
  .list-row:hover { background: var(--color-surface-elevated); }
  .list-row td { padding: var(--space-2) var(--space-3); vertical-align: middle; color: var(--color-text); }
  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .muted { color: var(--color-text-muted); }

  .canvas-toolbar {
    display: flex; align-items: center; gap: var(--space-4);
    padding: var(--space-2) var(--space-4); border-bottom: 1px solid var(--color-border);
    background: var(--color-surface); flex-shrink: 0; flex-wrap: wrap;
  }
  .tool-btn {
    display: flex; align-items: center; gap: var(--space-1);
    padding: var(--space-1) var(--space-2); background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius);
    color: var(--color-text-secondary); cursor: pointer; font-size: var(--text-xs);
    font-family: var(--font-body); transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .tool-btn:hover { border-color: var(--color-focus); color: var(--color-text); }
  .tool-btn.active { background: color-mix(in srgb, var(--color-focus) 12%, transparent); border-color: var(--color-focus); color: var(--color-focus); }
  .risk-toggle.active { background: color-mix(in srgb, var(--color-warning) 12%, transparent); border-color: #eab308; color: #eab308; }

  .layout-switcher {
    display: flex; align-items: center; gap: var(--space-1);
    background: var(--color-surface-elevated); border: 1px solid var(--color-border-strong);
    border-radius: var(--radius); padding: var(--space-1);
  }
  .layout-btn {
    padding: var(--space-1) var(--space-2); background: transparent; border: none;
    border-radius: calc(var(--radius) - 2px); color: var(--color-text-muted);
    font-size: var(--text-xs); font-family: var(--font-body); cursor: pointer;
    transition: all var(--transition-fast); white-space: nowrap;
  }
  .layout-btn:hover { color: var(--color-text); }
  .layout-btn.active { background: var(--color-link); color: var(--color-text-inverse); }
  .layout-spinner {
    display: inline-block; width: 12px; height: 12px;
    border: 2px solid var(--color-border); border-top-color: var(--color-focus);
    border-radius: 50%; animation: spin 0.6s linear infinite; margin-left: var(--space-1);
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .heatmap-legend { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-xs); color: var(--color-text-muted); }
  .hm-dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: var(--space-1); flex-shrink: 0; }
  .drill-back { border-color: var(--color-link); color: var(--color-link); }
  .drill-label { font-size: var(--text-xs); color: var(--color-text-secondary); }
  .drill-label strong { color: var(--color-text); font-family: var(--font-mono); }
  .node-count { font-size: var(--text-xs); color: var(--color-text-muted); font-family: var(--font-mono); }
  .legend { display: flex; gap: var(--space-3); align-items: center; flex-wrap: wrap; margin-left: auto; }
  .legend-item { display: flex; align-items: center; gap: var(--space-1); font-size: var(--text-xs); color: var(--color-text-muted); }
  .legend-dot { width: 8px; height: 8px; border-radius: var(--radius-sm); flex-shrink: 0; }

  .graph-area { flex: 1; display: flex; overflow: hidden; position: relative; }
  .graph-svg { flex: 1; width: 100%; height: 100%; background: var(--color-surface); cursor: grab; display: block; }
  .graph-svg.panning { cursor: grabbing; }
  .graph-edge { stroke: #334155; stroke-width: 1.5; stroke-opacity: 0.7; transition: stroke var(--transition-fast); }
  .graph-node { cursor: pointer; }
  .graph-node:hover path, .graph-node:hover ellipse { filter: brightness(1.3); }
  .graph-node.selected path, .graph-node.selected ellipse { filter: brightness(1.4); }
  .graph-node.highlighted path, .graph-node.highlighted ellipse {
    filter: brightness(1.5) drop-shadow(0 0 6px #facc15); stroke-width: 2.5;
  }
  .graph-node.spec-highlighted path, .graph-node.spec-highlighted ellipse {
    filter: brightness(1.4) drop-shadow(0 0 8px #a78bfa);
  }
  .graph-node.dimmed { opacity: 0.3; }

  .ctx-menu {
    position: fixed; z-index: 1000; background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius);
    box-shadow: 0 8px 24px color-mix(in srgb, black 40%, transparent); min-width: 160px; padding: var(--space-1) 0;
    font-size: var(--text-sm); font-family: var(--font-body);
  }
  .ctx-item {
    display: flex; align-items: center; gap: var(--space-2); width: 100%; padding: var(--space-2) var(--space-3);
    background: transparent; border: none; color: var(--color-text);
    cursor: pointer; text-align: left; font-size: var(--text-sm); font-family: var(--font-body);
    transition: background var(--transition-fast);
  }
  .ctx-item:hover:not(.disabled) { background: var(--color-surface); color: var(--color-link); }
  .ctx-item.disabled, .ctx-item:disabled { opacity: 0.4; cursor: default; }
  .ctx-separator { height: 1px; background: var(--color-border); margin: var(--space-1) 0; }

  .spec-legend {
    position: absolute; bottom: var(--space-4); left: var(--space-4);
    background: color-mix(in srgb, black 90%, transparent); border: 1px solid var(--color-border);
    border-radius: var(--radius); padding: var(--space-3); display: flex;
    flex-direction: column; gap: var(--space-2); min-width: 160px;
    backdrop-filter: blur(4px); pointer-events: none;
  }
  .spec-legend-title { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-muted); margin-bottom: var(--space-1); }
  .spec-legend-item { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-xs); color: var(--color-text-secondary); }
  .spec-legend-counts { display: flex; gap: var(--space-3); padding-top: var(--space-1); border-top: 1px solid var(--color-border); margin-top: var(--space-1); }
  .spec-count { font-size: var(--text-xs); font-family: var(--font-mono); font-weight: 600; }
  .spec-count.specced { color: var(--color-success); }
  .spec-count.unspecced { color: var(--color-danger); }

  .detail-panel { width: 280px; flex-shrink: 0; background: var(--color-surface); border-left: 1px solid var(--color-border); display: flex; flex-direction: column; overflow: hidden; }
  .panel-header { padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--color-border); background: var(--color-surface-elevated); flex-shrink: 0; }
  .panel-title-row { display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--space-1); }
  .panel-type { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-muted); }
  .close-btn { background: transparent; border: none; color: var(--color-text-muted); cursor: pointer; font-size: var(--text-lg); line-height: 1; padding: 0; transition: color var(--transition-fast); }
  .close-btn:hover { color: var(--color-text); }
  .panel-name { display: block; font-size: var(--text-base); font-weight: 600; color: var(--color-text); font-family: var(--font-mono); word-break: break-all; }
  .panel-qualified { display: block; font-size: var(--text-xs); color: var(--color-text-muted); font-family: var(--font-mono); margin-top: var(--space-1); word-break: break-all; }
  .panel-body { flex: 1; overflow-y: auto; padding: var(--space-3) var(--space-4); display: flex; flex-direction: column; gap: var(--space-3); }
  .panel-row { display: flex; align-items: flex-start; gap: var(--space-2); }
  .panel-section { display: flex; flex-direction: column; gap: var(--space-1); }
  .panel-label { font-size: var(--text-xs); color: var(--color-text-muted); font-weight: 500; text-transform: uppercase; letter-spacing: 0.05em; flex-shrink: 0; min-width: 64px; }
  .panel-val { font-size: var(--text-sm); color: var(--color-text); word-break: break-all; }
  .panel-val.mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .spec-link-btn { background: transparent; border: none; color: var(--color-link); font-family: var(--font-mono); font-size: var(--text-xs); cursor: pointer; padding: 0; text-align: left; word-break: break-all; text-decoration: underline; }
  .spec-link-btn:hover { color: var(--color-text); }
  .panel-doc { font-size: var(--text-sm); color: var(--color-text-secondary); margin: 0; line-height: 1.5; background: var(--color-surface-elevated); border-radius: var(--radius); padding: var(--space-2); font-style: italic; }
  .panel-metrics { display: flex; gap: var(--space-4); }
  .metric { display: flex; flex-direction: column; align-items: center; }
  .metric-val { font-size: var(--text-lg); font-weight: 700; font-family: var(--font-mono); color: var(--color-text); line-height: 1; }
  .metric-label { font-size: var(--text-xs); color: var(--color-text-muted); }

  .risk-panel { width: 260px; flex-shrink: 0; background: var(--color-surface); border-left: 1px solid var(--color-border); display: flex; flex-direction: column; overflow: hidden; }
  .risk-panel-header { padding: var(--space-2) var(--space-3); border-bottom: 1px solid var(--color-border); background: var(--color-surface-elevated); flex-shrink: 0; display: flex; flex-direction: column; gap: var(--space-1); }
  .risk-panel-title { font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .risk-panel-sub { font-size: var(--text-xs); color: var(--color-text-muted); }
  .risk-empty { flex: 1; display: flex; align-items: center; justify-content: center; font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; padding: var(--space-4); text-align: center; }
  .risk-table-wrap { flex: 1; overflow-y: auto; }
  .risk-table { width: 100%; border-collapse: collapse; font-size: var(--text-xs); }
  .risk-table thead tr { position: sticky; top: 0; background: var(--color-surface-elevated); z-index: 1; }
  .risk-table th { padding: var(--space-1) var(--space-2); text-align: left; border-bottom: 1px solid var(--color-border); }
  .sort-col { background: transparent; border: none; color: var(--color-text-muted); font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; cursor: pointer; padding: 0; font-family: var(--font-body); white-space: nowrap; }
  .sort-col:hover { color: var(--color-text); }
  .risk-row { cursor: pointer; border-bottom: 1px solid var(--color-border); transition: background var(--transition-fast); }
  .risk-row:hover { background: var(--color-surface-elevated); }
  .risk-row.highlighted { background: color-mix(in srgb, var(--color-warning) 8%, transparent); }
  .risk-row td { padding: var(--space-1) var(--space-2); vertical-align: middle; color: var(--color-text); }
  .risk-name { font-family: var(--font-mono); font-size: var(--text-xs); max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .risk-score-chip { display: inline-block; font-family: var(--font-mono); font-size: var(--text-xs); font-weight: 700; padding: var(--space-1); border-radius: var(--radius-sm); border: 1px solid transparent; background: color-mix(in srgb, var(--chip-color) 12%, transparent); color: var(--chip-color); border-color: color-mix(in srgb, var(--chip-color) 25%, transparent); }
  .risk-num { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text-secondary); text-align: right; }
  .risk-panel-footer { padding: var(--space-2) var(--space-3); border-top: 1px solid var(--color-border); background: var(--color-surface-elevated); flex-shrink: 0; }
  .risk-panel-hint { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; }
  .risk-detail-section { background: var(--color-surface-elevated); border-radius: var(--radius); padding: var(--space-2); border: 1px solid var(--color-border); }
  .risk-detail-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-2); margin-top: var(--space-2); }
  .risk-detail-item { display: flex; flex-direction: column; align-items: center; gap: var(--space-1); }
  .risk-detail-val { font-size: var(--text-sm); font-weight: 700; font-family: var(--font-mono); color: var(--color-text); line-height: 1; }
  .risk-detail-label { font-size: var(--text-xs); color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.04em; }

  .close-btn:focus-visible,
  .spec-link-btn:focus-visible,
  .sort-col:focus-visible,
  .tool-btn:focus-visible,
  .layout-btn:focus-visible,
  .banner-action:focus-visible,
  .ctx-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .list-row:focus-visible,
  .risk-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    background: var(--color-surface-elevated);
  }

  .graph-node:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 3px;
  }

  @media (prefers-reduced-motion: reduce) {
    .layout-spinner { animation: none; }
    .graph-edge { transition: none; }
    .tool-btn,
    .layout-btn,
    .list-row,
    .risk-row,
    .ctx-item {
      transition: none;
    }
    .graph-node,
    .graph-node:hover path,
    .graph-node:hover ellipse,
    .graph-node.selected path,
    .graph-node.selected ellipse,
    .graph-node.highlighted path,
    .graph-node.highlighted ellipse,
    .graph-node.spec-highlighted path,
    .graph-node.spec-highlighted ellipse {
      transition: none;
      filter: none;
    }
  }
</style>
