/**
 * Client-side validation for ViewQuery objects.
 *
 * Mirrors the critical checks from Rust's ViewQuery::validate()
 * (crates/gyre-common/src/view_query.rs) — focusing on checks that
 * would cause rendering errors if the query were applied as-is.
 *
 * @typedef {import('./types/view-query.ts').ViewQuery} ViewQuery
 */

const MAX_DEPTH = 100;

const VALID_SCOPE_TYPES = ['all', 'focus', 'filter', 'test_gaps', 'diff', 'concept'];

const KNOWN_EDGE_TYPES = [
  'calls', 'contains', 'implements', 'depends_on', 'dependson',
  'field_of', 'fieldof', 'returns', 'routes_to', 'routesto',
  'renders', 'persists_to', 'persiststo', 'governed_by', 'governedby',
  'produced_by', 'producedby',
];

const KNOWN_HEAT_METRICS = [
  'complexity', 'churn', 'churn_count_30d', 'incoming_calls',
  'outgoing_calls', 'test_coverage', 'field_count', 'test_fragility',
  'risk_score', 'span_duration', 'span_count', 'error_rate',
];

const VALID_DIRECTIONS = ['incoming', 'outgoing', 'both'];

const VALID_ZOOM_NAMES = ['fit', 'current'];

/**
 * Validate a ViewQuery and return a result with validity flag and error list.
 *
 * @param {unknown} query - The query object to validate.
 * @returns {{ valid: boolean; errors: string[] }}
 */
export function validateViewQuery(query) {
  const errors = [];

  // Must be an object
  if (!query || typeof query !== 'object' || Array.isArray(query)) {
    errors.push('View query must be a non-null object');
    return { valid: false, errors };
  }

  const q = /** @type {Record<string, unknown>} */ (query);

  // scope is required
  if (!q.scope || typeof q.scope !== 'object') {
    errors.push("Missing or invalid 'scope' field — must be an object with a 'type' property");
    return { valid: false, errors };
  }

  const scope = /** @type {Record<string, unknown>} */ (q.scope);

  // scope.type must be a known value
  if (typeof scope.type !== 'string' || !VALID_SCOPE_TYPES.includes(scope.type)) {
    errors.push(
      `Unknown scope type '${scope.type}' — must be one of: ${VALID_SCOPE_TYPES.join(', ')}`
    );
  }

  // Scope-specific validation
  validateScope(scope, errors);

  // Validate edge filter
  if (q.edges && typeof q.edges === 'object') {
    const edgeFilter = /** @type {Record<string, unknown>} */ (q.edges);
    validateEdgeList(edgeFilter.filter, 'edges.filter', errors);
    validateEdgeList(edgeFilter.exclude, 'edges.exclude', errors);
  }

  // Validate emphasis
  if (q.emphasis && typeof q.emphasis === 'object') {
    validateEmphasis(/** @type {Record<string, unknown>} */ (q.emphasis), errors);
  }

  // Validate zoom
  if (q.zoom !== undefined) {
    validateZoom(q.zoom, errors);
  }

  // Validate callout colors
  if (Array.isArray(q.callouts)) {
    for (const callout of q.callouts) {
      if (callout && typeof callout.color === 'string' && !isValidColor(callout.color)) {
        errors.push(
          `Invalid color '${callout.color}' in callout for node '${callout.node ?? '?'}' — expected hex, named CSS color, or hsl/rgb function`
        );
      }
    }
  }

  // Validate group colors
  if (Array.isArray(q.groups)) {
    for (const group of q.groups) {
      if (group && typeof group.color === 'string' && !isValidColor(group.color)) {
        errors.push(
          `Invalid color '${group.color}' in group '${group.name ?? '?'}' — expected hex, named CSS color, or hsl/rgb function`
        );
      }
    }
  }

  // Validate narrative step order uniqueness
  if (Array.isArray(q.narrative) && q.narrative.length > 0) {
    const seenOrders = new Set();
    for (const step of q.narrative) {
      if (step && typeof step.order === 'number') {
        if (seenOrders.has(step.order)) {
          errors.push(
            `Duplicate narrative step order ${step.order} for node '${step.node ?? '?'}' — each step must have a unique order`
          );
        }
        seenOrders.add(step.order);
      }
    }
  }

  return { valid: errors.length === 0, errors };
}

/**
 * Validate scope-specific fields.
 * @param {Record<string, unknown>} scope
 * @param {string[]} errors
 */
