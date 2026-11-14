<script>
  import { onDestroy, untrack } from 'svelte';
  import PlaybackControls from '../components/PlaybackControls.svelte';
  import { dispatchViewEvent, viewEventFromDom } from './viewEvents.js';
  import { layeredFlowLayout, flowEdgePath } from './flow-layout.js';
  import { ParticleLayer } from './particle-layer.js';
  import {
    SUCCESS_PARTICLE_COLOR,
    TRAIL_FADE_MS,
    rootSpanCases,
    spanRootMap,
    testCaseColor,
    particlesAtTime,
    nextSpanStart,
    spansTraversingEdge,
    nearestParticleToNode,
    trailSegments,
  } from './flow-particles.js';

  /**
   * FlowCanvas — the `"flow"` layout renderer (ui-layout.md §4 Flow Layout,
   * human-system-interface.md §3a Explorer Visualization).
   *
   * Architecture per spec: an SVG layer renders nodes and edges (clickable,
   * keyboard-accessible), and a Canvas overlay on top renders the animated
   * trace particles only — Canvas 2D up to 100 concurrent particles,
   * WebGL2 above that (particle-layer.js owns the switch). ExplorerCanvas
   * delegates to this component when `layout: "flow"` is active.
   *
   * Particle state is resolved exclusively through flow-particles.js
   * (particlesAtTime) so live playback, paused scrubbing, step-through and
   * the test-case filter all agree on positions for a given timestamp.
   */
  let {
    nodes = [],
    edges = [],
    traceData = null, // { spans: [], root_spans: [] } from GateTraceResponse
    lens = 'structural',
    playbackRequest = null, // { nonce } — start playback (generated flow views)
    highlightedSpanId = null, // span to highlight (evaluative tab click)
    focusRequest = null, // { nodeId, nonce } — pan camera to a node
    visibleNodeIds = null, // Set of node ids the active view query matched (flow layout scoping)
    onNodeDetail = () => {},
    onScopeDrill = null, // repo-boundary scope drill (delegated to the host)
  } = $props();

  function edgeSrc(e) { return e?.source_id ?? e?.from_node_id ?? e?.from; }
  function edgeTgt(e) { return e?.target_id ?? e?.to_node_id ?? e?.to; }
  function edgeType(e) { return (e?.edge_type ?? e?.type ?? '').toLowerCase(); }

  // ── Size / camera ────────────────────────────────────────────────────
  let rootEl = $state(null);
  let svgEl = $state(null);
  let particleContainerEl = $state(null);
  let W = $state(900);
  let H = $state(600);
  let cam = $state({ x: 0, y: 0, zoom: 1 });

  function worldToScreen(wx, wy) {
    return { x: (wx - cam.x) * cam.zoom + W / 2, y: (wy - cam.y) * cam.zoom + H / 2 };
  }
  function screenToWorld(sx, sy) {
    return { x: (sx - W / 2) / cam.zoom + cam.x, y: (sy - H / 2) / cam.zoom + cam.y };
  }

  let destroyed = false;
  onDestroy(() => { destroyed = true; });

  $effect(() => {
    const el = rootEl;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) {
        W = Math.round(width);
        H = Math.round(height);
      }
    });
    ro.observe(el);
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      W = Math.round(rect.width);
      H = Math.round(rect.height);
    }
    return () => ro.disconnect();
  });

  // ── Local drill scope (ui-layout.md §4: double-click drills to the next
  //    C4 level; contains-children scope filters this renderer's node set,
  //    repo-boundary drills delegate to the host shell) ─────────────────
  let scopeStack = $state([]); // [{id, name}]
  const scopeId = $derived(scopeStack.length ? scopeStack[scopeStack.length - 1].id : null);

  function childrenOf(nodeId) {
    const out = new Set();
    for (const e of edges ?? []) {
      if (edgeType(e) === 'contains' && edgeSrc(e) === nodeId) {
        const t = edgeTgt(e);
        if (t && t !== nodeId) out.add(t);
      }
    }
    return out;
  }

  let flowNodes = $derived.by(() => {
    // Active view query scope: a concept/filter query resolves to a node
    // set (ExplorerCanvas passes queryMatchedIds) — the flow layout shows
    // that set, the same "the scope filters the node set" contract the
    // registry renderers implement. Null/empty → full graph (unmatched
    // scopes show everything, mirroring the other renderers).
    let base = nodes;
    if (visibleNodeIds && visibleNodeIds.size > 0) {
      base = base.filter((n) => visibleNodeIds.has(n.id));
    }
    if (!scopeId) return base;
    const kids = childrenOf(scopeId);
    if (kids.size === 0) return base;
    return base.filter((n) => kids.has(n.id));
  });

  // ── Layout (flow-layout.js: depth columns along directed edges) ──────
  let layout = $derived.by(() => layeredFlowLayout(flowNodes, edges));
  let layoutMap = $derived(layout.items);
  let layoutList = $derived(layout.list);

  function fitView() {
    if (layoutList.length === 0 || W < 60 || H < 60) return;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const ln of layoutList) {
      minX = Math.min(minX, ln.x - ln.w / 2);
      maxX = Math.max(maxX, ln.x + ln.w / 2);
      minY = Math.min(minY, ln.y - ln.h / 2);
      maxY = Math.max(maxY, ln.y + ln.h / 2);
    }
    const bw = maxX - minX, bh = maxY - minY;
    const zoom = Math.max(0.15, Math.min(1.5, Math.min(W / (bw + 120), H / (bh + 120))));
    cam = { x: (minX + maxX) / 2, y: (minY + maxY) / 2, zoom };
  }

  // Fit when the node set changes (drill scope, view swap) or on first
  // valid measurement — not on plain resizes, which must not reset the
  // user's pan.
  let fittedOnce = false;
  let lastFitSignature = '';
  $effect(() => {
    const items = layoutList;
    const w = W, h = H;
    if (items.length === 0 || w < 60 || h < 60) return;
    const sig = items.length > 0
      ? `${items.length}:${items[0].id}:${items[items.length - 1].id}`
      : '';
    if (fittedOnce && sig === lastFitSignature) return;
    fittedOnce = true;
    lastFitSignature = sig;
    untrack(() => fitView());
  });

  // External camera focus request (detail-panel trace step click).
  $effect(() => {
    const req = focusRequest;
    if (!req?.nodeId) return;
    const ln = layoutMap.get(req.nodeId);
    if (!ln) return; // not laid out yet — retried when the layout changes
    untrack(() => {
      cam = { x: ln.x, y: ln.y, zoom: Math.max(cam.zoom, 1) };
    });
  });

  // ── Edge views (thickness ∝ call frequency in the trace when
  //    evaluative — explorer-canvas.md §5 Flow Traces) ──────────────────
  let edgeFrequency = $derived.by(() => {
    const freq = new Map();
    if (!traceData?.spans?.length) return freq;
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
  let maxEdgeFreq = $derived(Math.max(1, ...edgeFrequency.values()));

  let edgeViews = $derived.by(() => {
    const out = [];
    const seen = new Map(); // base id → occurrence count (parallel edges)
    for (const e of edges ?? []) {
      const srcId = edgeSrc(e);
      const tgtId = edgeTgt(e);
      if (!srcId || !tgtId || srcId === tgtId) continue;
      const s = layoutMap.get(srcId);
      const t = layoutMap.get(tgtId);
      if (!s || !t) continue;
      const type = edgeType(e);
      // Parallel edges (e.g. contains + calls between the same pair) must
      // not collide — the keyed each block would crash on duplicate keys.
      const base = `${type}:${srcId}->${tgtId}`;
      const n = (seen.get(base) ?? 0) + 1;
      seen.set(base, n);
      const freq = lens === 'evaluative' ? (edgeFrequency.get(`${srcId}->${tgtId}`) ?? 0) : 0;
      out.push({
        id: n === 1 ? base : `${base}#${n}`,
        srcId,
        tgtId,
        type,
        d: flowEdgePath(s, t),
        strokeWidth: type === 'contains' ? 0.8 : 1.1 + (freq / maxEdgeFreq) * 2.2,
        dashed: type === 'contains',
      });
    }
    return out;
  });

  // ── Per-node test-time statistics (ui-layout.md §4 Node badge metrics:
  //    span count, error rate, mean duration — computed from GateTrace
  //    spans, not production metrics) ───────────────────────────────────
  let nodeStats = $derived.by(() => {
    const m = new Map();
    if (!traceData?.spans?.length) return m;
    for (const s of traceData.spans) {
      const nid = s.graph_node_id;
      if (!nid) continue;
      if (!m.has(nid)) m.set(nid, { count: 0, errors: 0, totalUs: 0 });
      const st = m.get(nid);
      st.count++;
      st.totalUs += s.duration_us ?? 0;
      if ((s.status ?? '').toLowerCase() === 'error') st.errors++;
    }
    for (const [nid, st] of m) {
      m.set(nid, {
        spanCount: st.count,
        errorRate: st.count > 0 ? st.errors / st.count : 0,
        meanDuration: st.count > 0 ? st.totalUs / st.count : 0,
      });
    }
    return m;
  });

  function badgeChips(stats) {
    const meanUs = stats.meanDuration;
    const label = meanUs < 1000 ? `${Math.round(meanUs)}\u00B5s` : `${(meanUs / 1000).toFixed(1)}ms`;
    return [
      { label: `${stats.spanCount} spans`, bg: '#1e293b', fg: '#93c5fd' },
      { label: `${Math.round(stats.errorRate * 100)}% err`, bg: stats.errorRate > 0 ? '#7f1d1d' : '#1e293b', fg: stats.errorRate > 0 ? '#fca5a5' : '#94a3b8' },
      { label, bg: '#1e293b', fg: '#e2e8f0' },
    ].map((chip) => ({ ...chip, w: chip.label.length * 5.4 + 8 }));
  }

  // ── Particle overlay (Canvas 2D ≤100 concurrent, WebGL2 above — the
  //    switch lives in particle-layer.js). Declared BEFORE the playback
  //    effects: effects run in declaration order, and the paused-scrub
  //    effect below renders particles on its first run — the layer must
  //    already exist by then or the initial frame is dropped.
  let particleLayer = null;
  $effect(() => {
    const el = particleContainerEl;
    if (!el) return;
    particleLayer = new ParticleLayer(el, { className: 'flow-particle-canvas' });
    return () => {
      particleLayer?.destroy();
      particleLayer = null;
    };
  });

  // ── Playback state (mirrors ExplorerCanvas's evaluative machinery;
  //    resolution shared through flow-particles.js) ─────────────────────
  let playing = $state(false);
  let speed = $state(1.0); // clamped to [0.25, 10] — union of explorer-canvas
  //                        // §4 (0.25x–5x) and HSI §3a (1x/10x/step-through)
  let scrubber = $state(0); // 0..1 normalized time position
  let particles = $state([]);
  let testFilter = $state(null); // root span id, or null = all test cases

  let rootCases = $derived.by(() =>
    rootSpanCases(traceData?.spans).map((c) => ({
      ...c,
      color: testCaseColor(SUCCESS_PARTICLE_COLOR, c.index),
    }))
  );
  let spanRoots = $derived.by(() => spanRootMap(traceData?.spans));
  let caseIndexByRoot = $derived.by(() => {
    const m = new Map();
    for (const c of rootCases) m.set(c.spanId, c.index);
    return m;
  });

  // A new trace invalidates the selected test case — reset the filter
  // rather than animate nothing against a stale root span id.
  $effect(() => {
    const cases = rootCases;
    if (testFilter && !cases.some((c) => c.spanId === testFilter)) {
      testFilter = null;
    }
  });

  let timeline = $derived.by(() => {
    if (!traceData?.spans?.length || lens !== 'evaluative') return null;
    const spans = traceData.spans;
    const minTime = Math.min(...spans.map((s) => s.start_time ?? 0));
    const maxTime = Math.max(...spans.map((s) => (s.start_time ?? 0) + (s.duration_us || 0)));
    const totalDuration = maxTime - minTime || 1;
    return { minTime, maxTime, totalDuration, spans };
  });

  function computeParticlesAtTime(currentTime) {
    if (!timeline) return [];
    return particlesAtTime(timeline.spans, currentTime, {
      visibleNodeIds: layoutMap,
      testFilter,
      rootOf: spanRoots,
      caseIndexByRoot,
    });
  }

  // Particle trail position history (ui-layout.md §4 "Trail": opacity decay
  // over 200ms, 20px trail length). Keyed by span id; pruned by age inside
  // trailSegments on every render.
  const particleTrails = new Map(); // spanId → [{x, y, t}]
  function clearParticleTrails() {
    particleTrails.clear();
  }

  function advancePlayback(dt) {
    if (!timeline || !playing) return;
    const { minTime, totalDuration } = timeline;
    const traceSeconds = totalDuration / 1_000_000;
    const normalizedDuration = Math.max(0.5, Math.min(30.0, traceSeconds));
    const clampedSpeed = Math.max(0.25, Math.min(10.0, speed));
    const step = Math.max(0, dt / 1000) * clampedSpeed / normalizedDuration;
    scrubber = Math.min(1, Math.max(0, scrubber + step));
    if (scrubber >= 1) {
      playing = false; // single-play mode
      scrubber = 1;
    }
    particles = computeParticlesAtTime(minTime + scrubber * totalDuration);
  }

  // Paused interaction must still reposition particles: dragging the
  // scrubber, changing the test filter, or stepping while paused all
  // resolve particles at the (new) scrubber timestamp (ui-layout.md §4
  // "All particles freeze at their positions at that timestamp").
  $effect(() => {
    if (lens !== 'evaluative' || playing) return;
    const tl = timeline;
    if (!tl) {
      if (particles.length > 0) particles = [];
      return;
    }
    particles = computeParticlesAtTime(tl.minTime + scrubber * tl.totalDuration);
    untrack(() => renderParticles());
  });

  function startPlayback() {
    scrubber = 0;
    playing = true;
    clearParticleTrails();
    scheduleFrame();
  }

  /** HSI §3a step-through: advance to the next filtered span start. */
  function stepPlayback() {
    const tl = timeline;
    if (!tl) return;
    playing = false;
    const currentTime = tl.minTime + scrubber * tl.totalDuration;
    const next = nextSpanStart(tl.spans, currentTime, spanRoots, testFilter);
    scrubber = next == null ? 1 : (next - tl.minTime) / tl.totalDuration;
    clearParticleTrails();
    renderParticles();
  }

  // External playback request (HSI §3 generated "flow" views): nonce-based
  // so a repeated request for the same view re-triggers.
  $effect(() => {
    const req = playbackRequest;
    if (!req?.nonce) return;
    if ((traceData?.spans?.length ?? 0) === 0) return;
    untrack(() => startPlayback());
  });

  let traceElapsedDisplay = $derived.by(() => {
    if (!timeline) return '';
    const elapsed = scrubber * timeline.totalDuration;
    if (elapsed < 1000) return `${Math.round(elapsed)}\u00B5s`;
    if (elapsed < 1000000) return `${(elapsed / 1000).toFixed(1)}ms`;
    return `${(elapsed / 1000000).toFixed(2)}s`;
  });
  let traceTotalDisplay = $derived.by(() => {
    if (!timeline) return '';
    const total = timeline.totalDuration;
    if (total < 1000) return `${Math.round(total)}\u00B5s`;
    if (total < 1000000) return `${(total / 1000).toFixed(1)}ms`;
    return `${(total / 1000000).toFixed(2)}s`;
  });

  function renderParticles() {
    if (destroyed || !particleLayer) return;
    if (lens !== 'evaluative' || !timeline) {
      clearParticleTrails();
      particleLayer.clear();
      return;
    }
    const now = performance.now();
    const drawables = [];
    for (const p of particles) {
      const sln = layoutMap.get(p.fromId);
      const tln = layoutMap.get(p.toId);
      if (!sln || !tln) continue;
      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);
      const px = ss.x + (ts.x - ss.x) * p.progress;
      const py = ss.y + (ts.y - ss.y) * p.progress;
      if (px < -20 || px > W + 20 || py < -20 || py > H + 20) continue;

      const isHighlighted = highlightedSpanId && p.span?.span_id === highlightedSpanId;
      const isHovered = hoveredSpanId && p.span?.span_id === hoveredSpanId;
      // ui-layout.md §4: 4px particles scale up on hover (base 4 → 6).
      const radius = isHighlighted ? 8 + cam.zoom * 0.8
        : isHovered ? 6 + cam.zoom * 0.5
        : 4 + cam.zoom * 0.5;

      let history = particleTrails.get(p.span.span_id);
      if (!history) {
        history = [];
        particleTrails.set(p.span.span_id, history);
      }
      history.push({ x: px, y: py, t: now });
      const trail = trailSegments(history, now);

      drawables.push({
        x: px,
        y: py,
        radius,
        color: isHighlighted ? '#f59e0b' : p.color,
        alpha: isHighlighted ? 1.0 : 0.8,
        glowColor: isHighlighted ? '#f59e0b' : p.glow,
        glowBlur: isHighlighted ? 20 : 10,
        highlight: isHighlighted,
        trail,
      });
    }
    // Drop history for spans no longer animating.
    for (const [spanId, hist] of particleTrails) {
      if (hist.length === 0 || now - hist[hist.length - 1].t > TRAIL_FADE_MS) {
        particleTrails.delete(spanId);
      }
    }
    particleLayer.render(drawables, { width: W, height: H, dpr: window.devicePixelRatio || 1 });
  }

  // ── Render loop (guarded scheduling — see ExplorerCanvas's animLoop for
  //    the sync-rAF hazards this pattern avoids) ────────────────────────
  let animScheduled = false;
  let syncFrameDepth = 0;
  const SYNC_FRAME_MAX = 100;
  let lastTs = 0;

  function stopLoop() {
    animScheduled = false;
  }
  function runFrame(ts) {
    untrack(() => animLoop(ts));
  }
  function animLoop(ts) {
    if (destroyed) { stopLoop(); return; }
    const dt = lastTs ? Math.max(0, ts - lastTs) : 16;
    lastTs = ts;
    advancePlayback(dt);
    renderParticles();
    // Keep animating while playing or while any trail segment is still
    // inside its 200ms fade window (a paused trace's trails must decay,
    // not freeze mid-fade).
    const trailsAlive = [...particleTrails.values()].some(
      (h) => h.length > 0 && performance.now() - h[h.length - 1].t <= TRAIL_FADE_MS
    );
    if ((playing && timeline) || trailsAlive) {
      if (syncFrameDepth < SYNC_FRAME_MAX) {
        syncFrameDepth++;
        try {
          requestAnimationFrame(runFrame);
        } finally {
          syncFrameDepth--;
        }
      } else {
        stopLoop();
      }
    } else {
      stopLoop();
    }
  }
  function scheduleFrame() {
    if (destroyed || animScheduled) return;
    animScheduled = true;
    requestAnimationFrame(runFrame);
  }
  $effect(() => {
    if (destroyed) return;
    untrack(() => scheduleFrame());
    return () => { destroyed = true; };
  });

  // ── Interaction state ────────────────────────────────────────────────
  let isPanning = $state(false);
  let panStart = { x: 0, y: 0 };
  let panCamStart = { x: 0, y: 0 };
  let selectedNodeId = $state(null);
  let hoveredSpanId = $state(null);
  let tooltip = $state(null); // { x, y, kind: 'node'|'span', node }

  function localPoint(e) {
    const rect = rootEl?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  function particleHitTest(clientX, clientY) {
    if (lens !== 'evaluative' || particles.length === 0) return null;
    const rect = svgEl?.getBoundingClientRect();
    if (!rect) return null;
    const sx = clientX - rect.left;
    const sy = clientY - rect.top;
    const threshold = 12;
    for (const p of particles) {
      const sln = layoutMap.get(p.fromId);
      const tln = layoutMap.get(p.toId);
      if (!sln || !tln) continue;
      const ss = worldToScreen(sln.x, sln.y);
      const ts = worldToScreen(tln.x, tln.y);
      const px = ss.x + (ts.x - ss.x) * p.progress;
      const py = ss.y + (ts.y - ss.y) * p.progress;
      if (Math.hypot(sx - px, sy - py) < threshold) return p;
    }
    return null;
  }

  function spanTooltipNode(span) {
    return {
      id: `span-${span.span_id}`,
      name: span.operation_name ?? span.span_id,
      node_type: 'span',
      span_id: span.span_id,
      duration_us: span.duration_us,
      status: span.status,
      input_summary: span.input_summary,
      output_summary: span.output_summary,
    };
  }

  function emitFlowViewEvent(domEvent, entityType, entityId, data) {
    if (!domEvent) return;
    const ev = viewEventFromDom(domEvent, entityType, entityId, data);
    ev.handled = true;
    dispatchViewEvent(rootEl, ev);
  }

  function onMouseDown(e) {
    if (e.button !== 0) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    panCamStart = { x: cam.x, y: cam.y };
    e.preventDefault();
  }

  function onMouseMove(e) {
    if (isPanning) {
      const dx = e.clientX - panStart.x;
      const dy = e.clientY - panStart.y;
      cam = { ...cam, x: panCamStart.x - dx / cam.zoom, y: panCamStart.y - dy / cam.zoom };
      renderParticles();
      return;
    }
    const nodeG = e.target?.closest?.('[data-flow-node]');
    // Particle hover first (HSI §3a: hovering a span during playback shows
    // the input/output summary; ui-layout.md §4: the particle scales up).
    if (!nodeG && lens === 'evaluative') {
      const p = particleHitTest(e.clientX, e.clientY);
      const newSpanId = p?.span?.span_id ?? null;
      if (newSpanId !== hoveredSpanId) {
        hoveredSpanId = newSpanId;
        tooltip = p ? { ...localPoint(e), node: spanTooltipNode(p.span) } : null;
        renderParticles();
      } else if (p) {
        tooltip = { ...localPoint(e), node: spanTooltipNode(p.span) };
      }
      if (p) {
        if (svgEl) svgEl.style.cursor = 'pointer';
        return;
      }
    } else if (hoveredSpanId) {
      hoveredSpanId = null;
      renderParticles();
    }
    if (nodeG) {
      const ln = layoutMap.get(nodeG.dataset.flowNode);
      if (ln) {
        tooltip = { ...localPoint(e), node: ln.node };
        if (svgEl) svgEl.style.cursor = 'pointer';
        return;
      }
    }
    tooltip = null;
    if (svgEl) svgEl.style.cursor = 'grab';
  }

  function onMouseUp() {
    isPanning = false;
  }
  function onMouseLeave() {
    isPanning = false;
    hoveredSpanId = null;
    tooltip = null;
    renderParticles();
  }

  function onWheel(e) {
    e.preventDefault();
    const rect = svgEl?.getBoundingClientRect();
    if (!rect) return;
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const oldZoom = cam.zoom;
    const newZoom = Math.max(0.15, Math.min(4, oldZoom * Math.exp(-e.deltaY * 0.0012)));
    if (newZoom === oldZoom) return;
    const world = screenToWorld(sx, sy);
    cam = {
      x: world.x - (sx - W / 2) / newZoom,
      y: world.y - (sy - H / 2) / newZoom,
      zoom: newZoom,
    };
    renderParticles();
  }

  function onClick(e) {
    if (Math.abs(e.clientX - panStart.x) > 4 || Math.abs(e.clientY - panStart.y) > 4) return;

    // 1. Particle click (ui-layout.md §4: pauses playback, shows the full
    //    span detail — operation name, I/O summary, duration, status).
    if (lens === 'evaluative') {
      const p = particleHitTest(e.clientX, e.clientY);
      if (p) {
        if (playing) playing = false;
        const span = p.span;
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
        emitFlowViewEvent(e, 'node', `span-${span.span_id}`, span);
        return;
      }
    }

    // 2. Node click — pauses playback during animation and opens the detail
    //    panel with the span data for the particle at/nearest this node.
    const nodeG = e.target?.closest?.('[data-flow-node]');
    if (nodeG) {
      const ln = layoutMap.get(nodeG.dataset.flowNode);
      if (!ln) return;
      const node = ln.node;
      if (lens === 'evaluative' && playing && nodeStats.has(node.id)) {
        playing = false;
      }
      selectedNodeId = node.id;
      const enriched = { ...node };
      if (lens === 'evaluative') {
        const np = nearestParticleToNode(particles, node.id);
        if (np) {
          enriched._activeSpan = {
            span_id: np.span.span_id,
            operation_name: np.span.operation_name,
            duration_us: np.span.duration_us,
            status: np.span.status,
            input_summary: np.span.input_summary,
            output_summary: np.span.output_summary,
            progress: np.progress,
          };
        }
      }
      onNodeDetail(enriched);
      emitFlowViewEvent(e, 'node', node.id, enriched);
      return;
    }

    // 3. Edge click (ui-layout.md §4: shows all spans that traversed this
    //    edge with their timings).
    const edgeEl = e.target?.closest?.('[data-flow-edge]');
    if (edgeEl) {
      const srcId = edgeEl.dataset.src;
      const tgtId = edgeEl.dataset.tgt;
      const srcNode = nodes.find((n) => n.id === srcId);
      const tgtNode = nodes.find((n) => n.id === tgtId);
      const edgeInfo = {
        id: `edge-${srcId}-${tgtId}`,
        name: `${srcNode?.name ?? '?'} → ${tgtNode?.name ?? '?'}`,
        node_type: 'edge',
        edge_type: edgeEl.dataset.type ?? '',
        source_node: srcNode,
        target_node: tgtNode,
        file_path: srcNode?.file_path,
        doc_comment: `${(edgeEl.dataset.type ?? 'relationship').replace('_', ' ')} relationship from ${srcNode?.qualified_name ?? srcNode?.name ?? '?'} to ${tgtNode?.qualified_name ?? tgtNode?.name ?? '?'}`,
      };
      if (lens === 'evaluative' && traceData?.spans?.length) {
        const traversing = spansTraversingEdge(traceData.spans, srcId, tgtId);
        if (traversing.length > 0) edgeInfo.traversing_spans = traversing;
      }
      onNodeDetail(edgeInfo);
      emitFlowViewEvent(e, 'edge', `${srcId}->${tgtId}`, edgeInfo);
      return;
    }

    // 4. Background — dismiss the tooltip.
    tooltip = null;
  }

  function onDblClick(e) {
    const nodeG = e.target?.closest?.('[data-flow-node]');
    if (!nodeG) return;
    const ln = layoutMap.get(nodeG.dataset.flowNode);
    if (!ln) return;
    const node = ln.node;
    emitFlowViewEvent(e, 'node', node.id, node);
    // Scope drill (ui-layout.md §3): a node carrying repo_id with no
    // in-graph children drills across the repo boundary — the host shell
    // owns that scope change.
    if (onScopeDrill && node.repo_id && childrenOf(node.id).size === 0) {
      onScopeDrill(node);
      return;
    }
    // In-graph drill (§4 "Double-click node: drills down to the next C4
    // level"): scope this renderer to the node's contains-children.
    const kids = childrenOf(node.id);
    if (kids.size > 0) {
      scopeStack = [...scopeStack, { id: node.id, name: node.name ?? node.qualified_name ?? node.id }];
    }
  }

  function onNodeKeydown(e, ln) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    const node = ln.node;
    if (lens === 'evaluative' && playing && nodeStats.has(node.id)) {
      playing = false;
    }
    selectedNodeId = node.id;
    onNodeDetail(node);
  }

  function navigateScope(index) {
    scopeStack = index < 0 ? [] : scopeStack.slice(0, index + 1);
  }

  const TYPE_COLORS = {
    package: '#8b5cf6', module: '#8b5cf6', namespace: '#8b5cf6',
    function: '#3b82f6', method: '#3b82f6',
    endpoint: '#10b981', handler: '#10b981', service: '#06b6d4',
    type: '#f59e0b', struct: '#f59e0b', trait: '#f59e0b', class: '#f59e0b', interface: '#f59e0b',
    test: '#22c55e', file: '#64748b',
  };
  function typeColor(node) {
    return TYPE_COLORS[node?.node_type] ?? '#64748b';
  }

  let svgTransform = $derived(
    `translate(${W / 2 - cam.x * cam.zoom} ${H / 2 - cam.y * cam.zoom}) scale(${cam.zoom})`
  );
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape') { tooltip = null; } }} />

<div class="flow-canvas" bind:this={rootEl}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <svg
    bind:this={svgEl}
    class="flow-svg"
    width={W}
    height={H}
    role="application"
    aria-label="Flow layout graph (animated trace particles)"
    onmousedown={onMouseDown}
    onmousemove={onMouseMove}
    onmouseup={onMouseUp}
    onmouseleave={onMouseLeave}
    onwheel={onWheel}
    onclick={onClick}
    ondblclick={onDblClick}
  >
    <defs>
      <marker id="flow-arrow" viewBox="0 0 10 10" refX="9" refY="5"
        markerWidth="6" markerHeight="6" orient="auto-start-reverse">
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#475569"></path>
      </marker>
    </defs>
    <g transform={svgTransform}>
      {#each edgeViews as ev (ev.id)}
        <path
          class="flow-edge"
          class:flow-edge-contains={ev.dashed}
          d={ev.d}
          data-flow-edge={ev.id}
          data-src={ev.srcId}
          data-tgt={ev.tgtId}
          data-type={ev.type}
          stroke-width={ev.strokeWidth}
          marker-end="url(#flow-arrow)"
        ><title>{ev.type} edge</title></path>
      {/each}
      {#each layoutList as ln (ln.id)}
        <!-- svelte-ignore a11y_interactive_supports_focus, a11y_no_noninteractive_element_interactions -->
        <g
          class="flow-node"
          class:flow-node-selected={selectedNodeId === ln.id}
          class:flow-node-error={lens === 'evaluative' && (nodeStats.get(ln.id)?.errorRate ?? 0) > 0}
          data-flow-node={ln.id}
          role="button"
          tabindex="0"
          aria-label={`${ln.node.name ?? ln.id} (${ln.node.node_type ?? 'node'})`}
          transform={`translate(${ln.x - ln.w / 2} ${ln.y - ln.h / 2})`}
          onkeydown={(e) => onNodeKeydown(e, ln)}
        >
          <rect class="flow-node-rect" width={ln.w} height={ln.h} rx="6"
            stroke={typeColor(ln.node)}></rect>
          <text class="flow-node-label" x="8" y="19">{ln.node.name ?? ln.id}</text>
          {#if lens === 'evaluative' && nodeStats.get(ln.id)}
            {@const chips = badgeChips(nodeStats.get(ln.id))}
            {@const totalW = chips.reduce((a, c) => a + c.w + 2, 0)}
            {#each chips as chip, i (chip.label)}
              {@const chipX = ln.w - totalW + chips.slice(0, i).reduce((a, c) => a + c.w + 2, 0)}
              <g class="flow-badge" transform={`translate(${chipX} -14)`} aria-hidden="true">
                <rect width={chip.w} height="12" rx="3" fill={chip.bg}></rect>
                <text class="flow-badge-text" x={chip.w / 2} y="9">{chip.label}</text>
              </g>
            {/each}
          {/if}
        </g>
      {/each}
    </g>
  </svg>

  <!-- Particle overlay — Canvas 2D / WebGL2 (particles only, not
       interactive, per ui-layout.md §4 "Rendering"). -->
  <div class="flow-particles" bind:this={particleContainerEl} aria-hidden="true"></div>

  {#if scopeStack.length > 0}
    <div class="flow-scope-bar" role="navigation" aria-label="Flow drill scope">
      <button class="flow-scope-crumb" onclick={() => navigateScope(-1)} type="button">All</button>
      {#each scopeStack as crumb, i (crumb.id)}
        <span class="flow-scope-sep">›</span>
        <button class="flow-scope-crumb" class:flow-scope-current={i === scopeStack.length - 1}
          onclick={() => navigateScope(i)} type="button">{crumb.name}</button>
      {/each}
    </div>
  {/if}

  {#if tooltip && tooltip.node}
    <div class="flow-tooltip" role="tooltip" style="left: {tooltip.x + 14}px; top: {tooltip.y - 50}px">
      <span class="tooltip-type" style="color: {typeColor(tooltip.node)}">{tooltip.node.node_type}</span>
      <span class="tooltip-name">{tooltip.node.name}</span>
      {#if tooltip.node.qualified_name && tooltip.node.qualified_name !== tooltip.node.name}
        <span class="tooltip-qname">{tooltip.node.qualified_name}</span>
      {/if}
      {#if tooltip.node.file_path}
        <span class="tooltip-file">{tooltip.node.file_path}{tooltip.node.line_start ? `:${tooltip.node.line_start}` : ''}</span>
      {/if}
      {#if tooltip.node.node_type === 'span' && tooltip.node.span_id}
        <div class="tooltip-span-io">
          <span class="tooltip-span-meta">
            {tooltip.node.duration_us < 1000 ? `${Math.round(tooltip.node.duration_us)}\u00B5s` : `${(tooltip.node.duration_us / 1000).toFixed(1)}ms`}
            · {tooltip.node.status ?? 'ok'}
          </span>
          {#if tooltip.node.input_summary}
            <span class="tooltip-span-line"><span class="tooltip-span-k">in</span> {tooltip.node.input_summary}</span>
          {/if}
          {#if tooltip.node.output_summary}
            <span class="tooltip-span-line"><span class="tooltip-span-k">out</span> {tooltip.node.output_summary}</span>
          {/if}
        </div>
      {/if}
      {#if tooltip.node.node_type !== 'span' && nodeStats.get(tooltip.node.id)}
        {@const st = nodeStats.get(tooltip.node.id)}
        <div class="tooltip-stats">
          <span>{st.spanCount} spans</span>
          <span>{Math.round(st.errorRate * 100)}% errors</span>
          <span>mean {st.meanDuration < 1000 ? `${Math.round(st.meanDuration)}\u00B5s` : `${(st.meanDuration / 1000).toFixed(1)}ms`}</span>
        </div>
      {/if}
    </div>
  {/if}

  {#if lens === 'evaluative' && traceData?.spans?.length}
    <PlaybackControls
      evalPlaying={playing}
      evalScrubber={scrubber}
      evalSpeed={speed}
      evalParticles={particles}
      {traceData}
      {traceElapsedDisplay}
      {traceTotalDisplay}
      rootCases={rootCases}
      testFilter={testFilter}
      onPlayToggle={() => {
        playing = !playing;
        if (playing) scheduleFrame();
      }}
      onScrubberChange={(v) => { scrubber = v; clearParticleTrails(); renderParticles(); }}
      onSpeedChange={(v) => { speed = v; }}
      onStep={() => { stepPlayback(); }}
      onTestFilterChange={(id) => { testFilter = id; clearParticleTrails(); renderParticles(); }}
    />
  {/if}
</div>

<style>
  .flow-canvas {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 200px;
    background: #0f0f1a;
    overflow: hidden;
  }
  .flow-svg {
    display: block;
    width: 100%;
    height: 100%;
    cursor: grab;
    touch-action: none;
  }
  .flow-particles {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 2;
  }
  .flow-edge {
    fill: none;
    stroke: #334155;
    opacity: 0.7;
  }
  .flow-edge:hover {
    stroke: #64748b;
    opacity: 1;
  }
  .flow-edge-contains {
    stroke-dasharray: 4 4;
    opacity: 0.35;
    marker-end: none;
  }
  .flow-node { cursor: pointer; }
  .flow-node-rect {
    fill: #1e293b;
    stroke-width: 1.2;
  }
  .flow-node:hover .flow-node-rect { fill: #263449; }
  .flow-node-selected .flow-node-rect {
    stroke: #60a5fa;
    stroke-width: 2;
    filter: drop-shadow(0 0 4px rgba(96, 165, 250, 0.5));
  }
  .flow-node-error .flow-node-rect {
    stroke: #ef4444;
  }
  .flow-node-label {
    fill: #e2e8f0;
    font-size: 11px;
    font-family: 'SF Mono', Menlo, monospace;
    dominant-baseline: middle;
  }
  .flow-badge-text {
    fill: #cbd5e1;
    font-size: 8px;
    font-family: 'SF Mono', Menlo, monospace;
    text-anchor: middle;
  }
  .flow-scope-bar {
    position: absolute;
    top: 10px; left: 12px; z-index: 10;
    display: flex; align-items: center; gap: 4px;
    padding: 4px 8px;
    background: rgba(15, 15, 26, 0.9);
    border: 1px solid #1e293b;
    border-radius: 6px;
  }
  .flow-scope-crumb {
    background: none; border: none; padding: 2px 4px; cursor: pointer;
    color: #94a3b8; font-size: 11px; font-family: 'SF Mono', Menlo, monospace;
    border-radius: 4px;
  }
  .flow-scope-crumb:hover { background: #1e293b; color: #e2e8f0; }
  .flow-scope-current { color: #e2e8f0; font-weight: 600; }
  .flow-scope-sep { color: #475569; font-size: 11px; }
  .flow-tooltip {
    position: absolute;
    z-index: 20;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-width: 280px;
    padding: 8px 10px;
    background: rgba(15, 15, 26, 0.97);
    border: 1px solid #334155;
    border-radius: 8px;
    pointer-events: none;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
  }
  .tooltip-type { font-size: 10px; text-transform: uppercase; letter-spacing: 0.5px; }
  .tooltip-name { font-size: 13px; font-weight: 600; color: #e2e8f0; }
  .tooltip-qname { font-size: 10px; color: #94a3b8; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-file { font-size: 10px; color: #64748b; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-span-io { display: flex; flex-direction: column; gap: 2px; margin-top: 4px; }
  .tooltip-span-meta { font-size: 11px; color: #93c5fd; font-family: 'SF Mono', Menlo, monospace; }
  .tooltip-span-line { font-size: 10px; color: #94a3b8; }
  .tooltip-span-k {
    display: inline-block; margin-right: 4px; padding: 0 3px;
    background: #1e293b; border-radius: 3px; color: #60a5fa;
    font-family: 'SF Mono', Menlo, monospace; font-size: 9px;
  }
  .tooltip-stats {
    display: flex; gap: 8px; margin-top: 4px;
    font-size: 10px; color: #94a3b8; font-family: 'SF Mono', Menlo, monospace;
  }

  @media (prefers-reduced-motion: reduce) {
    .flow-node-selected .flow-node-rect { filter: none; }
  }
</style>
