/**
 * Generated-view helpers (HSI §3 "Generated Views (LLM-Powered, On-Demand)",
 * task-089).
 *
 * The workspace endpoint `POST /workspaces/:id/explorer-views/generate`
 * returns Server-Sent Events carrying a ViewSpec (ui-layout.md §4 grammar:
 * data + layout + encoding + annotations + explanation). The ExplorerCanvas
 * renders ViewQuery documents (view-query-grammar.md), so generated views
 * need a translation — that is what `viewSpecToViewQuery` does. It is a
 * lossy projection by design: the ViewSpec remains the source of truth (it
 * is what gets saved), the ViewQuery is only the canvas instruction.
 */

/**
 * Normalize a ViewSpec node type to the graph's lowercase form.
 * ViewSpec examples use TitleCase ("Module", "Endpoint"); graph nodes and
 * ViewQuery filters use lowercase ("module", "endpoint").
 * @param {string} t
 * @returns {string}
 */
export function normalizeNodeType(t) {
  return String(t ?? '').trim().toLowerCase();
}

/**
 * Normalize a ViewSpec edge type to the graph's snake_case form.
 * ViewSpec examples use CamelCase ("RoutesTo", "Contains"); GraphEdge
 * serializes as snake_case ("routes_to", "contains").
 * @param {string} t
 * @returns {string}
 */
export function normalizeEdgeType(t) {
  const s = String(t ?? '').trim();
  // "RoutesTo" → "routes_to" (insert _ before interior capitals), then
  // lowercase. Already-snake names ("routes_to") pass through unchanged.
  return s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
}

/**
 * Translate a generated ViewSpec into a ViewQuery the ExplorerCanvas can
 * render.
 *
 * Mapping (ViewSpec → ViewQuery):
 *   data.concept      → scope {type:'concept', seed_nodes:[concept],
 *                       seed_match:'substring', expand_edges,
 *                       expand_depth: data.depth}
 *                       (ui-layout.md §4 Data Layer: the ViewSpec `concept`
 *                       field is a case-insensitive SUBSTRING search on node
 *                       name/qualified_name — a text search, not the
 *                       manifest-concept mechanism — so the translated
 *                       concept scope marks its seeds for substring
 *                       matching; exact seed matching remains the default
 *                       for ad-hoc groupings and grammar-example queries)
 *   data.node_types   → scope {type:'filter', node_types}   (when no concept)
 *   data.filter.spec_path → scope {type:'filter', computed:"$governed_by('…')"}
 *   (none of the above) → scope {type:'all'}
 *   annotations       → callouts [{node, text}]
 *   name+explanation  → annotation {title: name, description: explanation}
 *                       (the canvas renders this as the sidebar annotation
 *                       alongside the graph, per HSI §3)
 *
 * @param {object} spec ViewSpec from the generate endpoint's complete event.
 * @returns {object|null} A ViewQuery, or null when spec is not an object.
 */
