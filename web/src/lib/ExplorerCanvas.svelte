<script>
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
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
    // Exposed for FlowCanvas overlay synchronization
    nodePositions = $bindable({}),
    currentViewBox = $bindable({ x: 0, y: 0, w: 900, h: 600 }),
  } = $props();

  // Shell context API (S4.1)
  const navigate = getContext('navigate');
  const goToRepoTab = getContext('goToRepoTab');
  const openDetailPanel = getContext('openDetailPanel');
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  // ── Layout engine ──────────────────────────────────────────────────────────
  let layoutEngine = $state('column');
  $effect(() => { layoutEngine = viewSpec?.layout ?? 'column'; });

  let nodePositionsMap = $state({});
  let layoutPending = $state(false);

  // Sync internal positions/viewBox to bindable props for overlay consumers (FlowCanvas)
  $effect(() => { nodePositions = nodePositionsMap; });
  $effect(() => { currentViewBox = viewBox; });

  // ── Pan/zoom ───────────────────────────────────────────────────────────────
  let svgEl = $state(null);
  let viewBox = $state({ x: 0, y: 0, w: 900, h: 600 });
  let isPanning = $state(false);
  let panStart = { x: 0, y: 0 };

  // ── Node selection ─────────────────────────────────────────────────────────
  let selectedNode = $state(null);

  // ── Risk heat-map ──────────────────────────────────────────────────────────
  let showRiskHeatmap = $state(false);
  let riskData = $state([]);
  let highlightedNodeId = $state(null);
  let riskSortBy = $state('score');

  $effect(() => {
    if (showRiskHeatmap && repoId) {
      let cancelled = false;
      const currentRepoId = repoId;
      api.repoGraphRisks(currentRepoId).then(data => {
        if (!cancelled) riskData = Array.isArray(data) ? data : [];
      }).catch(() => { if (!cancelled) riskData = []; });
      return () => { cancelled = true; };
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

  // ── Ghost overlays (from spec editing predictions) ─────────────────────────
  let ghostOverlays = $state([]); // [{ nodeId, type: 'new'|'modified'|'removed' }]
  let ghostByNodeId = $derived.by(() => {
    const m = new Map();
    for (const g of ghostOverlays) m.set(g.nodeId, g.type);
    return m;
  });

  // ── Spec detail panel (bidirectional nav — TASK-360) ───────────────────────
  let specPanelNode = $state(null);     // node currently shown in spec panel
  let specContent = $state('');         // fetched raw markdown
  let specLoading = $state(false);
  let specEditDraft = $state('');       // editable copy
  let specLlmInstruction = $state('');
  let specLlmStreaming = $state(false);
  let specLlmExplanation = $state('');
  let specLlmSuggestion = $state(null); // { diff, explanation } | null
  let specPredictTimer = null;

  // Fetch spec content when specPanelNode changes and has a spec_path
  $effect(() => {
    const node = specPanelNode;
    if (!node?.spec_path || !repoId) return;
    let cancelled = false;
    specLoading = true;
    specContent = '';
    specEditDraft = '';
    specLlmSuggestion = null;
    api.specContent(node.spec_path, repoId)
      .then(d => {
        if (!cancelled) {
          specContent = d?.content ?? '';
          specEditDraft = d?.content ?? '';
        }
      })
      .catch(() => { if (!cancelled) specContent = ''; })
      .finally(() => { if (!cancelled) specLoading = false; });
    return () => { cancelled = true; };
  });

  // Run graph predict only when spec draft differs from fetched content (debounced 800ms)
  $effect(() => {
    const draft = specEditDraft;
    const node = specPanelNode;
    // Skip predict if draft matches the original fetched content (no user edits yet)
    if (!draft || !node || !repoId || draft === specContent) return;
    clearTimeout(specPredictTimer);
    specPredictTimer = setTimeout(() => {
      api.graphPredict(repoId, { spec_path: node.spec_path, draft_content: draft })
        .then(result => {
          if (Array.isArray(result?.overlays)) ghostOverlays = result.overlays;
          else if (Array.isArray(result)) ghostOverlays = result;
        })
        .catch(() => { /* ignore prediction errors silently */ });
    }, 800);
    return () => { clearTimeout(specPredictTimer); };
  });

  function openSpecPanel(node) {
    specPanelNode = node;
    specLlmInstruction = '';
    specLlmExplanation = '';
    specLlmSuggestion = null;
    ghostOverlays = [];
  }

  function closeSpecPanel() {
    specPanelNode = null;
    ghostOverlays = [];
    clearTimeout(specPredictTimer);
  }

  async function sendSpecLlmInstruction() {
    if (!specLlmInstruction.trim() || specLlmStreaming || !repoId || !specPanelNode) return;
    const instruction = specLlmInstruction.trim();
    specLlmInstruction = '';
    specLlmStreaming = true;
    specLlmExplanation = '';
    specLlmSuggestion = null;
    try {
      const resp = await api.specsAssist(repoId, {
        spec_path: specPanelNode.spec_path,
        instruction,
        draft_content: specEditDraft || undefined,
      });
      if (!resp.ok) throw new Error(`LLM request failed: ${resp.status}`);
      const reader = resp.body?.getReader();
      if (!reader) throw new Error('No response body');
      const decoder = new TextDecoder();
      let buf = '';
      let done = false;
      while (!done) {
        const { value, done: streamDone } = await reader.read();
        done = streamDone;
        if (value) {
          buf += decoder.decode(value, { stream: true });
          const lines = buf.split('\n');
          buf = lines.pop() ?? '';
          for (const line of lines) {
            if (!line.startsWith('data: ')) continue;
            const raw = line.slice(6);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              if (parsed.event === 'partial' || parsed.type === 'partial') {
                specLlmExplanation += parsed.text ?? parsed.explanation ?? '';
              } else if (parsed.event === 'complete' || parsed.type === 'complete') {
                specLlmSuggestion = { diff: parsed.diff ?? [], explanation: parsed.explanation ?? specLlmExplanation };
                done = true; break;
              } else if (parsed.event === 'error' || parsed.type === 'error') {
                throw new Error(parsed.message ?? 'LLM error');
              }
            } catch (pe) {
              if (pe.message && !pe.message.startsWith('Unexpected token')) throw pe;
            }
          }
        }
      }
    } catch (e) {
      specLlmExplanation = `Error: ${e.message}`;
    } finally {
      specLlmStreaming = false;
    }
  }

  function acceptSpecSuggestion() {
    if (!specLlmSuggestion) return;
    let content = specEditDraft;
    for (const op of specLlmSuggestion.diff) {
      if (op.op === 'add') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const lineEnd = content.indexOf('\n', idx + op.path.length);
          const insertAt = lineEnd !== -1 ? lineEnd + 1 : content.length;
          content = content.slice(0, insertAt) + op.content + '\n' + content.slice(insertAt);
        } else {
          content += '\n' + op.content;
        }
      } else if (op.op === 'replace') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const end = content.slice(idx + op.path.length).match(/\n(#{1,6} )/);
          const endIdx = end?.index !== undefined ? idx + op.path.length + end.index + 1 : content.length;
          content = content.slice(0, idx) + op.path + '\n' + op.content + content.slice(endIdx);
        }
      } else if (op.op === 'remove') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const rest = content.slice(idx + op.path.length);
          const end = rest.match(/\n(#{1,6} )/);
          const endIdx = end?.index !== undefined ? idx + op.path.length + end.index + 1 : content.length;
          content = content.slice(0, idx) + content.slice(endIdx);
        }
      }
    }
    specEditDraft = content;
    specLlmSuggestion = null;
  }

  // ── Query param reading on mount (TASK-360: ?highlight_spec= and ?detail=node:) ──
  $effect(() => {
    if (!nodes.length) return;
    try {
      const params = new URLSearchParams(window.location.search);
      const highlightSpec = params.get('highlight_spec');
      const detailParam = params.get('detail');
      if (highlightSpec) {
        const matchIds = new Set(nodes.filter(n => n.spec_path === highlightSpec).map(n => n.id));
        if (matchIds.size) highlightedNodeIds = matchIds;
      }
      if (detailParam?.startsWith('node:')) {
        const nodeId = detailParam.slice(5);
        const node = nodes.find(n => n.id === nodeId);
        if (node) selectNode(node);
      }
    } catch { /* ignore URL parse errors */ }
  });

  let specCounts = $derived.by(() => {
    const specced = nodes.filter(n => !!n.spec_path).length;
    return { specced, unspecced: nodes.length - specced };
  });

  // ── Performance thresholds ─────────────────────────────────────────────────
  const THRESHOLD_FILTER = 500;
  const THRESHOLD_LIST   = 1000;
  let showAllPrivate = $state(false);

  let showPublicOnlyBanner = $derived.by(() => nodes.length > THRESHOLD_FILTER && nodes.length <= THRESHOLD_LIST);
  let showListFallback     = $derived.by(() => nodes.length > THRESHOLD_LIST && !drillNode);
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

  // Pre-compute max values for size encoding to avoid O(n^2) in encodedNodeSize
  let sizeEncodingMax = $derived.by(() => {
    const enc = viewSpec?.encoding?.size;
    if (!enc) return 1;
    let max = 1;
    for (const n of visibleNodes) {
      const v = n[enc.field] ?? 0;
      if (v > max) max = v;
    }
    return max;
  });

  function encodedNodeSize(node) {
    const enc = viewSpec?.encoding?.size;
    if (!enc) return 1;
    const val = node[enc.field] ?? 0;
    const [minS, maxS] = enc.range ?? [1, 2.5];
    return minS + (val / sizeEncodingMax) * (maxS - minS);
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
    const gen = ++layoutGeneration;

    if (!ns.length) { nodePositionsMap = {}; return; }

    if (eng === 'column') {
      nodePositionsMap = columnLayout(ns);
      return;
    }

    layoutPending = true;
    const w = svgEl?.clientWidth  ?? 900;
    const h = svgEl?.clientHeight ?? 600;

    computeLayout(eng, ns, es, w, h).then(pos => {
      if (gen !== layoutGeneration) return; // stale result
      nodePositionsMap = pos;
      layoutPending = false;
    }).catch(() => {
      if (gen !== layoutGeneration) return; // stale result
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

  // ── Touch handlers (parity with mouse for mobile/tablet) ────────────────
  let lastTouchDist = 0;

  function onTouchStart(e) {
    if (e.touches.length === 1) {
      const t = e.touches[0];
      if (e.target.closest('.graph-node')) return;
      isPanning = true;
      panStart = { x: t.clientX, y: t.clientY };
    } else if (e.touches.length === 2) {
      // Pinch-to-zoom: record initial distance
      isPanning = false;
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      lastTouchDist = Math.hypot(dx, dy);
    }
  }

  function onTouchMove(e) {
    if (e.touches.length === 1 && isPanning) {
      e.preventDefault();
      const t = e.touches[0];
      const dx = t.clientX - panStart.x;
      const dy = t.clientY - panStart.y;
      const scaleX = viewBox.w / (svgEl?.clientWidth  ?? 900);
      const scaleY = viewBox.h / (svgEl?.clientHeight ?? 600);
      viewBox = { ...viewBox, x: viewBox.x - dx * scaleX, y: viewBox.y - dy * scaleY };
      panStart = { x: t.clientX, y: t.clientY };
    } else if (e.touches.length === 2) {
      e.preventDefault();
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.hypot(dx, dy);
      if (lastTouchDist > 0) {
        const factor = lastTouchDist / dist;
        const midX = (e.touches[0].clientX + e.touches[1].clientX) / 2;
        const midY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
        const rect = svgEl?.getBoundingClientRect();
        const mx = rect ? (midX - rect.left) / rect.width  * viewBox.w + viewBox.x : viewBox.x + viewBox.w / 2;
        const my = rect ? (midY - rect.top)  / rect.height * viewBox.h + viewBox.y : viewBox.y + viewBox.h / 2;
        const newW = Math.max(viewBox.w / 5, Math.min(viewBox.w * 5, viewBox.w * factor));
        const scale = newW / viewBox.w;
        viewBox = { x: mx - (mx - viewBox.x) * scale, y: my - (my - viewBox.y) * scale, w: newW, h: viewBox.h * scale };
      }
      lastTouchDist = dist;
    }
  }

  function onTouchEnd() {
    isPanning = false;
    lastTouchDist = 0;
  }

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

  function closeDetail() { selectedNode = null; closeSpecPanel(); }

  function onDblClick(e) {
    const nodeEl = e.target.closest('.graph-node');
    if (!nodeEl) return;
    const nodeId = nodeEl.dataset.nodeId;
    const node = nodes.find(n => n.id === nodeId);
    if (!node) return;

    // Bidirectional Architecture ↔ Spec navigation (TASK-360):
    // Double-click opens the spec detail panel for this node.
    selectNode(node, e);
    openSpecPanel(node);

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

  function onKeydown(e) {
    if (e.key === 'Escape') contextMenu = null;
  }

  function onWindowClick() {
    if (contextMenu) contextMenu = null;
  }

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
    if (!node.spec_path) return;
    if (goToRepoTab) {
      goToRepoTab('specs', { path: node.spec_path });
    } else if (navigate) {
      navigate('specs');
    }
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

  function drillInFromList(node) {
    drillNode = node;
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
    const diff = Math.floor(Date.now() / 1000 - ts);
    if (diff < 60)    return `${diff}s ago`;
    if (diff < 3600)  return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function sortIcon(col) { return riskSortBy === col ? '▼' : '⇅'; }

  const LAYOUT_LABELS = { column: 'Column', graph: 'Force', hierarchical: 'Hierarchical', layered: 'Layered' };
</script>

<svelte:window onkeydown={onKeydown} onclick={onWindowClick} />

<div class="canvas-wrap">
  {#if !nodes.length}
    <EmptyState title={$t('explorer_canvas.no_graph')} description={$t('explorer_canvas.no_graph_desc')} />
  {:else if showListFallback}
    <div class="threshold-banner warning">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
      {$t('explorer_canvas.graph_too_large', { values: { count: nodes.length } })}
    </div>
    <div class="list-fallback-wrap">
      <table class="list-table" aria-label={$t('explorer_canvas.node_list_label')}>
        <thead><tr><th scope="col">{$t('explorer_canvas.legend_type')}</th><th scope="col">{$t('explorer_canvas.col_name')}</th><th scope="col">{$t('explorer_canvas.file')}</th><th scope="col">{$t('detail_panel.spec')}</th></tr></thead>
        <tbody>
          {#each nodes.slice(0, 1000) as node}
            <tr class="list-row" role="button" tabindex="0" aria-label={$t('explorer_canvas.select_node', { values: { name: node.name } })}
              onclick={() => selectNode(node)} onkeydown={(e) => e.key === 'Enter' && selectNode(node)}
              ondblclick={(e) => { e.stopPropagation(); drillInFromList(node); }}>
              <td><Badge variant="default" value={node.node_type ?? '?'} /></td>
              <td class="mono">{node.name}</td>
              <td class="mono muted">{node.file_path ?? ''}</td>
              <td>{node.spec_path ? node.spec_path.split('/').pop() : '—'}</td>
              <td>
                <button class="drill-in-btn" onclick={(e) => { e.stopPropagation(); drillInFromList(node); }}
                  aria-label={$t('explorer_canvas.drill_into_node', { values: { name: node.name } })}>
                  {$t('explorer_canvas.drill_in_btn')}
                </button>
              </td>
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
        <button class="banner-action" onclick={() => (showAllPrivate = true)}>{$t('explorer_canvas.show_all')}</button>
      </div>
    {/if}

    {#if viewSpec?.explanation}
      <div class="explanation-banner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 015.83 1c0 2-3 3-3 3"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {viewSpec.explanation}
      </div>
    {/if}

    <div class="canvas-toolbar">
      <button class="tool-btn" onclick={resetView} title={$t('explorer_canvas.reset_view')} aria-label={$t('explorer_canvas.reset_view')}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M3 12a9 9 0 109-9M3 12V7m0 5H8"/></svg>
        {$t('explorer_canvas.reset')}
      </button>
      {#if drillNode}
        <button class="tool-btn drill-back" onclick={exitDrillIn}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><path d="M19 12H5M12 5l-7 7 7 7"/></svg>
          {$t('explorer_canvas.full_graph')}
        </button>
        <span class="drill-label">{$t('explorer_canvas.drill_in')} <strong>{drillNode.name}</strong></span>
      {/if}

      <!-- Layout engine switcher -->
      <div class="layout-switcher" role="group" aria-label={$t('explorer_canvas.layout_engine_label')}>
        {#each Object.entries(LAYOUT_LABELS) as [eng, label]}
          <button class="layout-btn" class:active={layoutEngine === eng}
            onclick={() => (layoutEngine = eng)} aria-pressed={layoutEngine === eng} title={$t('explorer_canvas.switch_to_layout', { values: { label } })} aria-label={$t('explorer_canvas.switch_to_layout', { values: { label } })}>
            {label}
          </button>
        {/each}
        {#if layoutPending}<span class="layout-spinner" aria-label={$t('explorer_canvas.computing_layout')}></span>{/if}
      </div>

      <button class="tool-btn" class:active={specLinkageOn} onclick={() => (specLinkageOn = !specLinkageOn)}
        title={$t('explorer_canvas.toggle_spec_linkage')} aria-label={$t('explorer_canvas.toggle_spec_linkage')} aria-pressed={specLinkageOn}>{$t('explorer_canvas.spec_linkage')}</button>
      {#if specLinkageOn}
        <button class="tool-btn" class:active={showUnspeccedOnly} onclick={() => (showUnspeccedOnly = !showUnspeccedOnly)}
          title={$t('explorer_canvas.show_unspecced')} aria-label={$t('explorer_canvas.show_unspecced')} aria-pressed={showUnspeccedOnly}>
          {$t('explorer_canvas.unspecced_only')} ({specCounts.unspecced})
        </button>
      {/if}

      <button class="tool-btn risk-toggle" class:active={showRiskHeatmap} onclick={() => (showRiskHeatmap = !showRiskHeatmap)}
        title={showRiskHeatmap ? $t('explorer_canvas.disable_risk') : $t('explorer_canvas.enable_risk')} aria-label={showRiskHeatmap ? $t('explorer_canvas.disable_risk') : $t('explorer_canvas.enable_risk')} aria-pressed={showRiskHeatmap}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
          <circle cx="12" cy="12" r="9"/><path d="M12 8v4l3 3"/><path d="M8 12h1M15 12h1M12 8v1M12 15v1"/>
        </svg>
        {$t('explorer_canvas.risk_heatmap')}
      </button>
      {#if showRiskHeatmap}
        <span class="heatmap-legend">
          <span class="hm-dot" style="background:var(--color-success)"></span>{$t('explorer_canvas.risk_low')}
          <span class="hm-dot" style="background:var(--color-warning)"></span>{$t('explorer_canvas.risk_medium')}
          <span class="hm-dot" style="background:var(--color-danger)"></span>{$t('explorer_canvas.risk_high')}
        </span>
      {/if}

      <span class="node-count">{visibleNodes.length} {visibleNodes.length === 1 ? 'node' : 'nodes'} · {visibleEdges.length} {visibleEdges.length === 1 ? 'edge' : 'edges'}</span>

      {#if !showRiskHeatmap}
        <div class="legend">
          {#each [['legend_package','#7c5ff5'],['legend_module','#4a9eff'],['legend_type','#22c55e'],['legend_interface','#f59e0b'],['legend_function','#14b8a6'],['legend_endpoint','#ef4444'],['legend_component','#a78bfa'],['legend_table','#9ca3af'],['legend_constant','#fbbf24']] as [key, color]}
            <span class="legend-item"><span class="legend-dot" style="background:{color}"></span>{$t(`explorer_canvas.${key}`)}</span>
          {/each}
        </div>
      {/if}
    </div>

    <div class="graph-area" class:has-panel={!!selectedNode} class:has-risk-panel={showRiskHeatmap}>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <svg bind:this={svgEl} class="graph-svg" class:panning={isPanning}
        viewBox="{viewBox.x} {viewBox.y} {viewBox.w} {viewBox.h}"
        role="application"
        aria-label={$t('explorer_canvas.canvas_label')}
        onmousedown={onMouseDown} onmousemove={onMouseMove} onmouseup={onMouseUp}
        onmouseleave={onMouseUp} onwheel={onWheel} oncontextmenu={onContextMenu} ondblclick={onDblClick}
        ontouchstart={onTouchStart} ontouchmove={onTouchMove} ontouchend={onTouchEnd} ontouchcancel={onTouchEnd}>
        <defs>
          <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
            <path d="M0,0 L0,6 L8,3 z" fill="#475569" />
          </marker>
        </defs>

        <!-- Edges -->
        {#each visibleEdges as edge}
          {@const from = getPos(edge.source_id ?? edge.from_node_id ?? edge.from)}
          {@const to   = getPos(edge.target_id ?? edge.to_node_id ?? edge.to)}
          {@const mx = (from.x + to.x) / 2}
          {@const my = (from.y + to.y) / 2}
          {@const rawAngle = Math.atan2(to.y - from.y, to.x - from.x) * 180 / Math.PI}
          {@const labelAngle = rawAngle > 90 || rawAngle < -90 ? rawAngle + 180 : rawAngle}
          {@const label = edge.edge_type ?? ''}
          <g class="edge-group">
            <!-- Wide transparent hit area for hover -->
            <line class="edge-hit" x1={from.x} y1={from.y} x2={to.x} y2={to.y} />
            <line class="graph-edge" x1={from.x} y1={from.y} x2={to.x} y2={to.y}
              stroke={encodedEdgeColor(edge)} stroke-dasharray={encodedEdgeDash(edge)}
              marker-end="url(#arrow)" />
            {#if label}
              <text class="edge-label" x={mx} y={my - 5}
                text-anchor="middle" dominant-baseline="middle"
                transform="rotate({labelAngle}, {mx}, {my})"
                font-size="9" pointer-events="none"
                style="font-family: var(--font-mono); user-select:none">
                {label}
              </text>
            {/if}
          </g>
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
          {@const nodeStroke = isSelected ? '#fff' : isFindHighlighted ? '#facc15' : isSpecHighlighted ? '#a78bfa' : colors.stroke}
          {@const nodeStrokeWidth = isSelected || isHighlighted ? 2.5 : 1.5}
          {@const ghostType = ghostByNodeId.get(node.id)}
          {@const ghostStyle = ghostType === 'new' ? { stroke: '#22c55e', dasharray: '5 3' } : ghostType === 'modified' ? { stroke: '#eab308', dasharray: '5 3' } : ghostType === 'removed' ? { stroke: '#ef4444', dasharray: '5 3' } : null}
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
                stroke={nodeStroke} stroke-width={nodeStrokeWidth} opacity={opacity} />
            {:else if shape === 'ellipse'}
              <ellipse rx="28" ry="14" fill={colors.fill}
                stroke={nodeStroke} stroke-width={nodeStrokeWidth} opacity={opacity} />
            {:else if shape === 'hexagon'}
              <path d={hexPath(0, 0, 22)} fill={colors.fill}
                stroke={nodeStroke} stroke-width={nodeStrokeWidth} opacity={opacity} />
            {:else}
              <path d={rectPath(0, 0, 64, 28)} fill={colors.fill}
                stroke={nodeStroke} stroke-width={nodeStrokeWidth} opacity={opacity} />
            {/if}
            {#if ring}
              <circle class="spec-ring" r="36" fill="none" stroke={ring.color} stroke-width="2.5"
                stroke-dasharray={ring.dashed ? '4 3' : 'none'} opacity="0.85" pointer-events="none" />
            {/if}
            {#if ghostStyle}
              <rect class="ghost-overlay-border" x="-36" y="-18" width="72" height="36" rx="5"
                fill="none" stroke={ghostStyle.stroke} stroke-width="2" stroke-dasharray={ghostStyle.dasharray}
                opacity="0.85" pointer-events="none" aria-hidden="true" />
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
        <div class="spec-legend" aria-label={$t('explorer_canvas.spec_linkage_legend')}>
          <div class="spec-legend-title">{$t('explorer_canvas.spec_coverage')}</div>
          {#each [{ label: $t('explorer_canvas.high_confidence'), color: '#22c55e', dashed: false },{ label: $t('explorer_canvas.medium_confidence'), color: '#eab308', dashed: false },{ label: $t('explorer_canvas.low_confidence'), color: '#f97316', dashed: false },{ label: $t('explorer_canvas.unspecced_label'), color: '#ef4444', dashed: true }] as entry}
            <div class="spec-legend-item">
              <svg width="20" height="12" aria-hidden="true">
                <circle cx="6" cy="6" r="5" fill="none" stroke={entry.color} stroke-width="2" stroke-dasharray={entry.dashed ? '3 2' : 'none'} />
              </svg>
              <span>{entry.label}</span>
            </div>
          {/each}
          <div class="spec-legend-counts">
            <span class="spec-count specced">{specCounts.specced} {$t('explorer_canvas.specced')}</span>
            <span class="spec-count unspecced">{specCounts.unspecced} {$t('explorer_canvas.unspecced')}</span>
          </div>
        </div>
      {/if}

      {#if showRiskHeatmap}
        <div class="risk-panel" role="complementary" aria-label={$t('explorer_canvas.risk_heatmap_panel')}>
          <div class="risk-panel-header">
            <span class="risk-panel-title">{$t('explorer_canvas.risk_heatmap')}</span>
            <span class="risk-panel-sub">{$t('explorer_canvas.top_risk')}</span>
          </div>
          {#if riskData.length === 0}
            <div class="risk-empty">{repoId ? $t('explorer_canvas.loading_risk') : $t('explorer_canvas.no_risk_data')}</div>
          {:else}
            <div class="risk-table-wrap">
              <table class="risk-table" aria-label={$t('explorer_canvas.risk_table_label')}>
                <thead>
                  <tr>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'name')} aria-label={$t('explorer_canvas.sort_by_name')}>{$t('explorer_canvas.col_name')} {sortIcon('name')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'score')} aria-label={$t('explorer_canvas.sort_by_score')}>{$t('explorer_canvas.col_score')} {sortIcon('score')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'churn_rate')} aria-label={$t('explorer_canvas.sort_by_churn')}>{$t('explorer_canvas.col_churn')} {sortIcon('churn_rate')}</button></th>
                    <th scope="col"><button class="sort-col" onclick={() => (riskSortBy = 'fan_out')} aria-label={$t('explorer_canvas.sort_by_fan_out')}>{$t('explorer_canvas.col_fan_out')} {sortIcon('fan_out')}</button></th>
                  </tr>
                </thead>
                <tbody>
                  {#each topRiskNodes as entry}
                    {@const fill = riskFillColor(entry.score)}
                    <tr class="risk-row" class:highlighted={highlightedNodeId === entry.id} role="button" tabindex="0"
                      aria-label={$t('explorer_canvas.highlight_node', { values: { name: entry.name } })}
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
            <div class="risk-panel-footer"><span class="risk-panel-hint">{$t('explorer_canvas.risk_hint')}</span></div>
          {/if}
        </div>
      {/if}

      {#if selectedNode}
        {@const colors = nodeTypeColor(selectedNode.node_type)}
        <div class="detail-panel" role="complementary" aria-label={$t('explorer_canvas.node_details')}>
          <div class="panel-header" style="border-left: 3px solid {colors.stroke}">
            <div class="panel-title-row">
              <span class="panel-type">{selectedNode.node_type}</span>
              <button class="close-btn" onclick={closeDetail} aria-label={$t('explorer_canvas.close_detail')}>×</button>
            </div>
            <span class="panel-name">{selectedNode.name}</span>
            {#if selectedNode.qualified_name && selectedNode.qualified_name !== selectedNode.name}
              <span class="panel-qualified">{selectedNode.qualified_name}</span>
            {/if}
          </div>
          <div class="panel-body">
            {#if selectedNode.file_path}
              <div class="panel-row"><span class="panel-label">{$t('explorer_canvas.file')}</span><span class="panel-val mono">{selectedNode.file_path}:{selectedNode.line_start ?? ''}</span></div>
            {/if}
            {#if selectedNode.visibility}
              <div class="panel-row"><span class="panel-label">{$t('explorer_canvas.visibility')}</span><Badge variant="default" value={selectedNode.visibility} /></div>
            {/if}
            {#if selectedNode.spec_path}
              <div class="panel-row"><span class="panel-label">{$t('detail_panel.spec')}</span>
                <button class="spec-link-btn" onclick={() => { if (goToEntityDetail) goToEntityDetail('spec', selectedNode.spec_path, { path: selectedNode.spec_path }); else openDetailPanel?.({ type: 'spec', id: selectedNode.spec_path }); }} title={$t('explorer_canvas.navigate_to_spec')}>{selectedNode.spec_path}</button>
              </div>
            {/if}
            {#if selectedNode.spec_confidence}
              <div class="panel-row"><span class="panel-label">{$t('explorer_canvas.confidence')}</span>
                <Badge variant={selectedNode.spec_confidence === 'High' ? 'success' : selectedNode.spec_confidence === 'Medium' ? 'warning' : 'default'} value={selectedNode.spec_confidence} />
              </div>
            {/if}
            {#if showRiskHeatmap && riskByNodeId.has(selectedNode.id)}
              {@const risk  = riskByNodeId.get(selectedNode.id)}
              {@const score = riskScores.get(selectedNode.id)}
              <div class="panel-section risk-detail-section">
                <div class="panel-label">{$t('explorer_canvas.risk')}</div>
                <div class="risk-detail-grid">
                  <div class="risk-detail-item"><span class="risk-detail-val" style="color:{riskFillColor(score ?? 0)}">{score?.toFixed(2) ?? '?'}</span><span class="risk-detail-label">{$t('explorer_canvas.score')}</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{typeof risk.churn_rate === 'number' ? risk.churn_rate.toFixed(2) : risk.churn_rate ?? 0}</span><span class="risk-detail-label">{$t('explorer_canvas.churn')}</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{risk.fan_out ?? 0}</span><span class="risk-detail-label">{$t('explorer_canvas.fan_out')}</span></div>
                  <div class="risk-detail-item"><span class="risk-detail-val">{risk.fan_in ?? 0}</span><span class="risk-detail-label">{$t('explorer_canvas.fan_in')}</span></div>
                </div>
                <div class="panel-row" style="margin-top:4px"><span class="panel-label">{$t('detail_panel.spec')}</span><Badge variant={risk.spec_covered ? 'success' : 'warning'} value={risk.spec_covered ? $t('explorer_canvas.covered') : $t('explorer_canvas.missing')} /></div>
              </div>
            {/if}
            {#if selectedNode.doc_comment}
              <div class="panel-section"><div class="panel-label">{$t('explorer_canvas.doc')}</div><p class="panel-doc">{selectedNode.doc_comment}</p></div>
            {/if}
            <div class="panel-metrics">
              {#if selectedNode.complexity != null}<div class="metric"><span class="metric-val">{selectedNode.complexity}</span><span class="metric-label">complexity</span></div>{/if}
              {#if selectedNode.churn_count_30d != null}<div class="metric"><span class="metric-val">{selectedNode.churn_count_30d}</span><span class="metric-label">churn/30d</span></div>{/if}
            </div>
            {#if selectedNode.last_modified_at}<div class="panel-row"><span class="panel-label">{$t('explorer_canvas.modified')}</span><span class="panel-val">{relativeTime(selectedNode.last_modified_at)}</span></div>{/if}
            {#if selectedNode.last_modified_by}<div class="panel-row"><span class="panel-label">{$t('explorer_canvas.by_agent')}</span><span class="panel-val mono">{selectedNode.last_modified_by}</span></div>{/if}
          </div>
        </div>
      {/if}

      <!-- ── Spec detail panel (bidirectional nav TASK-360) ─────────────── -->
      {#if specPanelNode}
        <div class="spec-detail-panel" role="complementary" aria-label={$t('explorer_canvas.spec_detail_label', { values: { name: specPanelNode.name } })} data-testid="spec-detail-panel">
          <div class="spec-panel-header">
            <div class="spec-panel-title-row">
              <span class="spec-panel-label">{$t('explorer_canvas.governing_spec')}</span>
              <button class="close-btn" onclick={closeSpecPanel} aria-label={$t('explorer_canvas.close_spec_panel')}>×</button>
            </div>
            <span class="spec-panel-node-name">{specPanelNode.name}</span>
          </div>
          <div class="spec-panel-body">
            {#if !specPanelNode.spec_path}
              <div class="no-spec-state" data-testid="no-governing-spec">
                <p class="no-spec-text">{$t('explorer_canvas.no_governing_spec')}</p>
                <p class="no-spec-hint">{$t('explorer_canvas.no_spec_hint')}</p>
                <button class="create-spec-btn" onclick={() => { if (goToRepoTab) { goToRepoTab('specs', { create: 'true' }); } else { navigate?.('specs'); } }} data-testid="create-spec-btn">
                  {$t('explorer_canvas.create_spec')}
                </button>
              </div>
            {:else}
              <div class="spec-path-row">
                <span class="spec-path-label">{$t('detail_panel.path')}</span>
                <button class="spec-path-val mono spec-path-link" onclick={() => { if (goToEntityDetail) goToEntityDetail('spec', specPanelNode.spec_path, { path: specPanelNode.spec_path }); }} title="View spec details">{specPanelNode.spec_path.split('/').pop()?.replace(/\.md$/, '') ?? specPanelNode.spec_path}</button>
              </div>

              {#if specLoading}
                <div class="spec-loading">{$t('explorer_canvas.loading_spec')}</div>
              {:else if specContent}
                <textarea
                  class="spec-editor-textarea"
                  bind:value={specEditDraft}
                  aria-label={$t('explorer_canvas.spec_editor')}
                  spellcheck="false"
                  data-testid="spec-editor"
                ></textarea>

                {#if ghostOverlays.length > 0}
                  <div class="ghost-legend" data-testid="ghost-legend">
                    <span class="ghost-chip new">{$t('explorer_canvas.ghost_new')}</span>
                    <span class="ghost-chip modified">{$t('explorer_canvas.ghost_modified')}</span>
                    <span class="ghost-chip removed">{$t('explorer_canvas.ghost_removed')}</span>
                    <span class="ghost-label">{$t('explorer_canvas.predicted_impact')}</span>
                  </div>
                {/if}

                {#if specLlmSuggestion}
                  <div class="llm-suggestion" data-testid="llm-suggestion">
                    {#if specLlmSuggestion.explanation}
                      <p class="suggestion-expl">{specLlmSuggestion.explanation}</p>
                    {/if}
                    <div class="suggestion-btns">
                      <button class="suggestion-accept-btn" onclick={acceptSpecSuggestion}>{$t('explorer_canvas.accept')}</button>
                      <button class="suggestion-dismiss-btn" onclick={() => { specLlmSuggestion = null; }}>{$t('explorer_canvas.dismiss')}</button>
                    </div>
                  </div>
                {/if}

                {#if specLlmStreaming && specLlmExplanation}
                  <div class="llm-streaming" aria-live="polite">
                    <span class="streaming-lbl">{$t('explorer_canvas.thinking')}</span>
                    <p class="streaming-txt">{specLlmExplanation}</p>
                  </div>
                {/if}

                <div class="llm-input-row">
                  <textarea
                    class="llm-textarea"
                    bind:value={specLlmInstruction}
                    placeholder={$t('explorer_canvas.spec_change_placeholder')}
                    rows="2"
                    disabled={specLlmStreaming}
                    aria-label={$t('explorer_canvas.llm_instruction')}
                    onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); sendSpecLlmInstruction(); } }}
                  ></textarea>
                  <button
                    class="llm-send-btn"
                    onclick={sendSpecLlmInstruction}
                    disabled={!specLlmInstruction.trim() || specLlmStreaming}
                    aria-label={$t('explorer_canvas.send_llm')}
                  >
                    {specLlmStreaming ? '…' : '↑'}
                  </button>
                </div>
              {:else}
                <p class="spec-no-content">{$t('explorer_canvas.spec_unavailable')}</p>
              {/if}
            {/if}
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
    onclick={(e) => e.stopPropagation()} role="menu" tabindex="-1" aria-label={$t('explorer_canvas.context_menu_label')}>
    <button class="ctx-item" role="menuitem" onclick={() => ctxViewDetails(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/></svg>
      {$t('explorer_canvas.view_details')}
    </button>
    <button class="ctx-item" role="menuitem" onclick={() => ctxFindUsages(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><path d="M10 13a5 5 0 007.54.54l3-3a5 5 0 00-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 00-7.54-.54l-3 3a5 5 0 007.07 7.07l1.71-1.71"/></svg>
      {$t('explorer_canvas.find_usages')}
    </button>
    <button class="ctx-item" class:disabled={!contextMenu.node.spec_path} role="menuitem"
      onclick={() => ctxGoToSpec(contextMenu.node)} disabled={!contextMenu.node.spec_path}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
      {$t('explorer_canvas.go_to_spec')}
    </button>
    <div class="ctx-separator"></div>
    <button class="ctx-item" role="menuitem" onclick={() => ctxCopyName(contextMenu.node)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
      {$t('explorer_canvas.copy_name')}
    </button>
  </div>
{/if}

<style>
  .canvas-wrap { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .threshold-banner {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) var(--space-4); font-size: var(--text-xs); flex-shrink: 0;
  }
  .threshold-banner.info { background: color-mix(in srgb, var(--color-info) 10%, transparent); border-bottom: 1px solid color-mix(in srgb, var(--color-info) 30%, transparent); color: var(--color-info); }
  .threshold-banner.warning { background: color-mix(in srgb, var(--color-warning) 10%, transparent); border-bottom: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent); color: var(--color-warning); }
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
  .drill-in-btn {
    padding: var(--space-1) var(--space-2); background: transparent;
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-sm);
    color: var(--color-link); font-size: var(--text-xs); font-family: var(--font-body);
    cursor: pointer; white-space: nowrap; transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .drill-in-btn:hover { background: color-mix(in srgb, var(--color-link) 10%, transparent); border-color: var(--color-link); }
  .drill-in-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

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
  .risk-toggle.active { background: color-mix(in srgb, var(--color-warning) 12%, transparent); border-color: var(--color-warning); color: var(--color-warning); }

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

  .graph-area { flex: 1; display: flex; overflow: hidden; position: relative; contain: layout style; }
  .graph-svg { flex: 1; width: 100%; height: 100%; background: var(--color-surface); cursor: grab; display: block; }
  .graph-svg.panning { cursor: grabbing; }
  .graph-edge { stroke: var(--color-border-strong); stroke-width: 1.5; stroke-opacity: 0.7; transition: stroke var(--transition-fast); }
  .graph-node { cursor: pointer; }
  .graph-node:hover path, .graph-node:hover ellipse { filter: brightness(1.3); }
  .graph-node.selected path, .graph-node.selected ellipse { filter: brightness(1.4); }
  .graph-node.highlighted path, .graph-node.highlighted ellipse {
    filter: brightness(1.5); stroke-width: 2.5;
  }
  .graph-node.spec-highlighted path, .graph-node.spec-highlighted ellipse {
    filter: brightness(1.4);
  }
  .graph-node.dimmed { opacity: 0.3; }

  .ctx-menu {
    position: fixed; z-index: 1100; background: var(--color-surface-elevated);
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
    background: color-mix(in srgb, black 92%, transparent); border: 1px solid var(--color-border);
    border-radius: var(--radius); padding: var(--space-3); display: flex;
    flex-direction: column; gap: var(--space-2); min-width: 160px;
    pointer-events: none;
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

  /* ── Spec detail panel (TASK-360 bidirectional nav) ─────────────────────── */
  .spec-detail-panel {
    width: 320px; flex-shrink: 0; background: var(--color-surface);
    border-left: 1px solid var(--color-border); display: flex; flex-direction: column; overflow: hidden;
  }
  .spec-panel-header {
    padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated); flex-shrink: 0;
  }
  .spec-panel-title-row { display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--space-1); }
  .spec-panel-label { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-muted); }
  .spec-panel-node-name { display: block; font-size: var(--text-base); font-weight: 600; color: var(--color-text); font-family: var(--font-mono); word-break: break-all; }
  .spec-panel-body { flex: 1; overflow-y: auto; padding: var(--space-3) var(--space-4); display: flex; flex-direction: column; gap: var(--space-3); }
  .no-spec-state { display: flex; flex-direction: column; gap: var(--space-3); align-items: flex-start; padding: var(--space-2) 0; }
  .no-spec-text { font-size: var(--text-base); font-weight: 600; color: var(--color-text); margin: 0; }
  .no-spec-hint { font-size: var(--text-xs); color: var(--color-text-muted); margin: 0; line-height: 1.5; }
  .create-spec-btn {
    padding: var(--space-2) var(--space-4); background: var(--color-primary, #3b82f6);
    border: none; border-radius: var(--radius); color: #fff;
    font-family: var(--font-body); font-size: var(--text-sm); font-weight: 500; cursor: pointer;
  }
  .create-spec-btn:hover { opacity: 0.88; }
  .create-spec-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .spec-path-row { display: flex; align-items: flex-start; gap: var(--space-2); }
  .spec-path-label { font-size: var(--text-xs); color: var(--color-text-muted); font-weight: 500; text-transform: uppercase; letter-spacing: 0.05em; flex-shrink: 0; min-width: 40px; }
  .spec-path-val { font-size: var(--text-xs); color: var(--color-text-secondary); font-family: var(--font-mono); word-break: break-all; }
  .spec-path-link { background: none; border: none; cursor: pointer; padding: 0; text-align: left; color: var(--color-link, var(--color-primary)); }
  .spec-path-link:hover { text-decoration: underline; }
  .spec-loading { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; }
  .spec-no-content { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; margin: 0; }
  .spec-editor-textarea {
    width: 100%; min-height: 160px; max-height: 280px; box-sizing: border-box;
    padding: var(--space-2) var(--space-3); background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius);
    color: var(--color-text); font-family: var(--font-mono); font-size: var(--text-xs);
    line-height: 1.6; resize: vertical;
  }
  .spec-editor-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: -2px; border-color: var(--color-focus); }
  .ghost-legend {
    display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
    padding: var(--space-2) var(--space-2); background: var(--color-surface-elevated);
    border: 1px solid var(--color-border); border-radius: var(--radius); font-size: var(--text-xs);
  }
  .ghost-label { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; flex: 1; }
  .ghost-chip { font-size: 10px; font-family: var(--font-mono); padding: 1px 5px; border-radius: 3px; border: 1px dashed; }
  .ghost-chip.new { color: #22c55e; border-color: #22c55e; background: color-mix(in srgb, #22c55e 8%, transparent); }
  .ghost-chip.modified { color: #eab308; border-color: #eab308; background: color-mix(in srgb, #eab308 8%, transparent); }
  .ghost-chip.removed { color: #ef4444; border-color: #ef4444; background: color-mix(in srgb, #ef4444 8%, transparent); }
  .llm-suggestion {
    padding: var(--space-2) var(--space-3); border: 1px solid var(--color-primary, #3b82f6);
    border-radius: var(--radius); background: color-mix(in srgb, var(--color-primary, #3b82f6) 5%, transparent);
    display: flex; flex-direction: column; gap: var(--space-2);
  }
  .suggestion-expl { font-size: var(--text-xs); color: var(--color-text-secondary); margin: 0; line-height: 1.5; }
  .suggestion-btns { display: flex; gap: var(--space-2); }
  .suggestion-accept-btn {
    padding: var(--space-1) var(--space-3); background: var(--color-primary, #3b82f6);
    border: none; border-radius: var(--radius); color: #fff;
    font-size: var(--text-xs); font-family: var(--font-body); cursor: pointer;
  }
  .suggestion-dismiss-btn {
    padding: var(--space-1) var(--space-3); background: transparent;
    border: 1px solid var(--color-border-strong); border-radius: var(--radius);
    color: var(--color-text-secondary); font-size: var(--text-xs); font-family: var(--font-body); cursor: pointer;
  }
  .llm-streaming { padding: var(--space-2) var(--space-3); background: var(--color-surface-elevated); border: 1px solid var(--color-border); border-radius: var(--radius); }
  .streaming-lbl { font-size: var(--text-xs); color: var(--color-text-muted); font-weight: 500; display: block; margin-bottom: var(--space-1); }
  .streaming-txt { font-size: var(--text-xs); color: var(--color-text-secondary); margin: 0; line-height: 1.5; white-space: pre-wrap; }
  .llm-input-row { display: flex; gap: var(--space-2); align-items: flex-end; }
  .llm-textarea {
    flex: 1; min-height: 44px; max-height: 90px; box-sizing: border-box;
    padding: var(--space-2) var(--space-3); background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius);
    color: var(--color-text); font-family: var(--font-body); font-size: var(--text-sm); resize: vertical;
  }
  .llm-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: -2px; border-color: var(--color-focus); }
  .llm-textarea:disabled { opacity: 0.6; cursor: not-allowed; }
  .llm-send-btn {
    width: 34px; height: 34px; flex-shrink: 0; background: var(--color-primary, #3b82f6);
    border: none; border-radius: var(--radius); color: #fff;
    font-size: var(--text-base); cursor: pointer; display: flex; align-items: center; justify-content: center;
  }
  .llm-send-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .llm-send-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .suggestion-accept-btn:focus-visible,
  .suggestion-dismiss-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
</style>