function validateScope(scope, errors) {
  switch (scope.type) {
    case 'focus': {
      if (typeof scope.node !== 'string' || scope.node === '') {
        errors.push("Focus scope 'node' field must not be empty");
      }
      if (typeof scope.depth === 'number' && scope.depth > MAX_DEPTH) {
        errors.push(`Focus depth ${scope.depth} exceeds maximum of ${MAX_DEPTH}`);
      }
      if (typeof scope.direction === 'string' && !VALID_DIRECTIONS.includes(scope.direction)) {
        errors.push(
          `Invalid direction '${scope.direction}' in Focus scope — must be one of: ${VALID_DIRECTIONS.join(', ')}`
        );
      }
      validateEdgeList(scope.edges, 'Focus scope edges', errors);
      break;
    }
    case 'concept': {
      if (!Array.isArray(scope.seed_nodes) || scope.seed_nodes.length === 0) {
        errors.push("Concept scope 'seed_nodes' must not be empty");
      }
      if (typeof scope.expand_depth === 'number' && scope.expand_depth > MAX_DEPTH) {
        errors.push(`Concept expand_depth ${scope.expand_depth} exceeds maximum of ${MAX_DEPTH}`);
      }
      if (typeof scope.expand_direction === 'string' && !VALID_DIRECTIONS.includes(scope.expand_direction)) {
        errors.push(
          `Invalid expand_direction '${scope.expand_direction}' in Concept scope — must be one of: ${VALID_DIRECTIONS.join(', ')}`
        );
      }
      validateEdgeList(scope.expand_edges, 'Concept scope expand_edges', errors);
      break;
    }
    case 'diff': {
      if (typeof scope.from_commit !== 'string' || scope.from_commit === '') {
        errors.push("Diff scope 'from_commit' must not be empty");
      }
      if (typeof scope.to_commit !== 'string' || scope.to_commit === '') {
        errors.push("Diff scope 'to_commit' must not be empty");
      }
      if (
        typeof scope.from_commit === 'string' &&
        typeof scope.to_commit === 'string' &&
        scope.from_commit !== '' &&
        scope.from_commit === scope.to_commit
      ) {
        errors.push(
          `Diff from_commit and to_commit are identical ('${scope.from_commit}') — diff will be empty`
        );
      }
      break;
    }
    // 'all', 'test_gaps', 'filter' — no required fields that would break rendering
  }
}

/**
 * Validate emphasis fields.
 * @param {Record<string, unknown>} emphasis
 * @param {string[]} errors
 */
function validateEmphasis(emphasis, errors) {
  // dim_unmatched range
  if (typeof emphasis.dim_unmatched === 'number') {
    if (emphasis.dim_unmatched < 0 || emphasis.dim_unmatched > 1) {
      errors.push(
        `dim_unmatched ${emphasis.dim_unmatched} is out of range — must be between 0.0 and 1.0`
      );
    }
  }

  // tiered_colors must be non-empty array when present
  if (emphasis.tiered_colors !== undefined) {
    if (!Array.isArray(emphasis.tiered_colors) || emphasis.tiered_colors.length === 0) {
      errors.push('tiered_colors array must not be empty when provided');
    } else {
      for (const c of emphasis.tiered_colors) {
        if (typeof c === 'string' && !isValidColor(c)) {
          errors.push(
            `Invalid color '${c}' in tiered_colors — expected hex (#rgb, #rrggbb, #rrggbbaa), named CSS color, or hsl/rgb function`
          );
        }
      }
    }
  }

  // heat metric
  if (emphasis.heat && typeof emphasis.heat === 'object') {
    const heat = /** @type {Record<string, unknown>} */ (emphasis.heat);
    if (typeof heat.metric === 'string' && heat.metric !== '' && !KNOWN_HEAT_METRICS.includes(heat.metric)) {
      errors.push(
        `Unknown heat metric '${heat.metric}' — known metrics: ${KNOWN_HEAT_METRICS.join(', ')}`
      );
    }
  }

  // highlight.matched.color
  if (emphasis.highlight && typeof emphasis.highlight === 'object') {
    const highlight = /** @type {Record<string, unknown>} */ (emphasis.highlight);
    if (highlight.matched && typeof highlight.matched === 'object') {
      const matched = /** @type {Record<string, unknown>} */ (highlight.matched);
      if (typeof matched.color === 'string' && !isValidColor(matched.color)) {
        errors.push(
          `Invalid color '${matched.color}' in highlight.matched — expected hex, named CSS color, or hsl/rgb function`
        );
      }
    }
  }

  // badge template length
  if (emphasis.badges && typeof emphasis.badges === 'object') {
    const badges = /** @type {Record<string, unknown>} */ (emphasis.badges);
    if (typeof badges.template === 'string' && badges.template.length > 500) {
      errors.push(
        `Badge template length ${badges.template.length} exceeds maximum of 500 characters`
      );
    }
  }
}

/**
 * Validate zoom value.
 * @param {unknown} zoom
 * @param {string[]} errors
 */
function validateZoom(zoom, errors) {
  if (typeof zoom === 'string') {
    // Check if it's a number-as-string (serde(untagged) edge case)
    if (!isNaN(Number(zoom)) && zoom !== '') {
      errors.push(
        `Zoom value '${zoom}' looks like a number — use {"level": ${zoom}} instead of a plain string`
      );
    } else if (!VALID_ZOOM_NAMES.includes(zoom)) {
      errors.push(
        `Unknown zoom value '${zoom}' — must be "fit", "current", or {"level": N}`
      );
    }
  } else if (zoom && typeof zoom === 'object') {
    const z = /** @type {Record<string, unknown>} */ (zoom);
    if (typeof z.level === 'number') {
      if (z.level < 0.05 || z.level > 20.0) {
        errors.push(
          `Zoom level ${z.level} is out of range — must be between 0.05 and 20.0`
        );
      }
    }
  }
}

