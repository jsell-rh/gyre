/**
 * View-scope node resolution for the registry layout renderers
 * (ui-layout.md §10 Layout Engines).
 *
 * The unified ExplorerCanvas resolves ViewQuery scopes internally (query
 * matching, emphasis, dimming — see ExplorerCanvas.svelte). The registry
 * renderers (SvgGraphCanvas, SideBySideView, DiffView, the shell's list
 * fallback) take plain node/edge arrays, so the Explorer shell applies the
 * active view query's *scope* before handing data to them. This module is
 * that projection: it resolves the scope shapes a generated ViewSpec can
 * produce (see generated-view.js `viewSpecToViewQuery`) —
 *
 *   {type:'all'}                              → every node
 *   {type:'filter', node_types, name_pattern}  → predicate match
 *   {type:'filter', computed:"$governed_by('…')"} → spec-governed nodes
 *   {type:'concept', seed_nodes, …}            → seed match + edge BFS
 *
 * and returns `null` for anything else (no scope, or an unsupported scope
 * type), meaning "no projection — the caller shows the full graph". This
 * mirrors the canvas behavior for unresolved scopes (an unmatched scope
 * dims nothing and shows everything), so both renderers show the same node
 * set for the same ViewSpec.
 *
 * Semantics intentionally mirror ExplorerCanvas's `queryMatchedWithDepth`
 * for these scope shapes (including $governed_by, which resolves the same
 * way the canvas's `computeGovernedBy` does: spec_path match plus
 * governed_by edges into a spec node with that path/name).
 */

/**
 * Source node id of a graph edge (serialized under any of the client's
 * three field spellings).
 * @param {any} e
 * @returns {string}
 */
export function edgeSrc(e) {
  return e?.source_id ?? e?.from_node_id ?? e?.from;
}

/**
 * Target node id of a graph edge.
 * @param {any} e
 * @returns {string}
 */
export function edgeTgt(e) {
  return e?.target_id ?? e?.to_node_id ?? e?.to;
}

/** @typedef {import('./types/view-query.ts').ViewQuery} ViewQuery */

/**
 * $governed_by('path') — nodes whose spec_path matches, plus nodes with a
 * governed_by edge into a spec node carrying that path (or name).
 * @param {string} specPath
 * @param {any[]} nodes
 * @param {any[]} edges
 * @returns {Set<string>} matched node ids
 */
function governedBy(specPath, nodes, edges) {
  const result = new Set();
  for (const n of nodes) {
    if (n.deleted_at) continue;
    if (n.spec_path === specPath) result.add(n.id);
  }
  for (const e of edges) {
    if (e.deleted_at) continue;
    const et = String(e.edge_type ?? e.type ?? '').toLowerCase();
    if (et !== 'governed_by') continue;
    const tgtNode = nodes.find((n) => n.id === edgeTgt(e));
    if (tgtNode && (tgtNode.name === specPath || tgtNode.spec_path === specPath)) {
      const src = edgeSrc(e);
      if (src) result.add(src);
    }
  }
  return result;
}

/**
 * Concept scope: start from seed nodes and expand along the allowed edge
 * types up to the depth, honoring the direction. Mirrors the canvas's
 * concept traversal.
 *
 * Seed resolution (ui-layout.md §4 "Data source distinction"):
 *   - seed_match 'exact' (default): a seed names one node by name /
 *     qualified_name / id.
 *   - seed_match 'substring': a seed is a case-insensitive substring
 *     matched against every node's name and qualified_name — EVERY match
 *     becomes a seed (the ViewSpec `data.concept` mechanism, same
 *     predicate as the graph endpoint's `?concept=` filter).
 * @param {any} scope
 * @param {any[]} nodes
 * @param {any[]} edges
 * @returns {Set<string>} matched node ids
 */
