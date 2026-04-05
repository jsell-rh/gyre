<script>
  import { onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import EmptyState from './EmptyState.svelte';

  let {
    repoId = '',
    nodes = [],
    edges = [],
    activeQuery = null,
    filter = 'all',
    lens = 'structural',
    canvasState = $bindable({ selectedNode: null, zoom: 1, visibleGroups: [], breadcrumb: [] }),
    onNodeDetail = () => {},
    onInteractiveQuery = () => {},
    ghostOverlays = [],
    filters = null,
    traceData = null, // { spans: [], root_spans: [] } from GateTraceResponse
    queryResult = null, // { node_metrics: Record<string, number>, ... } from dry-run resolve
    assertionResults = [], // [{ line, assertion_text, passed, explanation }] from spec assertion check
    assertionSpecPath = null, // spec path currently being edited (for assertion badge rendering)
  } = $props();

  // ── Interactive $clicked mode ──────────────────────────────────────────
  // When a view query uses "$clicked" as scope.node, we store the template
  // and re-evaluate on each subsequent click, substituting the clicked node.
  let interactiveQueryTemplate = $state(null);

  $effect(() => {
    if (activeQuery?.scope?.node === '$clicked') {
      // Store template (use JSON round-trip to avoid Svelte 5 $state proxy issues)
      interactiveQueryTemplate = JSON.parse(JSON.stringify(activeQuery));
    } else if (!activeQuery) {
      interactiveQueryTemplate = null;
    }
  });

  // ── Color palette (depth-based HSL for tree groups) ──────────────────
  const TREE_HUES = [210, 260, 160, 30, 340, 50, 190, 300];
  function treeGroupColor(depth, childIndex) {
    const hue = TREE_HUES[(depth + (childIndex || 0)) % TREE_HUES.length];
    return {
      hue,
      border: `hsl(${hue}, 40%, 45%)`,
      fill: `hsla(${hue}, 35%, 20%, 0.5)`,
      fillSummary: `hsla(${hue}, 30%, 15%, 0.8)`,
    };
  }

  // Precomputed GovernedBy index: Set of node IDs that have a GovernedBy edge.
  // Rebuilt when edges change, used by specBorderColor for O(1) lookups instead of O(E) scans.
  let governedByIndex = $derived.by(() => {
    const idx = new Set();
    for (const e of edges) {
      if (e.deleted_at) continue;
      if (edgeType(e) === 'governed_by') idx.add(edgeSrc(e));
    }
    return idx;
  });

  function specBorderColor(node) {
    if (!node) return '#64748b';
    const conf = node.spec_confidence;
    // Check precomputed GovernedBy index (O(1) instead of O(E))
    const hasGovEdge = node.id && governedByIndex.has(node.id);
    // Green: explicit high confidence or confirmed GovernedBy edge (without conflicting heuristic)
    if (conf === 'high') return '#22c55e';
    if (hasGovEdge && !node.spec_path) return '#22c55e';
    // If both GovernedBy edge AND heuristic spec_path exist, check if they agree.
    // When they might conflict (spec_path from heuristic != GovernedBy spec), show amber.
    if (hasGovEdge && node.spec_path) {
      // Look up the GovernedBy edge target to compare with heuristic spec_path
      const govEdge = edges.find(e => !e.deleted_at && edgeSrc(e) === node.id && edgeType(e) === 'governed_by');
      if (govEdge) {
        const govSpecNode = nodes.find(n => n.id === edgeTgt(govEdge));
        const govSpecPath = govSpecNode?.file_path || govSpecNode?.name || '';
        // If they match, it's a confirmed strong signal
        if (govSpecPath && node.spec_path.includes(govSpecPath.replace(/^specs\//, ''))) {
          return '#22c55e';
        }
      }
      // Conflicting: GovernedBy points to different spec than heuristic spec_path
      // Show amber to surface the governance conflict to the user
      return '#eab308';
    }
    // Amber: spec_path present without GovernedBy edge — heuristic match only
    if (node.spec_path) return '#eab308';
    if (conf === 'medium' || conf === 'low') return '#eab308';
    // Red: no spec coverage
    return '#ef4444';
  }

  const EDGE_COLORS = {
    calls: '#60a5fa',
    implements: '#34d399',
    depends_on: '#64748b',
    field_of: '#94a3b8',
    routes_to: '#f97316',
    governed_by: '#fbbf24',
    renders: '#a78bfa',       // purple-400
    persists_to: '#2dd4bf',   // teal-400
    produced_by: '#fb923c',   // orange-400
    returns: '#38bdf8',       // sky-400
    contains: '#cbd5e1',      // slate-300
  };

  // ── Edge field normalization helpers ──────────────────────────────────
  // The API can return edges with different field names depending on the
  // serialization path. These helpers normalize access to avoid 50+ inline
  // fallback chains throughout the rendering code.
  function edgeSrc(e) { return e.source_id ?? e.from_node_id ?? e.from; }
  function edgeTgt(e) { return e.target_id ?? e.to_node_id ?? e.to; }
  function edgeType(e) { return (e.edge_type ?? e.type ?? '').toLowerCase(); }

  // ── Constants ────────────────────────────────────────────────────────
  const MINIMAP_W = 180;
  const MINIMAP_H = 110;
  const MIN_ZOOM = 0.05;
  const MAX_ZOOM = 20.0;
  const LERP_SPEED = 0.15;

  // ── Canvas state ─────────────────────────────────────────────────────
  let canvasEl = $state(null);
  let minimapEl = $state(null);
  let containerEl = $state(null);
  let W = $state(900);
  let H = $state(600);

  // Camera: world coordinates centered on screen
  let cam = { x: 0, y: 0, zoom: 0.5 };
  let targetCam = { x: 0, y: 0, zoom: 0.5 };
  let needsAnim = $state(true);

  // ── ResizeObserver: keep W/H in sync with container ────────────────
  $effect(() => {
    const el = containerEl;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) {
        W = Math.round(width);
        H = Math.round(height);
        scheduleRedraw();
      }
    });
    ro.observe(el);
    // Initial measurement
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      W = Math.round(rect.width);
      H = Math.round(rect.height);
    }
    return () => ro.disconnect();
  });

  let isPanning = $state(false);
  let panStart = { x: 0, y: 0 };
  let panCamStart = { x: 0, y: 0 };

  let selectedNodeId = $state(null);
  let multiSelectedIds = $state(new Set()); // Shift+Click multi-select for concept creation
  let hoveredNodeId = $state(null);
  let breadcrumb = $state([]);
  let animFrame = null;

  // Drill-down fade transition: dim unrelated nodes during zoom
  let drillFadeAlpha = $state(1.0); // 1.0 = fully visible, fading to 0.15
  let drillFadeTarget = $state(null); // nodeId being drilled into (or null)

  // Recent interaction trail for conversational context (sent with messages)
  let recentInteractions = $state([]);
  function trackInteraction(action) {
    recentInteractions = [...recentInteractions.slice(-9), action];
  }

  let tooltipNode = $state(null);
  let tooltipPos = $state({ x: 0, y: 0 });

  // Context menu state
  let contextMenu = $state(null); // { x, y, node }

  // Evaluative lens metric mode — defaults to trace-based when data exists
  let evaluativeMetric = $state('complexity'); // 'complexity' | 'churn' | 'incoming_calls' | 'test_coverage' | 'span_duration' | 'span_count' | 'error_rate'
  // Auto-select trace metric when trace data becomes available and user hasn't
  // explicitly chosen a trace metric (auto-upgrade from any structural metric)
  const TRACE_METRICS = new Set(['span_duration', 'span_count', 'error_rate']);
  $effect(() => {
    if (traceData?.spans?.length > 0 && !TRACE_METRICS.has(evaluativeMetric)) {
      evaluativeMetric = 'span_duration';
    }
  });
  let observableBannerVisible = $state(false); // show "requires telemetry" banner for observable lens
  let srAnnouncement = $state(''); // Screen reader announcements via ARIA live region

  // ── Ghost overlay state (spec editing preview) ──────────────────────
  // ghostOverlays: [{ id, name, type, action: 'add'|'change'|'remove', edges?: [] }]
  let ghostById = $derived.by(() => {
    const m = new Map();
    for (const g of ghostOverlays) m.set(g.id, g);
    return m;
  });
  let hasGhosts = $derived(ghostOverlays.length > 0);
  // Reset pulse cycle counter when ghost overlays change
  $effect(() => {
    if (ghostOverlays.length > 0) {
      ghostAnimCycles = 0;
      ghostPulsePhase = 0;
    }
  });
  let ghostPulsePhase = $state(0); // 0..1 pulsing animation
  let ghostAnimCycles = $state(0); // count of completed pulse cycles

  // Trace path numbering state (DFS execution order badges for "Trace from here")
  let tracePathOrder = $state(new Map()); // nodeId → step number (1-based)
  let tracePathEdges = $state([]); // [{fromId, toId, edgeType}] in DFS order

  // Pre-compute incoming call counts to avoid O(N*E) per anomaly check
  let incomingCallCounts = $derived.by(() => {
    const counts = new Map();
    for (const e of edges) {
      const tgt = edgeTgt(e);
      const et = edgeType(e);
      if (et === 'calls' && tgt) {
        counts.set(tgt, (counts.get(tgt) ?? 0) + 1);
      }
    }
    return counts;
  });

  // Anomaly detection state — visible in both structural and evaluative lenses
  // (spec: Risk Map is an overlay on the architecture canvas, not evaluative-only)
  let anomalies = $derived.by(() => {
    if (lens !== 'structural' && lens !== 'evaluative') return [];
    const results = [];
    const nodeById = new Map();
    for (const n of nodes) nodeById.set(n.id, n);

    // Pre-compute trait implementation counts
    const traitImplCounts = new Map(); // traitId → { total, tested }
    for (const e of edges) {
      const src = edgeSrc(e);
      const tgt = edgeTgt(e);
      const et = edgeType(e);
      if (et === 'implements') {
        const tgtNode = nodeById.get(tgt);
        if (tgtNode && (tgtNode.node_type === 'interface' || tgtNode.node_type === 'trait')) {
          if (!traitImplCounts.has(tgt)) traitImplCounts.set(tgt, { total: 0, tested: 0 });
          const entry = traitImplCounts.get(tgt);
          entry.total++;
          const srcNode = nodeById.get(src);
          if (srcNode?.test_coverage > 0 || srcNode?.test_node) entry.tested++;
        }
      }
    }

    // Compute distribution-relative thresholds (percentile-based, not hardcoded)
    const complexities = nodes.filter(n => n.complexity != null && !n.deleted_at).map(n => n.complexity);
    const coverages = nodes.filter(n => n.test_coverage != null && !n.deleted_at).map(n => n.test_coverage);
    const callCounts = [];
    for (const n of nodes) {
      if (!n.deleted_at && n.node_type !== 'tree-group') callCounts.push(incomingCallCounts.get(n.id) ?? 0);
    }
    function p90(arr) {
      if (arr.length === 0) return Infinity;
      const sorted = [...arr].sort((a, b) => a - b);
      const idx = Math.min(Math.floor(sorted.length * 0.9), sorted.length - 1);
      return sorted[idx];
    }
    function p10(arr) {
      if (arr.length === 0) return 0;
      const sorted = [...arr].sort((a, b) => a - b);
      const idx = Math.min(Math.floor(sorted.length * 0.1), sorted.length - 1);
      return sorted[idx];
    }
    const complexityThreshold = Math.max(10, p90(complexities));
    const coverageThreshold = Math.min(0.5, p10(coverages));
    const callThreshold = Math.max(3, p90(callCounts));

    for (const n of nodes) {
      if (n.node_type === 'tree-group' || n.deleted_at) continue;
      const calls = incomingCallCounts.get(n.id) ?? 0;

      // Pattern 1: High complexity + low test coverage (distribution-relative)
      if ((n.complexity ?? 0) > complexityThreshold && (n.test_coverage ?? 0) < coverageThreshold) {
        results.push({
          nodeId: n.id,
          nodeName: n.name ?? n.qualified_name ?? '?',
          message: `High complexity (${n.complexity}, p90=${complexityThreshold}) but low test coverage (${Math.round((n.test_coverage ?? 0) * 100)}%)`,
          severity: 'high',
        });
      }
      // Pattern 2: Heavily depended on with no spec (distribution-relative)
      if (calls > callThreshold && !n.spec_path) {
        results.push({
          nodeId: n.id,
          nodeName: n.name ?? n.qualified_name ?? '?',
          message: `Heavily depended on (${calls} callers, p90=${callThreshold}) with no spec`,
          severity: 'medium',
        });
      }
      // Pattern 3: Trait has N implementations but only M tested
      if (n.node_type === 'interface' || n.node_type === 'trait') {
        const entry = traitImplCounts.get(n.id);
        if (entry && entry.total > 1 && entry.tested < entry.total) {
          results.push({
            nodeId: n.id,
            nodeName: n.name ?? n.qualified_name ?? '?',
            message: `${entry.total} implementations but only ${entry.tested} tested`,
            severity: 'medium',
          });
        }
      }
      // Pattern 4: Type/interface created by agent but has no governing spec
      if ((n.node_type === 'type' || n.node_type === 'interface') && !n.spec_path && n.last_modified_by) {
        const hasGovEdge = governedByIndex.has(n.id);
        if (!hasGovEdge) {
          results.push({
            nodeId: n.id,
            nodeName: n.name ?? n.qualified_name ?? '?',
            message: `Agent-created ${n.node_type} has no governing spec`,
            severity: 'medium',
          });
        }
      }
      // Pattern 5: Orphan function — no callers (possible dead code)
      if (n.node_type === 'function' && !n.test_node && calls === 0) {
        results.push({
          nodeId: n.id,
          nodeName: n.name ?? n.qualified_name ?? '?',
          message: 'No callers \u2014 possible dead code',
          severity: 'low',
        });
      }
    }
    // Pattern 6: Co-change detection — modules that change together but have no shared spec.
    // Two heuristics: (a) same commit SHA, (b) modified within the same time window (1 hour).
    // This catches co-changes in separate commits within the same PR.
    const coChangeGroups = new Map(); // key → [node]
    for (const n of nodes) {
      if (n.deleted_at || n.node_type === 'tree-group') continue;
      // Group by SHA (exact same commit)
      if (n.last_modified_sha) {
        const key = `sha:${n.last_modified_sha}`;
        if (!coChangeGroups.has(key)) coChangeGroups.set(key, []);
        coChangeGroups.get(key).push(n);
      }
      // Group by time window (same hour, to catch separate commits in same PR)
      if (n.last_modified_at) {
        const hourBucket = Math.floor(n.last_modified_at / 3600);
        const key = `time:${hourBucket}`;
        if (!coChangeGroups.has(key)) coChangeGroups.set(key, []);
        coChangeGroups.get(key).push(n);
      }
    }
    // Deduplicate: track which node pairs we've already reported
    const reportedPairs = new Set();
    // Find groups of 3+ nodes from different modules that share a change pattern but no shared spec
    for (const [, group] of coChangeGroups) {
      if (group.length < 3) continue;
      const modules = new Set(group.map(n => n.file_path?.split('/').slice(0, -1).join('/') ?? ''));
      if (modules.size < 2) continue; // Same module, not interesting
      const specs = new Set(group.filter(n => n.spec_path).map(n => n.spec_path));
      if (specs.size > 0) continue; // Have shared spec governance
      // Dedup by first 2 node IDs
      const pairKey = group.slice(0, 2).map(n => n.id).sort().join(',');
      if (reportedPairs.has(pairKey)) continue;
      reportedPairs.add(pairKey);
      const names = group.slice(0, 3).map(n => n.name ?? '?').join(', ');
      results.push({
        nodeId: group[0].id,
        nodeName: names,
        message: `${group.length} nodes across ${modules.size} modules change together but have no shared spec`,
        severity: 'medium',
      });
    }
    // Sort by severity (high first), then take top 8
    const order = { high: 0, medium: 1, low: 2 };
    results.sort((a, b) => (order[a.severity] ?? 3) - (order[b.severity] ?? 3));
    return results.slice(0, 8);
  });
  let anomalyPanelOpen = $state(true);

  // Timeline scrubber state
  let timelineEnabled = $state(false);
  let timelineRange = $state([0, 100]); // percentage range [from, to]
  let timelineNodes = $derived.by(() => {
    if (!timelineEnabled || !nodes.length) return null;
    const allTimes = nodes.filter(n => n.first_seen_at).map(n => n.first_seen_at);
    if (allTimes.length === 0) return null;
    const minT = Math.min(...allTimes);
    const maxT = Math.max(...allTimes);
    if (maxT === minT) return null;
    const fromT = minT + (maxT - minT) * (timelineRange[0] / 100);
    const toT = minT + (maxT - minT) * (timelineRange[1] / 100);
    const visibleIds = new Set();
    const ghostIds = new Set(); // Nodes outside range shown as ghosts
    const ghostAdded = new Set(); // Nodes that will be added AFTER the selected range (future)
    const ghostRemoved = new Set(); // Nodes that existed BEFORE the range but are gone/not yet visible
    const totalWithTime = nodes.filter(n => n.first_seen_at && !n.deleted_at).length;
    for (const n of nodes) {
      if (n.deleted_at) continue;
      const t = n.first_seen_at || 0;
      if (t >= fromT && t <= toT) {
        visibleIds.add(n.id);
      } else if (n.first_seen_at) {
        ghostIds.add(n.id);
        if (t > toT) {
          ghostAdded.add(n.id); // Will be added in the future
        } else {
          ghostRemoved.add(n.id); // Existed before this range
        }
      }
    }
    // Nodes that were deleted during or after range
    for (const n of nodes) {
      if (n.deleted_at && n.first_seen_at && n.first_seen_at >= fromT && n.first_seen_at <= toT) {
        ghostRemoved.add(n.id);
      }
    }
    // Collect key moment markers (spec approvals, milestone-like events)
    const markers = [];
    for (const n of nodes) {
      if (n.spec_approved_at && n.spec_approved_at >= minT && n.spec_approved_at <= maxT) {
        markers.push({ time: n.spec_approved_at, label: `Spec: ${n.name ?? '?'}`, pct: ((n.spec_approved_at - minT) / (maxT - minT)) * 100 });
      }
      if (n.milestone_completed_at && n.milestone_completed_at >= minT && n.milestone_completed_at <= maxT) {
        markers.push({ time: n.milestone_completed_at, label: `Milestone: ${n.name ?? '?'}`, pct: ((n.milestone_completed_at - minT) / (maxT - minT)) * 100 });
      }
    }
    // Deduplicate markers that are very close together
    markers.sort((a, b) => a.time - b.time);
    const dedupedMarkers = [];
    for (const m of markers) {
      if (dedupedMarkers.length === 0 || Math.abs(m.pct - dedupedMarkers[dedupedMarkers.length - 1].pct) > 2) {
        dedupedMarkers.push(m);
      }
    }
    // Compute structural delta: what changed between the selected time range and now
    const delta = { added: 0, removed: 0, modified: 0, addedByType: new Map(), removedByType: new Map() };
    // Legacy byType kept for backward compat
    delta.byType = delta.addedByType;
    for (const n of nodes) {
      const t = n.first_seen_at || 0;
      const nt = n.node_type ?? 'unknown';
      // Nodes that appeared after the selected range end
      if (!n.deleted_at && t > toT) {
        delta.added++;
        delta.addedByType.set(nt, (delta.addedByType.get(nt) ?? 0) + 1);
      }
      // Nodes that were visible in the range but later deleted
      if (n.deleted_at && t >= fromT && t <= toT) {
        delta.removed++;
        delta.removedByType.set(nt, (delta.removedByType.get(nt) ?? 0) + 1);
      }
      // Nodes that existed before the range but were deleted during/after it
      if (n.deleted_at && t < fromT && n.deleted_at >= fromT) {
        delta.removed++;
        delta.removedByType.set(nt, (delta.removedByType.get(nt) ?? 0) + 1);
      }
      // Nodes modified within the range
      if (!n.deleted_at && t < fromT && n.last_modified_at && n.last_modified_at > fromT) {
        delta.modified++;
      }
    }
    const isFullRange = timelineRange[0] === 0 && timelineRange[1] === 100;
    return { visibleIds, ghostIds, ghostAdded, ghostRemoved, minT, maxT, fromT, toT, totalWithTime, markers: dedupedMarkers.slice(0, 10), delta, isFullRange };
  });

  // Canvas-scoped search state
  let searchOpen = $state(false);
  let searchQuery = $state('');
  let searchResults = $derived.by(() => {
    if (!searchQuery.trim()) return [];
    const q = searchQuery.toLowerCase();
    const results = [];
    for (const n of nodes) {
      if (results.length >= 20) break; // Early exit once we have enough matches
      if (
        n.name?.toLowerCase().includes(q) ||
        n.qualified_name?.toLowerCase().includes(q) ||
        n.node_type?.toLowerCase().includes(q) ||
        n.spec_path?.toLowerCase().includes(q)
      ) {
        results.push(n);
      }
    }
    return results;
  });
  let searchHighlightIds = $derived(new Set(searchResults.map(n => n.id)));
  let searchInputEl = $state(null);
  let searchSelectedIdx = $state(0); // Index into searchResults for keyboard navigation

  // Auto-zoom to fit search results when query changes
  $effect(() => {
    if (searchOpen && searchResults.length > 0 && searchResults.length <= 20) {
      // Compute bounding box of matched nodes and zoom to fit them
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      let found = 0;
      for (const r of searchResults) {
        const ln = layoutNodes.find(l => (l.node?.id ?? l.id) === r.id);
        if (ln) {
          minX = Math.min(minX, ln.x - ln.w / 2);
          maxX = Math.max(maxX, ln.x + ln.w / 2);
          minY = Math.min(minY, ln.y - ln.h / 2);
          maxY = Math.max(maxY, ln.y + ln.h / 2);
          found++;
        }
      }
      if (found > 0) {
        const cx = (minX + maxX) / 2;
        const cy = (minY + maxY) / 2;
        const bw = maxX - minX + 100;
        const bh = maxY - minY + 100;
        targetCam.x = cx;
        targetCam.y = cy;
        targetCam.zoom = Math.max(MIN_ZOOM, Math.min(W / bw, H / bh, 4) * 0.85);
        needsAnim = true;
        scheduleRedraw();
      }
    }
  });

  function zoomToNode(nodeId) {
    const ln = layoutNodes.find(l => (l.node?.id ?? l.id) === nodeId);
    if (ln) {
      targetCam.x = ln.x;
      targetCam.y = ln.y;
      targetCam.zoom = Math.min(W / (ln.w + 60), H / (ln.h + 60), 4) * 0.85;
      if (targetCam.zoom < 1) targetCam.zoom = 2;
      needsAnim = true;
      selectedNodeId = nodeId;
      scheduleRedraw();
    }
  }

  // Evaluative heat map config — separate from view queries so it overlays
  // on top of any active query rather than replacing it
  let evaluativeHeatConfig = $derived.by(() => {
    if (lens !== 'evaluative') return null;
    // When trace data exists and metric is span_duration, use trace-based heat
    return { metric: evaluativeMetric, palette: 'blue-red' };
  });

  // Pre-compute evaluative heat max values
  let evalHeatMaxValues = $derived.by(() => {
    if (!evaluativeHeatConfig) return new Map();
    const metric = evaluativeHeatConfig.metric;
    let max = 0;
    for (const node of nodes) {
      let v = getNodeMetricValue(metric, node);
      if (v > max) max = v;
    }
    const m = new Map();
    m.set(metric, max || 1);
    return m;
  });

  // Centralized metric value accessor for evaluative lens
  function getNodeMetricValue(metric, node) {
    if (!node) return 0;
    if (metric === 'incoming_calls') return incomingCallCounts.get(node.id) ?? 0;
    if (metric === 'complexity') return node.complexity ?? 0;
    if (metric === 'churn' || metric === 'churn_count_30d') return node.churn_count_30d ?? node.churn ?? 0;
    if (metric === 'test_coverage') return (node.test_coverage ?? 0) * 100;
    // Trace-based metrics (requires nodeSpanStats)
    if (metric === 'span_duration') {
      const stats = nodeSpanStats.get(node.id);
      return stats ? stats.meanDuration : 0;
    }
    if (metric === 'span_count') {
      const stats = nodeSpanStats.get(node.id);
      return stats ? stats.spanCount : 0;
    }
    if (metric === 'error_rate') {
      const stats = nodeSpanStats.get(node.id);
      return stats ? stats.errorRate * 100 : 0;
    }
    return 0;
  }

  function evaluativeNodeColor(nodeId, node) {
    if (!evaluativeHeatConfig || !node) return null;
    const metric = evaluativeHeatConfig.metric;
    const value = getNodeMetricValue(metric, node);
    if (value === 0) return null;
    const maxVal = evalHeatMaxValues.get(metric) ?? 1;
    const t = Math.min(1, value / maxVal);
    return heatColor(t, evaluativeHeatConfig.palette);
  }

  // Check if node has failed spans (for red glow effect)
  function nodeHasErrors(nodeId) {
    const stats = nodeSpanStats.get(nodeId);
    return stats && stats.errorRate > 0;
  }

  function onLensChange(newLens) {
    // Lens toggle simply changes the lens state — no query manipulation needed.
    // The evaluative overlay is computed independently from any active query.
    syncCanvasState();
    scheduleRedraw();
  }

  // Keep canvasState enriched with active filter, lens, and visible tree groups
  // so the LLM receives full context about what the user is looking at.
  // Deferred to avoid errors during initial mount (test environments).
  function syncCanvasState() {
    try {
      const visibleGroups = (rootLayoutNodes ?? [])
        .filter(ln => ln.kind === 'tree-group' && ln.treeNode?.label)
        .map(ln => ln.treeNode.label);
      canvasState = {
        ...canvasState,
        active_filter: filter,
        active_lens: lens,
        visible_tree_groups: visibleGroups,
        zoom_level: cam.zoom,
        recent_interactions: recentInteractions,
      };
    } catch { /* ignore during init */ }
  }

  // ── Evaluative lens: OTLP trace particle animation ──────────────────────
  let evalPlaying = $state(false);
  let evalSpeed = $state(1.0); // Clamped to [0.25, 5.0] per spec
  let evalScrubber = $state(0); // 0..1 normalized time position
  let evalParticles = $state([]); // [{edgeKey, progress, span, color}]

  // Pre-compute span-to-graph-node mapping and edge call frequencies
  let traceEdgeFrequency = $derived.by(() => {
    if (!traceData?.spans?.length) return new Map();
    const freq = new Map(); // "srcId->tgtId" → count
    // Map span parent→child to graph nodes via graph_node_id
    const spanById = new Map();
    for (const s of traceData.spans) spanById.set(s.span_id, s);
    for (const s of traceData.spans) {
      if (!s.parent_span_id || !s.graph_node_id) continue;
      const parent = spanById.get(s.parent_span_id);
      if (!parent?.graph_node_id) continue;
      const key = `${parent.graph_node_id}->${s.graph_node_id}`;
      freq.set(key, (freq.get(key) ?? 0) + 1);
    }
    return freq;
  });

  let traceMaxFreq = $derived(Math.max(1, ...traceEdgeFrequency.values()));

  // Per-node span statistics for evaluative tooltip (p50, p95, error rate)
  let nodeSpanStats = $derived.by(() => {
    if (!traceData?.spans?.length) return new Map();
    const statsMap = new Map(); // nodeId → { durations: [], errors: number, total: number }
    for (const s of traceData.spans) {
      const nid = s.graph_node_id;
      if (!nid) continue;
      if (!statsMap.has(nid)) statsMap.set(nid, { durations: [], errors: 0, total: 0 });
      const st = statsMap.get(nid);
      st.durations.push(s.duration_us);
      st.total++;
      if (s.status === 'error' || s.status === 'ERROR') st.errors++;
    }
    // Compute percentiles
    const result = new Map();
    for (const [nid, st] of statsMap) {
      const sorted = st.durations.slice().sort((a, b) => a - b);
      // Use linear interpolation for percentiles (correct for small samples)
      const percentile = (arr, p) => {
        if (arr.length === 0) return 0;
        if (arr.length === 1) return arr[0];
        const idx = p * (arr.length - 1);
        const lo = Math.floor(idx);
        const hi = Math.ceil(idx);
        const frac = idx - lo;
        return arr[lo] * (1 - frac) + arr[hi] * frac;
      };
      const p50 = percentile(sorted, 0.5);
      const p95 = percentile(sorted, 0.95);
      const mean = sorted.reduce((a, b) => a + b, 0) / sorted.length;
      result.set(nid, {
        spanCount: st.total,
        errorRate: st.total > 0 ? st.errors / st.total : 0,
        p50,
        p95,
        meanDuration: mean,
      });
    }
    return result;
  });

  // Build particles from trace data when evaluative lens is active
  let traceSpanTimeline = $derived.by(() => {
    if (!traceData?.spans?.length || lens !== 'evaluative') return null;
    const spans = traceData.spans;
    const minTime = Math.min(...spans.map(s => s.start_time));
    const maxTime = Math.max(...spans.map(s => s.start_time + s.duration_us));
    const totalDuration = maxTime - minTime || 1;
    return { minTime, maxTime, totalDuration, spans };
  });

  function tickParticles(dt) {
    if (!traceSpanTimeline || !evalPlaying) return;
    const { minTime, totalDuration, spans } = traceSpanTimeline;
    const spanById = new Map();
    for (const s of spans) spanById.set(s.span_id, s);

    // Advance scrubber: normalize playback to ~5 seconds for 1x speed
    // regardless of actual trace duration (microsecond or minute-long)
    const normalizedDuration = 5.0; // seconds for full playback at 1x
    const clampedSpeed = Math.max(0.25, Math.min(5.0, evalSpeed));
    const step = (dt / 1000) * clampedSpeed / normalizedDuration;
    evalScrubber = Math.min(1, evalScrubber + step);
    if (evalScrubber >= 1) {
      evalPlaying = false; // Stop at end (single-play mode)
      evalScrubber = 1;
    }

    const currentTime = minTime + evalScrubber * totalDuration;
    const newParticles = [];

    for (const span of spans) {
      if (!span.graph_node_id || !span.parent_span_id) continue;
      const parent = spanById.get(span.parent_span_id);
      if (!parent?.graph_node_id) continue;

      const spanStart = span.start_time;
      const spanEnd = span.start_time + span.duration_us;

      if (currentTime >= spanStart && currentTime <= spanEnd) {
        const progress = (currentTime - spanStart) / (span.duration_us || 1);
        const isError = span.status === 'error' || span.status === 'ERROR';
        newParticles.push({
          fromId: parent.graph_node_id,
          toId: span.graph_node_id,
          progress: Math.min(1, Math.max(0, progress)),
          span,
          color: isError ? '#ef4444' : '#60a5fa',
          glow: isError ? '#ef4444' : '#3b82f6',
        });
      }
    }
    evalParticles = newParticles;
  }

  // Computed elapsed time string for trace playback display
  let traceElapsedDisplay = $derived.by(() => {
    if (!traceSpanTimeline) return '';
    const elapsed = evalScrubber * traceSpanTimeline.totalDuration; // microseconds
    if (elapsed < 1000) return `${Math.round(elapsed)}\u00B5s`;
    if (elapsed < 1000000) return `${(elapsed / 1000).toFixed(1)}ms`;
    return `${(elapsed / 1000000).toFixed(2)}s`;
  });

  let traceTotalDisplay = $derived.by(() => {
    if (!traceSpanTimeline) return '';
    const total = traceSpanTimeline.totalDuration;
    if (total < 1000) return `${Math.round(total)}\u00B5s`;
    if (total < 1000000) return `${(total / 1000).toFixed(1)}ms`;
    return `${(total / 1000000).toFixed(2)}s`;
  });

  // Inertial zoom velocity
  let zoomVelocity = 0;
  let zoomDecayFrame = null;

  // ── Coordinate transforms ────────────────────────────────────────────
  function worldToScreen(wx, wy) {
    return { x: (wx - cam.x) * cam.zoom + W / 2, y: (wy - cam.y) * cam.zoom + H / 2 };
  }
  function screenToWorld(sx, sy) {
    return { x: (sx - W / 2) / cam.zoom + cam.x, y: (sy - H / 2) / cam.zoom + cam.y };
  }

  // ── Tree data structures ───────────────────────────────────────────
  let treeData = $derived.by(() => {
    const childToParent = new Map();
    const parentToChildren = new Map();
    const nodeById = new Map();
    for (const n of nodes) nodeById.set(n.id, n);
    for (const e of edges) {
      const etype = edgeType(e);
      if (etype !== 'contains') continue;
      const parentId = edgeSrc(e);
      const childId = edgeTgt(e);
      if (!parentId || !childId) continue;
      childToParent.set(childId, parentId);
      if (!parentToChildren.has(parentId)) parentToChildren.set(parentId, []);
      parentToChildren.get(parentId).push(childId);
    }
    return { childToParent, parentToChildren, nodeById };
  });

  // Descendant counts (recursive)
  let descendantCounts = $derived.by(() => {
    const counts = new Map();
    const { parentToChildren } = treeData;
    function count(id) {
      if (counts.has(id)) return counts.get(id);
      const children = parentToChildren.get(id) ?? [];
      let total = 1;
      for (const cid of children) total += count(cid);
      counts.set(id, total);
      return total;
    }
    for (const n of nodes) count(n.id);
    return counts;
  });

  // Non-contains edges for rendering
  let renderEdges = $derived.by(() => {
    // If an active view query specifies edge filters, use those exclusively
    // (this allows queries to show contains/field_of edges when requested).
    const queryEdgeFilter = activeQuery?.edges?.filter;
    if (queryEdgeFilter?.length) {
      return edges.filter(e => {
        const et = edgeType(e);
        return queryEdgeFilter.includes(et);
      });
    }
    // Default: hide structural hierarchy edges (contains, field_of) as they
    // are expressed via the treemap layout, not as rendered edge arrows.
    return edges.filter(e => {
      const et = edgeType(e);
      return et !== 'contains' && et !== 'field_of';
    });
  });

  // Parent map for root-ancestor lookup
  let parentMap = $derived.by(() => {
    const m = new Map();
    for (const e of edges) {
      const et = edgeType(e);
      if (et === 'contains') {
        m.set(edgeTgt(e), edgeSrc(e));
      }
    }
    return m;
  });

  // ── Path tree builder (from explore3.html prototype) ─────────────────
  // Builds a hierarchical tree from flat graph nodes by splitting
  // qualified_name on '.' / '::' / '/', collapsing single-child chains,
  // and promoting dominant children. This creates meaningful groups
  // regardless of whether the extractor emitted Contains edges.

  function buildPathTree(rootNodes, childToParent, parentToChildren, nodeById) {
    // PathTreeNode: synthetic grouping node
    const root = { name: 'root', fullPath: '', children: new Map(), graphNodes: [], totalDescendants: 0, treeDepth: 0 };

    for (const n of rootNodes) {
      // Use file_path for hierarchy (more reliable than qualified_name for Python)
      let pathStr = n.file_path ?? n.qualified_name ?? n.name ?? '';
      // Normalize: strip file extension, replace / with .
      pathStr = pathStr.replace(/\.(py|rs|go|ts|js|tsx|jsx|svelte|vue)$/, '').replace(/\//g, '.');
      // Remove trailing __init__ (Python package markers)
      pathStr = pathStr.replace(/\.__init__$/, '');
      if (!pathStr) pathStr = n.name ?? 'unknown';

      const parts = pathStr.split(/\.|::/);
      let current = root;

      for (let i = 0; i < parts.length; i++) {
        const segment = parts[i];
        if (!segment) continue;
        const path = parts.slice(0, i + 1).join('.');
        if (!current.children.has(segment)) {
          current.children.set(segment, {
            name: segment,
            fullPath: path,
            children: new Map(),
            graphNodes: [],
            totalDescendants: 0,
            treeDepth: i + 1,
          });
        }
        current = current.children.get(segment);
      }
      // Place this graph node at the leaf
      current.graphNodes.push(n);

      // Also collect child graph nodes (types, functions inside this module)
      const childIds = parentToChildren.get(n.id) ?? [];
      for (const cid of childIds) {
        const cn = nodeById.get(cid);
        if (cn) current.graphNodes.push(cn);
        // Grandchildren too
        for (const gcid of (parentToChildren.get(cid) ?? [])) {
          const gcn = nodeById.get(gcid);
          if (gcn) current.graphNodes.push(gcn);
        }
      }
    }

    // Compute totalDescendants bottom-up
    function computeDesc(node) {
      node.totalDescendants = node.graphNodes.length;
      for (const child of node.children.values()) {
        computeDesc(child);
        node.totalDescendants += child.totalDescendants;
      }
    }
    computeDesc(root);

    // Collapse single-child chains (e.g., src -> api -> iam becomes src.api.iam)
    function collapse(node) {
      for (const child of node.children.values()) collapse(child);
      while (node.children.size === 1 && node.graphNodes.length === 0 && node.fullPath !== '') {
        const [, child] = [...node.children.entries()][0];
        node.name = node.name + '.' + child.name;
        node.fullPath = child.fullPath;
        node.children = child.children;
        node.graphNodes = child.graphNodes;
      }
      // Promote dominant child: if one child has >90% of descendants, flatten
      if (node.children.size > 1 && node.graphNodes.length === 0 && node.fullPath !== '') {
        const kids = [...node.children.values()];
        const dominant = kids.reduce((a, b) => a.totalDescendants > b.totalDescendants ? a : b);
        if (dominant.totalDescendants > node.totalDescendants * 0.9) {
          node.children.delete(dominant.name);
          for (const [k, v] of dominant.children) node.children.set(k, v);
          collapse(node);
        }
      }
    }
    collapse(root);

    // Recompute depths after collapsing
    function reDepth(node, d) {
      node.treeDepth = d;
      for (const child of node.children.values()) reDepth(child, d + 1);
    }
    reDepth(root, 0);

    return root;
  }

  // ── Squarified treemap algorithm ───────────────────────────────────
  function squarify(items, x, y, w, h) {
    if (items.length === 0 || w <= 0 || h <= 0) return [];
    const total = items.reduce((s, i) => s + i.weight, 0);
    if (total <= 0) return [];
    if (items.length === 1) return [{ ...items[0], x: x + w / 2, y: y + h / 2, w, h }];
    const sorted = [...items].sort((a, b) => b.weight - a.weight);
    const results = [];
    doSquarifyLayout(sorted, x, y, w, h, total, results);
    return results;
  }

  function doSquarifyLayout(items, x, y, w, h, total, results) {
    if (items.length === 0) return;
    if (items.length === 1) {
      results.push({ ...items[0], x: x + w / 2, y: y + h / 2, w, h });
      return;
    }
    const horizontal = w >= h;
    const side = horizontal ? h : w;
    let row = [items[0]], rowW = items[0].weight;
    let bestAspect = worstAspect(row, rowW, side, total, w, h, horizontal);
    for (let i = 1; i < items.length; i++) {
      // Push to test candidate aspect ratio, then pop if worse (avoids O(k^2) spread copies)
      row.push(items[i]);
      const candW = rowW + items[i].weight;
      const candA = worstAspect(row, candW, side, total, w, h, horizontal);
      if (candA <= bestAspect) { rowW = candW; bestAspect = candA; }
      else { row.pop(); break; }
    }
    const rowFrac = rowW / total;
    const rowSize = horizontal ? w * rowFrac : h * rowFrac;
    let pos = 0;
    for (const item of row) {
      const frac = item.weight / rowW;
      const sz = side * frac;
      if (horizontal) {
        results.push({ ...item, x: x + rowSize / 2, y: y + pos + sz / 2, w: rowSize, h: sz });
      } else {
        results.push({ ...item, x: x + pos + sz / 2, y: y + rowSize / 2, w: sz, h: rowSize });
      }
      pos += sz;
    }
    const rem = items.slice(row.length), remW = total - rowW;
    if (rem.length > 0) {
      if (horizontal) doSquarifyLayout(rem, x + rowSize, y, w - rowSize, h, remW, results);
      else doSquarifyLayout(rem, x, y + rowSize, w, h - rowSize, remW, results);
    }
  }

  function worstAspect(row, rowW, side, total, w, h, horiz) {
    const rowSize = horiz ? w * (rowW / total) : h * (rowW / total);
    let worst = 0;
    for (const item of row) {
      const frac = item.weight / rowW;
      const sz = side * frac;
      const iw = horiz ? rowSize : sz, ih = horiz ? sz : rowSize;
      if (iw <= 0 || ih <= 0) continue;
      const a = Math.max(iw / ih, ih / iw);
      if (a > worst) worst = a;
    }
    return worst;
  }

  // ── Build layout from path tree ────────────────────────────────────
  let layoutNodes = $state([]);
  let layoutNodeMap = $state(new Map());
  // Track ghost node positions for edge rendering (ghost nodes aren't in layoutNodeMap)
  let ghostNodePositions = new Map(); // id -> { x, y, w, h }
  let prevNodeCount = 0;
  let prevBreadcrumbLen = -1;

  $effect(() => {
    const { childToParent, parentToChildren, nodeById } = treeData;
    const _bc = breadcrumb;
    const _f = filter;
    const _n = nodes.length;

    const isDataChange = _n !== prevNodeCount || breadcrumb.length !== prevBreadcrumbLen;
    prevNodeCount = _n;
    prevBreadcrumbLen = breadcrumb.length;

    if (nodes.length === 0) {
      layoutNodes = [];
      layoutNodeMap = new Map();
      return;
    }

    // Determine which nodes to show based on breadcrumb drill-down.
    // If breadcrumb is active, show only descendants of the last breadcrumb node.
    let visibleNodes;
    if (breadcrumb.length > 0) {
      const drillId = breadcrumb[breadcrumb.length - 1].id;
      const childIds = parentToChildren.get(drillId) ?? [];
      const descendantSet = new Set();
      const queue = [...childIds];
      while (queue.length > 0) {
        const cid = queue.pop();
        if (descendantSet.has(cid)) continue;
        descendantSet.add(cid);
        for (const gid of (parentToChildren.get(cid) ?? [])) queue.push(gid);
      }
      visibleNodes = nodes.filter(n => descendantSet.has(n.id));
      if (visibleNodes.length === 0) visibleNodes = nodes.filter(n => childIds.includes(n.id));
    } else {
      visibleNodes = nodes;
    }

    // Get root nodes (no Contains parent within the visible set)
    const visibleSet = new Set(visibleNodes.map(n => n.id));
    const rootNodes = visibleNodes.filter(n => {
      const parentId = childToParent.get(n.id);
      return !parentId || !visibleSet.has(parentId);
    });

    // Build path tree from root nodes
    const pathTreeRoot = buildPathTree(rootNodes, childToParent, parentToChildren, nodeById);

    // Get top-level children (after collapsing)
    const topKids = [...pathTreeRoot.children.values()].sort((a, b) => b.totalDescendants - a.totalDescendants);

    if (topKids.length === 0) {
      layoutNodes = [];
      layoutNodeMap = new Map();
      return;
    }

    // Compute world size
    const totalNodes = pathTreeRoot.totalDescendants;
    const aspect = W / H || 1.5;
    const areaPerNode = Math.max(1500, 3000 - Math.log10(totalNodes + 1) * 500);
    const area = totalNodes * areaPerNode;
    const layoutH = Math.sqrt(area / aspect);
    const layoutW = layoutH * aspect;

    const allLayoutNodes = [];
    const lnMap = new Map();

    // Recursively layout path tree nodes with squarified treemap
    function layoutPathTreeNode(ptChildren, x, y, w, h, parentLn, depth) {
      const kids = [...ptChildren].sort((a, b) => b.totalDescendants - a.totalDescendants);
      const items = kids.map(k => ({ ...k, weight: Math.max(1, k.totalDescendants) }));
      const rects = squarify(items, x, y, w, h);

      const gap = Math.max(2, Math.min(8, Math.min(w, h) * 0.01));

      for (let idx = 0; idx < rects.length; idx++) {
        const r = rects[idx];
        const gw = r.w - gap;
        const gh = r.h - gap;
        if (gw <= 2 || gh <= 2) continue;

        const hasChildren = r.children.size > 0 || r.graphNodes.length > 0;
        const treeNodeRef = r.children.size > 0 ? { children: r.children, graphNodes: r.graphNodes } : null;

        // Create a synthetic node for tree groups (they don't correspond to real graph nodes)
        const syntheticNode = {
          id: '__tree__' + r.fullPath,
          name: r.name,
          qualified_name: r.fullPath,
          node_type: depth === 0 ? 'package' : 'module',
          file_path: r.fullPath.replace(/\./g, '/'),
          spec_confidence: 'none',
        };

        const ln = {
          id: syntheticNode.id,
          kind: 'tree-group',
          x: r.x, y: r.y, w: gw, h: gh,
          label: r.name,
          node: syntheticNode,
          treeDepth: depth,
          parentTreeGroup: parentLn,
          totalChildren: r.totalDescendants,
          isLeafGraphNode: false,
          treeNode: treeNodeRef,
          childIndex: idx,
        };
        allLayoutNodes.push(ln);
        lnMap.set(ln.id, ln);

        // Layout children inside this cell
        const pad = Math.max(3, Math.min(gw, gh) * 0.02);
        const headerH = Math.max(10, Math.min(gw, gh) * 0.035);
        const cx = r.x - gw / 2 + pad;
        const cy = r.y - gh / 2 + pad + headerH;
        const cw = gw - pad * 2;
        const ch = gh - pad * 2 - headerH;

        if (cw > 5 && ch > 5) {
          if (r.children.size > 0 && r.graphNodes.length > 0) {
            // Both sub-directories AND direct graph nodes at this level.
            // Layout them together: sub-dirs as tree-groups, graph nodes as leaves,
            // all sharing the same squarified space.
            layoutMixed([...r.children.values()], r.graphNodes, cx, cy, cw, ch, ln, depth + 1);
          } else if (r.children.size > 0) {
            layoutPathTreeNode([...r.children.values()], cx, cy, cw, ch, ln, depth + 1);
          } else if (r.graphNodes.length > 0) {
            layoutLeafNodes(r.graphNodes, cx, cy, cw, ch, ln, depth + 1);
          }
        }
      }
    }

    // Layout mix of path-tree children (directories) and direct graph nodes together
    function layoutMixed(ptChildren, graphNodes, x, y, w, h, parentLn, depth) {
      // Build unified items array: tree-groups get their totalDescendants as weight,
      // graph nodes get their descendant count as weight
      const items = [];
      for (const pt of ptChildren) {
        items.push({
          kind: 'tree',
          ptNode: pt,
          weight: Math.max(1, pt.totalDescendants),
        });
      }
      for (const gn of graphNodes) {
        items.push({
          kind: 'graph',
          graphNode: gn,
          weight: nodeWeight(gn),
        });
      }

      const rects = squarify(items, x, y, w, h);
      const gap = Math.max(2, Math.min(8, Math.min(w, h) * 0.01));

      for (let idx = 0; idx < rects.length; idx++) {
        const r = rects[idx];
        const gw = r.w - gap;
        const gh = r.h - gap;
        if (gw <= 2 || gh <= 2) continue;

        if (r.kind === 'tree') {
          // Recurse into this as a path-tree group
          layoutPathTreeNode([r.ptNode], r.x - gw/2, r.y - gh/2, gw, gh, parentLn, depth);
        } else {
          // Render as a leaf graph node
          const gn = r.graphNode;
          const { parentToChildren: ptc } = treeData;
          const hasChildren = (ptc.get(gn.id) ?? []).length > 0;

          const ln = {
            id: gn.id,
            kind: hasChildren ? 'tree-group' : 'leaf',
            x: r.x, y: r.y, w: gw, h: gh,
            label: gn.name ?? '',
            node: gn,
            treeDepth: depth,
            parentTreeGroup: parentLn,
            totalChildren: (descendantCounts.get(gn.id) ?? 1) - 1,
            isLeafGraphNode: !hasChildren,
            treeNode: hasChildren ? { children: new Map(), graphNodes: [] } : null,
            childIndex: idx,
          };
          allLayoutNodes.push(ln);
          lnMap.set(ln.id, ln);

          if (hasChildren) {
            const childIds = (ptc.get(gn.id) ?? []);
            const childNodes = childIds.map(cid => nodeById.get(cid)).filter(Boolean);
            if (childNodes.length > 0) {
              const cpad = Math.max(2, Math.min(gw, gh) * 0.02);
              const cheader = Math.max(8, Math.min(gw, gh) * 0.03);
              const ccx = r.x - gw / 2 + cpad;
              const ccy = r.y - gh / 2 + cpad + cheader;
              const ccw = gw - cpad * 2;
              const cch = gh - cpad * 2 - cheader;
              if (ccw > 3 && cch > 3) {
                layoutLeafNodes(childNodes, ccx, ccy, ccw, cch, ln, depth + 1);
              }
            }
          }
        }
      }
    }

    // Layout actual graph nodes (functions, types, etc.) as leaf cells
    // Compute effective weight for a node: combines complexity and churn
    // to fulfill the spec's "size indicates complexity or churn" requirement.
    function nodeWeight(n) {
      const complexity = n.complexity ?? 0;
      const churn = n.churn_count_30d ?? n.churn ?? 0;
      const descendants = descendantCounts.get(n.id) ?? 1;
      const lineCount = n.line_end && n.line_start ? (n.line_end - n.line_start + 1) : 0;
      // Spec: "Size indicates complexity or churn."
      // Weighted sum: complexity is primary, churn is secondary signal,
      // line count is tertiary (proxy for complexity when metric is missing).
      // Add 1 baseline so even trivial nodes get visible cells.
      const signal = complexity * 2 + churn + (lineCount > 0 ? Math.log2(lineCount + 1) : 0);
      return Math.max(1, signal > 0 ? signal + 1 : descendants);
    }

    function layoutLeafNodes(graphNodes, x, y, w, h, parentLn, depth) {
      const items = graphNodes.map(n => ({
        id: n.id,
        node: n,
        weight: nodeWeight(n),
      }));
      const rects = squarify(items, x, y, w, h);
      const gap = Math.max(1, Math.min(4, Math.min(w, h) * 0.008));

      for (let idx = 0; idx < rects.length; idx++) {
        const r = rects[idx];
        const gw = r.w - gap;
        const gh = r.h - gap;
        if (gw <= 1 || gh <= 1) continue;

        const { parentToChildren: ptc } = treeData;
        const hasChildren = (ptc.get(r.id) ?? []).length > 0;

        const ln = {
          id: r.id,
          kind: hasChildren ? 'tree-group' : 'leaf',
          x: r.x, y: r.y, w: gw, h: gh,
          label: r.node.name ?? '',
          node: r.node,
          treeDepth: depth,
          parentTreeGroup: parentLn,
          totalChildren: (descendantCounts.get(r.id) ?? 1) - 1,
          isLeafGraphNode: !hasChildren,
          treeNode: hasChildren ? { children: new Map(), graphNodes: [] } : null,
          childIndex: idx,
        };
        allLayoutNodes.push(ln);
        lnMap.set(ln.id, ln);

        // Layout children of graph nodes (e.g., functions inside a type)
        if (hasChildren) {
          const childIds = (ptc.get(r.id) ?? []);
          const childNodes = childIds.map(cid => nodeById.get(cid)).filter(Boolean);
          if (childNodes.length > 0) {
            const cpad = Math.max(2, Math.min(gw, gh) * 0.02);
            const cheader = Math.max(8, Math.min(gw, gh) * 0.03);
            const ccx = r.x - gw / 2 + cpad;
            const ccy = r.y - gh / 2 + cpad + cheader;
            const ccw = gw - cpad * 2;
            const cch = gh - cpad * 2 - cheader;
            if (ccw > 3 && cch > 3) {
              layoutLeafNodes(childNodes, ccx, ccy, ccw, cch, ln, depth + 1);
            }
          }
        }
      }
    }

    const startX = -layoutW / 2;
    const startY = -layoutH / 2;
    layoutPathTreeNode(topKids, startX, startY, layoutW, layoutH, null, 0);

    layoutNodes = allLayoutNodes;
    layoutNodeMap = lnMap;

    if (isDataChange) {
      const fitZoom = Math.min(W / layoutW, H / layoutH) * 0.85;
      targetCam = { x: 0, y: 0, zoom: fitZoom };
      cam = { ...targetCam };
    }
    needsAnim = true;
  });

  // ── Zoom-dependent visibility ──────────────────────────────────────
  //
  // Key UX principle: let humans sit with high-level structure before
  // revealing details. Children should only appear when you've zoomed
  // in far enough that the parent container fills most of the screen.
  //
  // Summary mode: opaque box, centered label, descendant count
  //   → stays until the box is ~450px on screen
  // Container mode: transparent bg, children visible inside
  //   → children fade in when parent is 500-700px on screen
  //
  // This means at the initial overview, you see clean labeled boxes.
  // You have to deliberately zoom into a specific area to see inside it.

  function nodeOpacity(ln) {
    if (ln.kind === 'tree-group') return treeGroupOpacity(ln);

    // Leaf graph nodes: only visible when parent tree-group is very large on screen
    const sw = ln.w * cam.zoom;
    const sh = ln.h * cam.zoom;
    if (sw < 4 || sh < 3) return 0;

    if (ln.parentTreeGroup) {
      const ps = Math.min(ln.parentTreeGroup.w * cam.zoom, ln.parentTreeGroup.h * cam.zoom);
      // Parent must be 450px+ before leaf children appear (aligned with tree-group threshold)
      if (ps < 450) return 0;
      if (ps < 700) {
        const pf = (ps - 450) / 250;
        const ms = Math.min(sw, sh);
        const sf = ms < 8 ? Math.max(0, (ms - 4) / 4) : 1.0;
        return pf * sf;
      }
    }
    const ms = Math.min(sw, sh);
    if (ms < 8) return Math.max(0, (ms - 4) / 4);
    return 1.0;
  }

  function treeGroupOpacity(ln) {
    const sw = ln.w * cam.zoom;
    const sh = ln.h * cam.zoom;
    const ss = Math.min(sw, sh);
    if (ss < 10) return 0;

    if (ln.parentTreeGroup) {
      const ps = Math.min(ln.parentTreeGroup.w * cam.zoom, ln.parentTreeGroup.h * cam.zoom);
      // Children tree-groups only appear when parent is large enough to be in container mode
      if (ps < 400) return 0;
      if (ps < 600) return (ps - 400) / 200;
    }

    // Fade in small nodes
    if (ss < 20) return (ss - 10) / 10;

    // Fade out when this group fills the entire screen (becomes just background)
    if (ss > 2500) {
      if (ss > 5000) return 0;
      return 1.0 - (ss - 2500) / 2500;
    }
    return 1.0;
  }

  function isSummaryMode(ln) {
    if (ln.kind !== 'tree-group') return false;
    // Stay in summary mode until the box is quite large — this keeps
    // the view clean and lets humans read labels before diving deeper
    return Math.min(ln.w * cam.zoom, ln.h * cam.zoom) < 450;
  }

  // shouldShowChildren removed — dead code. Use isSummaryMode() (threshold 450px) instead.

  // ── Filter visibility ─────────────────────────────────────────────
  // Pre-compute call edge index
  let nodesWithCallsEdges = $derived.by(() => {
    const s = new Set();
    for (const e of edges) {
      if ((e.edge_type ?? e.type ?? '').toLowerCase() === 'calls') {
        s.add(edgeSrc(e));
        s.add(edgeTgt(e));
      }
    }
    return s;
  });

  // Map filter-panel categories to node_type sets
  const CATEGORY_NODE_TYPES = {
    boundaries: new Set(['module', 'crate', 'package', 'namespace']),
    interfaces: new Set(['endpoint', 'function', 'method', 'trait', 'interface']),
    data: new Set(['type', 'struct', 'enum', 'field', 'table', 'model']),
    specs: new Set(['spec', 'spec_file']),
  };

  function matchesActiveFilters(node) {
    if (!filters) return true;
    // Spec focus: highlight nodes governed by a specific spec (Vision Principle 3: specs as primary artifact)
    if (filters.focus_spec) {
      const specPath = filters.focus_spec;
      // Match nodes with this spec_path
      if (node.spec_path === specPath) return true;
      // Match nodes governed by this spec via GovernedBy edges
      const governed = edges.some(e => {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        const et = (e.edge_type ?? e.type ?? '').toLowerCase();
        if (et !== 'governed_by' || src !== node.id) return false;
        // Check if the target spec node matches
        const specNode = nodes.find(n => n.id === tgt);
        return specNode && (specNode.file_path === specPath || specNode.name === specPath || specNode.spec_path === specPath);
      });
      return governed;
    }
    // Focus on boundary/interface/data node
    if (filters.focus_node) {
      return node.id === filters.focus_node;
    }
    // Category check
    if (filters.categories && filters.categories.length > 0) {
      const nt = (node.node_type ?? '').toLowerCase();
      let matched = false;
      for (const cat of filters.categories) {
        if (CATEGORY_NODE_TYPES[cat]?.has(nt)) { matched = true; break; }
      }
      // If the node type doesn't fit any known category, show it if all categories are active
      if (!matched && filters.categories.length < 4) return false;
    }
    // Visibility check
    if (filters.visibility === 'public' && node.visibility === 'private') return false;
    if (filters.visibility === 'private' && node.visibility === 'public') return false;
    // Churn check
    if (filters.min_churn && (node.churn ?? 0) < filters.min_churn) return false;
    return true;
  }

  function filterOpacity(ln) {
    if (ln.kind === 'tree-group') return 1.0;
    if (!ln.node) return 0.1;
    // Apply active filters from filter panel
    if (filters && !matchesActiveFilters(ln.node)) return 0.1;
    if (filter === 'all') return 1.0;
    switch (filter) {
      case 'endpoints': return ln.node.node_type === 'endpoint' ? 1.0 : 0.1;
      case 'types': return (ln.node.node_type === 'type' || ln.node.node_type === 'interface' || ln.node.node_type === 'field') ? 1.0 : 0.1;
      case 'calls': return nodesWithCallsEdges.has(ln.node.id) ? 1.0 : 0.1;
      case 'dependencies': return 0.1;
      default: return 1.0;
    }
  }

  function filterEdge(edge) {
    if (filter === 'all') return true;
    const et = (edge.edge_type ?? edge.type ?? '').toLowerCase();
    switch (filter) {
      case 'endpoints': return et === 'calls' || et === 'routes_to';
      case 'types': return et === 'field_of' || et === 'depends_on';
      case 'calls': return et === 'calls';
      case 'dependencies': return et === 'depends_on' || et === 'calls';
      default: return true;
    }
  }

  // ── View query support ─────────────────────────────────────────────
  let adjacency = $derived.by(() => {
    const adj = new Map();
    for (const e of edges) {
      const src = edgeSrc(e);
      const tgt = edgeTgt(e);
      const et = edgeType(e);
      if (src && tgt) {
        if (!adj.has(src)) adj.set(src, []);
        adj.get(src).push({ targetId: tgt, edgeType: et });
        if (!adj.has(tgt)) adj.set(tgt, []);
        adj.get(tgt).push({ targetId: src, edgeType: et, reverse: true });
      }
    }
    return adj;
  });

  // ── Computed reference helpers (view-query-grammar.md) ───────────────
  // These resolve computed expressions like $callers(), $callees(), etc.
  // against the client-side adjacency graph for the filter scope.

  function resolveNodeByName(name) {
    return nodes.find(n => n.name === name || n.qualified_name === name || n.id === name);
  }

  function computeCallers(nodeName, depth = 10) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set([start.id]);
    const q = [{ id: start.id, d: 0 }];
    while (q.length > 0) {
      const { id, d } = q.shift();
      if (d >= depth) continue;
      for (const nb of (adjacency.get(id) ?? [])) {
        if (nb.edgeType === 'calls' && nb.reverse && !result.has(nb.targetId)) {
          result.add(nb.targetId);
          q.push({ id: nb.targetId, d: d + 1 });
        }
      }
    }
    return result;
  }

  function computeCallees(nodeName, depth = 10) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set([start.id]);
    const q = [{ id: start.id, d: 0 }];
    while (q.length > 0) {
      const { id, d } = q.shift();
      if (d >= depth) continue;
      for (const nb of (adjacency.get(id) ?? [])) {
        if (nb.edgeType === 'calls' && !nb.reverse && !result.has(nb.targetId)) {
          result.add(nb.targetId);
          q.push({ id: nb.targetId, d: d + 1 });
        }
      }
    }
    return result;
  }

  function computeImplementors(nodeName) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set();
    for (const nb of (adjacency.get(start.id) ?? [])) {
      if (nb.edgeType === 'implements' && nb.reverse) result.add(nb.targetId);
    }
    return result;
  }

  function computeGovernedBy(specPath) {
    const result = new Set();
    for (const n of nodes) {
      if (n.deleted_at) continue;
      if (n.spec_path === specPath) result.add(n.id);
    }
    for (const e of edges) {
      if (e.deleted_at) continue;
      const et = edgeType(e);
      if (et === 'governed_by') {
        const tgt = edgeTgt(e);
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode && (tgtNode.name === specPath || tgtNode.spec_path === specPath)) {
          result.add(edgeSrc(e));
        }
      }
    }
    return result;
  }

  // Edge types traversed for test reachability — matches backend TEST_REACHABILITY_EDGES.
  // Contains is intentionally excluded to avoid inflating coverage.
  const FRONTEND_TEST_EDGES = new Set(['calls', 'implements', 'routes_to']);
  const TESTABLE_TYPES = new Set(['function', 'method', 'endpoint', 'type', 'trait', 'class']);

  function computeTestUnreachable() {
    const testN = nodes.filter(n => n.test_node);
    const reachable = new Set(testN.map(n => n.id));
    const q = [...reachable];
    while (q.length > 0) {
      const id = q.shift();
      for (const nb of (adjacency.get(id) ?? [])) {
        if (reachable.has(nb.targetId) || !FRONTEND_TEST_EDGES.has(nb.edgeType) || nb.reverse) continue;
        reachable.add(nb.targetId);
        q.push(nb.targetId);
      }
    }
    const result = new Set();
    for (const n of nodes) {
      if (!n.test_node && TESTABLE_TYPES.has(n.node_type) && !reachable.has(n.id)) result.add(n.id);
    }
    return result;
  }

  function computeTestReachable() {
    const testN = nodes.filter(n => n.test_node);
    const reachable = new Set(testN.map(n => n.id));
    const q = [...reachable];
    while (q.length > 0) {
      const id = q.shift();
      for (const nb of (adjacency.get(id) ?? [])) {
        if (reachable.has(nb.targetId) || !FRONTEND_TEST_EDGES.has(nb.edgeType) || nb.reverse) continue;
        reachable.add(nb.targetId);
        q.push(nb.targetId);
      }
    }
    return reachable;
  }

  function computeWhere(property, operator, value) {
    const result = new Set();
    const numVal = Number(value);
    const isNumericOp = !isNaN(numVal);
    for (const n of nodes) {
      const raw = n[property];
      if (raw == null) continue; // Skip nodes without this property
      let match = false;
      if (isNumericOp) {
        const nodeVal = Number(raw);
        if (isNaN(nodeVal)) continue; // Non-numeric node value
        switch (operator) {
          case '>': match = nodeVal > numVal; break;
          case '<': match = nodeVal < numVal; break;
          case '>=': match = nodeVal >= numVal; break;
          case '<=': match = nodeVal <= numVal; break;
          case '=': case '==': match = Math.abs(nodeVal - numVal) < 1e-9; break;
          case '!=': match = Math.abs(nodeVal - numVal) >= 1e-9; break;
        }
      } else {
        // String comparison for non-numeric values
        const strVal = String(raw);
        switch (operator) {
          case '=': case '==': match = strVal === value; break;
          case '!=': match = strVal !== value; break;
          default: match = false; // Relational ops require numeric values
        }
      }
      if (match) result.add(n.id);
    }
    return result;
  }

  function computedSetOp(op, setA, setB) {
    switch (op) {
      case 'intersect': {
        const result = new Set();
        for (const id of setA) { if (setB.has(id)) result.add(id); }
        return result;
      }
      case 'union': return new Set([...setA, ...setB]);
      case 'diff': {
        const result = new Set();
        for (const id of setA) { if (!setB.has(id)) result.add(id); }
        return result;
      }
      default: return setA;
    }
  }

  function computeFields(nodeName) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set();
    for (const nb of (adjacency.get(start.id) ?? [])) {
      if (nb.edgeType === 'field_of' && nb.reverse) result.add(nb.targetId);
    }
    return result;
  }

  function computeDescendantsSet(nodeName) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set([start.id]);
    const q = [start.id];
    while (q.length > 0) {
      const id = q.shift();
      for (const nb of (adjacency.get(id) ?? [])) {
        if (nb.edgeType === 'contains' && !nb.reverse && !result.has(nb.targetId)) {
          result.add(nb.targetId);
          q.push(nb.targetId);
        }
      }
    }
    return result;
  }

  function computeAncestorsSet(nodeName) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const result = new Set([start.id]);
    const q = [start.id];
    while (q.length > 0) {
      const id = q.shift();
      for (const nb of (adjacency.get(id) ?? [])) {
        if (nb.edgeType === 'contains' && nb.reverse && !result.has(nb.targetId)) {
          result.add(nb.targetId);
          q.push(nb.targetId);
        }
      }
    }
    return result;
  }

  // Cache for all-nodes test fragility (computed once per graph change, O(T*(N+M))
  // using reverse BFS from each test node). For a single node query ($test_fragility(X)),
  // returns a set containing just that node if it has any test coverage.
  let _testFragilityCache = null;
  let _testFragilityCacheKey = '';

  function computeAllTestFragility() {
    // Cache key: node count + edge count (invalidates when graph changes)
    const key = `${nodes.length}:${edges.length}`;
    if (_testFragilityCache && _testFragilityCacheKey === key) return _testFragilityCache;

    const fragility = new Map(); // node_id -> count of distinct tests reaching it
    const testNodes = nodes.filter(n => n.test_node);

    // For each test node, do a BFS and increment fragility for each reached node.
    // This is O(T * (N+M)) total but runs once and is cached.
    for (const tn of testNodes) {
      const reached = new Set([tn.id]);
      const q = [tn.id];
      while (q.length > 0) {
        const id = q.shift();
        for (const nb of (adjacency.get(id) ?? [])) {
          if (FRONTEND_TEST_EDGES.has(nb.edgeType) && !nb.reverse && !reached.has(nb.targetId)) {
            reached.add(nb.targetId);
            q.push(nb.targetId);
          }
        }
      }
      for (const id of reached) {
        fragility.set(id, (fragility.get(id) || 0) + 1);
      }
    }
    _testFragilityCache = fragility;
    _testFragilityCacheKey = key;
    return fragility;
  }

  function computeTestFragility(nodeName) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const fragility = computeAllTestFragility();
    const count = fragility.get(start.id) || 0;
    if (count > 0) return new Set([start.id]);
    return new Set();
  }

  function computeReachable(nodeName, edgeTypes, direction, depth) {
    const start = resolveNodeByName(nodeName);
    if (!start) return new Set();
    const allowedEdges = new Set(edgeTypes.map(e => e.toLowerCase()));
    const result = new Set([start.id]);
    const q = [{ id: start.id, d: 0 }];
    while (q.length > 0) {
      const { id, d } = q.shift();
      if (d >= depth) continue;
      for (const nb of (adjacency.get(id) ?? [])) {
        if (!allowedEdges.has(nb.edgeType) || result.has(nb.targetId)) continue;
        if (direction === 'outgoing' && nb.reverse) continue;
        if (direction === 'incoming' && !nb.reverse) continue;
        result.add(nb.targetId);
        q.push({ id: nb.targetId, d: d + 1 });
      }
    }
    return result;
  }

  // Split arguments respecting balanced parentheses and brackets
  function splitBalancedArgs(s) {
    const parts = [];
    let depth = 0;
    let bracketDepth = 0;
    let last = 0;
    for (let i = 0; i < s.length; i++) {
      const c = s[i];
      if (c === '(') depth++;
      else if (c === ')') depth--;
      else if (c === '[') bracketDepth++;
      else if (c === ']') bracketDepth--;
      else if (c === ',' && depth === 0 && bracketDepth === 0) {
        parts.push(s.substring(last, i));
        last = i + 1;
      }
    }
    parts.push(s.substring(last));
    return parts;
  }

  // Resolve a computed expression string like "$callers(FooService, 5)" or "$test_unreachable"
  function resolveComputed(expr) {
    if (!expr || typeof expr !== 'string') return null;
    const e = expr.trim();

    // Simple references
    if (e === '$test_unreachable') return computeTestUnreachable();
    if (e === '$test_reachable') return computeTestReachable();
    if (e === '$clicked' || e === '$selected') {
      const selId = canvasState?.selectedNode?.id;
      if (selId) return new Set([selId]);
      return new Set();
    }

    // Parse function-style: $fn(...) with proper balanced parenthesis matching
    const dollarIdx = e.indexOf('$');
    const parenIdx = e.indexOf('(', dollarIdx);
    if (dollarIdx === 0 && parenIdx > 0 && e.endsWith(')')) {
      const fn = e.substring(1, parenIdx);
      const inner = e.substring(parenIdx + 1, e.length - 1);
      const args = splitBalancedArgs(inner);

      switch (fn) {
        case 'callers': return computeCallers(args[0]?.trim().replace(/^['"]|['"]$/g, ''), args[1] ? parseInt(args[1].trim()) : 10);
        case 'callees': return computeCallees(args[0]?.trim().replace(/^['"]|['"]$/g, ''), args[1] ? parseInt(args[1].trim()) : 10);
        case 'implementors': return computeImplementors(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'governed_by': return computeGovernedBy(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'where': return computeWhere(
          args[0]?.trim().replace(/^['"]|['"]$/g, ''),
          args[1]?.trim().replace(/^['"]|['"]$/g, ''),
          args[2]?.trim().replace(/^['"]|['"]$/g, '')
        );
        case 'fields': return computeFields(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'descendants': return computeDescendantsSet(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'ancestors': return computeAncestorsSet(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'test_fragility': return computeTestFragility(args[0]?.trim().replace(/^['"]|['"]$/g, ''));
        case 'reachable': {
          const nodeName = args[0]?.trim().replace(/^['"]|['"]$/g, '');
          const edgeTypesStr = args[1]?.trim() ?? 'calls';
          const direction = args[2]?.trim().replace(/^['"]|['"]$/g, '') ?? 'outgoing';
          const depth = args[3] ? parseInt(args[3].trim()) : 10;
          const edgeTypes = edgeTypesStr.replace(/[\[\]]/g, '').split(',').map(s => s.trim().replace(/^['"]|['"]$/g, ''));
          return computeReachable(nodeName, edgeTypes, direction, depth);
        }
        case 'intersect': {
          const a = resolveComputed(args[0]?.trim());
          const b = resolveComputed(args[1]?.trim());
          if (a && b) return computedSetOp('intersect', a, b);
          return a ?? b ?? new Set();
        }
        case 'union': {
          const a = resolveComputed(args[0]?.trim());
          const b = resolveComputed(args[1]?.trim());
          if (a && b) return computedSetOp('union', a, b);
          return a ?? b ?? new Set();
        }
        case 'diff': {
          const a = resolveComputed(args[0]?.trim());
          const b = resolveComputed(args[1]?.trim());
          if (a && b) return computedSetOp('diff', a, b);
          return a ?? new Set();
        }
      }
    }
    return null;
  }

  let queryMatchedWithDepth = $derived.by(() => {
    if (!activeQuery?.scope) return null;
    const scope = activeQuery.scope;

    if (scope.type === 'focus' && scope.node) {
      // $selected: resolves to current selection (one-time)
      // $clicked: resolves via interactiveQueryTemplate (re-runs on each click)
      const startName = (scope.node === '$selected' || scope.node === '$clicked')
        ? (canvasState?.selectedNode?.qualified_name ?? canvasState?.selectedNode?.name ?? '') : scope.node;
      if (!startName) return null;
      const startNode = nodes.find(n => n.name === startName || n.qualified_name === startName || n.id === startName);
      if (!startNode) return null;
      const allowed = new Set((scope.edges ?? ['calls']).map(e => e.toLowerCase()));
      const dir = scope.direction ?? 'both';
      const maxD = scope.depth ?? 5;
      const dm = new Map([[startNode.id, 0]]);
      const q = [{ id: startNode.id, depth: 0 }];
      while (q.length > 0) {
        const { id, depth } = q.shift();
        if (depth >= maxD) continue;
        for (const nb of (adjacency.get(id) ?? [])) {
          if (dm.has(nb.targetId)) continue;
          if (!allowed.has(nb.edgeType)) continue;
          if (dir === 'outgoing' && nb.reverse) continue;
          if (dir === 'incoming' && !nb.reverse) continue;
          dm.set(nb.targetId, depth + 1);
          q.push({ id: nb.targetId, depth: depth + 1 });
        }
      }
      return dm;
    }

    if (scope.type === 'test_gaps') {
      // Match backend TEST_REACHABILITY_EDGES: calls, implements, routes_to
      // NOTE: Contains is intentionally excluded — including it would make all
      // sibling functions in a module "reachable" just because one test exists
      // in the same module, inflating test coverage metrics.
      const TEST_REACHABILITY_EDGES = new Set(['calls', 'implements', 'routes_to']);
      const testN = nodes.filter(n => n.test_node);
      const reachable = new Set(testN.map(n => n.id));
      const q = [...reachable];
      while (q.length > 0) {
        const id = q.shift();
        for (const nb of (adjacency.get(id) ?? [])) {
          if (reachable.has(nb.targetId) || !TEST_REACHABILITY_EDGES.has(nb.edgeType) || nb.reverse) continue;
          reachable.add(nb.targetId);
          q.push(nb.targetId);
        }
      }
      const matched = new Map();
      for (const n of nodes) {
        const testableTypes = new Set(['function', 'method', 'endpoint', 'type', 'trait', 'class']);
        if (!n.test_node && testableTypes.has(n.node_type) && !reachable.has(n.id)) matched.set(n.id, 0);
      }
      return matched.size > 0 ? matched : null;
    }

    if (scope.type === 'filter') {
      // If a computed expression is specified, resolve it first
      if (scope.computed) {
        const computedSet = resolveComputed(scope.computed);
        if (computedSet && computedSet.size > 0) {
          const matched = new Map();
          for (const id of computedSet) matched.set(id, 0);
          return matched;
        }
        return null;
      }
      const matched = new Map();
      for (const n of nodes) {
        let m = true;
        if (scope.node_types?.length && !scope.node_types.includes(n.node_type)) m = false;
        if (scope.name_pattern) {
          const re = new RegExp(scope.name_pattern, 'i');
          if (!re.test(n.name ?? '') && !re.test(n.qualified_name ?? '')) m = false;
        }
        if (m) matched.set(n.id, 0);
      }
      return matched.size > 0 ? matched : null;
    }

    // Concept scope: start from seed nodes, expand along edges
    if (scope.type === 'concept') {
      const seeds = scope.seed_nodes ?? [];
      const expandEdges = new Set((scope.expand_edges ?? ['calls']).map(e => e.toLowerCase()));
      const maxDepth = scope.expand_depth ?? 2;
      const dir = scope.expand_direction ?? 'both';
      const dm = new Map();

      for (const seedName of seeds) {
        const seedNode = nodes.find(n =>
          n.name === seedName || n.qualified_name === seedName || n.id === seedName
        );
        if (!seedNode || dm.has(seedNode.id)) continue;
        dm.set(seedNode.id, 0);
        const q = [{ id: seedNode.id, depth: 0 }];
        while (q.length > 0) {
          const { id, depth } = q.shift();
          if (depth >= maxDepth) continue;
          for (const nb of (adjacency.get(id) ?? [])) {
            if (dm.has(nb.targetId)) continue;
            if (!expandEdges.has(nb.edgeType)) continue;
            if (dir === 'outgoing' && nb.reverse) continue;
            if (dir === 'incoming' && !nb.reverse) continue;
            dm.set(nb.targetId, depth + 1);
            q.push({ id: nb.targetId, depth: depth + 1 });
          }
        }
      }
      return dm.size > 0 ? dm : null;
    }

    // Diff scope: highlight nodes changed between two commits
    if (scope.type === 'diff') {
      // Diff requires commit_sha on nodes; filter to nodes changed since scope.from_commit
      const fromCommit = scope.from_commit;
      if (!fromCommit) return null;
      const matched = new Map();
      for (const n of nodes) {
        if (n.last_commit_sha && n.last_commit_sha !== fromCommit) {
          matched.set(n.id, 0);
        }
      }
      return matched.size > 0 ? matched : null;
    }

    return null;
  });

  let queryMatchedIds = $derived.by(() => queryMatchedWithDepth ? new Set(queryMatchedWithDepth.keys()) : null);

  let queryCallouts = $derived.by(() => {
    if (!activeQuery?.callouts?.length) return new Map();
    const m = new Map();
    for (const c of activeQuery.callouts) {
      const cName = c.node ?? c.node_name;
      const n = nodes.find(n => n.name === cName || n.qualified_name === cName);
      if (n) m.set(n.id, c.label ?? c.text ?? '');
    }
    return m;
  });

  // Does a tree-group contain any matched nodes?
  function treeGroupHasMatch(ln) {
    if (!queryMatchedIds) return true;
    if (queryMatchedIds.has(ln.id)) return true;
    // Check children
    for (const cln of layoutNodes) {
      if (cln.parentTreeGroup === ln && queryMatchedIds.has(cln.id)) return true;
      if (cln.parentTreeGroup === ln && cln.kind === 'tree-group' && treeGroupHasMatch(cln)) return true;
    }
    return false;
  }

  function queryNodeOpacity(ln) {
    const nodeId = ln.node?.id ?? ln.id;
    // Timeline filtering: dim nodes outside the time range
    if (timelineNodes && ln.kind !== 'tree-group') {
      if (!timelineNodes.visibleIds.has(nodeId)) return 0.08;
    }
    // Search highlighting: dim non-matching nodes
    if (searchOpen && searchQuery.trim() && searchHighlightIds.size > 0) {
      if (ln.kind === 'tree-group') return 0.6;
      return searchHighlightIds.has(nodeId) ? 1.0 : 0.1;
    }
    if (!queryMatchedIds) return 1.0;
    if (ln.kind === 'tree-group') return treeGroupHasMatch(ln) ? 1.0 : 0.15;
    return queryMatchedIds.has(nodeId) ? 1.0 : (activeQuery?.emphasis?.dim_unmatched ?? 0.12);
  }

  function queryNodeColor(ln) {
    if (!activeQuery) return null;
    const nodeId = ln.node?.id ?? ln.id;
    const node = ln.node ?? nodes.find(n => n.id === nodeId);

    // Heat map: color ALL nodes by metric value using a palette
    const heat = activeQuery.emphasis?.heat;
    if (heat?.metric && node) {
      const metric = heat.metric;
      let value = 0;

      // Prefer server-provided node_metrics from dry-run queryResult
      const serverMetrics = queryResult?.node_metrics;
      if (serverMetrics && typeof serverMetrics === 'object' && nodeId in serverMetrics) {
        value = serverMetrics[nodeId] ?? 0;
      } else {
        // Fallback: compute metric value client-side (using pre-computed index for calls)
        if (metric === 'incoming_calls') {
          value = incomingCallCounts.get(nodeId) ?? 0;
        } else if (metric === 'outgoing_calls') {
          // Count outgoing Calls edges from this node
          for (const nb of (adjacency.get(nodeId) ?? [])) {
            if (nb.edgeType === 'calls' && !nb.reverse) value++;
          }
        } else if (metric === 'complexity') {
          value = node.complexity ?? 0;
        } else if (metric === 'churn' || metric === 'churn_count_30d') {
          value = node.churn_count_30d ?? node.churn ?? 0;
        } else if (metric === 'test_coverage') {
          value = (node.test_coverage ?? 0) * 100;
        } else if (metric === 'field_count') {
          // Count FieldOf edges targeting this node
          for (const nb of (adjacency.get(nodeId) ?? [])) {
            if (nb.edgeType === 'field_of' && nb.reverse) value++;
          }
        } else if (metric === 'risk_score') {
          // risk_score = churn × complexity × (1 - test_coverage)
          const churn = node.churn_count_30d ?? node.churn ?? 0;
          const complexity = node.complexity ?? 0;
          const testCov = node.test_coverage ?? 0;
          value = churn * complexity * (1 - testCov);
        } else if (metric === 'test_fragility') {
          // Transitive test fragility: count of distinct test paths reaching this node
          // Uses cached all-nodes computation (O(T*(N+M)) once, then O(1) per lookup)
          const fragility = computeAllTestFragility();
          value = fragility.get(nodeId) || 0;
        }
      }
      // For heat maps, value=0 is valid (means low/none) — show the low end
      // of the palette. Only skip if this node type shouldn't be colored.
      if (value === 0 && metric !== 'risk_score' && metric !== 'test_coverage') return null;
      // Normalize: find max across all nodes (cached in the query resolver)
      const maxVal = heatMaxValues.get(metric) ?? 1;
      const t = Math.min(1, value / maxVal);
      return heatColor(t, heat.palette ?? 'blue-red');
    }

    if (!queryMatchedIds) return null;
    if (!queryMatchedIds.has(nodeId)) return null;
    const tc = activeQuery.emphasis?.tiered_colors;
    if (tc?.length && queryMatchedWithDepth) {
      const d = queryMatchedWithDepth.get(nodeId) ?? 0;
      return tc[Math.min(d, tc.length - 1)];
    }
    return activeQuery.emphasis?.highlight?.matched?.color ?? '#fbbf24';
  }

  // Heat map color interpolation
  function heatColor(t, palette) {
    if (palette === 'blue-red' || !palette) {
      // Blue (0) → Cyan → Yellow → Red (1)
      if (t < 0.33) { const u = t / 0.33; return `hsl(${210 - u * 30}, 70%, ${40 + u * 5}%)`; }
      if (t < 0.66) { const u = (t - 0.33) / 0.33; return `hsl(${180 - u * 140}, 70%, ${45 + u * 5}%)`; }
      const u = (t - 0.66) / 0.34;
      return `hsl(${40 - u * 40}, ${70 + u * 15}%, ${50 - u * 10}%)`;
    }
    if (palette === 'green-yellow-red') {
      // Green (0 = governed/covered) → Yellow (0.5 = partial) → Red (1 = unspecified/risky)
      if (t < 0.5) {
        const u = t / 0.5;
        return `hsl(${120 - u * 60}, ${65 + u * 10}%, ${35 + u * 10}%)`;
      }
      const u = (t - 0.5) / 0.5;
      return `hsl(${60 - u * 60}, ${75 + u * 10}%, ${45 - u * 5}%)`;
    }
    if (palette === 'purple-orange') {
      // Purple (0) → Orange (1) — good for coupling/fragility
      if (t < 0.5) {
        const u = t / 0.5;
        return `hsl(${280 - u * 40}, ${60 + u * 15}%, ${40 + u * 10}%)`;
      }
      const u = (t - 0.5) / 0.5;
      return `hsl(${240 - u * 210}, ${75 + u * 10}%, ${50 - u * 5}%)`;
    }
    // Fallback: simple blue-to-red gradient
    return `hsl(${(1 - t) * 240}, 70%, 45%)`;
  }

  // Pre-compute heat map max values for normalization
  let heatMaxValues = $derived.by(() => {
    const map = new Map();
    if (!activeQuery?.emphasis?.heat?.metric) return map;
    const metric = activeQuery.emphasis.heat.metric;
    let max = 0;

    // If server-provided node_metrics exist (from dry-run queryResult), use those for max
    const serverMetrics = queryResult?.node_metrics;
    if (serverMetrics && typeof serverMetrics === 'object') {
      for (const v of Object.values(serverMetrics)) {
        if (v > max) max = v;
      }
    }

    // Also scan client-side metrics (fallback or supplement)
    for (const node of nodes) {
      let v = 0;
      if (metric === 'incoming_calls') {
        v = incomingCallCounts.get(node.id) ?? 0;
      } else if (metric === 'complexity') {
        v = node.complexity ?? 0;
      } else if (metric === 'churn' || metric === 'churn_count_30d') {
        v = node.churn_count_30d ?? node.churn ?? 0;
      } else if (metric === 'test_coverage') {
        v = (node.test_coverage ?? 0) * 100;
      }
      if (v > max) max = v;
    }
    map.set(metric, max || 1);
    return map;
  });

  // ── Connected highlight (when a node is selected, highlight connected nodes) ──
  let connectedHighlight = $derived.by(() => {
    if (!selectedNodeId) return null;
    const connected = new Set([selectedNodeId]);
    for (const e of edges) {
      const src = edgeSrc(e);
      const tgt = edgeTgt(e);
      const et = edgeType(e);
      if (et === 'contains' || et === 'field_of') continue;
      if (src === selectedNodeId) connected.add(tgt);
      if (tgt === selectedNodeId) connected.add(src);
    }
    return connected.size > 1 ? connected : null;
  });

  // ── Text width cache (LRU-bounded) ──────────────────────────────
  const TEXT_CACHE_MAX = 5000;
  const textWidthCache = new Map();
  function measureText(ctx, text, font) {
    const key = font + '|' + text;
    if (textWidthCache.has(key)) return textWidthCache.get(key);
    ctx.font = font;
    const w = ctx.measureText(text).width;
    // Evict oldest entries when cache exceeds limit
    if (textWidthCache.size >= TEXT_CACHE_MAX) {
      const firstKey = textWidthCache.keys().next().value;
      textWidthCache.delete(firstKey);
    }
    textWidthCache.set(key, w);
    return w;
  }

  // ── Drawing ──────────────────────────────────────────────────────────

  function roundRect(ctx, x, y, w, h, r) {
    r = Math.min(r, w / 2, h / 2);
    if (r < 0) r = 0;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + w - r, y);
    ctx.quadraticCurveTo(x + w, y, x + w, y + r);
    ctx.lineTo(x + w, y + h - r);
    ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
    ctx.lineTo(x + r, y + h);
    ctx.quadraticCurveTo(x, y + h, x, y + h - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  }

  function drawDotGrid(ctx) {
    const spacing = 40;
    const dotSize = 0.8;
    const alpha = Math.min(0.3, 0.15 / cam.zoom);
    if (alpha < 0.01) return;
    ctx.fillStyle = `rgba(100,116,139,${alpha})`;
    const tl = screenToWorld(0, 0);
    const br = screenToWorld(W, H);
    const sx = Math.floor(tl.x / spacing) * spacing;
    const sy = Math.floor(tl.y / spacing) * spacing;
    let count = 0;
    for (let wx = sx; wx < br.x; wx += spacing) {
      for (let wy = sy; wy < br.y; wy += spacing) {
        if (++count > 3000) return;
        const s = worldToScreen(wx, wy);
        ctx.fillRect(s.x - dotSize / 2, s.y - dotSize / 2, dotSize, dotSize);
      }
    }
  }

  function isVisible(ln) {
    const s = worldToScreen(ln.x, ln.y);
    const hw = (ln.w / 2) * cam.zoom + 20;
    const hh = (ln.h / 2) * cam.zoom + 20;
    return s.x + hw > 0 && s.x - hw < W && s.y + hh > 0 && s.y - hh < H;
  }

  // Build parent→children index for hierarchical draw (prune invisible subtrees)
  let childrenIndex = $state(new Map()); // parentId → [child layout nodes]
  let rootLayoutNodes = $state([]);      // nodes with no parent
  $effect(() => {
    const idx = new Map();
    const roots = [];
    for (const ln of layoutNodes) {
      const pid = ln.parentTreeGroup?.id;
      if (pid) {
        if (!idx.has(pid)) idx.set(pid, []);
        idx.get(pid).push(ln);
      } else {
        roots.push(ln);
      }
    }
    // Sort roots: tree-groups first, then by depth
    roots.sort((a, b) => {
      const at = a.kind === 'tree-group' ? 0 : 1;
      const bt = b.kind === 'tree-group' ? 0 : 1;
      return at !== bt ? at - bt : (a.treeDepth || 0) - (b.treeDepth || 0);
    });
    childrenIndex = idx;
    rootLayoutNodes = roots;
  });

  // Track canvas size to avoid unnecessary resize
  let lastCanvasW = 0;
  let lastCanvasH = 0;

  function drawFrame() {
    const canvas = canvasEl;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    // Only resize canvas buffer when dimensions actually change
    const needsResize = canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr);
    if (needsResize) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      lastCanvasW = W;
      lastCanvasH = H;
    }

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // Background
    ctx.fillStyle = '#0f0f1a';
    ctx.fillRect(0, 0, W, H);

    // Dot grid
    drawDotGrid(ctx);

    if (rootLayoutNodes.length === 0) return;

    // Draw edges first (below nodes)
    drawEdges(ctx);

    // Draw evaluative trace particles (above edges, below nodes)
    if (lens === 'evaluative' && evalParticles.length > 0) {
      drawTraceParticles(ctx);
    }

    // Draw view query group boundaries
    if (activeQuery?.groups?.length) {
      const GROUP_COLORS = ['#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981', '#ef4444'];
      for (let gi = 0; gi < activeQuery.groups.length; gi++) {
        const group = activeQuery.groups[gi];
        const groupNodeNames = group.nodes ?? group.node_names ?? [];
        if (groupNodeNames.length === 0) continue;
        // Find bounding box of all nodes in this group
        let gMinX = Infinity, gMinY = Infinity, gMaxX = -Infinity, gMaxY = -Infinity;
        for (const name of groupNodeNames) {
          const n = nodes.find(nd => nd.name === name || nd.qualified_name === name || nd.id === name);
          if (!n) continue;
          const ln = layoutNodeMap.get(n.id);
          if (!ln) continue;
          const l = ln.x - ln.w / 2, r = ln.x + ln.w / 2;
          const t = ln.y - ln.h / 2, b = ln.y + ln.h / 2;
          if (l < gMinX) gMinX = l;
          if (r > gMaxX) gMaxX = r;
          if (t < gMinY) gMinY = t;
          if (b > gMaxY) gMaxY = b;
        }
        if (gMinX === Infinity) continue;
        const pad = 12 / cam.zoom;
        const gColor = GROUP_COLORS[gi % GROUP_COLORS.length];
        const tl = worldToScreen(gMinX - pad, gMinY - pad);
        const br = worldToScreen(gMaxX + pad, gMaxY + pad);
        const gw = br.x - tl.x, gh = br.y - tl.y;
        ctx.save();
        ctx.globalAlpha = 0.15;
        ctx.fillStyle = gColor;
        roundRect(ctx, tl.x, tl.y, gw, gh, 8);
        ctx.fill();
        ctx.globalAlpha = 0.6;
        ctx.strokeStyle = gColor;
        ctx.lineWidth = 2;
        ctx.setLineDash([6, 4]);
        roundRect(ctx, tl.x, tl.y, gw, gh, 8);
        ctx.stroke();
        ctx.setLineDash([]);
        // Group label
        const label = group.label ?? group.name ?? `Group ${gi + 1}`;
        ctx.globalAlpha = 0.85;
        ctx.fillStyle = gColor;
        ctx.font = 'bold 11px system-ui, sans-serif';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';
        ctx.fillText(label, tl.x + 6, tl.y - 3);
        ctx.restore();
      }
    }

    // Draw nodes — hierarchical traversal that prunes invisible subtrees
    function drawNodeRecursive(ln) {
      // Frustum cull
      if (!isVisible(ln)) return;

      let op = nodeOpacity(ln);

      // For tree-groups: even if the group itself is transparent (zoomed in
      // past it), we still need to draw its children. Only skip leaf nodes
      // and tree-groups that are too small to see.
      const isTreeGroup = ln.kind === 'tree-group';
      const ss = isTreeGroup ? Math.min(ln.w * cam.zoom, ln.h * cam.zoom) : 0;

      // Too small to see at all — skip this node and all children
      if (ss > 0 && ss < 10) return;
      if (!isTreeGroup && op < 0.01) return;

      // Semantic zoom: deliberate information hierarchy per zoom level
      // The spec's core abstraction: "default zoom shows boundaries and interfaces"
      // with "types, functions visible on drill-down."
      //
      // Overview (< 0.3): packages, modules only — boundaries
      // Architecture (0.3-0.6): + types, traits, interfaces, enums, specs — essential design
      // Integration (0.6-1.0): + endpoints, tables, classes, components — integration points
      // Detail (1.0-2.0): + functions, methods — implementation
      // Full (> 2.0): + fields, constants, enum variants — every detail
      if (!isTreeGroup && ln.node) {
        const nt = ln.node.node_type ?? '';
        const isQueryMatched = activeQuery && matchedNodes?.has(ln.node.id);
        // Always show query-matched nodes regardless of zoom
        if (!isQueryMatched) {
          if (cam.zoom < 0.3 && !['package', 'module'].includes(nt)) return;
          if (cam.zoom < 0.6 && ['function', 'method', 'endpoint', 'field', 'constant', 'table', 'component', 'class', 'enum_variant'].includes(nt)) return;
          if (cam.zoom < 1.0 && ['function', 'method', 'field', 'constant', 'enum_variant'].includes(nt)) return;
          if (cam.zoom < 2.0 && ['field', 'constant', 'enum_variant'].includes(nt)) return;
        }
      }

      // Draw this node if it has any opacity
      if (op > 0.01) {
        let drawOp = op * filterOpacity(ln);

        if (activeQuery && drawOp > 0.01) {
          const qOp = queryNodeOpacity(ln);
          if (qOp >= 0.8) drawOp = Math.max(drawOp, qOp);
          else drawOp *= qOp;
        }

        if (connectedHighlight && ln.node) {
          if (!connectedHighlight.has(ln.node.id)) drawOp *= 0.2;
        }

        // Drill-down fade: dim nodes not being drilled into
        if (drillFadeTarget && ln.node && ln.id !== drillFadeTarget) {
          // Check if this node is a child of the drill target
          let isChild = false;
          let p = ln.parentTreeGroup;
          while (p) {
            if (p.id === drillFadeTarget) { isChild = true; break; }
            p = p.parentTreeGroup;
          }
          if (!isChild) drawOp *= drillFadeAlpha;
        }

        if (drawOp > 0.01) {
          ctx.save();
          ctx.globalAlpha = drawOp;

          const s = worldToScreen(ln.x, ln.y);
          const sw = ln.w * cam.zoom;
          const sh = ln.h * cam.zoom;

          if (isTreeGroup) {
            drawTreeGroup(ctx, ln, s, sw, sh, drawOp);
          } else {
            drawLeafNode(ctx, ln, s, sw, sh, drawOp);
          }

          ctx.restore();
        }
      }

      // Recursively draw children if this tree-group is in container mode
      if (isTreeGroup && !isSummaryMode(ln)) {
        const children = childrenIndex.get(ln.id);
        if (children) {
          for (const child of children) {
            drawNodeRecursive(child);
          }
        }
      }
    }

    for (const root of rootLayoutNodes) {
      drawNodeRecursive(root);
    }

    // Draw timeline ghost outlines — directional coloring per spec:
    // Green dotted = will be added after selected range (future nodes)
    // Red dotted = existed before selected range or deleted (past nodes)
    if (timelineNodes && timelineNodes.ghostIds.size > 0) {
      for (const ghostId of timelineNodes.ghostIds) {
        const ln = layoutNodeMap.get(ghostId);
        if (!ln || ln.kind === 'tree-group') continue;
        if (!isVisible(ln)) continue;
        const sw = ln.w * cam.zoom;
        const sh = ln.h * cam.zoom;
        if (sw < 6 || sh < 4) continue;
        const s = worldToScreen(ln.x, ln.y);
        const isAdded = timelineNodes.ghostAdded?.has(ghostId);
        const isRemoved = timelineNodes.ghostRemoved?.has(ghostId);
        ctx.save();
        ctx.globalAlpha = isAdded ? 0.25 : 0.18;
        ctx.strokeStyle = isAdded ? '#22c55e' : isRemoved ? '#ef4444' : '#94a3b8';
        ctx.lineWidth = 1;
        ctx.setLineDash(isAdded ? [4, 3] : isRemoved ? [6, 2] : [4, 3]);
        roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, 3);
        ctx.stroke();
        // Removed nodes get a strikethrough line
        if (isRemoved && sw > 20) {
          ctx.globalAlpha = 0.3;
          ctx.strokeStyle = '#ef4444';
          ctx.lineWidth = 1.5;
          ctx.setLineDash([]);
          ctx.beginPath();
          ctx.moveTo(s.x - sw / 2 + 4, s.y);
          ctx.lineTo(s.x + sw / 2 - 4, s.y);
          ctx.stroke();
        }
        ctx.setLineDash([]);
        ctx.restore();
      }
    }

    // Draw callout labels
    for (const [nodeId, label] of queryCallouts) {
      const ln = layoutNodeMap.get(nodeId);
      if (!ln) continue;
      const s = worldToScreen(ln.x, ln.y);
      const sh = ln.h * cam.zoom;
      ctx.save();
      ctx.globalAlpha = 0.95;
      ctx.fillStyle = '#fbbf24';
      ctx.font = 'bold 11px system-ui';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'bottom';
      ctx.fillText(label, s.x, s.y - sh / 2 - 4);
      ctx.restore();
    }

    // Trace path connecting arrows with edge type labels
    if (tracePathEdges.length > 0) {
      for (const te of tracePathEdges) {
        const sln = layoutNodeMap.get(te.fromId);
        const tln = layoutNodeMap.get(te.toId);
        if (!sln || !tln) continue;
        if (!isVisible(sln) || !isVisible(tln)) continue;
        const ss = worldToScreen(sln.x, sln.y);
        const ts = worldToScreen(tln.x, tln.y);
        // Draw red connecting arrow
        drawArrow(ctx, ss.x, ss.y, ts.x, ts.y, '#ef4444', 0.7, 2.0);
        // Draw edge type label at midpoint
        const dx = ts.x - ss.x, dy = ts.y - ss.y;
        const len = Math.sqrt(dx * dx + dy * dy);
        if (len > 40) {
          const mx = (ss.x + ts.x) / 2, my = (ss.y + ts.y) / 2;
          ctx.save();
          ctx.globalAlpha = 0.85;
          const labelFont = "9px 'SF Mono', Menlo, monospace";
          const labelText = (te.edgeType ?? '').replace('_', ' ');
          const ltw = measureText(ctx, labelText, labelFont);
          let angle = Math.atan2(dy, dx);
          if (angle > Math.PI / 2) angle -= Math.PI;
          if (angle < -Math.PI / 2) angle += Math.PI;
          ctx.translate(mx, my);
          ctx.rotate(angle);
          ctx.font = labelFont;
          roundRect(ctx, -ltw / 2 - 4, -8, ltw + 8, 16, 3);
          ctx.fillStyle = 'rgba(239, 68, 68, 0.15)';
          ctx.fill();
          ctx.strokeStyle = 'rgba(239, 68, 68, 0.4)';
          ctx.lineWidth = 1;
          ctx.stroke();
          ctx.fillStyle = '#fca5a5';
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText(labelText, 0, 0);
          ctx.restore();
        }
      }
    }

    // Trace path numbered badges (BFS order from trace source) — RED circles
    if (tracePathOrder.size > 0) {
      for (const [nodeId, stepNum] of tracePathOrder) {
        const ln = layoutNodeMap.get(nodeId);
        if (!ln) continue;
        if (!isVisible(ln)) continue;
        const op = nodeOpacity(ln);
        if (op < 0.05) continue;
        const s = worldToScreen(ln.x, ln.y);
        const sw = ln.w * cam.zoom;
        const sh = ln.h * cam.zoom;
        if (sw < 20 || sh < 14) continue;
        const badgeR = Math.max(8, Math.min(14, sw * 0.09));
        const bx = s.x - sw / 2 + badgeR + 3;
        const by = s.y - sh / 2 + badgeR + 3;
        ctx.save();
        // Red circle with white border
        ctx.globalAlpha = 0.95;
        ctx.fillStyle = '#dc2626';
        ctx.beginPath();
        ctx.arc(bx, by, badgeR, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = '#ffffff';
        ctx.lineWidth = 1.5;
        ctx.stroke();
        // Step number in white
        ctx.fillStyle = '#ffffff';
        ctx.font = `bold ${Math.max(8, badgeR)}px system-ui`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(stepNum), bx, by);
        // Show governing spec annotation below badge if space permits
        const n = ln.node;
        if (n?.spec_path && sw > 60 && sh > 30) {
          const specLabel = n.spec_path.split('/').pop()?.replace('.md', '') ?? '';
          if (specLabel) {
            const specFs = Math.max(7, Math.min(9, sw * 0.06));
            ctx.fillStyle = 'rgba(34, 197, 94, 0.8)';
            ctx.font = `400 ${specFs}px system-ui`;
            ctx.textAlign = 'left';
            ctx.textBaseline = 'top';
            ctx.fillText(`\u2190 ${specLabel}`, bx + badgeR + 3, by - specFs / 2);
          }
        }
        ctx.restore();
      }
    }

    // Narrative step markers
    if (activeQuery?.narrative?.length) {
      for (let i = 0; i < activeQuery.narrative.length; i++) {
        const step = activeQuery.narrative[i];
        const stepNode = step.node ?? step.node_name;
        const n = nodes.find(n => n.name === stepNode || n.qualified_name === stepNode);
        if (!n) continue;
        const ln = layoutNodeMap.get(n.id);
        if (!ln) continue;
        const s = worldToScreen(ln.x, ln.y);
        const sw = ln.w * cam.zoom;
        ctx.save();
        ctx.globalAlpha = 0.95;
        ctx.fillStyle = '#3b82f6';
        ctx.beginPath();
        ctx.arc(s.x + sw / 2 + 12, s.y - ln.h * cam.zoom / 2, 10, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#ffffff';
        ctx.font = 'bold 10px system-ui';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(i + 1), s.x + sw / 2 + 12, s.y - ln.h * cam.zoom / 2);
        ctx.restore();
      }
    }

    // ── Ghost overlay rendering (spec editing preview) ────────────────
    if (hasGhosts) {
      drawGhostOverlays(ctx);
    }

    drawMinimap();
  }

  // ── Ghost overlay drawing ─────────────────────────────────────────
  function drawGhostOverlays(ctx) {
    // Structural lens: "No particles. No animation. Pure structure."
    // Ghost overlays still render (they represent spec predictions) but without pulse.
    const basePulse = lens === 'structural' ? 0 : 0.5 + 0.5 * Math.sin(ghostPulsePhase * Math.PI * 2);
    ghostNodePositions.clear();

    for (const ghost of ghostOverlays) {
      // Try to find existing layout node for change/remove actions
      const existingLn = layoutNodeMap.get(ghost.id);

      // Confidence-modulated opacity: higher confidence = more opaque ghost.
      // confidence is 0-1 or a string like 'high'/'medium'/'low'.
      let confAlpha = 1.0;
      if (ghost.confidence != null) {
        if (typeof ghost.confidence === 'number') {
          confAlpha = Math.max(0.3, ghost.confidence);
        } else if (ghost.confidence === 'low') {
          confAlpha = 0.4;
        } else if (ghost.confidence === 'medium') {
          confAlpha = 0.7;
        }
        // 'high' or unrecognized → 1.0
      }
      // Confirmed ghosts (from full preview) get solid borders, no pulsing.
      // Unconfirmed (predictions) get pulsing dotted borders.
      const pulse = ghost.confirmed ? 0 : basePulse * confAlpha;

      if (ghost.action === 'add') {
        drawGhostNewNode(ctx, ghost, pulse);
      } else if (ghost.action === 'change' && existingLn) {
        drawGhostChangeNode(ctx, existingLn, ghost, pulse);
      } else if (ghost.action === 'remove' && existingLn) {
        drawGhostRemoveNode(ctx, existingLn, ghost, pulse);
      }

      // Draw ghost edges
      if (ghost.edges?.length) {
        for (const edge of ghost.edges) {
          drawGhostEdge(ctx, edge, ghost.action, pulse);
        }
      }
    }
  }

  function drawGhostNewNode(ctx, ghost, pulse) {
    // Position new ghost nodes near related nodes or in visible area
    let wx = 0, wy = 0;
    let gw = 100, gh = 40;
    let positioned = false;

    // Try to position near a related edge target
    if (ghost.edges?.length) {
      const relatedId = ghost.edges[0].target ?? ghost.edges[0].source;
      const relLn = layoutNodeMap.get(relatedId);
      if (relLn) {
        wx = relLn.x + relLn.w * 0.8;
        wy = relLn.y + relLn.h * 0.3;
        gw = relLn.w * 0.7;
        gh = relLn.h * 0.7;
        positioned = true;
      }
    }

    // Fallback: position near the center of visible nodes, not at world (0,0)
    if (!positioned) {
      // Use the current camera center as fallback position
      wx = cam.x + (layoutNodes.length > 0 ? layoutNodes[0].w * 0.5 : 0);
      wy = cam.y;
      // Try to find a node with a similar name to position near
      const ghostNameLower = (ghost.name ?? '').toLowerCase();
      if (ghostNameLower && layoutNodes.length > 0) {
        let bestLn = null;
        let bestScore = 0;
        for (const ln of layoutNodes) {
          if (ln.kind === 'tree-group') continue;
          const lnName = (ln.label ?? '').toLowerCase();
          // Simple substring match
          if (lnName && ghostNameLower.includes(lnName.split('.').pop())) {
            const score = lnName.length;
            if (score > bestScore) { bestScore = score; bestLn = ln; }
          }
        }
        if (bestLn) {
          wx = bestLn.x + bestLn.w * 0.8;
          wy = bestLn.y + bestLn.h * 0.3;
          gw = bestLn.w * 0.7;
          gh = bestLn.h * 0.7;
        }
      }
    }

    // Store ghost position so drawGhostEdge can find it
    if (ghost.id) {
      ghostNodePositions.set(ghost.id, { x: wx, y: wy, w: gw, h: gh });
    }

    const s = worldToScreen(wx, wy);
    const sw = gw * cam.zoom;
    const sh = gh * cam.zoom;
    if (sw < 8 || sh < 6) return;

    const r = Math.min(6, sw * 0.08);

    ctx.save();
    // Modulate opacity by confidence: high=full, medium=medium, low=faint
    const confAlpha = ghost.confidence === 'low' ? 0.25 : ghost.confidence === 'medium' ? 0.45 : 0.55;
    ctx.globalAlpha = confAlpha + 0.25 * pulse;

    // Confirmed: solid green border. Predicted: pulsing dotted green border.
    if (ghost.confirmed) {
      ctx.setLineDash([]);
      ctx.strokeStyle = '#22c55e';
      ctx.lineWidth = 2.5;
    } else {
      ctx.setLineDash([6, 4]);
      ctx.strokeStyle = '#22c55e';
      ctx.lineWidth = 2;
    }
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.stroke();

    // Semi-transparent green fill
    ctx.fillStyle = 'rgba(34, 197, 94, 0.08)';
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.fill();

    // Label
    if (sw > 30 && sh > 14) {
      const fontSize = Math.max(8, Math.min(13, Math.min(sw * 0.14, sh * 0.4)));
      ctx.fillStyle = '#22c55e';
      ctx.font = `500 ${fontSize}px 'SF Mono', Menlo, monospace`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      let label = ghost.name ?? '+ new';
      const maxW = sw - 8;
      const tw = measureText(ctx, label, ctx.font);
      if (tw > maxW) {
        while (measureText(ctx, label + '\u2026', ctx.font) > maxW && label.length > 3) label = label.slice(0, -1);
        label += '\u2026';
      }
      ctx.fillText(label, s.x, s.y);

      // Type label + confidence indicator below
      const hasType = sh > 30 && sw > 50 && ghost.type;
      if (hasType) {
        const typeSize = Math.max(7, fontSize * 0.7);
        ctx.fillStyle = 'rgba(34, 197, 94, 0.6)';
        ctx.font = `400 ${typeSize}px system-ui`;
        const typeLabel = ghost.confidence ? `${ghost.type} (${ghost.confidence})` : ghost.type;
        ctx.fillText(typeLabel, s.x, s.y + fontSize * 0.7 + 2);
      }

      // Reason text below type (if enough vertical space)
      if (sh > 50 && sw > 80 && ghost.reason) {
        const reasonSize = Math.max(7, fontSize * 0.6);
        ctx.fillStyle = 'rgba(34, 197, 94, 0.4)';
        ctx.font = `400 ${reasonSize}px system-ui`;
        let reason = ghost.reason;
        const reasonMaxW = sw - 12;
        const rw = measureText(ctx, reason, ctx.font);
        if (rw > reasonMaxW) {
          while (measureText(ctx, reason + '\u2026', ctx.font) > reasonMaxW && reason.length > 5) reason = reason.slice(0, -1);
          reason += '\u2026';
        }
        const reasonY = hasType ? s.y + fontSize * 0.7 + reasonSize + 6 : s.y + fontSize * 0.7 + 4;
        ctx.fillText(reason, s.x, reasonY);
      }
    }

    ctx.setLineDash([]);
    ctx.restore();
  }

  function drawGhostChangeNode(ctx, ln, ghost, pulse) {
    if (!isVisible(ln)) return;
    const s = worldToScreen(ln.x, ln.y);
    const sw = ln.w * cam.zoom;
    const sh = ln.h * cam.zoom;
    if (sw < 8 || sh < 6) return;

    const r = Math.min(6, sw * 0.08);

    ctx.save();
    const confAlpha = ghost.confidence === 'low' ? 0.3 : ghost.confidence === 'medium' ? 0.5 : 0.6;
    ctx.globalAlpha = confAlpha + 0.25 * pulse;

    // Yellow highlight overlay
    ctx.fillStyle = 'rgba(234, 179, 8, 0.12)';
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.fill();

    // Yellow border: solid if confirmed, dotted if predicted
    ctx.setLineDash(ghost.confirmed ? [] : [6, 4]);
    ctx.strokeStyle = '#eab308';
    ctx.lineWidth = ghost.confirmed ? 3 : 2.5;
    roundRect(ctx, s.x - sw / 2 - 2, s.y - sh / 2 - 2, sw + 4, sh + 4, r + 1);
    ctx.stroke();

    // "Changed" badge
    if (sw > 50 && sh > 20) {
      const badgeText = '\u0394'; // delta symbol
      const bx = s.x + sw / 2 - 2;
      const by = s.y - sh / 2 + 2;
      ctx.fillStyle = '#854d0e';
      ctx.beginPath();
      ctx.arc(bx, by, 8, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = '#fde047';
      ctx.font = 'bold 10px system-ui';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(badgeText, bx, by);
    }

    // Reason text (if space permits)
    if (sw > 80 && sh > 30 && ghost.reason) {
      const reasonSize = Math.max(7, Math.min(10, sw * 0.06));
      ctx.fillStyle = 'rgba(234, 179, 8, 0.5)';
      ctx.font = `400 ${reasonSize}px system-ui`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      let reason = ghost.reason;
      const reasonMaxW = sw - 12;
      const rw = measureText(ctx, reason, ctx.font);
      if (rw > reasonMaxW) {
        while (measureText(ctx, reason + '\u2026', ctx.font) > reasonMaxW && reason.length > 5) reason = reason.slice(0, -1);
        reason += '\u2026';
      }
      ctx.fillText(reason, s.x, s.y + sh / 2 - reasonSize - 4);
    }

    ctx.setLineDash([]);
    ctx.restore();
  }

  function drawGhostRemoveNode(ctx, ln, ghost, pulse) {
    if (!isVisible(ln)) return;
    const s = worldToScreen(ln.x, ln.y);
    const sw = ln.w * cam.zoom;
    const sh = ln.h * cam.zoom;
    if (sw < 8 || sh < 6) return;

    const r = Math.min(6, sw * 0.08);

    ctx.save();
    const confAlpha = ghost.confidence === 'low' ? 0.25 : ghost.confidence === 'medium' ? 0.35 : 0.45;
    ctx.globalAlpha = confAlpha + 0.2 * pulse;

    // Red semi-transparent overlay
    ctx.fillStyle = 'rgba(239, 68, 68, 0.15)';
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.fill();

    // Red dotted border
    ctx.setLineDash([4, 4]);
    ctx.strokeStyle = '#ef4444';
    ctx.lineWidth = 2;
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.stroke();

    // Strikethrough line
    ctx.setLineDash([]);
    ctx.strokeStyle = '#ef4444';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(s.x - sw / 2 + 4, s.y);
    ctx.lineTo(s.x + sw / 2 - 4, s.y);
    ctx.stroke();

    ctx.restore();
  }

  function drawGhostEdge(ctx, edge, action, pulse) {
    // Check both real layout nodes and ghost node positions
    const srcLn = layoutNodeMap.get(edge.source) ?? ghostNodePositions.get(edge.source);
    const tgtLn = layoutNodeMap.get(edge.target) ?? ghostNodePositions.get(edge.target);
    if (!srcLn || !tgtLn) return;

    const ss = worldToScreen(srcLn.x, srcLn.y);
    const ts = worldToScreen(tgtLn.x, tgtLn.y);

    // Frustum cull
    if (ss.x < -50 && ts.x < -50) return;
    if (ss.x > W + 50 && ts.x > W + 50) return;
    if (ss.y < -50 && ts.y < -50) return;
    if (ss.y > H + 50 && ts.y > H + 50) return;

    ctx.save();
    ctx.globalAlpha = 0.4 + 0.3 * pulse;
    ctx.setLineDash([6, 4]);

    if (action === 'add') {
      ctx.strokeStyle = '#22c55e';
    } else if (action === 'change') {
      ctx.strokeStyle = '#eab308';
    } else {
      ctx.strokeStyle = '#ef4444';
    }

    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(ss.x, ss.y);
    ctx.lineTo(ts.x, ts.y);
    ctx.stroke();

    // Arrowhead
    const angle = Math.atan2(ts.y - ss.y, ts.x - ss.x);
    const aLen = 8;
    ctx.beginPath();
    ctx.moveTo(ts.x, ts.y);
    ctx.lineTo(ts.x - aLen * Math.cos(angle - 0.4), ts.y - aLen * Math.sin(angle - 0.4));
    ctx.moveTo(ts.x, ts.y);
    ctx.lineTo(ts.x - aLen * Math.cos(angle + 0.4), ts.y - aLen * Math.sin(angle + 0.4));
    ctx.stroke();

    ctx.setLineDash([]);
    ctx.restore();
  }

  function drawTreeGroup(ctx, ln, s, sw, sh, op) {
    const summary = isSummaryMode(ln);
    const depth = ln.treeDepth || 0;
    const colors = treeGroupColor(depth, ln.childIndex || 0);
    const r = Math.min(16, Math.min(sw, sh) * 0.06);

    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);

    if (summary) {
      // Summary mode: filled box with centered label
      let summaryFill = colors.fillSummary;
      let borderColor = colors.border;

      const qColor = queryNodeColor(ln);
      if (qColor && ln.id !== selectedNodeId) {
        borderColor = qColor;
        summaryFill = qColor + '18';
      }

      ctx.fillStyle = summaryFill;
      ctx.fill();
      ctx.strokeStyle = ln.id === selectedNodeId ? '#ef4444' : borderColor;
      ctx.lineWidth = ln.id === selectedNodeId ? 2.5 : 1.5;
      ctx.stroke();

      // Label
      const fontSize = Math.max(8, Math.min(22, Math.min(sw, sh) * 0.22));
      ctx.fillStyle = '#e2e8f0';
      ctx.font = `600 ${fontSize}px system-ui, -apple-system, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      let label = ln.label;
      const maxLabelW = sw - 16;
      const tw = measureText(ctx, label, ctx.font);
      if (tw > maxLabelW && label.length > 5) {
        while (measureText(ctx, label + '\u2026', ctx.font) > maxLabelW && label.length > 3) label = label.slice(0, -1);
        label += '\u2026';
      }
      ctx.fillText(label, s.x, s.y - fontSize * 0.3);

      // Descendant count
      const subSize = Math.max(7, Math.min(13, fontSize * 0.65));
      ctx.fillStyle = '#64748b';
      ctx.font = `400 ${subSize}px 'SF Mono', Menlo, monospace`;
      ctx.fillText(`${ln.totalChildren.toLocaleString()} nodes`, s.x, s.y + fontSize * 0.55);

      // Sub-group count
      if (ln.treeNode?.children?.size > 0) {
        ctx.fillText(`${ln.treeNode.children.size} groups`, s.x, s.y + fontSize * 0.55 + subSize + 2);
      }
    } else {
      // Container mode: subtle border, transparent fill
      const screenSize = Math.min(sw, sh);
      const fillAlpha = Math.max(0.03, Math.min(0.25, 200 / screenSize));
      const containerHue = TREE_HUES[depth % TREE_HUES.length];

      ctx.fillStyle = `hsla(${containerHue}, 25%, 18%, ${fillAlpha})`;
      ctx.fill();

      const borderAlpha = Math.max(0.15, Math.min(0.7, 300 / screenSize));
      ctx.globalAlpha = op * borderAlpha;
      const borderColor = ln.id === selectedNodeId ? '#ef4444' : colors.border;
      ctx.strokeStyle = borderColor;
      ctx.lineWidth = ln.id === selectedNodeId ? 2 : 1;
      ctx.stroke();
      ctx.globalAlpha = op;

      // Label at top-left of container
      const fontSize = Math.max(7, Math.min(14, sh * 0.04));
      const labelAlpha = Math.max(0.3, Math.min(0.8, 400 / screenSize));
      ctx.globalAlpha = op * labelAlpha;
      ctx.fillStyle = '#94a3b8';
      ctx.font = `500 ${fontSize}px system-ui, sans-serif`;
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      const lx = s.x - sw / 2 + 6;
      const ly = s.y - sh / 2 + 4;
      let label = ln.label;
      if (ln.totalChildren > 0) label += ` (${ln.totalChildren.toLocaleString()})`;
      ctx.fillText(label, lx, ly);
      ctx.globalAlpha = op;
    }
  }

  function drawLeafNode(ctx, ln, s, sw, sh, op) {
    const n = ln.node;
    const r = Math.min(6, sw * 0.08);
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);

    // Fill — spec coverage is ALWAYS the base color (green=governed, amber=suggested, red=no spec).
    // Per spec: "The structural topology is always visible underneath. Evaluative data is overlaid."
    const qColor = queryNodeColor(ln);
    const isHeatMap = !!activeQuery?.emphasis?.heat?.metric;

    // Base fill: always show spec governance coloring (structural lens always visible)
    let fillColor = 'rgba(20,28,48,0.9)';
    if (!qColor || !isHeatMap) {
      const hasGovEdge = n?.id && governedByIndex.has(n.id);
      if (n?.spec_path || hasGovEdge) {
        fillColor = 'rgba(34,197,94,0.30)';                             // green (has spec)
      } else {
        const conf = n?.spec_confidence;
        if (conf === 'high') fillColor = 'rgba(34,197,94,0.30)';        // green
        else if (conf === 'medium') fillColor = 'rgba(234,179,8,0.25)'; // amber
        else if (conf === 'low') fillColor = 'rgba(249,115,22,0.20)';   // orange
        else fillColor = 'rgba(239,68,68,0.18)';                        // red (no spec)
      }
    }
    if (isHeatMap && qColor) {
      // Heat map / evaluative overlay: blend heat color over the structural base
      fillColor = qColor + 'cc'; // ~80% alpha — structural border still visible
    }
    ctx.fillStyle = fillColor;
    ctx.fill();

    // Multi-select highlight (Shift+Click for concept creation)
    const isMultiSelected = multiSelectedIds.has(ln.id);
    if (isMultiSelected) {
      ctx.save();
      ctx.strokeStyle = '#a78bfa'; // purple for multi-select
      ctx.lineWidth = 3;
      ctx.setLineDash([4, 3]);
      roundRect(ctx, s.x - sw / 2 - 2, s.y - sh / 2 - 2, sw + 4, sh + 4, r + 1);
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.restore();
    }

    // Border — colored by spec confidence, width scaled by churn
    let borderColor = ln.id === selectedNodeId ? '#ef4444' : isMultiSelected ? '#a78bfa' : specBorderColor(n);
    const churnWidth = 1 + Math.min((n?.churn_count_30d ?? 0) / 3, 4);
    let borderWidth = ln.id === selectedNodeId ? 2 : churnWidth;
    if (qColor && ln.id !== selectedNodeId) {
      if (isHeatMap) {
        // Heat map mode: keep spec governance border color (structural info)
        // but add a subtle inner glow with the heat color. The heat fill is
        // applied above; spec border remains visible per spec requirement
        // "structural topology is always visible underneath."
        borderWidth = churnWidth;
        // Draw a thin inner border with heat color for visual emphasis
        ctx.save();
        ctx.strokeStyle = qColor;
        ctx.lineWidth = 1;
        ctx.globalAlpha = 0.5;
        roundRect(ctx, s.x - sw / 2 + 2, s.y - sh / 2 + 2, sw - 4, sh - 4, Math.max(0, r - 2));
        ctx.stroke();
        ctx.restore();
      } else {
        // Non-heat query highlight: tint fill + glow border
        ctx.fillStyle = qColor + '44';
        roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
        ctx.fill();
        // Glow
        ctx.save();
        ctx.shadowColor = qColor;
        ctx.shadowBlur = 8;
        ctx.strokeStyle = qColor;
        ctx.lineWidth = 2.5;
        roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
        ctx.stroke();
        ctx.restore();
        borderWidth = 2;
      }
    }

    ctx.strokeStyle = borderColor;
    ctx.lineWidth = borderWidth;
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.stroke();

    // Evaluative lens overlay: semi-transparent heat color on top of structural
    const evalColor = evaluativeNodeColor(n?.id, n);
    if (evalColor && !qColor) {
      ctx.save();
      ctx.globalAlpha = 0.35;
      ctx.fillStyle = evalColor;
      roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
      ctx.fill();
      // Evaluative border glow
      ctx.globalAlpha = 0.6;
      ctx.strokeStyle = evalColor;
      ctx.lineWidth = 2;
      roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
      ctx.stroke();
      ctx.restore();
    }

    // Red glow on nodes with failed spans (evaluative lens)
    if (lens === 'evaluative' && n?.id && nodeHasErrors(n.id) && sw > 20) {
      ctx.save();
      ctx.shadowColor = '#ef4444';
      ctx.shadowBlur = 12;
      ctx.strokeStyle = '#ef4444';
      ctx.lineWidth = 2.5;
      ctx.globalAlpha = 0.7;
      roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
      ctx.stroke();
      ctx.shadowBlur = 0;
      ctx.restore();
    }

    // Label
    if (sw > 30 && sh > 14) {
      const fontSize = Math.max(8, Math.min(13, Math.min(sw * 0.14, sh * 0.4)));
      ctx.fillStyle = '#e2e8f0';
      ctx.font = `500 ${fontSize}px 'SF Mono', Menlo, monospace`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      let label = n?.name ?? '';
      const maxW = sw - 8;
      const tw = measureText(ctx, label, ctx.font);
      if (tw > maxW) {
        while (measureText(ctx, label + '\u2026', ctx.font) > maxW && label.length > 3) label = label.slice(0, -1);
        label += '\u2026';
      }
      ctx.fillText(label, s.x, s.y);

      // Node type indicator (small text below name)
      if (sh > 30 && sw > 50) {
        const typeSize = Math.max(7, fontSize * 0.7);
        ctx.fillStyle = '#64748b';
        ctx.font = `400 ${typeSize}px system-ui`;
        ctx.fillText(n?.node_type ?? '', s.x, s.y + fontSize * 0.7 + 2);
      }
    }

    // Badge rendering (from view query emphasis.badges or evaluative metrics)
    if (sw > 40 && sh > 25) {
      const badges = activeQuery?.emphasis?.badges;
      if (badges?.metric && n) {
        let badgeValue = 0;
        if (badges.metric === 'incoming_calls') {
          badgeValue = incomingCallCounts.get(n.id) ?? 0;
        } else if (badges.metric === 'complexity') badgeValue = n.complexity ?? 0;
        else if (badges.metric === 'churn') badgeValue = n.churn_count_30d ?? 0;
        else if (badges.metric === 'test_coverage') badgeValue = n.test_coverage != null ? Math.round(n.test_coverage * 100) : 0;
        if (badgeValue > 0) {
          const badgeText = (badges.template ?? '{{count}}').replace('{{count}}', String(badgeValue));
          const bx = s.x + sw / 2 - 4;
          const by = s.y - sh / 2 + 4;
          const bfs = Math.max(7, Math.min(10, sw * 0.1));
          ctx.save();
          ctx.font = `700 ${bfs}px 'SF Mono', Menlo, monospace`;
          const bw = measureText(ctx, badgeText, ctx.font) + 6;
          ctx.fillStyle = '#1e293b';
          ctx.beginPath();
          ctx.roundRect(bx - bw, by - 2, bw, bfs + 4, 3);
          ctx.fill();
          ctx.fillStyle = '#94a3b8';
          ctx.textAlign = 'right';
          ctx.textBaseline = 'top';
          ctx.fillText(badgeText, bx - 3, by);
          ctx.restore();
        }
      }
      // Evaluative metric badge (when evaluative lens is active)
      if (evalColor && n && sw > 50 && sh > 30) {
        let metricVal = 0;
        let metricLabel = '';
        if (evaluativeMetric === 'incoming_calls') { metricVal = incomingCallCounts.get(n.id) ?? 0; metricLabel = `${metricVal} calls`; }
        else if (evaluativeMetric === 'complexity') { metricVal = n.complexity ?? 0; metricLabel = `cx:${metricVal}`; }
        else if (evaluativeMetric === 'churn' || evaluativeMetric === 'churn_count_30d') { metricVal = n.churn_count_30d ?? n.churn ?? 0; metricLabel = `${metricVal} churn`; }
        else if (evaluativeMetric === 'test_coverage') { metricVal = Math.round((n.test_coverage ?? 0) * 100); metricLabel = `${metricVal}% cov`; }
        // Trace-based metrics from span data
        else if (evaluativeMetric === 'span_duration') {
          const stats = nodeSpanStats.get(n.id);
          if (stats) { metricVal = stats.meanDuration; metricLabel = metricVal < 1000 ? `${Math.round(metricVal)}\u00B5s` : `${(metricVal / 1000).toFixed(1)}ms`; }
        }
        else if (evaluativeMetric === 'span_count') {
          const stats = nodeSpanStats.get(n.id);
          if (stats) { metricVal = stats.spanCount; metricLabel = `${metricVal} spans`; }
        }
        else if (evaluativeMetric === 'error_rate') {
          const stats = nodeSpanStats.get(n.id);
          if (stats) { metricVal = stats.errorRate * 100; metricLabel = `${metricVal.toFixed(0)}% err`; }
        }
        if (metricVal > 0) {
          const bx = s.x + sw / 2 - 4;
          const by = s.y - sh / 2 + 4;
          const bfs = Math.max(7, Math.min(9, sw * 0.08));
          ctx.save();
          ctx.font = `700 ${bfs}px 'SF Mono', Menlo, monospace`;
          const bw = measureText(ctx, metricLabel, ctx.font) + 6;
          ctx.fillStyle = evalColor + '88';
          ctx.beginPath();
          ctx.roundRect(bx - bw, by - 2, bw, bfs + 4, 3);
          ctx.fill();
          ctx.fillStyle = '#fff';
          ctx.textAlign = 'right';
          ctx.textBaseline = 'top';
          ctx.fillText(metricLabel, bx - 3, by);
          ctx.restore();
        }
      }
      // Test node badge
      if (n?.test_node && sw > 50 && sh > 30) {
        const bx = s.x - sw / 2 + 4;
        const by = s.y - sh / 2 + 4;
        ctx.save();
        ctx.font = 'bold 8px system-ui';
        ctx.fillStyle = '#166534';
        ctx.beginPath();
        ctx.roundRect(bx, by - 1, 26, 11, 3);
        ctx.fill();
        ctx.fillStyle = '#bbf7d0';
        ctx.textAlign = 'left';
        ctx.textBaseline = 'top';
        ctx.fillText('TEST', bx + 2, by + 1);
        ctx.restore();
      }
    }

    // Spec assertion badges (§9: green checkmark / red X on governed nodes)
    if (assertionBadges.size > 0 && n?.id && sw > 40 && sh > 20) {
      const badge = assertionBadges.get(n.id);
      if (badge) {
        const bx = s.x + sw / 2 - 4;
        const by = s.y + sh / 2 - 4;
        const bfs = Math.max(7, Math.min(10, sw * 0.08));
        ctx.save();
        if (badge.failed > 0) {
          // Red X badge for failing assertions
          ctx.fillStyle = '#7f1d1d';
          ctx.beginPath();
          ctx.roundRect(bx - bfs * 2.5, by - bfs - 2, bfs * 2.5, bfs + 4, 3);
          ctx.fill();
          ctx.fillStyle = '#fca5a5';
          ctx.font = `700 ${bfs}px system-ui`;
          ctx.textAlign = 'right';
          ctx.textBaseline = 'bottom';
          ctx.fillText(`✗ ${badge.failed}/${badge.total}`, bx - 2, by);
        } else {
          // Green checkmark badge for all passing assertions
          ctx.fillStyle = '#14532d';
          ctx.beginPath();
          ctx.roundRect(bx - bfs * 2.5, by - bfs - 2, bfs * 2.5, bfs + 4, 3);
          ctx.fill();
          ctx.fillStyle = '#86efac';
          ctx.font = `700 ${bfs}px system-ui`;
          ctx.textAlign = 'right';
          ctx.textBaseline = 'bottom';
          ctx.fillText(`✓ ${badge.total}`, bx - 2, by);
        }
        ctx.restore();
      }
    }

    // Hovered state
    if (ln.id === hoveredNodeId) {
      ctx.strokeStyle = '#93c5fd';
      ctx.lineWidth = 1.5;
      roundRect(ctx, s.x - sw / 2 - 1, s.y - sh / 2 - 1, sw + 2, sh + 2, r + 1);
      ctx.stroke();
    }
  }

  function drawTraceParticles(ctx) {
    for (const p of evalParticles) {
      const sln = layoutNodeMap.get(p.fromId);
      const tln = layoutNodeMap.get(p.toId);
      if (!sln || !tln) continue;

      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);

      // Interpolate position along edge
      const px = ss.x + (ts.x - ss.x) * p.progress;
      const py = ss.y + (ts.y - ss.y) * p.progress;

      // Frustum cull
      if (px < -20 || px > W + 20 || py < -20 || py > H + 20) continue;

      const radius = 4 + cam.zoom * 0.5;

      // Glow
      ctx.save();
      ctx.shadowColor = p.glow;
      ctx.shadowBlur = 10;
      ctx.globalAlpha = 0.8;
      ctx.fillStyle = p.color;
      ctx.beginPath();
      ctx.arc(px, py, radius, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();

      // Core
      ctx.save();
      ctx.globalAlpha = 1;
      ctx.fillStyle = '#ffffff';
      ctx.beginPath();
      ctx.arc(px, py, radius * 0.4, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }

  function drawEdges(ctx) {
    // At low zoom, draw bundled inter-group edges instead of individual edges.
    // This prevents visual clutter while preserving connectivity awareness.
    if (cam.zoom < 0.5) {
      drawBundledEdges(ctx);
      return;
    }

    const maxEdges = 1500;
    let count = 0;

    for (const e of renderEdges) {
      if (count >= maxEdges) break;
      if (!filterEdge(e)) continue;

      const srcId = edgeSrc(e);
      const tgtId = edgeTgt(e);
      const sln = layoutNodeMap.get(srcId);
      const tln = layoutNodeMap.get(tgtId);
      if (!sln || !tln) continue;

      const sOp = nodeOpacity(sln);
      const tOp = nodeOpacity(tln);
      const alpha = Math.min(sOp, tOp) * 0.5;
      if (alpha < 0.02) continue;

      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);

      // Frustum cull
      if (ss.x < -50 && ts.x < -50) continue;
      if (ss.x > W + 50 && ts.x > W + 50) continue;
      if (ss.y < -50 && ts.y < -50) continue;
      if (ss.y > H + 50 && ts.y > H + 50) continue;

      const et = edgeType(e);
      let color = EDGE_COLORS[et] ?? '#64748b';
      let edgeAlpha = alpha;
      let lineWidth = 1.2;

      // Edge thickness by frequency:
      // Evaluative: use OTLP trace span frequency
      // Structural: use call edge count (how many callers reference a target)
      if (lens === 'evaluative' && traceEdgeFrequency.size > 0) {
        const freqKey = `${srcId}->${tgtId}`;
        const freq = traceEdgeFrequency.get(freqKey) ?? 0;
        if (freq > 0) {
          lineWidth = 1.5 + (freq / traceMaxFreq) * 4;
          edgeAlpha = Math.max(alpha, 0.6);
        }
      } else if (et === 'calls') {
        // In structural lens, thicken high-traffic edges by incoming call count
        const inCount = (adjacency.get(tgtId) ?? []).filter(nb => nb.edgeType === 'calls' && nb.reverse).length;
        if (inCount > 1) lineWidth = 1.2 + Math.min(inCount / 5, 3);
      }

      // Connected highlight
      if (connectedHighlight) {
        if (connectedHighlight.has(srcId) && connectedHighlight.has(tgtId)) {
          color = '#ef4444';
          edgeAlpha = Math.max(alpha, 0.9);
          lineWidth = 2.5;
        } else {
          edgeAlpha = alpha * 0.15;
        }
      }

      // Query edge filter
      if (queryMatchedIds) {
        if (!queryMatchedIds.has(srcId) || !queryMatchedIds.has(tgtId)) {
          edgeAlpha *= 0.1;
        }
        if (activeQuery?.edges?.filter?.length) {
          if (!activeQuery.edges.filter.includes(et)) continue;
        }
      }

      drawArrow(ctx, ss.x, ss.y, ts.x, ts.y, color, edgeAlpha, lineWidth);
      count++;

      // Edge labels at high zoom
      if (cam.zoom > 3 && count < 50) {
        const dx = ts.x - ss.x, dy = ts.y - ss.y;
        const len = Math.sqrt(dx * dx + dy * dy);
        if (len > 80) {
          const mx = (ss.x + ts.x) / 2, my = (ss.y + ts.y) / 2;
          ctx.save();
          ctx.globalAlpha = edgeAlpha * 0.85;
          const labelFont = "10px 'SF Mono', Menlo, monospace";
          const labelText = et.replace('_', ' ');
          const ltw = measureText(ctx, labelText, labelFont);
          let angle = Math.atan2(dy, dx);
          if (angle > Math.PI / 2) angle -= Math.PI;
          if (angle < -Math.PI / 2) angle += Math.PI;
          ctx.translate(mx, my);
          ctx.rotate(angle);
          ctx.font = labelFont;
          roundRect(ctx, -ltw / 2 - 5, -10, ltw + 10, 20, 4);
          ctx.fillStyle = 'rgba(15,15,26,0.85)';
          ctx.fill();
          ctx.fillStyle = '#64748b';
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText(labelText, 0, 0);
          ctx.restore();
        }
      }
    }
  }

  // Draw bundled inter-group edges at low zoom for structural awareness.
  // Groups edges by their source/target tree-group containers and draws
  // aggregate arrows with thickness proportional to edge count.
  function drawBundledEdges(ctx) {
    // Find visible summary-mode tree groups
    const summaryGroups = layoutNodes.filter(ln =>
      ln.kind === 'tree-group' && isSummaryMode(ln) && nodeOpacity(ln) > 0.1
    );
    if (summaryGroups.length === 0) return;

    // Map each graph node to its closest visible summary group
    const nodeToGroup = new Map();
    function mapDescendants(treeNode, groupId) {
      if (!treeNode) return;
      for (const gn of (treeNode.graphNodes ?? [])) {
        nodeToGroup.set(gn.id, groupId);
      }
      if (treeNode.children) {
        for (const child of treeNode.children.values()) {
          mapDescendants(child, groupId);
        }
      }
    }
    for (const ln of summaryGroups) {
      if (!ln.treeNode) continue;
      mapDescendants(ln.treeNode, ln.id);
    }

    // Also map leaf layout nodes to their parent tree group
    for (const ln of layoutNodes) {
      if (ln.kind !== 'leaf' || !ln.node) continue;
      if (nodeToGroup.has(ln.node.id)) continue;
      // Walk up parent chain to find summary group
      let p = ln.parentTreeGroup;
      while (p) {
        if (summaryGroups.some(sg => sg.id === p.id)) {
          nodeToGroup.set(ln.node.id, p.id);
          break;
        }
        p = p.parentTreeGroup;
      }
    }

    // Bundle edges by source/target group
    const bundles = new Map();
    for (const e of renderEdges) {
      const srcId = edgeSrc(e);
      const tgtId = edgeTgt(e);
      const sg = nodeToGroup.get(srcId);
      const tg = nodeToGroup.get(tgtId);
      if (!sg || !tg || sg === tg) continue;

      const key = sg < tg ? `${sg}|${tg}` : `${tg}|${sg}`;
      if (!bundles.has(key)) bundles.set(key, { sg, tg, count: 0 });
      bundles.get(key).count++;
    }

    for (const [, bundle] of bundles) {
      const sln = layoutNodeMap.get(bundle.sg);
      const tln = layoutNodeMap.get(bundle.tg);
      if (!sln || !tln) continue;

      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);

      // Frustum cull
      if (ss.x < -50 && ts.x < -50) continue;
      if (ss.x > W + 50 && ts.x > W + 50) continue;
      if (ss.y < -50 && ts.y < -50) continue;
      if (ss.y > H + 50 && ts.y > H + 50) continue;

      const thickness = Math.max(1, Math.min(5, Math.log2(bundle.count + 1)));
      const alpha = Math.min(0.6, 0.2 + bundle.count / 100);

      drawArrow(ctx, ss.x, ss.y, ts.x, ts.y, '#64748b', alpha, thickness);

      // Label bundle count
      if (thickness > 1.5) {
        const mx = (ss.x + ts.x) / 2;
        const my = (ss.y + ts.y) / 2;
        ctx.save();
        ctx.globalAlpha = Math.max(alpha, 0.5);
        const labelFont = "9px 'SF Mono', Menlo, monospace";
        ctx.font = labelFont;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const label = `${bundle.count} edges`;
        const tw = measureText(ctx, label, labelFont);
        roundRect(ctx, mx - tw / 2 - 4, my - 8, tw + 8, 16, 4);
        ctx.fillStyle = 'rgba(15,15,26,0.85)';
        ctx.fill();
        ctx.fillStyle = '#94a3b8';
        ctx.fillText(label, mx, my);
        ctx.restore();
      }
    }
  }

  function drawArrow(ctx, x1, y1, x2, y2, color, alpha, width) {
    const dx = x2 - x1, dy = y2 - y1, len = Math.sqrt(dx * dx + dy * dy);
    if (len < 5) return;
    const ux = dx / len, uy = dy / len;
    const headLen = Math.min(8, len * 0.2);
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = color;
    ctx.lineWidth = width || 1.2;
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2 - ux * headLen, y2 - uy * headLen);
    ctx.stroke();
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.moveTo(x2, y2);
    ctx.lineTo(x2 - ux * headLen - uy * headLen * 0.35, y2 - uy * headLen + ux * headLen * 0.35);
    ctx.lineTo(x2 - ux * headLen + uy * headLen * 0.35, y2 + uy * headLen - ux * headLen * 0.35);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  let minimapDirty = true;
  let lastMinimapDpr = 0;
  function drawMinimap() {
    const minimap = minimapEl;
    if (!minimap) return;
    const ctx = minimap.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    // Only resize buffer when DPR changes (resizing clears canvas)
    if (dpr !== lastMinimapDpr) {
      minimap.width = MINIMAP_W * dpr;
      minimap.height = MINIMAP_H * dpr;
      lastMinimapDpr = dpr;
      minimapDirty = true;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    ctx.fillStyle = '#0f0f1a';
    ctx.fillRect(0, 0, MINIMAP_W, MINIMAP_H);

    if (layoutNodes.length === 0) return;

    // Find bounds
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const ln of layoutNodes) {
      if (ln.kind !== 'tree-group' || ln.treeDepth > 0) continue;
      const l = ln.x - ln.w / 2, r = ln.x + ln.w / 2;
      const t = ln.y - ln.h / 2, b = ln.y + ln.h / 2;
      if (l < minX) minX = l;
      if (r > maxX) maxX = r;
      if (t < minY) minY = t;
      if (b > maxY) maxY = b;
    }
    if (minX === Infinity) return;

    const mw = maxX - minX, mh = maxY - minY;
    const scale = Math.min((MINIMAP_W - 8) / mw, (MINIMAP_H - 8) / mh);

    ctx.save();
    ctx.translate(MINIMAP_W / 2 - (minX + mw / 2) * scale, MINIMAP_H / 2 - (minY + mh / 2) * scale);
    ctx.scale(scale, scale);

    for (const ln of layoutNodes) {
      if (ln.treeDepth > 1) continue;
      const depth = ln.treeDepth || 0;
      const colors = treeGroupColor(depth, 0);
      ctx.globalAlpha = ln.kind === 'tree-group' ? 0.5 : 0.2;
      ctx.fillStyle = ln.kind === 'tree-group' ? colors.fillSummary : '#334155';
      ctx.fillRect(ln.x - ln.w / 2, ln.y - ln.h / 2, ln.w, ln.h);
      if (ln.kind === 'tree-group') {
        ctx.strokeStyle = colors.border;
        ctx.lineWidth = 1 / scale;
        ctx.globalAlpha = 0.4;
        ctx.strokeRect(ln.x - ln.w / 2, ln.y - ln.h / 2, ln.w, ln.h);
      }
    }

    // Viewport
    const tl = screenToWorld(0, 0);
    const br = screenToWorld(W, H);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = '#60a5fa';
    ctx.lineWidth = 2 / scale;
    ctx.strokeRect(tl.x, tl.y, br.x - tl.x, br.y - tl.y);

    ctx.restore();
  }

  // ── Camera animation loop ──────────────────────────────────────────

  function lerpCam() {
    cam.x += (targetCam.x - cam.x) * LERP_SPEED;
    cam.y += (targetCam.y - cam.y) * LERP_SPEED;
    cam.zoom += (targetCam.zoom - cam.zoom) * LERP_SPEED;
    if (Math.abs(cam.zoom - targetCam.zoom) < 0.0005) cam.zoom = targetCam.zoom;
    // Clamp zoom to prevent NaN/Infinity in screenToWorld calculations
    cam.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, cam.zoom));
  }

  let lastAnimTime = 0;
  function animLoop(timestamp) {
    const dt = lastAnimTime ? (timestamp - lastAnimTime) : 16;
    lastAnimTime = timestamp;
    lerpCam();

    // Advance ghost pulse animation (1 cycle per 1.5 seconds at ~60fps)
    // Stop after 3 full cycles to avoid perpetual CPU usage
    if (hasGhosts && ghostAnimCycles < 3) {
      const prev = ghostPulsePhase;
      ghostPulsePhase = (ghostPulsePhase + 0.011) % 1;
      if (ghostPulsePhase < prev) ghostAnimCycles++;
    }

    // Advance drill-down fade transition (smooth ease-out for entering, ease-in for leaving)
    if (drillFadeTarget && drillFadeAlpha > 0.12) {
      // Ease-out: fast at start, slower near end for smooth perception
      const delta = (drillFadeAlpha - 0.12) * 0.12;
      drillFadeAlpha = Math.max(0.12, drillFadeAlpha - Math.max(delta, 0.01));
    } else if (!drillFadeTarget && drillFadeAlpha < 1.0) {
      // Ease-in: gradual restoration
      const delta = (1.0 - drillFadeAlpha) * 0.1;
      drillFadeAlpha = Math.min(1.0, drillFadeAlpha + Math.max(delta, 0.01));
    } else if (drillFadeTarget && drillFadeAlpha <= 0.12) {
      drillFadeTarget = null; // Fade complete
    }

    // Advance evaluative trace particles
    if (lens === 'evaluative' && evalPlaying) {
      tickParticles(dt);
    }

    drawFrame();

    // Keep animating if camera is still moving or always (for smooth interactions)
    const dx = Math.abs(cam.x - targetCam.x);
    const dy = Math.abs(cam.y - targetCam.y);
    const dz = Math.abs(cam.zoom - targetCam.zoom);
    // Keep animating when camera moving or a one-time redraw is needed.
    // For ghosts: only animate the pulse for 3 cycles then stop to save CPU.
    const ghostNeedsAnim = hasGhosts && ghostAnimCycles < 3;
    const particlesPlaying = lens === 'evaluative' && evalPlaying;
    const fading = drillFadeTarget || drillFadeAlpha < 1.0;
    if (dx > 0.1 || dy > 0.1 || dz > 0.0001 || needsAnim || ghostNeedsAnim || particlesPlaying || fading) {
      needsAnim = false;
      animFrame = requestAnimationFrame(animLoop);
    } else {
      animFrame = null;
    }
  }

  function scheduleRedraw() {
    if (destroyed) return;
    needsAnim = true;
    if (!animFrame) animFrame = requestAnimationFrame(animLoop);
  }

  // ── Interaction handlers ──────────────────────────────────────────

  function hitTest(clientX, clientY) {
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return null;
    const sx = clientX - rect.left;
    const sy = clientY - rect.top;
    const world = screenToWorld(sx, sy);

    // Hierarchical hit test — only descend into visible containers
    function hitRecursive(nodes) {
      let best = null;
      for (const ln of nodes) {
        const l = ln.x - ln.w / 2, r = ln.x + ln.w / 2;
        const t = ln.y - ln.h / 2, b = ln.y + ln.h / 2;
        if (world.x < l || world.x > r + 1 || world.y < t || world.y > b + 1) continue;

        // This node contains the point
        const op = nodeOpacity(ln);
        if (op < 0.05) continue;

        best = ln;

        // If it's a tree-group in container mode, check children for deeper hit
        if (ln.kind === 'tree-group' && !isSummaryMode(ln)) {
          const children = childrenIndex.get(ln.id);
          if (children) {
            const childHit = hitRecursive(children);
            if (childHit) best = childHit;
          }
        }
      }
      return best;
    }

    return hitRecursive(rootLayoutNodes);
  }

  function onMouseDown(e) {
    if (e.button !== 0) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    panCamStart = { x: targetCam.x, y: targetCam.y };
    e.preventDefault();
  }

  function onMouseMove(e) {
    const hit = hitTest(e.clientX, e.clientY);
    const newHovered = hit?.id ?? null;

    if (newHovered !== hoveredNodeId) {
      hoveredNodeId = newHovered;
      if (hit) {
        tooltipNode = hit.node;
        tooltipPos = { x: e.clientX, y: e.clientY };
      } else {
        tooltipNode = null;
      }
      scheduleRedraw();
    } else if (tooltipNode) {
      tooltipPos = { x: e.clientX, y: e.clientY };
    }

    if (canvasEl) {
      canvasEl.style.cursor = isPanning ? 'grabbing' : hit ? 'pointer' : 'grab';
    }

    if (isPanning) {
      const dx = e.clientX - panStart.x;
      const dy = e.clientY - panStart.y;
      targetCam.x = panCamStart.x - dx / cam.zoom;
      targetCam.y = panCamStart.y - dy / cam.zoom;
      scheduleRedraw();
    }
  }

  function onMouseUp() {
    isPanning = false;
  }

  function onMouseLeave() {
    isPanning = false;
    hoveredNodeId = null;
    tooltipNode = null;
    scheduleRedraw();
  }

  // Last mouse position for inertial zoom
  let lastWheelMouse = { x: 0, y: 0 };

  function onWheel(e) {
    e.preventDefault();

    // Accumulate velocity for inertial zoom
    const impulse = e.deltaY > 0 ? -0.08 : 0.08;
    zoomVelocity += impulse;
    // Clamp velocity to prevent extreme zoom
    zoomVelocity = Math.max(-0.5, Math.min(0.5, zoomVelocity));

    const rect = canvasEl?.getBoundingClientRect();
    if (rect) {
      lastWheelMouse.x = e.clientX - rect.left;
      lastWheelMouse.y = e.clientY - rect.top;
    }

    // Apply immediate zoom step
    applyZoomAtMouse(zoomVelocity * 0.6);

    // Start inertial decay loop
    if (!zoomDecayFrame) {
      zoomDecayFrame = requestAnimationFrame(zoomDecayLoop);
    }
  }

  function applyZoomAtMouse(delta) {
    const factor = 1 + delta;
    const newZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, targetCam.zoom * factor));
    const sx = lastWheelMouse.x;
    const sy = lastWheelMouse.y;
    const worldX = (sx - W / 2) / targetCam.zoom + targetCam.x;
    const worldY = (sy - H / 2) / targetCam.zoom + targetCam.y;
    targetCam.x = worldX - (sx - W / 2) / newZoom;
    targetCam.y = worldY - (sy - H / 2) / newZoom;
    targetCam.zoom = newZoom;
    scheduleRedraw();
  }

  function zoomDecayLoop() {
    zoomVelocity *= 0.85; // Exponential decay
    if (Math.abs(zoomVelocity) < 0.001) {
      zoomVelocity = 0;
      zoomDecayFrame = null;
      return;
    }
    applyZoomAtMouse(zoomVelocity * 0.3);
    zoomDecayFrame = requestAnimationFrame(zoomDecayLoop);
  }

  // Particle hit testing: find if a click hit an animated trace particle
  function particleHitTest(clientX, clientY) {
    if (lens !== 'evaluative' || evalParticles.length === 0) return null;
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return null;
    const sx = clientX - rect.left;
    const sy = clientY - rect.top;
    const threshold = 12;

    for (const p of evalParticles) {
      const sln = layoutNodeMap.get(p.fromId);
      const tln = layoutNodeMap.get(p.toId);
      if (!sln || !tln) continue;
      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);
      const px = ss.x + (ts.x - ss.x) * p.progress;
      const py = ss.y + (ts.y - ss.y) * p.progress;
      const dist = Math.sqrt((sx - px) ** 2 + (sy - py) ** 2);
      if (dist < threshold) return p;
    }
    return null;
  }

  // Edge hit testing: find edge closest to click point within a threshold
  function edgeHitTest(clientX, clientY) {
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect || cam.zoom < 0.3) return null;
    const sx = clientX - rect.left;
    const sy = clientY - rect.top;
    const threshold = 8; // pixels

    let best = null;
    let bestDist = threshold;
    for (const e of renderEdges) {
      const srcId = edgeSrc(e);
      const tgtId = edgeTgt(e);
      const sln = layoutNodeMap.get(srcId);
      const tln = layoutNodeMap.get(tgtId);
      if (!sln || !tln) continue;
      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);
      // Point-to-line-segment distance
      const dx = ts.x - ss.x, dy = ts.y - ss.y;
      const len2 = dx * dx + dy * dy;
      if (len2 < 100) continue; // Too short to click
      let t = ((sx - ss.x) * dx + (sy - ss.y) * dy) / len2;
      t = Math.max(0, Math.min(1, t));
      const px = ss.x + t * dx, py = ss.y + t * dy;
      const dist = Math.sqrt((sx - px) ** 2 + (sy - py) ** 2);
      if (dist < bestDist) {
        bestDist = dist;
        const srcNode = nodes.find(n => n.id === srcId);
        const tgtNode = nodes.find(n => n.id === tgtId);
        const et = edgeType(e);
        best = { edge: e, edgeType: et, source: srcNode, target: tgtNode };
      }
    }
    return best;
  }

  function onClick(e) {
    if (Math.abs(e.clientX - panStart.x) > 4 || Math.abs(e.clientY - panStart.y) > 4) return;

    const hit = hitTest(e.clientX, e.clientY);
    if (hit) {
      // Shift+Click: multi-select for concept creation
      if (e.shiftKey) {
        const newSet = new Set(multiSelectedIds);
        if (newSet.has(hit.id)) {
          newSet.delete(hit.id);
        } else {
          newSet.add(hit.id);
        }
        multiSelectedIds = newSet;
        scheduleRedraw();
        return;
      }
      // Regular click: clear multi-select
      if (multiSelectedIds.size > 0) {
        multiSelectedIds = new Set();
      }
      selectedNodeId = hit.id;
      trackInteraction(`click:${hit.node.name ?? hit.node.id}(${hit.node.node_type})`);
      canvasState = {
        ...canvasState,
        selectedNode: {
          id: hit.node.id,
          name: hit.node.name ?? '',
          node_type: hit.node.node_type ?? '',
          qualified_name: hit.node.qualified_name ?? '',
        },
        zoom: cam.zoom,
        breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })),
        recent_interactions: recentInteractions,
      };
      // Enrich with trace path data if a trace is active
      const enrichedNode = { ...hit.node };
      if (tracePathOrder.size > 0) {
        const stepNum = tracePathOrder.get(hit.node.id);
        if (stepNum != null) {
          enrichedNode._traceStep = stepNum;
          enrichedNode._traceTotal = tracePathOrder.size;
          // Build step-by-step trace list for the detail panel
          enrichedNode._traceSteps = buildTraceStepList();
        }
      }
      onNodeDetail(enrichedNode);

      // Interactive $clicked mode: re-evaluate query template with the clicked node
      if (interactiveQueryTemplate) {
        const q = JSON.parse(JSON.stringify(interactiveQueryTemplate));
        q.scope.node = hit.node.qualified_name || hit.node.name || hit.node.id;
        if (q.annotation?.title) {
          q.annotation.title = q.annotation.title.replace(/\$name/g, hit.node.name ?? '');
        }
        if (q.annotation?.description) {
          q.annotation.description = q.annotation.description.replace(/\$name/g, hit.node.name ?? '');
        }
        onInteractiveQuery(q);
      }
    } else {
      // No node hit — check for particle click (evaluative lens)
      const particleHit = particleHitTest(e.clientX, e.clientY);
      if (particleHit) {
        // Show span detail in the detail panel
        const span = particleHit.span;
        onNodeDetail({
          id: `span-${span.span_id}`,
          name: span.operation_name,
          node_type: 'span',
          span_id: span.span_id,
          duration_us: span.duration_us,
          status: span.status,
          service_name: span.service_name,
          attributes: span.attributes,
          input_summary: span.input_summary,
          output_summary: span.output_summary,
          graph_node_id: span.graph_node_id,
        });
        scheduleRedraw();
        return;
      }
      // Check for edge click
      const edgeHit = edgeHitTest(e.clientX, e.clientY);
      if (edgeHit) {
        // Show edge relationship in detail panel
        const edgeInfo = {
          id: `edge-${edgeHit.source?.id}-${edgeHit.target?.id}`,
          name: `${edgeHit.source?.name ?? '?'} → ${edgeHit.target?.name ?? '?'}`,
          node_type: 'edge',
          edge_type: edgeHit.edgeType,
          source_node: edgeHit.source,
          target_node: edgeHit.target,
          file_path: edgeHit.source?.file_path,
          doc_comment: `${edgeHit.edgeType.replace('_', ' ')} relationship from ${edgeHit.source?.qualified_name ?? edgeHit.source?.name ?? '?'} to ${edgeHit.target?.qualified_name ?? edgeHit.target?.name ?? '?'}`,
        };
        onNodeDetail(edgeInfo);
      } else {
        selectedNodeId = null;
        canvasState = { ...canvasState, selectedNode: null };
        onNodeDetail(null);
      }
    }
    scheduleRedraw();
  }

  // Drill into a node with smooth spatial zoom transition.
  // Zooms INTO the clicked node's bounding box, fades unrelated nodes,
  // then renders children in the same layout style.
  function drillInto(node) {
    const children = treeData.parentToChildren.get(node.id) ?? [];
    if (children.length === 0) return;

    // Find the layout node to get its world coordinates
    const ln = layoutNodes.find(l => (l.node?.id ?? l.id) === node.id);
    const wx = ln?.x ?? cam.x;
    const wy = ln?.y ?? cam.y;
    const ww = ln?.w ?? 200;
    const wh = ln?.h ?? 200;

    // Start fade transition: dim unrelated nodes (animated in render loop)
    drillFadeTarget = node.id;
    drillFadeAlpha = 1.0;
    needsAnim = true; // Ensure the render loop picks up the fade animation
    trackInteraction(`drill:${node.name ?? node.id}`);
    breadcrumb = [...breadcrumb, { id: node.id, name: node.name ?? node.qualified_name ?? '?' }];
    selectedNodeId = null;
    canvasState = { ...canvasState, selectedNode: null, breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })), recent_interactions: recentInteractions };
    onNodeDetail(null);

    // Smooth spatial zoom: target camera to center on this node and fit it
    targetCam.x = wx;
    targetCam.y = wy;
    const fitZoom = Math.min(W / (ww + 60), H / (wh + 60), 4) * 0.85;
    targetCam.zoom = Math.max(fitZoom, cam.zoom * 1.5);
    needsAnim = true;
    scheduleRedraw();
  }

  function onDblClick(e) {
    const hit = hitTest(e.clientX, e.clientY);
    if (!hit) return;

    // Progressive drill-down via Contains edges (explorer-canvas.md §Progressive Drill-Down):
    // Double-click → filter to children, update breadcrumb, smooth zoom transition.
    const node = hit.node;
    if (node) {
      const children = treeData.parentToChildren.get(node.id) ?? [];
      if (children.length > 0) {
        drillInto(node);
        return;
      }
    }

    if (hit.kind === 'tree-group') {
      // Zoom into this tree group smoothly
      targetCam.x = hit.x;
      targetCam.y = hit.y;
      const fitZoom = Math.min(W / hit.w, H / hit.h) * 0.85;
      targetCam.zoom = Math.max(fitZoom, cam.zoom * 1.5);
      scheduleRedraw();
    } else if (hit.isLeafGraphNode) {
      // Zoom into leaf node — open its detail panel
      selectedNodeId = node?.id;
      onNodeDetail(node);
      targetCam.x = hit.x;
      targetCam.y = hit.y;
      targetCam.zoom = Math.max(cam.zoom * 2, 3);
      scheduleRedraw();
    }
  }

  // Context menu
  function onContextMenu(e) {
    e.preventDefault();
    const hit = hitTest(e.clientX, e.clientY);
    if (!hit) {
      contextMenu = null;
      return;
    }
    const rect = containerEl?.getBoundingClientRect();
    let menuX = e.clientX - (rect?.left ?? 0);
    let menuY = e.clientY - (rect?.top ?? 0);
    // Clamp to viewport to prevent overflow clipping (menu is ~220px wide, ~350px tall)
    const menuW = 220, menuH = 350;
    if (rect) {
      if (menuX + menuW > rect.width) menuX = Math.max(0, rect.width - menuW);
      if (menuY + menuH > rect.height) menuY = Math.max(0, rect.height - menuH);
    }
    contextMenu = {
      x: menuX,
      y: menuY,
      node: hit.node,
      hit,
    };
  }

  function contextMenuAction(action) {
    if (!contextMenu) return;
    const node = contextMenu.node;
    contextMenu = null;
    if (action === 'trace') {
      // Trace from here: show causal flow using Calls + RoutesTo edges.
      // Depth 15 to capture full reachable subgraph while staying responsive.
      onInteractiveQuery({
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls', 'routes_to'], direction: 'outgoing', depth: 15 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#22c55e', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'routes_to'] },
        zoom: 'fit',
        annotation: { title: `Trace from: ${node.name}`, description: `Causal flow showing execution order` },
        _trace: true,
      });
    } else if (action === 'blast') {
      onInteractiveQuery({
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls', 'implements', 'field_of', 'depends_on'], direction: 'incoming', depth: 10 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'implements', 'field_of', 'depends_on'] },
        zoom: 'fit',
        annotation: { title: `Blast radius: ${node.name}`, description: `What would break if this changes?` },
      });
    } else if (action === 'callers') {
      onInteractiveQuery({
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls'], direction: 'incoming', depth: 15 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls'] },
        zoom: 'fit',
        annotation: { title: `Callers of: ${node.name}`, description: `Who calls this?` },
      });
    } else if (action === 'callees') {
      onInteractiveQuery({
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls', 'routes_to'], direction: 'outgoing', depth: 15 },
        emphasis: { tiered_colors: ['#3b82f6', '#60a5fa', '#93c5fd', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'routes_to'] },
        zoom: 'fit',
        annotation: { title: `Callees of: ${node.name}`, description: `What does this call?` },
      });
    } else if (action === 'spec') {
      if (node.spec_path) {
        onNodeDetail({ ...node, _action: 'view_spec' });
      }
    } else if (action === 'create_spec') {
      // Open spec editor with a template for the uncovered node
      const suggestedPath = `specs/system/${(node.name ?? 'new').toLowerCase().replace(/[^a-z0-9]+/g, '-')}.md`;
      onNodeDetail({ ...node, _action: 'create_spec', suggested_spec_path: suggestedPath });
    } else if (action === 'detail') {
      selectedNodeId = node.id;
      onNodeDetail(node);
    } else if (action === 'provenance') {
      onNodeDetail({ ...node, _action: 'view_provenance' });
    } else if (action === 'history') {
      onNodeDetail({ ...node, _action: 'view_history' });
    } else if (action === 'open_in_code') {
      // Open in the detail panel's code tab instead of a new browser tab,
      // preserving all canvas state (zoom, query, conversation, drill-down).
      onNodeDetail({ ...node, _action: 'view_code' });
    } else if (action === 'drill') {
      // Drill into this node via Contains edges with smooth zoom transition
      const children = treeData.parentToChildren.get(node.id) ?? [];
      if (children.length > 0) {
        drillInto(node);
      }
    }
  }

  // Build a step-by-step trace list for the detail panel.
  // Returns an array of { step, nodeId, name, type, specPath, edges } sorted by step number.
  function buildTraceStepList() {
    if (tracePathOrder.size === 0) return [];
    const nodeMap = new Map();
    for (const n of nodes) nodeMap.set(n.id, n);
    const steps = [];
    for (const [nodeId, stepNum] of tracePathOrder) {
      const n = nodeMap.get(nodeId);
      if (!n) continue;
      // Find edges from this step to the next
      const outEdges = tracePathEdges.filter(e => e.fromId === nodeId);
      steps.push({
        step: stepNum,
        nodeId,
        name: n.name ?? n.qualified_name ?? '?',
        qualifiedName: n.qualified_name ?? '',
        type: n.node_type ?? '',
        specPath: n.spec_path ?? null,
        outEdges: outEdges.map(e => ({ toId: e.toId, edgeType: e.edgeType, toStep: e.toStep })),
      });
    }
    steps.sort((a, b) => a.step - b.step);
    return steps;
  }

  // Navigate breadcrumb with smooth zoom-out animation.
  // index=-1 means go to root, otherwise go to breadcrumb[index].
  function navigateBreadcrumb(index) {
    const prevBreadcrumb = [...breadcrumb];
    if (index < 0) {
      breadcrumb = [];
    } else {
      breadcrumb = breadcrumb.slice(0, index + 1);
    }
    selectedNodeId = null;
    canvasState = { ...canvasState, selectedNode: null, breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })), recent_interactions: recentInteractions };
    onNodeDetail(null);

    // Smooth zoom-out: animate camera to the target parent node or fit all.
    if (breadcrumb.length === 0) {
      // Going to root: fit the entire tree
      const allBounds = computeAllNodesBounds();
      if (allBounds) {
        targetCam.x = allBounds.cx;
        targetCam.y = allBounds.cy;
        targetCam.zoom = Math.min(W / (allBounds.w + 100), H / (allBounds.h + 100), 2) * 0.9;
      }
    } else {
      // Going to a parent: zoom out to show that node's children
      const parentId = breadcrumb[breadcrumb.length - 1].id;
      const ln = layoutNodes.find(l => (l.node?.id ?? l.id) === parentId);
      if (ln) {
        targetCam.x = ln.x;
        targetCam.y = ln.y;
        targetCam.zoom = Math.min(W / (ln.w + 60), H / (ln.h + 60), 4) * 0.85;
      }
    }
    needsAnim = true;
    scheduleRedraw();
  }

  // Compute bounding box of all visible layout nodes.
  function computeAllNodesBounds() {
    if (layoutNodes.length === 0) return null;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const ln of layoutNodes) {
      minX = Math.min(minX, ln.x - ln.w / 2);
      minY = Math.min(minY, ln.y - ln.h / 2);
      maxX = Math.max(maxX, ln.x + ln.w / 2);
      maxY = Math.max(maxY, ln.y + ln.h / 2);
    }
    return { cx: (minX + maxX) / 2, cy: (minY + maxY) / 2, w: maxX - minX, h: maxY - minY };
  }

  // Keyboard: Escape to zoom out to root, / or Cmd+K to search
  function onKeyDown(e) {
    // Cmd+K / Ctrl+K: global search (spec: "Cmd+K is global search")
    if (e.key === 'k' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      searchOpen = true;
      searchQuery = '';
      requestAnimationFrame(() => searchInputEl?.focus());
      return;
    }
    if (e.key === '/' && !searchOpen) {
      e.preventDefault();
      searchOpen = true;
      searchQuery = '';
      requestAnimationFrame(() => searchInputEl?.focus());
      return;
    }
    if (e.key === 'Escape') {
      if (searchOpen) { searchOpen = false; searchQuery = ''; return; }
      if (contextMenu) {
        contextMenu = null;
        return;
      }
      if (activeQuery) {
        onInteractiveQuery(null);
        return;
      }
      if (breadcrumb.length > 0) {
        breadcrumb = [];
        selectedNodeId = null;
        canvasState = { ...canvasState, selectedNode: null, breadcrumb: [] };
        onNodeDetail(null);
      } else {
        // Fit all
        targetCam = { x: 0, y: 0, zoom: cam.zoom };
        scheduleRedraw();
      }
      return;
    }

    // Keyboard pan (arrow keys) — move 100 world units per press
    const PAN_STEP = 100 / cam.zoom;
    if (e.key === 'ArrowLeft') { e.preventDefault(); targetCam.x -= PAN_STEP; scheduleRedraw(); return; }
    if (e.key === 'ArrowRight') { e.preventDefault(); targetCam.x += PAN_STEP; scheduleRedraw(); return; }
    if (e.key === 'ArrowUp') { e.preventDefault(); targetCam.y -= PAN_STEP; scheduleRedraw(); return; }
    if (e.key === 'ArrowDown') { e.preventDefault(); targetCam.y += PAN_STEP; scheduleRedraw(); return; }

    // Tab/Shift+Tab: cycle through visible nodes for keyboard accessibility
    if (e.key === 'Tab') {
      e.preventDefault();
      const visibleNodes = layoutNodes
        .filter(ln => ln.node && !ln.isTreeGroup && nodeOpacity(ln) > 0.3)
        .sort((a, b) => a.x - b.x || a.y - b.y);
      if (visibleNodes.length === 0) return;
      const currentIdx = visibleNodes.findIndex(ln => ln.node?.id === selectedNodeId);
      const nextIdx = e.shiftKey
        ? (currentIdx <= 0 ? visibleNodes.length - 1 : currentIdx - 1)
        : (currentIdx < 0 ? 0 : (currentIdx + 1) % visibleNodes.length);
      const ln = visibleNodes[nextIdx];
      if (ln?.node) {
        selectedNodeId = ln.node.id;
        // Announce to screen readers
        srAnnouncement = `${ln.node.node_type}: ${ln.node.name}`;
        // Center camera on selected node
        targetCam.x = ln.x;
        targetCam.y = ln.y;
        onNodeDetail(ln.node);
        scheduleRedraw();
      }
      return;
    }

    // Enter: drill into selected node
    if (e.key === 'Enter' && selectedNodeId) {
      const ln = layoutNodes.find(l => (l.node?.id ?? l.id) === selectedNodeId);
      if (ln?.node) {
        const children = treeData.parentToChildren.get(ln.node.id) ?? [];
        if (children.length > 0) drillInto(ln.node);
        else onNodeDetail(ln.node);
      }
      return;
    }

    // Keyboard zoom (+/- or =/-)
    if (e.key === '=' || e.key === '+') {
      e.preventDefault();
      targetCam.zoom = Math.min(MAX_ZOOM, targetCam.zoom * 1.2);
      scheduleRedraw();
      return;
    }
    if (e.key === '-') {
      e.preventDefault();
      targetCam.zoom = Math.max(MIN_ZOOM, targetCam.zoom / 1.2);
      scheduleRedraw();
      return;
    }

    // Tab: cycle through selectable nodes
    if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      const selectableNodes = layoutNodes.filter(ln => ln.isLeafGraphNode && nodeOpacity(ln) > 0.1);
      if (selectableNodes.length === 0) return;
      const currentIdx = selectedNodeId ? selectableNodes.findIndex(ln => ln.id === selectedNodeId) : -1;
      const nextIdx = e.shiftKey
        ? (currentIdx <= 0 ? selectableNodes.length - 1 : currentIdx - 1)
        : (currentIdx + 1) % selectableNodes.length;
      const next = selectableNodes[nextIdx];
      selectedNodeId = next.id;
      onNodeDetail(next.node);
      targetCam.x = next.x;
      targetCam.y = next.y;
      canvasState = { ...canvasState, selectedNode: next.node };
      scheduleRedraw();
      return;
    }

    // Enter: open detail panel for selected node
    if (e.key === 'Enter' && selectedNodeId) {
      const ln = layoutNodeMap.get(selectedNodeId);
      if (ln?.node) onNodeDetail(ln.node);
      return;
    }
  }

  // ── Minimap click-to-navigate ──────────────────────────────────────
  function minimapToWorld(clientX, clientY) {
    const rect = minimapEl?.getBoundingClientRect();
    if (!rect) return null;
    const mx = clientX - rect.left;
    const my = clientY - rect.top;
    // Reproduce the minimap transform to invert it
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const ln of layoutNodes) {
      if (ln.kind !== 'tree-group' || ln.treeDepth > 0) continue;
      const l = ln.x - ln.w / 2, r = ln.x + ln.w / 2;
      const t = ln.y - ln.h / 2, b = ln.y + ln.h / 2;
      if (l < minX) minX = l;
      if (r > maxX) maxX = r;
      if (t < minY) minY = t;
      if (b > maxY) maxY = b;
    }
    if (minX === Infinity) return null;
    const mw = maxX - minX, mh = maxY - minY;
    const scale = Math.min((MINIMAP_W - 8) / mw, (MINIMAP_H - 8) / mh);
    const ox = MINIMAP_W / 2 - (minX + mw / 2) * scale;
    const oy = MINIMAP_H / 2 - (minY + mh / 2) * scale;
    const wx = (mx - ox) / scale;
    const wy = (my - oy) / scale;
    return { x: wx, y: wy };
  }

  function onMinimapClick(e) {
    const w = minimapToWorld(e.clientX, e.clientY);
    if (!w) return;
    targetCam.x = w.x;
    targetCam.y = w.y;
    scheduleRedraw();
    e.stopPropagation();
  }

  let minimapDragging = $state(false);

  function onMinimapMouseDown(e) {
    if (e.button !== 0) return;
    minimapDragging = true;
    onMinimapClick(e);
    const onMove = (ev) => {
      if (!minimapDragging) return;
      const w = minimapToWorld(ev.clientX, ev.clientY);
      if (w) { targetCam.x = w.x; targetCam.y = w.y; scheduleRedraw(); }
    };
    const onUp = () => { minimapDragging = false; window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    e.stopPropagation();
    e.preventDefault();
  }

  // ── Touch handlers ────────────────────────────────────────────────
  let lastTouchDist = 0;

  function onTouchStart(e) {
    if (e.touches.length === 1) {
      isPanning = true;
      panStart = { x: e.touches[0].clientX, y: e.touches[0].clientY };
      panCamStart = { x: targetCam.x, y: targetCam.y };
    } else if (e.touches.length === 2) {
      isPanning = false;
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      lastTouchDist = Math.hypot(dx, dy);
    }
  }

  function onTouchMove(e) {
    if (e.touches.length === 1 && isPanning) {
      e.preventDefault();
      const dx = e.touches[0].clientX - panStart.x;
      const dy = e.touches[0].clientY - panStart.y;
      targetCam.x = panCamStart.x - dx / cam.zoom;
      targetCam.y = panCamStart.y - dy / cam.zoom;
      scheduleRedraw();
    } else if (e.touches.length === 2) {
      e.preventDefault();
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.hypot(dx, dy);
      if (lastTouchDist > 0) {
        const scale = dist / lastTouchDist;
        const newZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, targetCam.zoom * scale));
        // Zoom toward the pinch center (midpoint between two fingers)
        const rect = canvasEl?.getBoundingClientRect();
        if (rect) {
          const cx = (e.touches[0].clientX + e.touches[1].clientX) / 2 - rect.left;
          const cy = (e.touches[0].clientY + e.touches[1].clientY) / 2 - rect.top;
          const worldBefore = screenToWorld(cx, cy);
          targetCam.zoom = newZoom;
          const worldAfter = screenToWorld(cx, cy);
          targetCam.x -= (worldAfter.x - worldBefore.x);
          targetCam.y -= (worldAfter.y - worldBefore.y);
        } else {
          targetCam.zoom = newZoom;
        }
        scheduleRedraw();
      }
      lastTouchDist = dist;
    }
  }

  function onTouchEnd() {
    isPanning = false;
    lastTouchDist = 0;
  }

  // NOTE: ResizeObserver is created once at the top of the component (line ~108).
  // Do not create a second one here — it causes double W/H updates and double redraws.

  // Trigger redraws on reactive state changes (NOT hoveredNodeId — that triggers scheduleRedraw directly)
  $effect(() => {
    const _ = [selectedNodeId, activeQuery, queryMatchedIds, queryCallouts, connectedHighlight, filter, lens, ghostOverlays, queryResult];
    scheduleRedraw();
  });

  // View query zoom directive: auto-zoom to fit highlighted nodes after query is applied
  $effect(() => {
    if (!activeQuery?.zoom) return;
    // Handle { level: N } zoom
    if (typeof activeQuery.zoom === 'object' && activeQuery.zoom.level != null) {
      targetCam.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, activeQuery.zoom.level));
      scheduleRedraw();
      return;
    }
    if (activeQuery.zoom === 'current') return;
    if (activeQuery.zoom !== 'fit') return;
    if (!queryMatchedIds || queryMatchedIds.size === 0) return;
    // Find bounding box of all matched nodes in layout
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const ln of layoutNodes) {
      const nid = ln.node?.id ?? ln.id;
      if (!queryMatchedIds.has(nid)) continue;
      const l = ln.x - ln.w / 2, r = ln.x + ln.w / 2;
      const t = ln.y - ln.h / 2, b = ln.y + ln.h / 2;
      if (l < minX) minX = l;
      if (r > maxX) maxX = r;
      if (t < minY) minY = t;
      if (b > maxY) maxY = b;
    }
    if (minX === Infinity) return;
    const mw = maxX - minX, mh = maxY - minY;
    const fitZoom = Math.min(W / (mw || 1), H / (mh || 1)) * 0.8;
    targetCam = { x: (minX + maxX) / 2, y: (minY + maxY) / 2, zoom: Math.min(fitZoom, MAX_ZOOM) };
    scheduleRedraw();
  });

  // Compute trace path ordering and connecting edges when a trace query is active.
  // Uses depth-first ordering to represent call-stack execution order: when A calls B
  // and B calls C, the order is A(1)→B(2)→C(3) — which matches the actual execution
  // stack. For functions with multiple sequential callees, children are sorted
  // alphabetically for determinism (we lack source-order information from static analysis).
  $effect(() => {
    // Detect trace queries: focus scope with outgoing direction (Calls/RoutesTo traversal)
    const isTrace = activeQuery?.scope?.type === 'focus' &&
      (activeQuery.scope.direction === 'outgoing' || activeQuery._trace) &&
      queryMatchedWithDepth;
    if (!isTrace) {
      tracePathOrder = new Map();
      tracePathEdges = [];
      return;
    }

    // Build outgoing adjacency for DFS from matched edges
    const matchedIds = new Set(queryMatchedWithDepth.keys());
    const outAdj = new Map();
    for (const e of edges) {
      const srcId = edgeSrc(e);
      const tgtId = edgeTgt(e);
      const et = edgeType(e);
      if ((et === 'calls' || et === 'routes_to' || et === 'routesto') &&
          matchedIds.has(srcId) && matchedIds.has(tgtId)) {
        if (!outAdj.has(srcId)) outAdj.set(srcId, []);
        outAdj.get(srcId).push({ toId: tgtId, edgeType: et });
      }
    }

    // Find root node (the focus node)
    const rootNode = activeQuery.scope.node;
    let rootId = null;
    if (rootNode === '$clicked' || rootNode === '$selected') {
      rootId = canvasState.selectedNode?.id;
    } else {
      // Find by name
      const found = nodes.find(n => n.name === rootNode || n.qualified_name === rootNode || n.id === rootNode);
      rootId = found?.id;
    }

    if (!rootId || !matchedIds.has(rootId)) {
      // Fallback: use BFS depth ordering
      const entries = [...queryMatchedWithDepth.entries()].sort((a, b) => a[1] - b[1]);
      const ordered = new Map();
      for (let i = 0; i < entries.length; i++) ordered.set(entries[i][0], i + 1);
      tracePathOrder = ordered;
      tracePathEdges = [];
      return;
    }

    // DFS traversal to assign execution order
    const ordered = new Map();
    const traceEdges = [];
    const visited = new Set();
    let step = 0;

    function dfs(nodeId) {
      if (visited.has(nodeId)) return;
      visited.add(nodeId);
      step++;
      ordered.set(nodeId, step);

      const children = outAdj.get(nodeId) ?? [];
      // Sort children by name for deterministic ordering
      children.sort((a, b) => {
        const na = nodes.find(n => n.id === a.toId)?.name ?? '';
        const nb = nodes.find(n => n.id === b.toId)?.name ?? '';
        return na.localeCompare(nb);
      });

      for (const child of children) {
        if (!visited.has(child.toId)) {
          traceEdges.push({
            fromId: nodeId, toId: child.toId,
            edgeType: child.edgeType,
            fromStep: ordered.get(nodeId),
            toStep: step + 1
          });
          dfs(child.toId);
        }
      }
    }

    dfs(rootId);

    // Add remaining matched nodes not reachable via DFS (disconnected components)
    for (const nodeId of matchedIds) {
      if (!ordered.has(nodeId)) {
        step++;
        ordered.set(nodeId, step);
      }
    }

    tracePathOrder = ordered;
    tracePathEdges = traceEdges;
  });

  // Sync canvasState zoom
  $effect(() => {
    if (Math.abs(cam.zoom - (canvasState.zoom ?? 1)) > 0.01) {
      canvasState = { ...canvasState, zoom: cam.zoom };
    }
  });

  let destroyed = false;
  onDestroy(() => {
    destroyed = true;
    if (animFrame) { cancelAnimationFrame(animFrame); animFrame = null; }
    if (zoomDecayFrame) { cancelAnimationFrame(zoomDecayFrame); zoomDecayFrame = null; }
  });

  // ── Spec assertion badge state ──────────────────────────────────────
  // Maps governed node IDs to assertion pass/fail status when spec editor is open.
  // Assertions reference subjects like module("name"), type("name") — match against graph nodes.
  let assertionBadges = $derived.by(() => {
    if (!assertionSpecPath || !assertionResults?.length) return new Map();
    const badges = new Map(); // nodeId → { passed: number, failed: number, total: number }
    for (const r of assertionResults) {
      // Extract subject from assertion_text: module("X"), type("X"), function("X"), endpoint("X")
      const subjMatch = r.assertion_text?.match(/^(module|type|function|endpoint)\("([^"]+)"\)/);
      if (!subjMatch) continue;
      const subjName = subjMatch[2];
      // Find matching node
      const node = nodes.find(n =>
        n.name === subjName || n.qualified_name === subjName ||
        n.qualified_name?.endsWith(`::${subjName}`) || n.qualified_name?.endsWith(`.${subjName}`)
      );
      if (!node) continue;
      if (!badges.has(node.id)) badges.set(node.id, { passed: 0, failed: 0, total: 0 });
      const b = badges.get(node.id);
      b.total++;
      if (r.passed) b.passed++;
      else b.failed++;
    }
    return badges;
  });

  let legendItems = $derived(lens === 'evaluative' ? [
    ['Low flow', 'hsl(220,70%,45%)'],
    ['Medium', 'hsl(30,70%,45%)'],
    ['High flow', 'hsl(0,70%,45%)'],
    ['OK span', '#60a5fa'],
    ['Error span', '#ef4444'],
  ] : [
    ['Has spec', '#22c55e'],
    ['Suggested', '#eab308'],
    ['No spec', '#ef4444'],
    ['Calls', '#60a5fa'],
    ['Implements', '#34d399'],
  ]);
</script>

<div class="treemap-container">
  <!-- Multi-select concept creation bar -->
  {#if multiSelectedIds.size > 0}
    <div class="concept-creation-bar" role="status">
      <span class="concept-count">{multiSelectedIds.size} nodes selected</span>
      <button class="concept-create-btn" type="button" onclick={() => {
        const seedNodes = [...multiSelectedIds].map(id => {
          const n = nodes.find(nd => nd.id === id);
          return n?.qualified_name ?? n?.name ?? id;
        });
        const conceptQuery = {
          scope: { type: 'concept', seed_nodes: seedNodes, expand_edges: ['calls', 'implements', 'contains'], expand_depth: 1 },
          emphasis: { highlight: { matched: { color: '#a78bfa' } }, dim_unmatched: 0.12 },
          zoom: 'fit',
          annotation: { title: `Concept: ${seedNodes.length} seed nodes`, description: `{{count}} related nodes across {{group_count}} modules` },
        };
        onInteractiveQuery(conceptQuery);
        multiSelectedIds = new Set();
      }}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="3"/><path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"/></svg>
        Create Concept
      </button>
      <button class="concept-clear-btn" type="button" onclick={() => { multiSelectedIds = new Set(); scheduleRedraw(); }}>Clear</button>
      <span class="concept-hint">Shift+Click to add/remove</span>
    </div>
  {/if}

  <!-- Query annotation -->
  {#if activeQuery?.annotation?.title}
    {@const matchCount = queryMatchedIds?.size ?? '?'}
    {@const groupCount = (() => {
      // Compute distinct parent modules of matched nodes (not just group definition count)
      if (!queryMatchedIds || queryMatchedIds.size === 0) return '0';
      const parents = new Set();
      for (const id of queryMatchedIds) {
        const n = nodes.find(nd => nd.id === id);
        if (n?.qualified_name) {
          const parts = n.qualified_name.split(/[:.]/);
          if (parts.length > 1) parents.add(parts.slice(0, -1).join('.'));
          else parents.add(n.qualified_name);
        }
      }
      return String(parents.size);
    })()}
    {@const resolveVars = (t) => t?.replace(/\$name/g, canvasState?.selectedNode?.name ?? '')
      .replace(/\{\{count\}\}/g, String(matchCount))
      .replace(/\{\{group_count\}\}/g, groupCount) ?? ''}
    <div class="query-annotation" role="status">
      <div class="annotation-content">
        <span class="annotation-title">{resolveVars(activeQuery.annotation.title)}</span>
        {#if activeQuery.annotation.description}
          <span class="annotation-desc">{resolveVars(activeQuery.annotation.description)}</span>
        {/if}
      </div>
      {#if interactiveQueryTemplate}
        <span class="annotation-interactive-badge">click mode</span>
      {/if}
      <button class="annotation-clear" onclick={() => { onInteractiveQuery(null); interactiveQueryTemplate = null; }} title="Clear" type="button" aria-label="Clear view query">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>
  {/if}

  <!-- Preview mode indicator (ghost overlays active) -->
  {#if hasGhosts}
    <div class="preview-mode-bar" role="status" data-testid="preview-mode-indicator">
      <div class="preview-mode-content">
        <span class="preview-pulse-dot"></span>
        <span class="preview-mode-label">Preview Mode</span>
        <span class="preview-mode-count">{ghostOverlays.length} predicted {ghostOverlays.length === 1 ? 'change' : 'changes'}</span>
      </div>
      <div class="ghost-legend-chips">
        {#if ghostOverlays.some(g => g.action === 'add')}
          <span class="ghost-chip ghost-chip-add">+ New ({ghostOverlays.filter(g => g.action === 'add').length})</span>
        {/if}
        {#if ghostOverlays.some(g => g.action === 'change')}
          <span class="ghost-chip ghost-chip-change">{'\u0394'} Changed ({ghostOverlays.filter(g => g.action === 'change').length})</span>
        {/if}
        {#if ghostOverlays.some(g => g.action === 'remove')}
          <span class="ghost-chip ghost-chip-remove">{'\u2212'} Removed ({ghostOverlays.filter(g => g.action === 'remove').length})</span>
        {/if}
        {#if ghostOverlays.some(g => g.confidence)}
          {@const confs = ghostOverlays.filter(g => g.confidence)}
          {@const highCount = confs.filter(g => g.confidence === 'high').length}
          {@const medCount = confs.filter(g => g.confidence === 'medium').length}
          {@const lowCount = confs.filter(g => g.confidence === 'low').length}
          <span class="ghost-chip ghost-chip-conf" title="Prediction confidence breakdown">
            {#if highCount > 0}<span class="conf-high">{highCount}H</span>{/if}
            {#if medCount > 0}<span class="conf-med">{medCount}M</span>{/if}
            {#if lowCount > 0}<span class="conf-low">{lowCount}L</span>{/if}
          </span>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Toolbar -->
  <div class="treemap-toolbar">
    <div class="filter-group" role="group" aria-label="Filter presets">
      {#each [['all', 'All'], ['endpoints', 'Endpoints'], ['types', 'Types'], ['calls', 'Calls'], ['dependencies', 'Dependencies']] as [key, label]}
        <button class="tb-btn" class:active={filter === key} onclick={() => { filter = key; syncCanvasState(); scheduleRedraw(); }} aria-pressed={filter === key} type="button">{label}</button>
      {/each}
    </div>

    <div class="tb-sep"></div>

    <div class="lens-group" role="group" aria-label="Lens toggle">
      <button class="tb-btn" class:active={lens === 'structural'} onclick={() => { lens = 'structural'; onLensChange('structural'); }} aria-pressed={lens === 'structural'} type="button">Structural</button>
      <button class="tb-btn" class:active={lens === 'evaluative'} onclick={() => { lens = 'evaluative'; onLensChange('evaluative'); }} aria-pressed={lens === 'evaluative'} title="Overlay test/trace data on the structural topology" type="button">Evaluative</button>
      <button class="tb-btn tb-btn-observable" type="button" title="Observable lens — requires production OpenTelemetry collector integration" onclick={() => { observableBannerVisible = true; }}>Observable</button>
    </div>

    {#if lens === 'evaluative'}
      <div class="eval-metric-group" role="group" aria-label="Evaluative metric">
        {#if traceData?.spans?.length}
          <!-- Trace-based metrics (primary evaluative data per spec) -->
          {#each [['span_duration', 'Duration'], ['span_count', 'Spans'], ['error_rate', 'Errors']] as [key, label]}
            <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => { evaluativeMetric = key; onLensChange('evaluative'); }} type="button">{label}</button>
          {/each}
          <span class="tb-sep-v"></span>
          <span class="eval-label">Static:</span>
        {/if}
        <!-- Static analysis metrics (structural overlay for repos without trace data) -->
        {#each [['complexity', 'Complexity'], ['churn', 'Churn'], ['incoming_calls', 'Call Count'], ['test_coverage', 'Test Coverage']] as [key, label]}
          <button class="tb-btn tb-btn-sm" class:active={evaluativeMetric === key} onclick={() => { evaluativeMetric = key; onLensChange('evaluative'); }} type="button">{label}</button>
        {/each}
      </div>
      {#if traceData?.spans?.length}
        <div class="eval-playback" role="group" aria-label="Trace playback">
          <button class="tb-btn tb-btn-sm" onclick={() => { evalPlaying = !evalPlaying; if (evalPlaying) scheduleRedraw(); }} type="button" title={evalPlaying ? 'Pause' : 'Play'}>
            {evalPlaying ? '\u23F8' : '\u25B6'}
          </button>
          <input type="range" min="0" max="100" value={Math.round(evalScrubber * 100)}
            oninput={(e) => { evalScrubber = parseInt(e.target.value) / 100; scheduleRedraw(); }}
            class="eval-scrubber" title="Trace timeline position" />
          <select class="eval-speed" value={evalSpeed}
            onchange={(e) => { evalSpeed = parseFloat(e.target.value); }}>
            <option value="0.25">0.25x</option>
            <option value="0.5">0.5x</option>
            <option value="1">1x</option>
            <option value="2">2x</option>
            <option value="5">5x</option>
          </select>
          <span class="eval-particle-count">{evalParticles.length} spans</span>
        </div>
      {:else}
        <span class="eval-no-trace">No trace data</span>
      {/if}
      <div class="tb-sep"></div>
    {/if}

    <div class="treemap-legend">
      {#each legendItems as [label, color]}
        <span class="legend-item">
          <span class="legend-swatch" style="background: {color}"></span>
          <span class="legend-label">{label}</span>
        </span>
      {/each}
    </div>

    <button class="tb-btn" class:active={timelineEnabled} onclick={() => { timelineEnabled = !timelineEnabled; scheduleRedraw(); }} title="Toggle architecture timeline" type="button" aria-pressed={timelineEnabled}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="vertical-align: -2px; margin-right: 2px"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
      Timeline
    </button>

    {#if anomalies.length > 0 && !anomalyPanelOpen}
      <button class="tb-btn anomaly-reopen" onclick={() => { anomalyPanelOpen = true; }} title="Show anomalies ({anomalies.length})" type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="vertical-align: -2px; margin-right: 2px"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        {anomalies.length}
      </button>
    {/if}

    <span class="zoom-ind">{cam.zoom.toFixed(2)}x</span>
    <span class="treemap-stats">{nodes.length} nodes</span>
  </div>

  <!-- Canvas -->
  <div class="treemap-canvas-area" bind:this={containerEl}>
    {#if nodes.length === 0}
      <EmptyState
        title={$t('explorer_treemap.empty_title')}
        description={$t('explorer_treemap.empty_desc')}
      />
    {:else}
      <canvas
        bind:this={canvasEl}
        class="treemap-canvas"
        onmousedown={onMouseDown}
        onmousemove={onMouseMove}
        onmouseup={onMouseUp}
        onmouseleave={onMouseLeave}
        onwheel={onWheel}
        onclick={(e) => { contextMenu = null; onClick(e); }}
        ondblclick={onDblClick}
        oncontextmenu={onContextMenu}
        onkeydown={onKeyDown}
        ontouchstart={onTouchStart}
        ontouchmove={onTouchMove}
        ontouchend={onTouchEnd}
        ontouchcancel={onTouchEnd}
        role="application"
        aria-label="Architecture explorer canvas"
        tabindex="0"
      ></canvas>

      <!-- Screen reader: announce selected node -->
      <div class="sr-only" aria-live="polite" aria-atomic="true">
        {#if selectedNodeId}
          {@const sel = layoutNodeMap.get(selectedNodeId)}
          {#if sel?.node}
            Selected: {sel.node.name} ({sel.node.node_type}){sel.node.spec_path ? `, governed by ${sel.node.spec_path}` : ', no spec'}
          {/if}
        {/if}
      </div>

      <!-- Tooltip -->
      {#if tooltipNode && !isPanning}
        <div class="treemap-tooltip" style="left: {tooltipPos.x + 14}px; top: {tooltipPos.y - 50}px" role="tooltip">
          <span class="tooltip-type" style="color: {specBorderColor(tooltipNode)}">{tooltipNode.node_type}</span>
          <span class="tooltip-name">{tooltipNode.name}</span>
          {#if tooltipNode.qualified_name && tooltipNode.qualified_name !== tooltipNode.name}
            <span class="tooltip-qname">{tooltipNode.qualified_name}</span>
          {/if}
          {#if tooltipNode.file_path}
            <span class="tooltip-file">{tooltipNode.file_path}:{tooltipNode.line_start}</span>
          {/if}
          {#if (descendantCounts.get(tooltipNode.id) ?? 0) > 1}
            <span class="tooltip-count">{descendantCounts.get(tooltipNode.id)} items</span>
          {/if}
          {#if tooltipNode.spec_path}
            <span class="tooltip-spec">spec: {tooltipNode.spec_path}</span>
          {/if}
          <div class="tooltip-eval">
            {#if tooltipNode.test_node}<span class="tooltip-test-badge">test function</span>{/if}
            {#if tooltipNode.complexity != null || tooltipNode.churn_count_30d != null || tooltipNode.test_coverage != null}
              <div class="tooltip-insight">
                {#if (tooltipNode.complexity ?? 0) > 30}
                  <span class="insight-warn">High complexity ({tooltipNode.complexity}) — candidate for decomposition</span>
                {:else if (tooltipNode.complexity ?? 0) > 15}
                  <span class="insight-note">Moderate complexity ({tooltipNode.complexity})</span>
                {:else if tooltipNode.complexity != null}
                  <span class="insight-ok">Simple ({tooltipNode.complexity})</span>
                {/if}
                {#if (tooltipNode.churn_count_30d ?? 0) > 15}
                  <span class="insight-warn">Frequently changed ({tooltipNode.churn_count_30d}/month) — may need stabilization</span>
                {:else if (tooltipNode.churn_count_30d ?? 0) > 5}
                  <span class="insight-note">Active development ({tooltipNode.churn_count_30d}/month)</span>
                {:else if tooltipNode.churn_count_30d != null && tooltipNode.churn_count_30d > 0}
                  <span class="insight-ok">Stable ({tooltipNode.churn_count_30d}/month)</span>
                {/if}
                {#if tooltipNode.test_coverage != null && tooltipNode.test_coverage < 0.3}
                  <span class="insight-warn">Low test coverage ({Math.round(tooltipNode.test_coverage * 100)}%)</span>
                {:else if tooltipNode.test_coverage != null && tooltipNode.test_coverage < 0.7}
                  <span class="insight-note">Partial coverage ({Math.round(tooltipNode.test_coverage * 100)}%)</span>
                {:else if tooltipNode.test_coverage != null}
                  <span class="insight-ok">Well tested ({Math.round(tooltipNode.test_coverage * 100)}%)</span>
                {/if}
                {#if (tooltipNode.complexity ?? 0) > 20 && (tooltipNode.test_coverage == null || tooltipNode.test_coverage < 0.5)}
                  <span class="insight-risk">Risk: complex + undertested</span>
                {/if}
              </div>
            {/if}
            {#if nodeSpanStats.get(tooltipNode.id)}
              {@const spanSt = nodeSpanStats.get(tooltipNode.id)}
              <div class="tooltip-span-stats">
                <span>p50: {spanSt.p50 < 1000 ? `${Math.round(spanSt.p50)}\u00B5s` : `${(spanSt.p50 / 1000).toFixed(1)}ms`}</span>
                <span>p95: {spanSt.p95 < 1000 ? `${Math.round(spanSt.p95)}\u00B5s` : `${(spanSt.p95 / 1000).toFixed(1)}ms`}</span>
                {#if spanSt.p95 > spanSt.p50 * 5}
                  <span class="insight-warn">High tail latency (p95/p50 = {(spanSt.p95 / Math.max(1, spanSt.p50)).toFixed(1)}x)</span>
                {/if}
                {#if spanSt.errorRate > 0.05}
                  <span class="insight-warn">Error rate: {Math.round(spanSt.errorRate * 100)}%</span>
                {:else if spanSt.errorRate > 0}
                  <span class="insight-note">Error rate: {Math.round(spanSt.errorRate * 100)}%</span>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Minimap -->
      <div class="treemap-minimap" aria-hidden="true">
        <canvas bind:this={minimapEl} style="width: {MINIMAP_W}px; height: {MINIMAP_H}px" onclick={onMinimapClick} onmousedown={onMinimapMouseDown}></canvas>
      </div>

      <!-- Context menu -->
      {#if contextMenu}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="ctx-menu-backdrop" onclick={() => { contextMenu = null; }} oncontextmenu={(e) => { e.preventDefault(); contextMenu = null; }}></div>
        <div class="ctx-menu" style="left: {contextMenu.x}px; top: {contextMenu.y}px" role="menu">
          <div class="ctx-menu-header">
            <span class="ctx-node-type">{contextMenu.node.node_type}</span>
            <span class="ctx-node-name">{contextMenu.node.name}</span>
          </div>
          <div class="ctx-sep"></div>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('trace')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/></svg>
            Trace from here
          </button>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('blast')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/></svg>
            Blast radius
          </button>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('callers')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><polyline points="15 10 20 15 15 20"/><path d="M4 4v7a4 4 0 004 4h12"/></svg>
            Show callers
          </button>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('callees')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><polyline points="9 18 15 12 9 6"/></svg>
            Show callees
          </button>
          {#if (treeData.parentToChildren.get(contextMenu.node.id) ?? []).length > 0}
            <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('drill')}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M13 17l5-5-5-5M6 17l5-5-5-5"/></svg>
              Drill into
            </button>
          {/if}
          <div class="ctx-sep"></div>
          {#if contextMenu.node.spec_path}
            <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('spec')}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
              View spec
            </button>
          {:else}
            <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('create_spec')}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
              Create spec
            </button>
          {/if}
          <div class="ctx-sep"></div>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('detail')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
            View details
          </button>
          {#if contextMenu.node.file_path}
            <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('open_in_code')}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>
              Open in code
            </button>
          {/if}
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('provenance')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 013 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
            View provenance
          </button>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('history')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
            View history
          </button>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Canvas-scoped search overlay -->
  {#if searchOpen}
    <div class="canvas-search">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/></svg>
      <input
        bind:this={searchInputEl}
        type="text"
        class="canvas-search-input"
        placeholder="Search entities..."
        value={searchQuery}
        oninput={(e) => { searchQuery = e.target.value; searchSelectedIdx = 0; scheduleRedraw(); }}
        onkeydown={(e) => {
          if (e.key === 'Escape') { searchOpen = false; searchQuery = ''; scheduleRedraw(); }
          if (e.key === 'ArrowDown' && searchResults.length > 0) {
            e.preventDefault();
            const maxVisible = Math.min(searchResults.length, 20) - 1;
            searchSelectedIdx = Math.min(searchSelectedIdx + 1, maxVisible);
            const hit = searchResults[searchSelectedIdx];
            zoomToNode(hit.id);
            // Scroll selected item into view
            const items = containerEl?.querySelectorAll('.search-result-item');
            items?.[searchSelectedIdx]?.scrollIntoView({ block: 'nearest' });
          }
          if (e.key === 'ArrowUp' && searchResults.length > 0) {
            e.preventDefault();
            searchSelectedIdx = Math.max(searchSelectedIdx - 1, 0);
            const hit = searchResults[searchSelectedIdx];
            zoomToNode(hit.id);
            const items = containerEl?.querySelectorAll('.search-result-item');
            items?.[searchSelectedIdx]?.scrollIntoView({ block: 'nearest' });
          }
          if (e.key === 'Enter' && searchResults.length > 0) {
            const hit = searchResults[searchSelectedIdx];
            canvasState = { ...canvasState, selectedNode: { id: hit.id, name: hit.name, node_type: hit.node_type, qualified_name: hit.qualified_name } };
            onNodeDetail(hit);
            zoomToNode(hit.id);
            searchOpen = false; searchQuery = '';
          }
        }}
        aria-label="Search entities"
      />
      {#if searchResults.length > 0}
        <span class="canvas-search-count">{searchResults.length} matches</span>
      {/if}
      <button class="canvas-search-close" onclick={() => { searchOpen = false; searchQuery = ''; scheduleRedraw(); }} aria-label="Close search" type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>
    {#if searchResults.length > 0}
      <div class="canvas-search-results" role="listbox" aria-label="Search results" style="max-height: 320px; overflow-y: auto;">
        {#each searchResults.slice(0, 20) as result, idx}
          <button class="search-result-item" class:active={idx === searchSelectedIdx} role="option" aria-selected={idx === searchSelectedIdx}
            onclick={() => {
              canvasState = { ...canvasState, selectedNode: { id: result.id, name: result.name, node_type: result.node_type, qualified_name: result.qualified_name } };
              onNodeDetail(result);
              zoomToNode(result.id);
              searchOpen = false; searchQuery = '';
            }}
            type="button"
          >
            <span class="sr-type" style="color: {specBorderColor(result)}">{result.node_type}</span>
            <span class="sr-name">{result.name}</span>
            {#if result.file_path}<span class="sr-file">{result.file_path}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
  {/if}

  <!-- Multi-select action bar (concept creation from selection) -->
  {#if multiSelectedIds.size > 1}
    <div class="multi-select-bar">
      <span class="ms-count">{multiSelectedIds.size} selected</span>
      <button class="ms-action" onclick={() => {
        const seedNodes = [...multiSelectedIds].map(id => {
          const n = nodes.find(n => n.id === id);
          return n?.name ?? id;
        });
        onInteractiveQuery?.({
          scope: { type: 'concept', seed_nodes: seedNodes, expand_edges: ['calls', 'implements'], expand_depth: 2 },
          emphasis: { highlight: { matched: { color: '#a78bfa', label: 'Concept' } }, dim_unmatched: 0.15 },
          zoom: 'fit',
          annotation: { title: 'Ad-hoc concept', description: '{{count}} nodes from selection' },
        });
        multiSelectedIds = new Set();
        scheduleRedraw();
      }} type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
        Create Concept
      </button>
      <button class="ms-action" onclick={() => { multiSelectedIds = new Set(); scheduleRedraw(); }} type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        Clear
      </button>
    </div>
  {/if}

  <!-- Timeline scrubber -->
  {#if timelineEnabled}
    <div class="timeline-scrubber" role="group" aria-label="Architecture timeline">
      <button class="timeline-close" onclick={() => { timelineEnabled = false; scheduleRedraw(); }} title="Close timeline" type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
      <span class="timeline-label">
        {#if timelineNodes}
          {new Date(timelineNodes.fromT * 1000).toLocaleDateString()} — {new Date(timelineNodes.toT * 1000).toLocaleDateString()}
        {:else}
          Timeline
        {/if}
      </span>
      <div class="timeline-slider-track">
        <input
          type="range"
          class="timeline-slider"
          min="0" max="100"
          value={timelineRange[0]}
          oninput={(e) => { timelineRange = [Number(e.target.value), timelineRange[1]]; scheduleRedraw(); }}
          aria-label="Timeline start"
        />
        <input
          type="range"
          class="timeline-slider"
          min="0" max="100"
          value={timelineRange[1]}
          oninput={(e) => { timelineRange = [timelineRange[0], Number(e.target.value)]; scheduleRedraw(); }}
          aria-label="Timeline end"
        />
        {#if timelineNodes?.markers?.length}
          <div class="timeline-markers" aria-hidden="true">
            {#each timelineNodes.markers as marker}
              <div class="timeline-marker" style="left: {marker.pct}%" title={marker.label}>
                <div class="timeline-marker-line"></div>
                <div class="timeline-marker-label">{marker.label}</div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      {#if timelineNodes}
        <span class="timeline-count">
          Showing {timelineNodes.visibleIds.size} of {timelineNodes.totalWithTime} nodes
          {#if timelineNodes.ghostIds.size > 0}
            ({timelineNodes.ghostIds.size} ghosted)
          {/if}
        </span>
      {/if}
    </div>
    {#if timelineNodes && !timelineNodes.isFullRange && (timelineNodes.delta.added > 0 || timelineNodes.delta.removed > 0 || timelineNodes.delta.modified > 0)}
      <div class="timeline-delta-panel" role="status" aria-label="Timeline delta summary">
        <div class="timeline-delta-header">Delta vs. full range</div>
        <div class="timeline-delta-rows">
          {#if timelineNodes.delta.added > 0}
            <div class="timeline-delta-row">
              <span class="timeline-delta-badge delta-added">+{timelineNodes.delta.added}</span>
              <span class="timeline-delta-detail">
                {#each [...timelineNodes.delta.addedByType.entries()].sort((a,b) => b[1] - a[1]) as [typ, cnt]}
                  <span class="timeline-delta-type">{cnt} {typ}{cnt > 1 ? 's' : ''}</span>
                {/each}
              </span>
            </div>
          {/if}
          {#if timelineNodes.delta.removed > 0}
            <div class="timeline-delta-row">
              <span class="timeline-delta-badge delta-removed">-{timelineNodes.delta.removed}</span>
              <span class="timeline-delta-detail">
                {#each [...timelineNodes.delta.removedByType.entries()].sort((a,b) => b[1] - a[1]) as [typ, cnt]}
                  <span class="timeline-delta-type">{cnt} {typ}{cnt > 1 ? 's' : ''}</span>
                {/each}
              </span>
            </div>
          {/if}
          {#if timelineNodes.delta.modified > 0}
            <div class="timeline-delta-row">
              <span class="timeline-delta-badge delta-modified">~{timelineNodes.delta.modified}</span>
              <span class="timeline-delta-detail">modified</span>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {/if}

  <!-- Breadcrumb -->
  {#if breadcrumb.length > 0}
    <div class="treemap-breadcrumb" role="navigation" aria-label="Drill-down path">
      <button class="breadcrumb-item root" onclick={() => { navigateBreadcrumb(-1); }} type="button" aria-label="Go to root">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>
        Root
      </button>
      {#each breadcrumb as crumb, i}
        <span class="breadcrumb-sep" aria-hidden="true">&rsaquo;</span>
        <button class="breadcrumb-item" class:current={i === breadcrumb.length - 1} onclick={() => { navigateBreadcrumb(i); }} type="button">{crumb.name}</button>
      {/each}
    </div>
  {/if}

  <!-- Observable lens notice banner -->
  {#if observableBannerVisible}
    <div class="observable-banner" role="status" aria-live="polite">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="flex-shrink:0">
        <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
      </svg>
      <span>Observable lens requires an OpenTelemetry collector. Configure <code>GYRE_OTLP_ENDPOINT</code> to see live SLIs, error rates, and latency on the architecture canvas.</span>
      <button class="observable-banner-close" onclick={() => { observableBannerVisible = false; }} type="button" title="Dismiss">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>
  {/if}

  <!-- Evaluative trace playback overlay bar -->
  {#if lens === 'evaluative' && traceData?.spans?.length}
    <div class="trace-playback-bar" role="toolbar" aria-label="Trace playback controls">
      <button class="trace-pb-btn" onclick={() => { evalPlaying = !evalPlaying; if (evalPlaying) scheduleRedraw(); }} type="button" title={evalPlaying ? 'Pause trace playback' : 'Play trace playback'}>
        {#if evalPlaying}
          <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="currentColor" width="14" height="14"><polygon points="5,3 19,12 5,21"/></svg>
        {/if}
      </button>
      <input type="range" min="0" max="1000" value={Math.round(evalScrubber * 1000)}
        oninput={(e) => { evalScrubber = parseInt(e.target.value) / 1000; scheduleRedraw(); }}
        class="trace-pb-scrubber" title="Trace timeline position" />
      <span class="trace-pb-time">{traceElapsedDisplay} / {traceTotalDisplay}</span>
      <div class="trace-pb-sep"></div>
      <select class="trace-pb-speed" value={evalSpeed}
        onchange={(e) => { evalSpeed = parseFloat(e.target.value); }}>
        <option value="0.25">0.25x</option>
        <option value="0.5">0.5x</option>
        <option value="1">1x</option>
        <option value="2">2x</option>
        <option value="5">5x</option>
      </select>
      <span class="trace-pb-particles">{evalParticles.length} active spans</span>
    </div>
  {/if}

  <!-- Anomaly panel (evaluative lens) -->
  {#if (lens === 'structural' || lens === 'evaluative') && anomalies.length > 0 && anomalyPanelOpen}
    <div class="anomaly-panel" role="complementary" aria-label="Detected anomalies">
      <div class="anomaly-header">
        <span class="anomaly-title">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" style="vertical-align: -1px">
            <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>
          </svg>
          Anomalies ({anomalies.length})
        </span>
        <button class="anomaly-close" onclick={() => { anomalyPanelOpen = false; }} title="Dismiss" type="button">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      {#each anomalies as anomaly}
        <button
          class="anomaly-item anomaly-{anomaly.severity}"
          onclick={() => {
            const n = nodes.find(nd => nd.id === anomaly.nodeId);
            if (n) { selectedNodeId = n.id; onNodeDetail(n); }
          }}
          type="button"
        >
          <span class="anomaly-node-name">{anomaly.nodeName}</span>
          <span class="anomaly-message">{anomaly.message}</span>
        </button>
      {/each}
    </div>
  {/if}

  <!-- Screen reader live region for accessibility -->
  <div class="sr-only" aria-live="polite" aria-atomic="true">
    {srAnnouncement}
  </div>
</div>

<style>
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  .treemap-container { display: flex; flex-direction: column; height: 100%; overflow: hidden; background: #0f0f1a; position: relative; }

  .treemap-toolbar {
    display: flex; align-items: center; gap: 4px;
    padding: 6px 12px; background: rgba(15,15,26,0.95);
    border-bottom: 1px solid #1e293b; flex-shrink: 0; flex-wrap: wrap;
  }

  .filter-group, .lens-group { display: flex; gap: 2px; }

  .tb-btn {
    padding: 5px 14px; border: none; border-radius: 7px; font-size: 13px; font-weight: 500;
    cursor: pointer; background: transparent; color: #94a3b8; transition: all 0.15s;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .tb-btn:hover:not(:disabled) { background: rgba(51,65,85,0.5); color: #e2e8f0; }
  .tb-btn.active { background: #1e293b; color: #e2e8f0; box-shadow: 0 1px 4px rgba(0,0,0,0.3); }
  .tb-btn:disabled { opacity: 0.35; cursor: not-allowed; }
  .tb-btn-sm { font-size: 11px; padding: 4px 8px; }
  .tb-btn-disabled { opacity: 0.35; cursor: not-allowed; }
  .tb-btn-observable { opacity: 0.5; font-style: italic; }
  .tb-btn-observable:hover { opacity: 0.8; }
  .tb-btn-observable::after { content: ''; display: inline-block; width: 6px; height: 6px; background: #475569; border-radius: 50%; margin-left: 4px; vertical-align: middle; }
  .eval-metric-group { display: flex; gap: 2px; align-items: center; }
  .eval-label { font-size: 10px; color: #64748b; margin: 0 2px; white-space: nowrap; }
  .eval-playback { display: flex; gap: 4px; align-items: center; margin-left: 4px; }
  .eval-scrubber { width: 80px; accent-color: #ef4444; height: 4px; cursor: pointer; }
  .eval-speed { background: rgba(15,15,26,0.9); color: #94a3b8; border: 1px solid #334155; border-radius: 4px; padding: 2px 4px; font-size: 11px; font-family: 'SF Mono', Menlo, monospace; cursor: pointer; }
  .eval-particle-count { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; white-space: nowrap; }
  .eval-no-trace { font-size: 11px; color: #64748b; font-style: italic; margin-left: 4px; }

  /* Observable lens notice banner */
  .observable-banner {
    position: absolute; top: 12px; left: 50%; transform: translateX(-50%); z-index: 45;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px; background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
    font-size: 12px; color: #94a3b8; font-family: system-ui, -apple-system, sans-serif;
    animation: observable-fade-in 0.2s ease-out;
  }
  .observable-banner-close {
    display: flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; background: transparent; border: none;
    border-radius: 4px; color: #64748b; cursor: pointer; margin-left: 4px;
  }
  .observable-banner-close:hover { background: #1e293b; color: #e2e8f0; }
  @keyframes observable-fade-in { from { opacity: 0; transform: translateX(-50%) translateY(-8px); } to { opacity: 1; transform: translateX(-50%) translateY(0); } }

  /* Trace playback overlay bar */
  .trace-playback-bar {
    position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); z-index: 40;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px; background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
  }
  .trace-pb-btn {
    display: flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border: none; border-radius: 6px;
    background: #1e293b; color: #60a5fa; cursor: pointer;
    transition: all 0.15s;
  }
  .trace-pb-btn:hover { background: #334155; color: #93c5fd; }
  .trace-pb-scrubber { width: 160px; accent-color: #60a5fa; height: 4px; cursor: pointer; }
  .trace-pb-time {
    font-size: 11px; color: #e2e8f0; font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap; min-width: 100px;
  }
  .trace-pb-sep { width: 1px; height: 20px; background: #334155; }
  .trace-pb-speed {
    background: rgba(15, 15, 26, 0.9); color: #94a3b8; border: 1px solid #334155;
    border-radius: 4px; padding: 4px 6px; font-size: 11px;
    font-family: 'SF Mono', Menlo, monospace; cursor: pointer;
  }
  .trace-pb-particles { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; white-space: nowrap; }

  .tb-sep { width: 1px; height: 20px; background: #334155; margin: 0 4px; }
  .tb-sep-v { width: 1px; height: 16px; background: #475569; margin: 0 2px; display: inline-block; vertical-align: middle; }

  .treemap-legend { display: flex; align-items: center; gap: 12px; margin-left: auto; }
  .legend-item { display: flex; align-items: center; gap: 4px; }
  .legend-swatch { width: 10px; height: 10px; border-radius: 3px; flex-shrink: 0; }
  .legend-label { font-size: 11px; color: #94a3b8; }

  .zoom-ind {
    font-size: 12px; color: #64748b; font-family: 'SF Mono', Menlo, monospace;
    padding: 2px 8px; background: #1e293b; border-radius: 4px;
  }
  .treemap-stats { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; }

  .treemap-canvas-area { flex: 1; position: relative; overflow: hidden; min-height: 200px; }
  .treemap-canvas { display: block; width: 100%; height: 100%; touch-action: none; cursor: grab; }
  .treemap-canvas:focus-visible { outline: 2px solid #3b82f6; outline-offset: -2px; }

  .treemap-tooltip {
    position: fixed; z-index: 100; background: rgba(15,15,26,0.95); border: 1px solid #334155;
    border-radius: 8px; padding: 8px 12px; display: flex; flex-direction: column; gap: 3px;
    pointer-events: none; box-shadow: 0 8px 32px rgba(0,0,0,0.6); max-width: 360px;
    backdrop-filter: blur(12px);
  }
  .tooltip-type { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; }
  .tooltip-name { font-size: 14px; font-weight: 600; color: #f1f5f9; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-qname { font-size: 10px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-file { font-size: 10px; color: #475569; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-count, .tooltip-spec { font-size: 10px; color: #64748b; }
  .tooltip-eval { display: flex; flex-direction: column; gap: 2px; margin-top: 4px; border-top: 1px solid #1e293b; padding-top: 4px; }
  .tooltip-eval span { font-size: 10px; color: #94a3b8; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-test-badge { color: #22c55e !important; font-weight: 600; }
  .tooltip-insight { display: flex; flex-direction: column; gap: 1px; }
  .tooltip-insight span { font-size: 10px; }
  .insight-ok { color: #4ade80; }
  .insight-note { color: #fbbf24; }
  .insight-warn { color: #f97316; font-weight: 500; }
  .insight-risk { color: #ef4444; font-weight: 600; font-size: 11px !important; }
  .tooltip-span-stats { margin-top: 2px; border-top: 1px dashed #334155; padding-top: 2px; }
  .tooltip-span-stats span { display: inline-block; margin-right: 6px; font-size: 10px; color: #94a3b8; font-family: 'SF Mono', Menlo, monospace; }

  .treemap-minimap {
    position: absolute; bottom: 12px; right: 12px; border: 1px solid #334155;
    border-radius: 8px; overflow: hidden; background: #0f0f1a;
    box-shadow: 0 4px 16px rgba(0,0,0,0.5); opacity: 0.8; transition: opacity 0.15s;
    cursor: pointer;
  }
  .treemap-minimap:hover { opacity: 1; }

  .treemap-breadcrumb {
    display: flex; align-items: center; gap: 4px;
    padding: 6px 12px; border-top: 1px solid #1e293b;
    background: rgba(15,15,26,0.95); flex-shrink: 0; overflow-x: auto;
  }
  .breadcrumb-item {
    display: flex; align-items: center; gap: 4px;
    padding: 3px 10px; background: transparent; border: none; border-radius: 4px;
    color: #60a5fa; font-size: 12px; font-family: 'SF Mono', Menlo, monospace;
    cursor: pointer; transition: background 0.15s; white-space: nowrap;
  }
  .breadcrumb-item:hover { background: #1e293b; }
  .breadcrumb-item.current { color: #f1f5f9; font-weight: 600; }
  .breadcrumb-item.root { color: #94a3b8; }
  .breadcrumb-sep { color: #475569; font-size: 14px; user-select: none; }

  .query-annotation {
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 12px; background: #172554; border-bottom: 1px solid #1e3a5f; flex-shrink: 0;
  }
  .annotation-content { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .annotation-title { font-size: 13px; font-weight: 600; color: #e2e8f0; }
  .annotation-desc { font-size: 11px; color: #94a3b8; }
  .annotation-clear {
    display: flex; align-items: center; justify-content: center;
    width: 24px; height: 24px; background: transparent; border: none;
    border-radius: 4px; color: #94a3b8; cursor: pointer;
  }
  .annotation-clear:hover { background: #1e293b; color: #e2e8f0; }

  .annotation-interactive-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    border-radius: 4px;
    background: rgba(59, 130, 246, 0.2);
    color: #60a5fa;
    border: 1px solid rgba(59, 130, 246, 0.3);
    flex-shrink: 0;
  }

  /* Context menu */
  .ctx-menu-backdrop {
    position: absolute; inset: 0; z-index: 39;
  }
  .ctx-menu {
    position: absolute; z-index: 40; min-width: 200px;
    background: rgba(15, 15, 26, 0.97); border: 1px solid #334155;
    border-radius: 10px; padding: 4px; backdrop-filter: blur(16px);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
  }
  .ctx-menu-header {
    padding: 8px 12px 4px; display: flex; flex-direction: column; gap: 2px;
  }
  .ctx-node-type {
    font-size: 10px; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.5px; color: #64748b;
  }
  .ctx-node-name {
    font-size: 13px; font-weight: 600; color: #e2e8f0;
    font-family: 'SF Mono', Menlo, monospace;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ctx-sep {
    height: 1px; background: #1e293b; margin: 4px 8px;
  }
  .ctx-item {
    display: flex; align-items: center; gap: 10px;
    width: 100%; padding: 8px 12px; border: none; border-radius: 6px;
    background: transparent; color: #cbd5e1; font-size: 13px;
    cursor: pointer; transition: background 0.1s;
    font-family: system-ui, -apple-system, sans-serif;
    text-align: left;
  }
  .ctx-item:hover { background: #1e293b; color: #f1f5f9; }
  .ctx-item svg { flex-shrink: 0; color: #64748b; }
  .ctx-item:hover svg { color: #94a3b8; }

  /* Canvas search overlay */
  .canvas-search {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 12px; background: rgba(15,15,26,0.97);
    border-bottom: 1px solid #334155; flex-shrink: 0;
  }
  .canvas-search svg { color: #64748b; flex-shrink: 0; }
  .canvas-search-input {
    flex: 1; background: transparent; border: none; outline: none;
    color: #e2e8f0; font-size: 14px; font-family: 'SF Mono', Menlo, monospace;
  }
  .canvas-search-input::placeholder { color: #475569; }
  .canvas-search-count { font-size: 11px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; }
  .canvas-search-close {
    display: flex; align-items: center; justify-content: center;
    width: 24px; height: 24px; background: transparent; border: none;
    border-radius: 4px; color: #94a3b8; cursor: pointer;
  }
  .canvas-search-close:hover { background: #1e293b; color: #e2e8f0; }

  .canvas-search-results {
    position: absolute; top: auto; left: 12px; right: 12px; z-index: 50;
    background: rgba(15,15,26,0.97); border: 1px solid #334155;
    border-radius: 8px; padding: 4px; max-height: 280px; overflow-y: auto;
    backdrop-filter: blur(16px); box-shadow: 0 8px 32px rgba(0,0,0,0.6);
  }
  .search-result-item {
    display: flex; align-items: center; gap: 8px; width: 100%;
    padding: 8px 12px; border: none; border-radius: 6px; background: transparent;
    color: #cbd5e1; font-size: 13px; cursor: pointer; text-align: left;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .search-result-item:hover { background: #1e293b; color: #f1f5f9; }
  .sr-type { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; width: 60px; flex-shrink: 0; }
  .sr-name { font-weight: 600; font-family: 'SF Mono', Menlo, monospace; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sr-file { font-size: 10px; color: #475569; font-family: 'SF Mono', Menlo, monospace; margin-left: auto; flex-shrink: 0; }

  /* Multi-select action bar */
  .multi-select-bar {
    position: absolute; bottom: 56px; left: 50%; transform: translateX(-50%);
    display: flex; align-items: center; gap: 8px;
    background: rgba(15, 15, 26, 0.95); border: 1px solid rgba(100, 116, 139, 0.3);
    border-radius: 8px; padding: 6px 12px; z-index: 20;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }
  .ms-count {
    color: #a78bfa; font-size: 12px; font-weight: 600;
  }
  .ms-action {
    display: flex; align-items: center; gap: 4px;
    background: rgba(100, 116, 139, 0.15); border: 1px solid rgba(100, 116, 139, 0.25);
    color: #cbd5e1; border-radius: 6px; padding: 4px 10px;
    font-size: 11px; cursor: pointer; transition: background 0.15s;
  }
  .ms-action:hover { background: rgba(100, 116, 139, 0.3); color: #f1f5f9; }
  .ms-action svg { flex-shrink: 0; }

  /* Timeline scrubber */
  .timeline-scrubber {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 12px; background: rgba(15,15,26,0.95);
    border-top: 1px solid #1e293b; flex-shrink: 0;
  }
  .timeline-close {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; background: transparent; border: none;
    border-radius: 4px; color: #64748b; cursor: pointer;
  }
  .timeline-close:hover { background: #1e293b; color: #e2e8f0; }
  .timeline-label {
    font-size: 11px; color: #94a3b8; font-family: 'SF Mono', Menlo, monospace;
    min-width: 180px;
  }
  .timeline-slider-track {
    flex: 1; position: relative; display: flex; flex-direction: column; gap: 2px;
  }
  .timeline-slider {
    width: 100%; height: 4px; accent-color: #ef4444;
    appearance: auto; cursor: pointer;
  }
  .timeline-markers {
    position: absolute; top: -14px; left: 0; right: 0; height: 12px;
    pointer-events: none;
  }
  .timeline-marker {
    position: absolute; top: 0; transform: translateX(-50%);
  }
  .timeline-marker-line {
    width: 2px; height: 30px; background: #fbbf24; opacity: 0.6;
    margin: 0 auto;
  }
  .timeline-marker-label {
    font-size: 8px; color: #fbbf24; font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap; text-align: center; opacity: 0.8;
    max-width: 80px; overflow: hidden; text-overflow: ellipsis;
    pointer-events: auto;
  }
  .timeline-count {
    font-size: 10px; color: #64748b; font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap; min-width: 160px; text-align: right;
  }

  /* Timeline delta floating panel */
  .timeline-delta-panel {
    position: absolute; bottom: 52px; right: 12px;
    background: rgba(15,15,26,0.96); border: 1px solid #334155;
    border-radius: 6px; padding: 8px 10px; z-index: 30;
    box-shadow: 0 4px 12px rgba(0,0,0,0.4);
    font-family: 'SF Mono', Menlo, monospace; font-size: 11px;
    min-width: 160px; max-width: 280px;
  }
  .timeline-delta-header {
    color: #94a3b8; font-size: 10px; text-transform: uppercase;
    letter-spacing: 0.05em; margin-bottom: 6px;
  }
  .timeline-delta-rows { display: flex; flex-direction: column; gap: 4px; }
  .timeline-delta-row { display: flex; align-items: baseline; gap: 6px; }
  .timeline-delta-badge {
    font-weight: 700; font-size: 12px; min-width: 32px;
  }
  .delta-added { color: #4ade80; }
  .delta-removed { color: #f87171; }
  .delta-modified { color: #fbbf24; }
  .timeline-delta-detail {
    color: #94a3b8; font-size: 10px;
    display: flex; flex-wrap: wrap; gap: 4px;
  }
  .timeline-delta-type {
    background: rgba(148,163,184,0.1); border-radius: 3px;
    padding: 1px 4px;
  }

  /* Anomaly panel */
  .anomaly-panel {
    position: absolute; bottom: 12px; right: 12px; z-index: 40;
    width: 320px; max-height: 260px; overflow-y: auto;
    background: rgba(15, 15, 26, 0.95); border: 1px solid #334155;
    border-radius: 8px; box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
  }
  .anomaly-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 8px 12px; border-bottom: 1px solid #1e293b;
  }
  .anomaly-title {
    font-size: 11px; font-weight: 700; color: #fbbf24;
    font-family: system-ui, -apple-system, sans-serif;
    display: flex; align-items: center; gap: 6px;
  }
  .anomaly-close {
    display: flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; background: transparent; border: none;
    border-radius: 4px; color: #64748b; cursor: pointer;
  }
  .anomaly-close:hover { background: #1e293b; color: #e2e8f0; }
  .anomaly-item {
    display: flex; flex-direction: column; gap: 2px; width: 100%;
    padding: 8px 12px; border: none; background: transparent;
    cursor: pointer; text-align: left; border-bottom: 1px solid #1e293b;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .anomaly-item:last-child { border-bottom: none; }
  .anomaly-item:hover { background: #1e293b; }
  .anomaly-node-name {
    font-size: 11px; font-weight: 600; color: #e2e8f0;
    font-family: 'SF Mono', Menlo, monospace;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .anomaly-message {
    font-size: 10px; color: #94a3b8;
  }
  .anomaly-high .anomaly-node-name { color: #fca5a5; }
  .anomaly-high .anomaly-message { color: #ef4444; }
  .anomaly-medium .anomaly-node-name { color: #fde68a; }
  .anomaly-medium .anomaly-message { color: #eab308; }
  .anomaly-low .anomaly-node-name { color: #cbd5e1; }
  .anomaly-low .anomaly-message { color: #64748b; }

  /* Preview mode bar (ghost overlays) */
  .preview-mode-bar {
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 12px; background: linear-gradient(90deg, #172554, #1a2e05);
    border-bottom: 1px solid #1e3a5f; flex-shrink: 0;
  }
  .preview-mode-content {
    display: flex; align-items: center; gap: 8px;
  }
  .preview-pulse-dot {
    width: 8px; height: 8px; border-radius: 50%; background: #22c55e;
    animation: ghost-pulse 1.5s ease-in-out infinite;
  }
  .preview-mode-label {
    font-size: 13px; font-weight: 600; color: #86efac;
    font-family: system-ui, -apple-system, sans-serif;
  }
  .preview-mode-count {
    font-size: 11px; color: #64748b;
    font-family: 'SF Mono', Menlo, monospace;
  }
  .ghost-legend-chips {
    display: flex; align-items: center; gap: 6px;
  }
  .ghost-chip {
    font-size: 10px; font-weight: 600; padding: 2px 8px;
    border-radius: 4px; font-family: 'SF Mono', Menlo, monospace;
  }
  .ghost-chip-add {
    color: #22c55e; background: rgba(34, 197, 94, 0.15);
    border: 1px solid rgba(34, 197, 94, 0.3);
  }
  .ghost-chip-change {
    color: #eab308; background: rgba(234, 179, 8, 0.15);
    border: 1px solid rgba(234, 179, 8, 0.3);
  }
  .ghost-chip-remove {
    color: #ef4444; background: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.3);
  }
  .ghost-chip-conf {
    color: #94a3b8; background: rgba(148, 163, 184, 0.1);
    border: 1px solid rgba(148, 163, 184, 0.25);
    display: inline-flex; gap: 4px;
  }
  .conf-high { color: #22c55e; }
  .conf-med { color: #eab308; }
  .conf-low { color: #ef4444; }

  @keyframes ghost-pulse {
    0%, 100% { opacity: 0.4; transform: scale(0.8); }
    50% { opacity: 1; transform: scale(1.2); }
  }

  /* Concept creation bar (multi-select) */
  .concept-creation-bar {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 12px; background: rgba(167, 139, 250, 0.1);
    border: 1px solid rgba(167, 139, 250, 0.3); border-radius: 8px;
    margin: 6px 12px 0;
  }
  .concept-count { font-size: 12px; color: #a78bfa; font-weight: 600; white-space: nowrap; }
  .concept-create-btn {
    display: flex; align-items: center; gap: 4px;
    padding: 4px 12px; background: rgba(167, 139, 250, 0.2); border: 1px solid #a78bfa;
    border-radius: 6px; color: #e2e8f0; font-size: 12px; cursor: pointer; font-weight: 500;
    transition: all 0.15s;
  }
  .concept-create-btn:hover { background: rgba(167, 139, 250, 0.35); }
  .concept-clear-btn {
    padding: 3px 8px; background: transparent; border: 1px solid #475569;
    border-radius: 4px; color: #94a3b8; font-size: 11px; cursor: pointer;
  }
  .concept-clear-btn:hover { background: #1e293b; color: #e2e8f0; }
  .concept-hint { font-size: 10px; color: #64748b; font-style: italic; margin-left: auto; }

  @media (prefers-reduced-motion: reduce) {
    .tb-btn, .breadcrumb-item, .treemap-minimap { transition: none; }
    .preview-pulse-dot { animation: none; opacity: 1; }
  }
</style>
