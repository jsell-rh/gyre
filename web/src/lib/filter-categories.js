// Filter-category classification for the Explorer in-view filter panel
// (system-explorer.md §1, ui-navigation.md §3 "In-view filter panel").
//
// The panel's category checkboxes (Boundaries, Interfaces, Data, Specs)
// toggle node visibility on the canvas; the Concepts section lists
// predefined concepts discovered from the knowledge graph (name tokens
// shared across multiple nodes — cross-cutting concerns like "auth" or
// "retry"). The classification and concept discovery live here so the
// mappings are unit-testable and the components consume the same code
// path (same pattern as canvas-filters.js).

// Node types per category. Mirrors the type vocabulary the knowledge
// graph extraction emits (GraphNode.node_type, lowercase).
const BOUNDARY_TYPES = new Set(['module', 'package', 'crate', 'namespace', 'boundary', 'component']);
const INTERFACE_TYPES = new Set(['trait', 'interface', 'protocol', 'abstract_class', 'endpoint', 'handler']);
const DATA_TYPES = new Set(['struct', 'enum', 'type', 'table', 'model', 'class', 'union', 'field']);
const SPEC_TYPES = new Set(['spec']);

/** Category ids in panel order (Concepts is separate — see discoverConcepts). */
export const CATEGORY_IDS = ['boundaries', 'interfaces', 'data', 'specs'];

/**
 * Classify a graph node into a filter-panel category.
 * @param {object} node Graph node (node_type, spec_path).
 * @returns {'boundaries'|'interfaces'|'data'|'specs'|null} Category id, or
 *   null when the node's type is none of the four categories (functions,
 *   methods, constants… stay visible regardless of category checkboxes —
 *   they are the leaf content inside the categories).
 */
export function nodeCategory(node) {
  const nt = String(node?.node_type ?? '').toLowerCase();
  if (BOUNDARY_TYPES.has(nt)) return 'boundaries';
  if (INTERFACE_TYPES.has(nt)) return 'interfaces';
  if (DATA_TYPES.has(nt)) return 'data';
  if (SPEC_TYPES.has(nt)) return 'specs';
  return null;
}

// Stopwords: generic code vocabulary that names no concept. A token must
// appear in multiple distinct nodes to count as a concept, and these words
// describe mechanics (get/set/manager/…) rather than domain concerns.
const CONCEPT_STOPWORDS = new Set([
  'new', 'get', 'set', 'add', 'del', 'delete', 'remove', 'update', 'create',
  'list', 'find', 'check', 'run', 'make', 'handle', 'handler', 'manager',
  'service', 'controller', 'wrapper', 'builder', 'factory', 'helper',
  'utils', 'util', 'common', 'core', 'main', 'test', 'tests', 'mock',
  'data', 'info', 'config', 'state', 'status', 'type', 'types', 'name',
  'id', 'ids', 'error', 'errors', 'result', 'results', 'value', 'values',
  'item', 'items', 'index', 'count', 'size', 'len', 'length', 'default',
  'public', 'private', 'internal', 'spec', 'specs', 'api', 'app', 'lib',
  'src', 'web', 'server', 'client', 'impl', 'impls', 'detail', 'details',
  'the', 'and', 'for', 'with', 'from', 'into', 'this', 'that', 'self',
]);

/** Split an identifier into lowercase word tokens.
 *  "PaymentRetryPolicy" → ["payment","retry","policy"];
 *  "graph_store" → ["graph","store"]; "a::b" → ["a","b"]. */
function nameTokens(name) {
  return String(name ?? '')
    .split(/[^a-zA-Z0-9]+/)           // snake/kebab/qualified separators
    .map((seg) => seg.replace(/([a-z0-9])([A-Z])/g, '$1 $2')) // camelCase
    .join(' ')
    .split(/[^a-zA-Z0-9]+/)
    .map((t) => t.toLowerCase())
    .filter((t) => t.length >= 3 && !CONCEPT_STOPWORDS.has(t) && !/^\d+$/.test(t));
}

/**
 * Discover predefined concepts from the knowledge graph: name tokens that
 * occur in multiple distinct nodes are cross-cutting concerns (e.g. every
 * node mentioning "auth" forms the Authentication concept). The same
 * substring semantics as GET /repos/:id/graph/concept/:name decide node
 * membership, so a panel concept and a concept search agree.
 *
 * @param {object[]} nodes Graph nodes.
 * @param {{ minNodes?: number, maxConcepts?: number }} [opts]
 * @returns {{ name: string, count: number }[]}
 */
export function discoverConcepts(nodes, opts = {}) {
  const minNodes = opts.minNodes ?? 2;
  const maxConcepts = opts.maxConcepts ?? 12;
  const byToken = new Map();
  for (const n of nodes ?? []) {
    if (!n || n.deleted_at) continue;
    const name = n.qualified_name ?? n.name ?? '';
    const tokens = new Set(nameTokens(name));
    for (const t of tokens) {
      if (!byToken.has(t)) byToken.set(t, new Set());
      byToken.get(t).add(n.id ?? name);
    }
  }
  const concepts = [];
  for (const [token, ids] of byToken) {
    if (ids.size >= minNodes) concepts.push({ name: token, count: ids.size });
  }
  concepts.sort((a, b) => (b.count - a.count) || a.name.localeCompare(b.name));
  return concepts.slice(0, maxConcepts);
}

/**
 * Does a node belong to a concept? Same matching as the server's
 * GET /repos/:id/graph/concept/:name — case-insensitive substring on
 * name or qualified_name.
 * @param {object} node
 * @param {string} concept
 */
export function nodeMatchesConcept(node, concept) {
  const c = String(concept ?? '').toLowerCase();
  if (!c) return false;
  const name = String(node?.name ?? '').toLowerCase();
  const qn = String(node?.qualified_name ?? '').toLowerCase();
  return name.includes(c) || qn.includes(c);
}

/**
 * Node ids hidden by the filter panel's current state.
 *
 * A node is hidden when its category checkbox is unchecked, or when
 * concepts are selected and the node matches none of them (concept
 * selection focuses the canvas on the concept's elements).
 *
 * @param {object[]} nodes Graph nodes.
 * @param {{ hiddenCategories?: string[]|Set<string>, concepts?: string[] }} state
 * @returns {Set<string>}
 */
export function hiddenNodeIds(nodes, state = {}) {
  const hidden = new Set(
    Array.isArray(state.hiddenCategories)
      ? state.hiddenCategories
      : [...(state.hiddenCategories ?? [])]
  );
  const concepts = (state.concepts ?? []).filter(Boolean);
  const out = new Set();
  if (hidden.size === 0 && concepts.length === 0) return out;
  for (const n of nodes ?? []) {
    if (!n || n.deleted_at) continue;
    if (hidden.size > 0 && hidden.has(nodeCategory(n))) {
      out.add(n.id);
      continue;
    }
    if (concepts.length > 0 && !concepts.some((c) => nodeMatchesConcept(n, c))) {
      out.add(n.id);
    }
  }
  return out;
}

/**
 * Per-category node counts for the panel's badges.
 * @param {object[]} nodes
 * @returns {Record<string, number>}
 */
export function categoryCounts(nodes) {
  const counts = { boundaries: 0, interfaces: 0, data: 0, specs: 0 };
  for (const n of nodes ?? []) {
    if (!n || n.deleted_at) continue;
    const c = nodeCategory(n);
    if (c) counts[c] += 1;
  }
  return counts;
}
