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
  } = $props();

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

  function specBorderColor(node) {
    if (!node) return '#64748b';
    const conf = node.spec_confidence;
    if (conf === 'high') return '#22c55e';
    if (conf === 'medium') return '#eab308';
    if (conf === 'low') return '#f97316';
    return '#64748b';
  }

  const EDGE_COLORS = {
    calls: '#60a5fa',
    implements: '#34d399',
    depends_on: '#64748b',
    field_of: '#94a3b8',
    routes_to: '#f97316',
    governed_by: '#fbbf24',
  };

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

  let isPanning = $state(false);
  let panStart = { x: 0, y: 0 };
  let panCamStart = { x: 0, y: 0 };

  let selectedNodeId = $state(null);
  let hoveredNodeId = $state(null);
  let breadcrumb = $state([]);
  let animFrame = null;

  let tooltipNode = $state(null);
  let tooltipPos = $state({ x: 0, y: 0 });

  // Context menu state
  let contextMenu = $state(null); // { x, y, node }

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
      const etype = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (etype !== 'contains') continue;
      const parentId = e.source_id ?? e.from_node_id ?? e.from;
      const childId = e.target_id ?? e.to_node_id ?? e.to;
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
    return edges.filter(e => {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      return et !== 'contains' && et !== 'field_of';
    });
  });

  // Parent map for root-ancestor lookup
  let parentMap = $derived.by(() => {
    const m = new Map();
    for (const e of edges) {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'contains') {
        m.set(e.target_id ?? e.to_node_id ?? e.to, e.source_id ?? e.from_node_id ?? e.from);
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
      const cand = [...row, items[i]], candW = rowW + items[i].weight;
      const candA = worstAspect(cand, candW, side, total, w, h, horizontal);
      if (candA <= bestAspect) { row = cand; rowW = candW; bestAspect = candA; }
      else break;
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

    // Get root nodes (no Contains parent)
    const rootNodes = nodes.filter(n => !childToParent.has(n.id));

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
          weight: Math.max(1, descendantCounts.get(gn.id) ?? 1),
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
    function layoutLeafNodes(graphNodes, x, y, w, h, parentLn, depth) {
      const items = graphNodes.map(n => ({
        id: n.id,
        node: n,
        weight: Math.max(1, descendantCounts.get(n.id) ?? 1),
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
      // Parent must be 500px+ before leaf children appear
      if (ps < 500) return 0;
      if (ps < 700) {
        const pf = (ps - 500) / 200;
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

  function shouldShowChildren(ln) {
    if (ln.kind !== 'tree-group') return false;
    return Math.min(ln.w * cam.zoom, ln.h * cam.zoom) > 400;
  }

  // ── Filter visibility ─────────────────────────────────────────────
  // Pre-compute call edge index
  let nodesWithCallsEdges = $derived.by(() => {
    const s = new Set();
    for (const e of edges) {
      if ((e.edge_type ?? e.type ?? '').toLowerCase() === 'calls') {
        s.add(e.source_id ?? e.from_node_id ?? e.from);
        s.add(e.target_id ?? e.to_node_id ?? e.to);
      }
    }
    return s;
  });

  function filterOpacity(ln) {
    if (filter === 'all') return 1.0;
    if (ln.kind === 'tree-group') return 1.0;
    if (!ln.node) return 0.1;
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
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (src && tgt) {
        if (!adj.has(src)) adj.set(src, []);
        adj.get(src).push({ targetId: tgt, edgeType: et });
        if (!adj.has(tgt)) adj.set(tgt, []);
        adj.get(tgt).push({ targetId: src, edgeType: et, reverse: true });
      }
    }
    return adj;
  });

  let queryMatchedWithDepth = $derived.by(() => {
    if (!activeQuery?.scope) return null;
    const scope = activeQuery.scope;

    if (scope.type === 'focus' && scope.node) {
      const startName = scope.node === '$selected' || scope.node === '$clicked'
        ? canvasState?.selectedNode?.name ?? '' : scope.node;
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
      const testN = nodes.filter(n => n.test_node);
      const reachable = new Set(testN.map(n => n.id));
      const q = [...reachable];
      while (q.length > 0) {
        const id = q.shift();
        for (const nb of (adjacency.get(id) ?? [])) {
          if (reachable.has(nb.targetId) || nb.edgeType !== 'calls' || nb.reverse) continue;
          reachable.add(nb.targetId);
          q.push(nb.targetId);
        }
      }
      const matched = new Map();
      for (const n of nodes) {
        if (!n.test_node && n.node_type === 'function' && !reachable.has(n.id)) matched.set(n.id, 0);
      }
      return matched.size > 0 ? matched : null;
    }

    if (scope.type === 'filter') {
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
      const n = nodes.find(n => n.name === c.node_name || n.qualified_name === c.node_name);
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
    if (!queryMatchedIds) return 1.0;
    if (ln.kind === 'tree-group') return treeGroupHasMatch(ln) ? 1.0 : 0.15;
    const nodeId = ln.node?.id ?? ln.id;
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
      // Compute metric value for this node
      let value = 0;
      if (metric === 'incoming_calls') {
        for (const e of edges) {
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          if (tgt === nodeId && (e.edge_type ?? e.type ?? '').toLowerCase() === 'calls') value++;
        }
      } else if (metric === 'complexity') {
        value = node.complexity ?? 0;
      } else if (metric === 'churn') {
        value = node.churn ?? 0;
      } else if (metric === 'test_fragility') {
        // Count distinct test paths reaching this node
        for (const e of edges) {
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          if (tgt === nodeId && (e.edge_type ?? e.type ?? '').toLowerCase() === 'calls') {
            const src = e.source_id ?? e.from_node_id ?? e.from;
            const srcNode = nodes.find(n => n.id === src);
            if (srcNode?.test_node) value++;
          }
        }
      }
      if (value === 0) return null;
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
    return `hsl(${(1 - t) * 240}, 70%, 45%)`;
  }

  // Pre-compute heat map max values for normalization
  let heatMaxValues = $derived.by(() => {
    const map = new Map();
    if (!activeQuery?.emphasis?.heat?.metric) return map;
    const metric = activeQuery.emphasis.heat.metric;
    let max = 0;
    for (const node of nodes) {
      let v = 0;
      if (metric === 'incoming_calls') {
        for (const e of edges) {
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          if (tgt === node.id && (e.edge_type ?? e.type ?? '').toLowerCase() === 'calls') v++;
        }
      } else if (metric === 'complexity') {
        v = node.complexity ?? 0;
      } else if (metric === 'churn') {
        v = node.churn ?? 0;
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
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'contains' || et === 'field_of') continue;
      if (src === selectedNodeId) connected.add(tgt);
      if (tgt === selectedNodeId) connected.add(src);
    }
    return connected.size > 1 ? connected : null;
  });

  // ── Text width cache ──────────────────────────────────────────────
  const textWidthCache = new Map();
  function measureText(ctx, text, font) {
    const key = font + '|' + text;
    if (textWidthCache.has(key)) return textWidthCache.get(key);
    ctx.font = font;
    const w = ctx.measureText(text).width;
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

    // Narrative step markers
    if (activeQuery?.narrative?.length) {
      for (let i = 0; i < activeQuery.narrative.length; i++) {
        const step = activeQuery.narrative[i];
        const n = nodes.find(n => n.name === step.node_name || n.qualified_name === step.node_name);
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

    drawMinimap();
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

    // Fill
    ctx.fillStyle = 'rgba(20,28,48,0.9)';
    ctx.fill();

    // Border — colored by spec confidence
    let borderColor = ln.id === selectedNodeId ? '#ef4444' : specBorderColor(n);
    let borderWidth = ln.id === selectedNodeId ? 2 : 1;

    const qColor = queryNodeColor(ln);
    if (qColor && ln.id !== selectedNodeId) {
      borderColor = qColor;
      // Fill tint
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

    ctx.strokeStyle = borderColor;
    ctx.lineWidth = borderWidth;
    roundRect(ctx, s.x - sw / 2, s.y - sh / 2, sw, sh, r);
    ctx.stroke();

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

    // Hovered state
    if (ln.id === hoveredNodeId) {
      ctx.strokeStyle = '#93c5fd';
      ctx.lineWidth = 1.5;
      roundRect(ctx, s.x - sw / 2 - 1, s.y - sh / 2 - 1, sw + 2, sh + 2, r + 1);
      ctx.stroke();
    }
  }

  function drawEdges(ctx) {
    if (cam.zoom < 0.3) return; // Don't draw edges at very low zoom

    const maxEdges = 1500;
    let count = 0;

    for (const e of renderEdges) {
      if (count >= maxEdges) break;
      if (!filterEdge(e)) continue;

      const srcId = e.source_id ?? e.from_node_id ?? e.from;
      const tgtId = e.target_id ?? e.to_node_id ?? e.to;
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

      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      let color = EDGE_COLORS[et] ?? '#64748b';
      let edgeAlpha = alpha;
      let lineWidth = 1.2;

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

  function drawMinimap() {
    const minimap = minimapEl;
    if (!minimap) return;
    const ctx = minimap.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    minimap.width = MINIMAP_W * dpr;
    minimap.height = MINIMAP_H * dpr;
    ctx.scale(dpr, dpr);

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
  }

  function animLoop() {
    lerpCam();
    drawFrame();

    // Keep animating if camera is still moving or always (for smooth interactions)
    const dx = Math.abs(cam.x - targetCam.x);
    const dy = Math.abs(cam.y - targetCam.y);
    const dz = Math.abs(cam.zoom - targetCam.zoom);
    if (dx > 0.1 || dy > 0.1 || dz > 0.0001 || needsAnim) {
      needsAnim = false;
      animFrame = requestAnimationFrame(animLoop);
    } else {
      animFrame = null;
    }
  }

  function scheduleRedraw() {
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
        if (world.x < l || world.x > r || world.y < t || world.y > b) continue;

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

  function onClick(e) {
    if (Math.abs(e.clientX - panStart.x) > 4 || Math.abs(e.clientY - panStart.y) > 4) return;

    const hit = hitTest(e.clientX, e.clientY);
    if (hit) {
      selectedNodeId = hit.id;
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
      };
      onNodeDetail(hit.node);
    } else {
      selectedNodeId = null;
      canvasState = { ...canvasState, selectedNode: null };
      onNodeDetail(null);
    }
    scheduleRedraw();
  }

  function onDblClick(e) {
    const hit = hitTest(e.clientX, e.clientY);
    if (!hit) return;

    if (hit.kind === 'tree-group') {
      // Zoom into this tree group smoothly
      targetCam.x = hit.x;
      targetCam.y = hit.y;
      const fitZoom = Math.min(W / hit.w, H / hit.h) * 0.85;
      targetCam.zoom = Math.max(fitZoom, cam.zoom * 1.5);
      scheduleRedraw();
    } else if (hit.isLeafGraphNode) {
      // Zoom into leaf node
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
    contextMenu = {
      x: e.clientX - (rect?.left ?? 0),
      y: e.clientY - (rect?.top ?? 0),
      node: hit.node,
      hit,
    };
  }

  function contextMenuAction(action) {
    if (!contextMenu) return;
    const node = contextMenu.node;
    contextMenu = null;
    if (action === 'trace') {
      // Trace from here: activate a blast-radius query centered on this node
      activeQuery = {
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls', 'implements'], direction: 'both', depth: 10 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#22c55e', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'implements'] },
        zoom: 'fit',
        annotation: { title: `Trace from: ${node.name}`, description: `Showing all connected nodes via calls/implements edges` },
      };
    } else if (action === 'blast') {
      activeQuery = {
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls', 'implements', 'field_of', 'depends_on'], direction: 'incoming', depth: 10 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'implements', 'field_of', 'depends_on'] },
        zoom: 'fit',
        annotation: { title: `Blast radius: ${node.name}`, description: `What would break if this changes?` },
      };
    } else if (action === 'callers') {
      activeQuery = {
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls'], direction: 'incoming', depth: 5 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls'] },
        zoom: 'fit',
        annotation: { title: `Callers of: ${node.name}`, description: `Who calls this?` },
      };
    } else if (action === 'callees') {
      activeQuery = {
        scope: { type: 'focus', node: node.name ?? node.qualified_name, edges: ['calls'], direction: 'outgoing', depth: 5 },
        emphasis: { tiered_colors: ['#3b82f6', '#60a5fa', '#93c5fd', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls'] },
        zoom: 'fit',
        annotation: { title: `Callees of: ${node.name}`, description: `What does this call?` },
      };
    } else if (action === 'spec') {
      if (node.spec_path) {
        onNodeDetail({ ...node, _action: 'view_spec' });
      }
    } else if (action === 'detail') {
      selectedNodeId = node.id;
      onNodeDetail(node);
    }
  }

  // Keyboard: Escape to zoom out to root
  function onKeyDown(e) {
    if (e.key === 'Escape') {
      if (contextMenu) {
        contextMenu = null;
        return;
      }
      if (activeQuery) {
        activeQuery = null;
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
    }
    // / key focuses search (if chat panel exists)
    if (e.key === '/' && !e.ctrlKey && !e.metaKey) {
      const chatInput = document.querySelector('.explorer-chat-input');
      if (chatInput) {
        e.preventDefault();
        chatInput.focus();
      }
    }
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
        targetCam.zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, targetCam.zoom * (dist / lastTouchDist)));
        scheduleRedraw();
      }
      lastTouchDist = dist;
    }
  }

  function onTouchEnd() {
    isPanning = false;
    lastTouchDist = 0;
  }

  // ── Resize ────────────────────────────────────────────────────────
  $effect(() => {
    if (!containerEl) return;
    const ro = new ResizeObserver(entries => {
      for (const entry of entries) {
        W = entry.contentRect.width;
        H = entry.contentRect.height;
        scheduleRedraw();
      }
    });
    ro.observe(containerEl);
    return () => ro.disconnect();
  });

  // Trigger redraws on reactive state changes (NOT hoveredNodeId — that triggers scheduleRedraw directly)
  $effect(() => {
    const _ = [selectedNodeId, activeQuery, queryMatchedIds, queryCallouts, connectedHighlight, filter, lens];
    scheduleRedraw();
  });

  // Sync canvasState zoom
  $effect(() => {
    if (Math.abs(cam.zoom - (canvasState.zoom ?? 1)) > 0.01) {
      canvasState = { ...canvasState, zoom: cam.zoom };
    }
  });

  onDestroy(() => {
    if (animFrame) cancelAnimationFrame(animFrame);
    if (zoomDecayFrame) cancelAnimationFrame(zoomDecayFrame);
  });

  const legendItems = [
    ['Package', '#64748b'],
    ['Module', '#3b82f6'],
    ['Type', '#10b981'],
    ['Interface', '#8b5cf6'],
    ['Function', '#f59e0b'],
    ['Endpoint', '#f43f5e'],
  ];
</script>

<div class="treemap-container">
  <!-- Query annotation -->
  {#if activeQuery?.annotation?.title}
    <div class="query-annotation" role="status">
      <div class="annotation-content">
        <span class="annotation-title">{activeQuery.annotation.title.replace('$name', canvasState?.selectedNode?.name ?? '').replace('{{count}}', queryMatchedIds?.size ?? '?')}</span>
        {#if activeQuery.annotation.description}
          <span class="annotation-desc">{activeQuery.annotation.description.replace('{{count}}', queryMatchedIds?.size ?? '?')}</span>
        {/if}
      </div>
      <button class="annotation-clear" onclick={() => { activeQuery = null; }} title="Clear" type="button" aria-label="Clear view query">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    </div>
  {/if}

  <!-- Toolbar -->
  <div class="treemap-toolbar">
    <div class="filter-group" role="group" aria-label="Filter presets">
      {#each [['all', 'All'], ['endpoints', 'Endpoints'], ['types', 'Types'], ['calls', 'Calls'], ['dependencies', 'Dependencies']] as [key, label]}
        <button class="tb-btn" class:active={filter === key} onclick={() => { filter = key; scheduleRedraw(); }} aria-pressed={filter === key} type="button">{label}</button>
      {/each}
    </div>

    <div class="tb-sep"></div>

    <div class="lens-group" role="group" aria-label="Lens toggle">
      <button class="tb-btn" class:active={lens === 'structural'} onclick={() => { lens = 'structural'; }} aria-pressed={lens === 'structural'} type="button">Structural</button>
      <button class="tb-btn" disabled title="Evaluative (coming soon)" type="button">Evaluative</button>
      <button class="tb-btn" disabled title="Observable (requires production telemetry)" type="button">Observable</button>
    </div>

    <div class="treemap-legend">
      {#each legendItems as [label, color]}
        <span class="legend-item">
          <span class="legend-swatch" style="background: {color}"></span>
          <span class="legend-label">{label}</span>
        </span>
      {/each}
    </div>

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
        </div>
      {/if}

      <!-- Minimap -->
      <div class="treemap-minimap" aria-hidden="true">
        <canvas bind:this={minimapEl} style="width: {MINIMAP_W}px; height: {MINIMAP_H}px"></canvas>
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
          {#if contextMenu.node.spec_path}
            <div class="ctx-sep"></div>
            <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('spec')}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
              View spec
            </button>
          {/if}
          <div class="ctx-sep"></div>
          <button class="ctx-item" role="menuitem" onclick={() => contextMenuAction('detail')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
            View details
          </button>
        </div>
      {/if}
    {/if}
  </div>

  <!-- Breadcrumb -->
  {#if breadcrumb.length > 0}
    <div class="treemap-breadcrumb" role="navigation" aria-label="Drill-down path">
      <button class="breadcrumb-item root" onclick={() => { breadcrumb = []; selectedNodeId = null; onNodeDetail(null); }} type="button" aria-label="Go to root">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>
        Root
      </button>
      {#each breadcrumb as crumb, i}
        <span class="breadcrumb-sep" aria-hidden="true">&rsaquo;</span>
        <button class="breadcrumb-item" class:current={i === breadcrumb.length - 1} onclick={() => { breadcrumb = breadcrumb.slice(0, i + 1); selectedNodeId = null; onNodeDetail(null); }} type="button">{crumb.name}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .treemap-container { display: flex; flex-direction: column; height: 100%; overflow: hidden; background: #0f0f1a; }

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

  .tb-sep { width: 1px; height: 20px; background: #334155; margin: 0 4px; }

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

  .treemap-minimap {
    position: absolute; bottom: 12px; right: 12px; border: 1px solid #334155;
    border-radius: 8px; overflow: hidden; background: #0f0f1a;
    box-shadow: 0 4px 16px rgba(0,0,0,0.5); opacity: 0.8; transition: opacity 0.15s;
    pointer-events: none;
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

  @media (prefers-reduced-motion: reduce) {
    .tb-btn, .breadcrumb-item, .treemap-minimap { transition: none; }
  }
</style>
