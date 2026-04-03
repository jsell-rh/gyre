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
  } = $props();

  // ── Constants ────────────────────────────────────────────────────────────
  const NODE_COLORS = {
    package:   { fill: '#475569', stroke: '#94a3b8', label: 'Package' },
    module:    { fill: '#3b82f6', stroke: '#60a5fa', label: 'Module' },
    type:      { fill: '#10b981', stroke: '#34d399', label: 'Type' },
    interface: { fill: '#8b5cf6', stroke: '#a78bfa', label: 'Interface' },
    function:  { fill: '#f59e0b', stroke: '#fbbf24', label: 'Function' },
    endpoint:  { fill: '#f43f5e', stroke: '#fb7185', label: 'Endpoint' },
    component: { fill: '#6366f1', stroke: '#818cf8', label: 'Component' },
    table:     { fill: '#6b7280', stroke: '#9ca3af', label: 'Table' },
    constant:  { fill: '#d97706', stroke: '#fbbf24', label: 'Constant' },
  };
  const DEFAULT_COLOR = { fill: '#334155', stroke: '#64748b', label: 'Other' };

  const ZOOM_THRESHOLDS = { low: 0.4, medium: 1.0 };
  const NODE_PAD = 6;
  const GROUP_PAD = 16;
  const GROUP_HEADER = 24;
  const MIN_NODE_W = 80;
  const MIN_NODE_H = 32;
  const MINIMAP_W = 160;
  const MINIMAP_H = 100;

  // ── Reactive state ─────────────────────────────────────────────────────
  let canvasEl = $state(null);
  let minimapEl = $state(null);
  let containerEl = $state(null);
  let canvasW = $state(900);
  let canvasH = $state(600);
  let offsetX = $state(0);
  let offsetY = $state(0);
  let zoom = $state(1);
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });
  let panOffset = $state({ x: 0, y: 0 });
  let hoveredNodeId = $state(null);
  let selectedNodeId = $state(null);
  let breadcrumb = $state([]); // [{id, name, type}]
  let animFrame = null;
  let needsRedraw = $state(true);

  // ── Computed: parent map from Contains edges ──────────────────────────
  let parentMap = $derived.by(() => {
    const m = new Map();
    for (const e of edges) {
      const etype = e.edge_type ?? e.type ?? '';
      if (etype.toLowerCase() === 'contains') {
        const child = e.target_id ?? e.to_node_id ?? e.to;
        const parent = e.source_id ?? e.from_node_id ?? e.from;
        if (child && parent) m.set(child, parent);
      }
    }
    return m;
  });

  // ── Computed: filtered nodes ──────────────────────────────────────────
  let filteredNodes = $derived.by(() => {
    let result = nodes;
    if (filter === 'endpoints') result = result.filter(n => n.node_type === 'endpoint');
    else if (filter === 'types') result = result.filter(n => n.node_type === 'type' || n.node_type === 'interface');
    else if (filter === 'calls') result = result.filter(n => n.node_type === 'function' || n.node_type === 'endpoint');
    else if (filter === 'dependencies') {
      const depEdges = edges.filter(e => (e.edge_type ?? '').toLowerCase() === 'depends_on');
      const depIds = new Set();
      for (const e of depEdges) {
        depIds.add(e.source_id ?? e.from_node_id ?? e.from);
        depIds.add(e.target_id ?? e.to_node_id ?? e.to);
      }
      result = result.filter(n => depIds.has(n.id));
    }

    // Drill-down: show only children of current breadcrumb node
    if (breadcrumb.length > 0) {
      const currentParentId = breadcrumb[breadcrumb.length - 1].id;
      result = result.filter(n => parentMap.get(n.id) === currentParentId);
    }

    return result;
  });

  // ── Computed: grouped layout ──────────────────────────────────────────
  let groups = $derived.by(() => {
    const nodeById = new Map(nodes.map(n => [n.id, n]));
    const groupMap = new Map(); // parentId -> nodes[]

    for (const node of filteredNodes) {
      const parentId = parentMap.get(node.id);
      const groupKey = parentId ?? '__root__';
      if (!groupMap.has(groupKey)) groupMap.set(groupKey, []);
      groupMap.get(groupKey).push(node);
    }

    const sorted = [...groupMap.entries()].sort((a, b) => b[1].length - a[1].length);
    return sorted.map(([key, groupNodes]) => {
      const parent = key !== '__root__' ? nodeById.get(key) : null;
      return {
        id: key,
        name: parent?.name ?? parent?.qualified_name ?? 'Root',
        type: parent?.node_type ?? 'root',
        nodes: groupNodes,
      };
    });
  });

  // ── Treemap layout computation ───────────────────────────────────────
  let layoutRects = $derived.by(() => {
    const totalNodes = filteredNodes.length;
    if (totalNodes === 0) return { groups: [], nodes: new Map() };

    const availW = Math.max(canvasW * 2, 900);
    const availH = Math.max(canvasH * 2, 600);

    // Squarified treemap for groups
    const groupRects = squarify(
      groups.map(g => ({ id: g.id, weight: g.nodes.length, data: g })),
      0, 0, availW, availH
    );

    // Layout nodes within each group
    const nodeRects = new Map();
    for (const gr of groupRects) {
      const g = gr.data;
      const innerX = gr.x + GROUP_PAD;
      const innerY = gr.y + GROUP_PAD + GROUP_HEADER;
      const innerW = gr.w - GROUP_PAD * 2;
      const innerH = gr.h - GROUP_PAD * 2 - GROUP_HEADER;

      if (innerW <= 0 || innerH <= 0) continue;

      const cols = Math.max(1, Math.floor(innerW / (MIN_NODE_W + NODE_PAD)));
      const nodeW = (innerW - (cols - 1) * NODE_PAD) / cols;
      const nodeH = MIN_NODE_H;

      g.nodes.forEach((node, idx) => {
        const col = idx % cols;
        const row = Math.floor(idx / cols);
        nodeRects.set(node.id, {
          x: innerX + col * (nodeW + NODE_PAD),
          y: innerY + row * (nodeH + NODE_PAD),
          w: nodeW,
          h: nodeH,
          node,
        });
      });
    }

    return { groups: groupRects, nodes: nodeRects };
  });

  // ── Squarified treemap algorithm ─────────────────────────────────────
  function squarify(items, x, y, w, h) {
    if (items.length === 0) return [];
    if (items.length === 1) {
      return [{ ...items[0], x, y, w, h }];
    }

    const totalWeight = items.reduce((s, i) => s + i.weight, 0);
    if (totalWeight === 0) return [];

    const sorted = [...items].sort((a, b) => b.weight - a.weight);
    const results = [];
    layoutRow(sorted, x, y, w, h, totalWeight, results);
    return results;
  }

  function layoutRow(items, x, y, w, h, totalWeight, results) {
    if (items.length === 0) return;
    if (items.length === 1) {
      results.push({ ...items[0], x, y, w, h });
      return;
    }

    const horizontal = w >= h;
    const side = horizontal ? h : w;

    let row = [items[0]];
    let rowWeight = items[0].weight;
    let bestWorst = worst(row, rowWeight, side, totalWeight);

    for (let i = 1; i < items.length; i++) {
      const candidate = [...row, items[i]];
      const candidateWeight = rowWeight + items[i].weight;
      const candidateWorst = worst(candidate, candidateWeight, side, totalWeight);

      if (candidateWorst <= bestWorst) {
        row = candidate;
        rowWeight = candidateWeight;
        bestWorst = candidateWorst;
      } else {
        break;
      }
    }

    const rowFraction = rowWeight / totalWeight;
    const rowSize = horizontal ? w * rowFraction : h * rowFraction;
    let pos = 0;

    for (const item of row) {
      const itemFraction = item.weight / rowWeight;
      const itemSize = side * itemFraction;

      if (horizontal) {
        results.push({ ...item, x: x, y: y + pos, w: rowSize, h: itemSize });
      } else {
        results.push({ ...item, x: x + pos, y: y, w: itemSize, h: rowSize });
      }
      pos += itemSize;
    }

    const remaining = items.slice(row.length);
    const remainingWeight = totalWeight - rowWeight;

    if (remaining.length > 0) {
      if (horizontal) {
        layoutRow(remaining, x + rowSize, y, w - rowSize, h, remainingWeight, results);
      } else {
        layoutRow(remaining, x, y + rowSize, w, h - rowSize, remainingWeight, results);
      }
    }
  }

  function worst(row, rowWeight, side, totalWeight) {
    const s = (rowWeight / totalWeight) * side * side;
    let maxRatio = 0;
    for (const item of row) {
      const area = (item.weight / totalWeight) * side * side;
      const r = area > 0 ? Math.max(s / area, area / s) : Infinity;
      if (r > maxRatio) maxRatio = r;
    }
    return maxRatio;
  }

  // ── View query rendering helpers ─────────────────────────────────────
  let queryMatchedIds = $derived.by(() => {
    if (!activeQuery?.scope) return null;
    const scope = activeQuery.scope;
    const matched = new Set();
    for (const node of nodes) {
      let match = true;
      if (scope.node_types?.length && !scope.node_types.includes(node.node_type)) match = false;
      if (scope.modules?.length) {
        const parentId = parentMap.get(node.id);
        const parent = parentId ? nodes.find(n => n.id === parentId) : null;
        if (!parent || !scope.modules.some(m => (parent.name ?? '').includes(m) || (parent.qualified_name ?? '').includes(m))) match = false;
      }
      if (scope.name_pattern) {
        const re = new RegExp(scope.name_pattern, 'i');
        if (!re.test(node.name ?? '') && !re.test(node.qualified_name ?? '')) match = false;
      }
      if (match) matched.add(node.id);
    }
    return matched;
  });

  let queryCallouts = $derived.by(() => {
    if (!activeQuery?.callouts?.length) return new Map();
    const m = new Map();
    for (const c of activeQuery.callouts) {
      const node = nodes.find(n => n.name === c.node_name || n.qualified_name === c.node_name);
      if (node) m.set(node.id, c.label ?? c.text ?? '');
    }
    return m;
  });

  let queryGroups = $derived.by(() => {
    if (!activeQuery?.groups?.length) return [];
    return activeQuery.groups.map(g => {
      const memberIds = [];
      for (const name of (g.members ?? [])) {
        const node = nodes.find(n => n.name === name || n.qualified_name === name);
        if (node) memberIds.push(node.id);
      }
      return { label: g.label ?? '', memberIds, color: g.color ?? '#3b82f6' };
    });
  });

  // ── Semantic zoom: determine visible node types ──────────────────────
  let visibleTypes = $derived.by(() => {
    if (zoom < ZOOM_THRESHOLDS.low) {
      return new Set(['package', 'module']);
    } else if (zoom < ZOOM_THRESHOLDS.medium) {
      return new Set(['package', 'module', 'type', 'interface', 'component', 'table']);
    }
    return null; // show all
  });

  // ── Canvas rendering ─────────────────────────────────────────────────
  function getNodeColor(nodeType) {
    return NODE_COLORS[nodeType] ?? DEFAULT_COLOR;
  }

  function drawCanvas() {
    const canvas = canvasEl;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    canvas.width = canvasW * dpr;
    canvas.height = canvasH * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, canvasW, canvasH);

    const { groups: groupRects, nodes: nodeRects } = layoutRects;
    if (groupRects.length === 0) return;

    ctx.save();
    ctx.translate(-offsetX * zoom, -offsetY * zoom);
    ctx.scale(zoom, zoom);

    const dimOpacity = activeQuery?.emphasis?.dim_unmatched ?? 0.15;

    // Draw group backgrounds
    for (const gr of groupRects) {
      const isDimmed = queryMatchedIds && !gr.data.nodes.some(n => queryMatchedIds.has(n.id));
      ctx.globalAlpha = isDimmed ? dimOpacity : 0.12;
      const groupColor = getNodeColor(gr.data.type);
      ctx.fillStyle = groupColor.fill;
      ctx.beginPath();
      roundRect(ctx, gr.x, gr.y, gr.w, gr.h, 6);
      ctx.fill();

      // Group border
      ctx.globalAlpha = isDimmed ? dimOpacity * 0.5 : 0.3;
      ctx.strokeStyle = groupColor.stroke;
      ctx.lineWidth = 1;
      ctx.stroke();

      // Group label
      ctx.globalAlpha = isDimmed ? dimOpacity : 0.85;
      ctx.fillStyle = '#e2e8f0';
      ctx.font = `600 ${Math.max(10, 12 / Math.max(zoom, 0.3))}px var(--font-body, system-ui)`;
      ctx.textBaseline = 'top';
      ctx.fillText(
        truncateText(ctx, gr.data.name, gr.w - GROUP_PAD * 2),
        gr.x + GROUP_PAD,
        gr.y + GROUP_PAD
      );
    }

    // Draw query group bounding boxes
    for (const qg of queryGroups) {
      if (qg.memberIds.length === 0) continue;
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (const id of qg.memberIds) {
        const rect = nodeRects.get(id);
        if (!rect) continue;
        minX = Math.min(minX, rect.x);
        minY = Math.min(minY, rect.y);
        maxX = Math.max(maxX, rect.x + rect.w);
        maxY = Math.max(maxY, rect.y + rect.h);
      }
      if (minX === Infinity) continue;
      const pad = 8;
      ctx.globalAlpha = 0.25;
      ctx.strokeStyle = qg.color;
      ctx.lineWidth = 2;
      ctx.setLineDash([6, 3]);
      ctx.beginPath();
      roundRect(ctx, minX - pad, minY - pad - 16, maxX - minX + pad * 2, maxY - minY + pad * 2 + 16, 4);
      ctx.stroke();
      ctx.setLineDash([]);

      // Group label
      ctx.globalAlpha = 0.7;
      ctx.fillStyle = qg.color;
      ctx.font = '600 11px var(--font-body, system-ui)';
      ctx.textBaseline = 'bottom';
      ctx.fillText(qg.label, minX - pad + 4, minY - pad - 2);
    }

    // Draw nodes
    for (const [nodeId, rect] of nodeRects) {
      const node = rect.node;

      // Semantic zoom filter
      if (visibleTypes && !visibleTypes.has(node.node_type)) continue;

      const isSelected = nodeId === selectedNodeId;
      const isHovered = nodeId === hoveredNodeId;
      const isMatched = queryMatchedIds ? queryMatchedIds.has(nodeId) : true;
      const colors = getNodeColor(node.node_type);

      ctx.globalAlpha = isMatched ? 1 : dimOpacity;

      // Node background
      ctx.fillStyle = isSelected ? lighten(colors.fill, 0.3) : isHovered ? lighten(colors.fill, 0.15) : colors.fill;
      ctx.beginPath();
      roundRect(ctx, rect.x, rect.y, rect.w, rect.h, 4);
      ctx.fill();

      // Node border
      ctx.strokeStyle = isSelected ? '#ffffff' : isHovered ? colors.stroke : lighten(colors.stroke, -0.1);
      ctx.lineWidth = isSelected ? 2 : 1;
      ctx.stroke();

      // Emphasis highlight ring
      if (activeQuery && isMatched && queryMatchedIds) {
        const emphColor = activeQuery.emphasis?.color ?? '#fbbf24';
        ctx.strokeStyle = emphColor;
        ctx.lineWidth = 2;
        ctx.globalAlpha = 0.7;
        ctx.beginPath();
        roundRect(ctx, rect.x - 2, rect.y - 2, rect.w + 4, rect.h + 4, 6);
        ctx.stroke();
        ctx.globalAlpha = isMatched ? 1 : dimOpacity;
      }

      // Node label
      if (rect.w > 30 && zoom > 0.25) {
        ctx.fillStyle = '#f1f5f9';
        ctx.font = `500 ${Math.min(12, rect.h * 0.4)}px var(--font-mono, monospace)`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(
          truncateText(ctx, node.name ?? '', rect.w - 8),
          rect.x + rect.w / 2,
          rect.y + rect.h / 2
        );
      }

      // Callout label
      const callout = queryCallouts.get(nodeId);
      if (callout) {
        ctx.globalAlpha = 0.9;
        ctx.fillStyle = '#fbbf24';
        ctx.font = '600 10px var(--font-body, system-ui)';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        ctx.fillText(callout, rect.x + rect.w / 2, rect.y - 4);
      }
    }

    // Draw narrative steps
    if (activeQuery?.narrative?.length) {
      for (let i = 0; i < activeQuery.narrative.length; i++) {
        const step = activeQuery.narrative[i];
        const node = nodes.find(n => n.name === step.node_name || n.qualified_name === step.node_name);
        if (!node) continue;
        const rect = nodeRects.get(node.id);
        if (!rect) continue;

        // Step number circle
        ctx.globalAlpha = 0.9;
        ctx.fillStyle = '#3b82f6';
        ctx.beginPath();
        ctx.arc(rect.x + rect.w + 8, rect.y, 10, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = '#ffffff';
        ctx.font = 'bold 10px var(--font-body, system-ui)';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(i + 1), rect.x + rect.w + 8, rect.y);
      }
    }

    ctx.restore();
    ctx.globalAlpha = 1;

    // Draw minimap
    drawMinimap();
  }

  function drawMinimap() {
    const minimap = minimapEl;
    if (!minimap) return;
    const ctx = minimap.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    minimap.width = MINIMAP_W * dpr;
    minimap.height = MINIMAP_H * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, MINIMAP_W, MINIMAP_H);

    const { groups: groupRects, nodes: nodeRects } = layoutRects;
    if (groupRects.length === 0) return;

    // Compute bounds
    let maxX = 0, maxY = 0;
    for (const gr of groupRects) {
      maxX = Math.max(maxX, gr.x + gr.w);
      maxY = Math.max(maxY, gr.y + gr.h);
    }
    if (maxX === 0 || maxY === 0) return;

    const scaleX = MINIMAP_W / maxX;
    const scaleY = MINIMAP_H / maxY;
    const scale = Math.min(scaleX, scaleY) * 0.95;

    ctx.save();
    ctx.translate(2, 2);
    ctx.scale(scale, scale);

    // Minimap background
    ctx.fillStyle = '#0f172a';
    ctx.fillRect(0, 0, maxX, maxY);

    // Group rects
    for (const gr of groupRects) {
      const colors = getNodeColor(gr.data.type);
      ctx.fillStyle = colors.fill;
      ctx.globalAlpha = 0.4;
      ctx.fillRect(gr.x, gr.y, gr.w, gr.h);
    }

    // Node dots
    ctx.globalAlpha = 0.8;
    for (const [, rect] of nodeRects) {
      const colors = getNodeColor(rect.node.node_type);
      ctx.fillStyle = colors.stroke;
      ctx.fillRect(rect.x, rect.y, Math.max(2, rect.w), Math.max(2, rect.h));
    }

    // Viewport rectangle
    ctx.globalAlpha = 1;
    ctx.strokeStyle = '#60a5fa';
    ctx.lineWidth = 2 / scale;
    const vx = offsetX;
    const vy = offsetY;
    const vw = canvasW / zoom;
    const vh = canvasH / zoom;
    ctx.strokeRect(vx, vy, vw, vh);

    ctx.restore();
  }

  function roundRect(ctx, x, y, w, h, r) {
    r = Math.min(r, w / 2, h / 2);
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

  function truncateText(ctx, text, maxWidth) {
    if (ctx.measureText(text).width <= maxWidth) return text;
    let t = text;
    while (t.length > 1 && ctx.measureText(t + '...').width > maxWidth) t = t.slice(0, -1);
    return t + '...';
  }

  function lighten(hex, amount) {
    const num = parseInt(hex.replace('#', ''), 16);
    const r = Math.min(255, Math.max(0, ((num >> 16) & 0xff) + Math.round(255 * amount)));
    const g = Math.min(255, Math.max(0, ((num >> 8) & 0xff) + Math.round(255 * amount)));
    const b = Math.min(255, Math.max(0, (num & 0xff) + Math.round(255 * amount)));
    return `rgb(${r},${g},${b})`;
  }

  // ── Interaction handlers ─────────────────────────────────────────────
  function screenToCanvas(clientX, clientY) {
    const rect = canvasEl?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    const sx = clientX - rect.left;
    const sy = clientY - rect.top;
    return {
      x: sx / zoom + offsetX,
      y: sy / zoom + offsetY,
    };
  }

  function nodeAtPoint(cx, cy) {
    const { nodes: nodeRects } = layoutRects;
    // Iterate in reverse for topmost
    for (const [nodeId, rect] of nodeRects) {
      if (visibleTypes && !visibleTypes.has(rect.node.node_type)) continue;
      if (cx >= rect.x && cx <= rect.x + rect.w && cy >= rect.y && cy <= rect.y + rect.h) {
        return { id: nodeId, ...rect };
      }
    }
    return null;
  }

  function onMouseDown(e) {
    if (e.button !== 0) return;
    isPanning = true;
    panStart = { x: e.clientX, y: e.clientY };
    panOffset = { x: offsetX, y: offsetY };
    e.preventDefault();
  }

  function onMouseMove(e) {
    const pos = screenToCanvas(e.clientX, e.clientY);
    const hit = nodeAtPoint(pos.x, pos.y);
    hoveredNodeId = hit?.id ?? null;

    if (canvasEl) {
      canvasEl.style.cursor = isPanning ? 'grabbing' : hit ? 'pointer' : 'grab';
    }

    if (isPanning) {
      const dx = e.clientX - panStart.x;
      const dy = e.clientY - panStart.y;
      offsetX = panOffset.x - dx / zoom;
      offsetY = panOffset.y - dy / zoom;
      scheduleRedraw();
    }
  }

  function onMouseUp() {
    isPanning = false;
  }

  function onWheel(e) {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 0.9 : 1.11;
    const newZoom = Math.max(0.1, Math.min(8, zoom * factor));

    // Zoom toward mouse position
    const rect = canvasEl?.getBoundingClientRect();
    if (rect) {
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const worldX = mx / zoom + offsetX;
      const worldY = my / zoom + offsetY;
      offsetX = worldX - mx / newZoom;
      offsetY = worldY - my / newZoom;
    }

    zoom = newZoom;
    scheduleRedraw();
  }

  function onClick(e) {
    const pos = screenToCanvas(e.clientX, e.clientY);
    const hit = nodeAtPoint(pos.x, pos.y);

    if (hit) {
      selectedNodeId = hit.id;
      canvasState = {
        ...canvasState,
        selectedNode: hit.node,
        zoom,
        breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })),
      };
    } else {
      selectedNodeId = null;
      canvasState = { ...canvasState, selectedNode: null, zoom, breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })) };
    }
    scheduleRedraw();
  }

  function onDblClick(e) {
    const pos = screenToCanvas(e.clientX, e.clientY);
    const hit = nodeAtPoint(pos.x, pos.y);

    if (hit) {
      const node = hit.node;
      // Check if this node has children
      const hasChildren = edges.some(edge => {
        const etype = (edge.edge_type ?? edge.type ?? '').toLowerCase();
        const parentId = edge.source_id ?? edge.from_node_id ?? edge.from;
        return etype === 'contains' && parentId === node.id;
      });

      if (hasChildren) {
        breadcrumb = [...breadcrumb, { id: node.id, name: node.name ?? node.qualified_name ?? '', type: node.node_type }];
        selectedNodeId = null;
        offsetX = 0;
        offsetY = 0;
        zoom = 1;
        canvasState = {
          ...canvasState,
          selectedNode: null,
          zoom: 1,
          breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })),
        };
        scheduleRedraw();
      }
    }
  }

  function navigateBreadcrumb(index) {
    if (index < 0) {
      breadcrumb = [];
    } else {
      breadcrumb = breadcrumb.slice(0, index + 1);
    }
    selectedNodeId = null;
    offsetX = 0;
    offsetY = 0;
    zoom = 1;
    canvasState = {
      ...canvasState,
      selectedNode: null,
      zoom: 1,
      breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })),
    };
    scheduleRedraw();
  }

  // ── Touch handlers ───────────────────────────────────────────────────
  let lastTouchDist = 0;

  function onTouchStart(e) {
    if (e.touches.length === 1) {
      isPanning = true;
      panStart = { x: e.touches[0].clientX, y: e.touches[0].clientY };
      panOffset = { x: offsetX, y: offsetY };
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
      offsetX = panOffset.x - dx / zoom;
      offsetY = panOffset.y - dy / zoom;
      scheduleRedraw();
    } else if (e.touches.length === 2) {
      e.preventDefault();
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.hypot(dx, dy);
      if (lastTouchDist > 0) {
        const factor = dist / lastTouchDist;
        zoom = Math.max(0.1, Math.min(8, zoom * factor));
        scheduleRedraw();
      }
      lastTouchDist = dist;
    }
  }

  function onTouchEnd() {
    isPanning = false;
    lastTouchDist = 0;
  }

  // ── Resize observer ──────────────────────────────────────────────────
  let resizeObserver = null;

  $effect(() => {
    if (!containerEl) return;
    resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        canvasW = entry.contentRect.width;
        canvasH = entry.contentRect.height;
        scheduleRedraw();
      }
    });
    resizeObserver.observe(containerEl);
    return () => resizeObserver?.disconnect();
  });

  // ── Render loop ──────────────────────────────────────────────────────
  function scheduleRedraw() {
    needsRedraw = true;
  }

  $effect(() => {
    // Track all reactive dependencies that should trigger redraw
    const _ = [layoutRects, zoom, offsetX, offsetY, hoveredNodeId, selectedNodeId, activeQuery, queryMatchedIds, queryCallouts, queryGroups, visibleTypes];
    scheduleRedraw();
  });

  $effect(() => {
    if (!needsRedraw) return;
    needsRedraw = false;
    if (animFrame) cancelAnimationFrame(animFrame);
    animFrame = requestAnimationFrame(() => {
      drawCanvas();
      animFrame = null;
    });
  });

  // Sync canvasState zoom
  $effect(() => {
    canvasState = { ...canvasState, zoom };
  });

  onDestroy(() => {
    if (animFrame) cancelAnimationFrame(animFrame);
    resizeObserver?.disconnect();
  });

  // ── Legend items ─────────────────────────────────────────────────────
  const legendItems = Object.entries(NODE_COLORS);
