// Flow-layout node positioning (ui-layout.md §4 Flow Layout). Pure logic —
// no DOM — so the span→position mapping is unit-testable without a
// component. Nodes are arranged in depth columns left→right along the
// graph's directed edges (data-flow direction); within a column nodes are
// stacked and vertically centered. Contains edges do not drive depth
// (containment is not data flow) but are preserved for drawing.

export const FLOW_NODE_W = 150;
export const FLOW_NODE_H = 30;
export const FLOW_COL_GAP = 90;
export const FLOW_ROW_GAP = 42;

function edgeSrc(e) { return e?.source_id ?? e?.from_node_id ?? e?.from; }
function edgeTgt(e) { return e?.target_id ?? e?.to_node_id ?? e?.to; }
function edgeType(e) { return (e?.edge_type ?? e?.type ?? '').toLowerCase(); }

/**
 * Layered flow layout for a node set.
 *
 * @param {Array<any>} nodes graph nodes ({id, name, node_type, ...})
 * @param {Array<any>} edges graph edges (any type; 'contains' ignored for depth)
 * @returns {{ items: Map<string, {id: string, node: any, x: number, y: number,
 *            w: number, h: number, depth: number}>, list: Array<same> }}
 *          `list` is ordered depth-then-y for deterministic rendering.
 */
export function layeredFlowLayout(nodes, edges) {
  const items = new Map();
  if (!nodes?.length) return { items, list: [] };

  const ids = new Set(nodes.map((n) => n.id));
  // Directed flow adjacency (exclude containment — it is structure, not
  // data flow) restricted to nodes present in the set.
  const outgoing = new Map(); // id → [target ids]
  const incoming = new Map(); // id → [source ids]
  for (const e of edges ?? []) {
    const src = edgeSrc(e);
    const tgt = edgeTgt(e);
    if (!src || !tgt || src === tgt) continue;
    if (!ids.has(src) || !ids.has(tgt)) continue;
    if (edgeType(e) === 'contains') continue;
    if (!outgoing.has(src)) outgoing.set(src, []);
    outgoing.get(src).push(tgt);
    if (!incoming.has(tgt)) incoming.set(tgt, []);
    incoming.get(tgt).push(src);
  }

  // Depth = longest path from a root (a node with no incoming flow edges).
  // Longest-path (not BFS) so a diamond's rejoining node lands in the
  // deeper column, keeping left→right edge direction acyclic in drawing.
  // Memoized DFS with a visiting set: a cycle's back-edge contributes 0, so
  // recursion always terminates instead of inflating depths.
  const depthMemo = new Map();
  function depthOf(id, visiting) {
    if (depthMemo.has(id)) return depthMemo.get(id);
    if (visiting.has(id)) return 0; // cycle back-edge
    visiting.add(id);
    let d = 0;
    for (const src of incoming.get(id) ?? []) {
      d = Math.max(d, depthOf(src, visiting) + 1);
    }
    visiting.delete(id);
    depthMemo.set(id, d);
    return d;
  }
  const depth = new Map();
  for (const n of nodes) depth.set(n.id, depthOf(n.id, new Set()));

  // Column buckets.
  const columns = new Map(); // depth → [nodes]
  for (const n of nodes) {
    const d = depth.get(n.id) ?? 0;
    if (!columns.has(d)) columns.set(d, []);
    columns.get(d).push(n);
  }

  const colX = (d) => d * (FLOW_NODE_W + FLOW_COL_GAP);
  for (const [d, colNodes] of columns) {
    // Deterministic order: name, then id.
    colNodes.sort((a, b) => (a.name ?? a.id).localeCompare(b.name ?? b.id));
    const x = colX(d);
    for (let i = 0; i < colNodes.length; i++) {
      const y = (i - (colNodes.length - 1) / 2) * (FLOW_NODE_H + FLOW_ROW_GAP);
      const item = {
        id: colNodes[i].id,
        node: colNodes[i],
        x,
        y,
        w: FLOW_NODE_W,
        h: FLOW_NODE_H,
        depth: d,
      };
      items.set(colNodes[i].id, item);
    }
  }

  const list = [...items.values()].sort((a, b) =>
    a.depth - b.depth || a.y - b.y || a.id.localeCompare(b.id));
  return { items, list };
}

/**
 * Cubic bezier path (world coordinates) from the source node's right edge
 * to the target node's left edge. Falls back to a bottom-loop shape when
 * the target sits left of the source (backward / same-column edges) so the
 * arrow never crosses the node bodies head-on.
 *
 * @param {{x: number, y: number, w: number, h: number}} s source layout item
 * @param {{x: number, y: number, w: number, h: number}} t target layout item
 * @returns {string} SVG path `d` attribute
 */
export function flowEdgePath(s, t) {
  const x1 = s.x + s.w / 2;
  const y1 = s.y;
  const x2 = t.x - t.w / 2;
  const y2 = t.y;
  if (x2 >= x1) {
    const mx = (x1 + x2) / 2;
    return `M ${x1} ${y1} C ${mx} ${y1} ${mx} ${y2} ${x2} ${y2}`;
  }
  // Backward edge: exit right, loop under, enter from the right side too.
  const yb = Math.max(s.y + s.h / 2, t.y + t.h / 2) + FLOW_ROW_GAP;
  const sx = s.x + s.w / 2 + 24;
  const tx = t.x + t.w / 2 + 24;
  return `M ${x1} ${y1} C ${sx} ${y1} ${sx} ${yb} ${s.x} ${yb} ` +
    `C ${tx} ${yb} ${tx} ${y2} ${t.x + t.w / 2} ${y2}`;
}