function conceptScope(scope, nodes, edges) {
  const seeds = scope.seed_nodes ?? [];
  const substring = String(scope.seed_match ?? 'exact') === 'substring';
  const expandEdges = new Set(
    (scope.expand_edges ?? ['calls']).map((e) => String(e).toLowerCase())
  );
  const maxDepth = scope.expand_depth ?? 2;
  const dir = scope.expand_direction ?? 'both';

  // Adjacency: id → [{ targetId, reverse }] (reverse = traversed against
  // the edge direction, i.e. incoming). Only edges whose endpoints both
  // exist in the node set are linked.
  const nodeIds = new Set(nodes.map((n) => n.id));
  const adjacency = new Map();
  const link = (from, to, reverse) => {
    if (!adjacency.has(from)) adjacency.set(from, []);
    adjacency.get(from).push({ targetId: to, reverse });
  };
  for (const e of edges) {
    if (e.deleted_at) continue;
    const et = String(e.edge_type ?? e.type ?? '').toLowerCase();
    if (!expandEdges.has(et)) continue;
    const s = edgeSrc(e);
    const t = edgeTgt(e);
    if (!s || !t || !nodeIds.has(s) || !nodeIds.has(t)) continue;
    link(s, t, false);
    link(t, s, true);
  }

  const matched = new Set();
  const queue = [];
  for (const seedName of seeds) {
    // Resolve the seed to seed node(s): every substring match, or the one
    // node the seed names exactly.
    const seedNodes = substring
      ? nodes.filter(
          (n) =>
            String(n.name ?? '').toLowerCase().includes(seedName.toLowerCase()) ||
            String(n.qualified_name ?? '').toLowerCase().includes(seedName.toLowerCase())
        )
      : [nodes.find((n) => n.name === seedName || n.qualified_name === seedName || n.id === seedName)].filter(
          Boolean
        );
    for (const seedNode of seedNodes) {
      if (matched.has(seedNode.id)) continue;
      matched.add(seedNode.id);
      queue.push({ id: seedNode.id, depth: 0 });
    }
  }
  while (queue.length > 0) {
    const { id, depth } = queue.shift();
    if (depth >= maxDepth) continue;
    for (const nb of adjacency.get(id) ?? []) {
      if (matched.has(nb.targetId)) continue;
      if (dir === 'outgoing' && nb.reverse) continue;
      if (dir === 'incoming' && !nb.reverse) continue;
      matched.add(nb.targetId);
      queue.push({ id: nb.targetId, depth: depth + 1 });
    }
  }
  return matched;
}

/**
 * Resolve a ViewQuery scope to the node set a layout renderer should show.
 *
 * @param {ViewQuery['scope'] | null | undefined} scope
 * @param {any[]} nodes  full graph nodes
 * @param {any[]} edges  full graph edges
 * @returns {any[] | null} matched nodes, or null when the scope is absent
 *   or unsupported (caller shows the full graph — same as the canvas,
 *   which leaves unmatched scopes undimmed).
 */
export function resolveScopeNodes(scope, nodes, edges) {
  if (!scope || typeof scope !== 'object') return null;

  if (scope.type === 'all') {
    return nodes;
  }

  if (scope.type === 'filter') {
    let ids = null;
    if (typeof scope.computed === 'string' && scope.computed.trim() !== '') {
      // Only $governed_by('…') is producible from a generated ViewSpec
      // (viewSpecToViewQuery); other computed expressions resolve to null
      // here → full graph, matching the canvas's unmatched-scope behavior.
      const m = /^\$governed_by\((['"])((?:\\.|(?!\1).)*)\1\)$/.exec(scope.computed.trim());
      if (m) {
        ids = governedBy(m[2].replace(/\\'/g, "'"), nodes, edges);
        if (ids.size === 0) return null;
      }
    } else {
      const matched = nodes.filter((n) => {
        if (scope.node_types?.length && !scope.node_types.includes(n.node_type)) return false;
        if (scope.name_pattern) {
          const re = new RegExp(scope.name_pattern, 'i');
          if (!re.test(n.name ?? '') && !re.test(n.qualified_name ?? '')) return false;
        }
        return true;
      });
      if (matched.length === 0) return null;
      return matched;
    }
    if (!ids) return null;
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const result = [...ids].map((id) => byId.get(id)).filter(Boolean);
    return result.length > 0 ? result : null;
  }

  if (scope.type === 'concept') {
    const ids = conceptScope(scope, nodes, edges);
    if (ids.size === 0) return null;
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const result = [...ids].map((id) => byId.get(id)).filter(Boolean);
    return result.length > 0 ? result : null;
  }

  return null;
}