</script>

<div class="treemap-container">
  <!-- Filter presets -->
  <div class="treemap-toolbar">
    <div class="filter-group" role="group" aria-label={$t('explorer_treemap.filter_presets')}>
      {#each [['all', 'All'], ['endpoints', 'Endpoints'], ['types', 'Types'], ['calls', 'Calls'], ['dependencies', 'Dependencies']] as [key, label]}
        <button
          class="filter-btn"
          class:active={filter === key}
          onclick={() => { /* parent controls filter */ }}
          aria-pressed={filter === key}
          type="button"
        >{label}</button>
      {/each}
    </div>

    <div class="lens-group" role="group" aria-label={$t('explorer_treemap.lens_toggle')}>
      <button
        class="lens-btn active"
        aria-pressed={lens === 'structural'}
        title="Structural lens"
        type="button"
      >Structural</button>
      <button
        class="lens-btn"
        disabled
        title="Evaluative lens (coming soon)"
        type="button"
      >Evaluative</button>
      <button
        class="lens-btn"
        disabled
        title="Observable lens (coming soon)"
        type="button"
      >Observable</button>
    </div>

    <div class="treemap-legend">
      {#each legendItems as [type, colors]}
        <span class="legend-item">
          <span class="legend-dot" style="background: {colors.stroke}"></span>
          <span class="legend-label">{colors.label}</span>
        </span>
      {/each}
    </div>

    <span class="treemap-stats">
      {filteredNodes.length} nodes
    </span>
  </div>

  <!-- Canvas area -->
  <div class="treemap-canvas-area" bind:this={containerEl}>
    {#if filteredNodes.length === 0}
      <EmptyState
        title={$t('explorer_treemap.empty_title')}
        description={nodes.length > 0 ? $t('explorer_treemap.empty_filtered') : $t('explorer_treemap.empty_desc')}
      />
    {:else}
      <canvas
        bind:this={canvasEl}
        class="treemap-canvas"
        style="width: {canvasW}px; height: {canvasH}px"
        onmousedown={onMouseDown}
        onmousemove={onMouseMove}
        onmouseup={onMouseUp}
        onmouseleave={onMouseUp}
        onwheel={onWheel}
        onclick={onClick}
        ondblclick={onDblClick}
        ontouchstart={onTouchStart}
        ontouchmove={onTouchMove}
        ontouchend={onTouchEnd}
        ontouchcancel={onTouchEnd}
        role="application"
        aria-label={$t('explorer_treemap.canvas_label')}
        tabindex="0"
      ></canvas>

      <!-- Minimap -->
      <div class="treemap-minimap" aria-hidden="true">
        <canvas
          bind:this={minimapEl}
          style="width: {MINIMAP_W}px; height: {MINIMAP_H}px"
        ></canvas>
      </div>
    {/if}
  </div>

  <!-- Breadcrumb bar -->
  {#if breadcrumb.length > 0}
    <div class="treemap-breadcrumb" role="navigation" aria-label={$t('explorer_treemap.breadcrumb')}>
      <button
        class="breadcrumb-item root"
        onclick={() => navigateBreadcrumb(-1)}
        type="button"
        aria-label={$t('explorer_treemap.go_root')}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true">
          <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
        </svg>
        Root
      </button>
      {#each breadcrumb as crumb, i}
        <span class="breadcrumb-sep" aria-hidden="true">/</span>
        <button
          class="breadcrumb-item"
          class:current={i === breadcrumb.length - 1}
          onclick={() => navigateBreadcrumb(i)}
          type="button"
          aria-current={i === breadcrumb.length - 1 ? 'location' : undefined}
        >{crumb.name}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .treemap-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--color-surface);
  }

  /* ── Toolbar ──────────────────────────────────────────────────────── */
  .treemap-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .filter-group, .lens-group {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: var(--space-1);
  }

  .filter-btn, .lens-btn {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .filter-btn:hover:not(:disabled), .lens-btn:hover:not(:disabled) {
    color: var(--color-text);
  }

  .filter-btn.active, .lens-btn.active {
    background: var(--color-link);
    color: #fff;
  }

  .filter-btn:disabled, .lens-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .filter-btn:focus-visible, .lens-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .treemap-legend {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
    margin-left: auto;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .legend-dot {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .legend-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .treemap-stats {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  /* ── Canvas area ─────────────────────────────────────────────────── */
  .treemap-canvas-area {
    flex: 1;
    position: relative;
    overflow: hidden;
    min-height: 200px;
  }

  .treemap-canvas {
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
  }

  .treemap-canvas:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Minimap ─────────────────────────────────────────────────────── */
  .treemap-minimap {
    position: absolute;
    bottom: var(--space-3);
    right: var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    overflow: hidden;
    background: #0f172a;
    box-shadow: 0 4px 12px color-mix(in srgb, black 50%, transparent);
    opacity: 0.85;
    transition: opacity var(--transition-fast);
    pointer-events: none;
  }

  .treemap-minimap:hover {
    opacity: 1;
  }

  /* ── Breadcrumb ──────────────────────────────────────────────────── */
  .treemap-breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-4);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .breadcrumb-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-link);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    cursor: pointer;
    transition: background var(--transition-fast);
    white-space: nowrap;
  }

  .breadcrumb-item:hover {
    background: var(--color-surface);
  }

  .breadcrumb-item.current {
    color: var(--color-text);
    font-weight: 600;
    cursor: default;
  }

  .breadcrumb-item.root {
    color: var(--color-text-muted);
  }

  .breadcrumb-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .breadcrumb-sep {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    user-select: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .filter-btn, .lens-btn, .breadcrumb-item, .treemap-minimap {
      transition: none;
    }
  }
</style>
