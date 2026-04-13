/**
 * Layout engines for ExplorerCanvas.
 *
 * Engines:
 *   column       — group nodes by type in vertical columns (simple, always-sync)
 *   graph        — d3-force physics simulation (sync via .tick())
 *   hierarchical — ELK top-down layered (async, returns Promise)
 *   layered      — ELK left-to-right layered (async, returns Promise)
 *
 * All sync engines return { [nodeId]: { x, y } }.
 * Async engines return Promise<{ [nodeId]: { x, y } }>.
 */

// ─── Column layout ────────────────────────────────────────────────────────────

const TYPE_ORDER = [
  'package', 'module', 'type', 'interface',
  'function', 'endpoint', 'component', 'table', 'constant',
];

export function columnLayout(nodes) {
  if (!nodes.length) return {};

  const byType = {};
  for (const n of nodes) {
    const t = n.node_type ?? 'unknown';
    (byType[t] = byType[t] ?? []).push(n);
  }

  const cols = Object.keys(byType).sort((a, b) => {
    const ai = TYPE_ORDER.indexOf(a);
    const bi = TYPE_ORDER.indexOf(b);
    return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
  });

  const positions = {};
  cols.forEach((col, ci) => {
    byType[col].forEach((n, ri) => {
      positions[n.id] = { x: 80 + ci * 160, y: 60 + ri * 60 };
    });
  });
  return positions;
}

// ─── d3-force layout ─────────────────────────────────────────────────────────

/**
 * Force-directed layout using d3-force.
 * Runs synchronously for `ticks` iterations (no RAF required).
 * Returns positions immediately after simulation settles.
 */
export async function forceLayout(nodes, edges, width = 900, height = 600, ticks = 300) {
  if (!nodes.length) return {};

  // Dynamic import — use d3-force directly instead of the full d3 umbrella
  const { forceSimulation, forceLink, forceManyBody, forceCollide, forceCenter } =
    await import('d3-force');

  // d3-force mutates node objects — clone to avoid side effects
  const ns = nodes.map(n => ({ id: n.id, x: width / 2, y: height / 2 }));
  const nodeIndex = new Map(ns.map((n, i) => [n.id, i]));

  const es = [];
  for (const e of edges) {
    const sid = e.source_id ?? e.from_node_id ?? e.from;
    const tid = e.target_id ?? e.to_node_id ?? e.to;
    if (nodeIndex.has(sid) && nodeIndex.has(tid)) {
      es.push({ source: nodeIndex.get(sid), target: nodeIndex.get(tid) });
    }
  }

  const sim = forceSimulation(ns)
    .force('link', forceLink(es).distance(80).strength(0.5))
    .force('charge', forceManyBody().strength(-200))
    .force('collide', forceCollide(30))
    .force('center', forceCenter(width / 2, height / 2))
    .stop(); // don't start RAF loop

  // Run synchronously
  sim.tick(ticks);

  const result = {};
  for (const n of ns) result[n.id] = { x: n.x, y: n.y };
  return result;
}

// ─── ELK layout ──────────────────────────────────────────────────────────────

/**
 * ELK-based hierarchical or layered layout.
 * @param {'DOWN'|'RIGHT'} direction — DOWN for hierarchical, RIGHT for layered
 */
export async function elkLayout(nodes, edges, direction = 'DOWN') {
  if (!nodes.length) return {};

  const { default: ELK } = await import('elkjs/lib/elk.bundled.js');
  const elk = new ELK();

  const nodeSet = new Set(nodes.map(n => n.id));
  const elkEdges = [];
  for (const e of edges) {
    const sid = e.source_id ?? e.from_node_id ?? e.from;
    const tid = e.target_id ?? e.to_node_id ?? e.to;
    if (nodeSet.has(sid) && nodeSet.has(tid) && sid !== tid) {
      elkEdges.push({
        id: e.id ?? `${sid}-${tid}`,
        sources: [sid],
        targets: [tid],
      });
    }
  }

  const graph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': direction,
      'elk.spacing.nodeNode': '40',
      'elk.layered.spacing.nodeNodeBetweenLayers': '80',
    },
    children: nodes.map(n => ({ id: n.id, width: 120, height: 40 })),
    edges: elkEdges,
  };

  try {
    const result = await elk.layout(graph);
    const positions = {};
    for (const child of result.children ?? []) {
      positions[child.id] = {
        x: child.x + child.width / 2,
        y: child.y + child.height / 2,
      };
    }
    return positions;
  } catch (err) {
    // ELK failed (e.g., in test env without WASM) — fall back to column layout
    console.warn('[layout-engines] ELK layout failed, falling back to column layout:', err?.message ?? err);
    return columnLayout(nodes);
  }
}

// ─── Dispatcher ───────────────────────────────────────────────────────────────

/**
 * Compute positions for the given layout engine.
 * Always returns a Promise<{ [nodeId]: { x, y } }>.
 *
 * @param {'column'|'graph'|'hierarchical'|'layered'} engine
 */
export async function computeLayout(engine, nodes, edges, width, height) {
  if (engine === 'graph') return forceLayout(nodes, edges, width, height);
  if (engine === 'hierarchical') return elkLayout(nodes, edges, 'DOWN');
  if (engine === 'layered')      return elkLayout(nodes, edges, 'RIGHT');
  return columnLayout(nodes);
}