export function viewSpecToViewQuery(spec) {
  if (!spec || typeof spec !== 'object' || Array.isArray(spec)) return null;
  const data = spec.data ?? {};

  let scope;
  if (typeof data.concept === 'string' && data.concept.trim() !== '') {
    scope = {
      type: 'concept',
      seed_nodes: [data.concept.trim()],
      // ViewSpec data.concept is the substring mechanism (ui-layout.md §4):
      // 'auth' seeds every node whose name/qualified_name contains it.
      seed_match: 'substring',
      expand_direction: 'both',
    };
    const edges = (Array.isArray(data.edge_types) ? data.edge_types : [])
      .map(normalizeEdgeType)
      .filter(Boolean);
    if (edges.length > 0) scope.expand_edges = edges;
    // ui-layout.md §4: depth 0 = matched nodes only (no expansion), so a
    // depth of 0 pins expand_depth 0 rather than falling through to the
    // concept scope's default expansion depth.
    if (typeof data.depth === 'number' && data.depth >= 0) {
      scope.expand_depth = Math.min(Math.floor(data.depth), 100);
    }
  } else if (Array.isArray(data.node_types) && data.node_types.length > 0) {
    scope = {
      type: 'filter',
      node_types: data.node_types.map(normalizeNodeType).filter(Boolean),
    };
  } else if (typeof data.filter?.spec_path === 'string' && data.filter.spec_path.trim() !== '') {
    // Escape embedded quotes so the spec path cannot break out of the
    // computed-expression string literal.
    const escaped = data.filter.spec_path.trim().replace(/'/g, "\\'");
    scope = { type: 'filter', computed: `$governed_by('${escaped}')` };
  } else {
    scope = { type: 'all' };
  }

  const query = { scope };

  // annotations → callouts (the canvas draws these pinned to nodes)
  if (Array.isArray(spec.annotations) && spec.annotations.length > 0) {
    query.callouts = spec.annotations
      .filter(
        (a) =>
          a &&
          typeof a.node_name === 'string' &&
          a.node_name.trim() !== '' &&
          typeof a.text === 'string' &&
          a.text.trim() !== ''
      )
      .map((a) => ({ node: a.node_name, text: a.text }));
    if (query.callouts.length === 0) delete query.callouts;
  }

  // name + explanation → the canvas query annotation (sidebar annotation)
  const title = typeof spec.name === 'string' && spec.name.trim() !== '' ? spec.name.trim() : null;
  const explanation =
    typeof spec.explanation === 'string' && spec.explanation.trim() !== ''
      ? spec.explanation.trim()
      : null;
  if (title || explanation) {
    query.annotation = {
      ...(title ? { title } : {}),
      ...(explanation ? { description: explanation } : {}),
    };
  }

  query.zoom = 'fit';
  return query;
}

/**
 * Translate a stored explorer-view document into the ViewQuery the canvas
 * renders. Workspace explorer-views accept BOTH grammars (the server's
 * create/update validation classifies by top-level keys — view-query-
 * grammar.md ViewQuery or ui-layout.md §4 ViewSpec), so a stored record's
 * `spec` may be either:
 *
 *   - a ViewQuery (e.g. an ad-hoc concept grouping saved by the canvas'
 *     "Create concept" dialog — scope.type 'concept' with exact seeds):
 *     returned as a deep copy, concept scope intact;
 *   - a ViewSpec (e.g. a generated view saved from the Ask flow): passed
 *     through viewSpecToViewQuery.
 *
 * Without this classification a ViewQuery-shaped spec would be flattened to
 * {scope:{type:'all'}} by the ViewSpec translator — silently discarding the
 * saved concept grouping on reload.
 *
 * @param {object} spec Stored explorer-view document (either grammar).
 * @returns {object|null} A ViewQuery, or null when spec is not an object.
 */
export function viewDocumentToQuery(spec) {
  if (!spec || typeof spec !== 'object' || Array.isArray(spec)) return null;
  // ViewQuery grammar marker: a top-level `scope` object with a string
  // `type` (ViewSpec's top-level keys are name/data/layout/… — disjoint
  // from `scope`, per the server's grammar classification).
  if (
    spec.scope &&
    typeof spec.scope === 'object' &&
    !Array.isArray(spec.scope) &&
    typeof spec.scope.type === 'string'
  ) {
    return JSON.parse(JSON.stringify(spec));
  }
  return viewSpecToViewQuery(spec);
}

/**
 * Parse the SSE body of a generate call into its events.
 *
 * The endpoint emits `event: partial` (progress) and `event: complete`
 * (final payload: {view_spec, explanation, fallback?}) frames.
 *
 * @param {string} text Raw SSE body text.
 * @returns {{ partial: object|null, complete: object|null }}
 */
export function parseGenerateSse(text) {
  const out = { partial: null, complete: null };
  if (typeof text !== 'string') return out;
  // SSE frames are separated by blank lines; each frame's fields are
  // "event: <name>" and "data: <json>" lines.
  for (const frame of text.split(/\n\n+/)) {
    let event = null;
    let data = null;
    for (const line of frame.split('\n')) {
      if (line.startsWith('event:')) event = line.slice(6).trim();
      else if (line.startsWith('data:')) data = line.slice(5).trim();
    }
    if (event === null || data === null) continue;
    let parsed;
    try {
      parsed = JSON.parse(data);
    } catch {
      continue;
    }
    if (event === 'partial') out.partial = parsed;
    else if (event === 'complete') out.complete = parsed;
  }
  return out;
}

/**
 * Read a generate-endpoint fetch Response and return the parsed events.
 * Handles both streamed and fully-buffered bodies.
 * @param {Response} resp
 * @returns {Promise<{ partial: object|null, complete: object|null }>}
 */
export async function readGenerateSse(resp) {
  const text = await resp.text();
  return parseGenerateSse(text);
}