/**
 * Validate an array of edge type strings.
 * @param {unknown} edges
 * @param {string} fieldName
 * @param {string[]} errors
 */
function validateEdgeList(edges, fieldName, errors) {
  if (!Array.isArray(edges)) return;
  for (const e of edges) {
    if (typeof e === 'string' && !KNOWN_EDGE_TYPES.includes(e.toLowerCase())) {
      errors.push(
        `Unknown edge type '${e}' in ${fieldName} — known types: ${KNOWN_EDGE_TYPES.join(', ')}`
      );
    }
  }
}

// ── Color validation ─────────────────────────────────────────────────────────

const CSS_NAMED_COLORS = new Set([
  'aliceblue', 'antiquewhite', 'aqua', 'aquamarine', 'azure', 'beige',
  'bisque', 'black', 'blanchedalmond', 'blue', 'blueviolet', 'brown',
  'burlywood', 'cadetblue', 'chartreuse', 'chocolate', 'coral',
  'cornflowerblue', 'cornsilk', 'crimson', 'cyan', 'darkblue', 'darkcyan',
  'darkgoldenrod', 'darkgray', 'darkgreen', 'darkgrey', 'darkkhaki',
  'darkmagenta', 'darkolivegreen', 'darkorange', 'darkorchid', 'darkred',
  'darksalmon', 'darkseagreen', 'darkslateblue', 'darkslategray',
  'darkslategrey', 'darkturquoise', 'darkviolet', 'deeppink', 'deepskyblue',
  'dimgray', 'dimgrey', 'dodgerblue', 'firebrick', 'floralwhite',
  'forestgreen', 'fuchsia', 'gainsboro', 'ghostwhite', 'gold', 'goldenrod',
  'gray', 'green', 'greenyellow', 'grey', 'honeydew', 'hotpink', 'indianred',
  'indigo', 'ivory', 'khaki', 'lavender', 'lavenderblush', 'lawngreen',
  'lemonchiffon', 'lightblue', 'lightcoral', 'lightcyan',
  'lightgoldenrodyellow', 'lightgray', 'lightgreen', 'lightgrey', 'lightpink',
  'lightsalmon', 'lightseagreen', 'lightskyblue', 'lightslategray',
  'lightslategrey', 'lightsteelblue', 'lightyellow', 'lime', 'limegreen',
  'linen', 'magenta', 'maroon', 'mediumaquamarine', 'mediumblue',
  'mediumorchid', 'mediumpurple', 'mediumseagreen', 'mediumslateblue',
  'mediumspringgreen', 'mediumturquoise', 'mediumvioletred', 'midnightblue',
  'mintcream', 'mistyrose', 'moccasin', 'navajowhite', 'navy', 'oldlace',
  'olive', 'olivedrab', 'orange', 'orangered', 'orchid', 'palegoldenrod',
  'palegreen', 'paleturquoise', 'palevioletred', 'papayawhip', 'peachpuff',
  'peru', 'pink', 'plum', 'powderblue', 'purple', 'red', 'rosybrown',
  'royalblue', 'saddlebrown', 'salmon', 'sandybrown', 'seagreen', 'seashell',
  'sienna', 'silver', 'skyblue', 'slateblue', 'slategray', 'slategrey',
  'snow', 'springgreen', 'steelblue', 'tan', 'teal', 'thistle', 'tomato',
  'transparent', 'turquoise', 'violet', 'wheat', 'white', 'whitesmoke',
  'yellow', 'yellowgreen',
]);

/**
 * Check if a string is a valid CSS color value.
 * Accepts: hex (#rgb, #rrggbb, #rrggbbaa), named CSS colors,
 * hsl/rgb/rgba/hsla functions.
 * @param {string} s
 * @returns {boolean}
 */
function isValidColor(s) {
  const trimmed = s.trim();
  if (!trimmed) return false;

  // Hex colors
  if (trimmed.startsWith('#')) {
    const hex = trimmed.slice(1);
    if (![3, 4, 6, 8].includes(hex.length)) return false;
    return /^[0-9a-fA-F]+$/.test(hex);
  }

  // CSS functions
  if (/^(?:rgb|rgba|hsl|hsla)\(/.test(trimmed)) {
    if (!trimmed.endsWith(')')) return false;
    const parenStart = trimmed.indexOf('(') + 1;
    const args = trimmed.slice(parenStart, -1);
    return /^[0-9,.\s%\-/]+$/.test(args);
  }

  // Named CSS colors
  return CSS_NAMED_COLORS.has(trimmed.toLowerCase());
}
