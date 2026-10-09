/**
 * TypeScript types for the ViewSpec grammar (ui-layout.md §4).
 *
 * These mirror the Rust types in crates/gyre-common/src/view_spec.rs.
 * The grammar is the interface through which LLMs create visualizations
 * and humans save/share views. Four layers:
 *
 *   data      — what to pull from the knowledge graph
 *   layout    — spatial arrangement
 *   encoding  — data attributes → visual properties
 *   highlight — emphasis on specific nodes/edges
 *
 * Grammar rules enforced by `validateViewSpec` (client-side mirror of the
 * server-side `validate_view_spec` — belt and suspenders):
 *   - layout must be a known name (closed set; new layouts are added via
 *     the layout registry, which extends this list)
 *   - `flow` requires `data.trace_source`
 *   - `data.filter.spec_path` requires `data.repo_id` (spec paths are repo-scoped)
 *   - `side-by-side` requires both `left` and `right` sub-views
 *   - side-by-side sub-views cannot themselves be `side-by-side` (max depth 1)
 *   - sub-views support `data`, `layout`, `encoding` only — no `name`,
 *     `annotations`, `explanation`, `highlight`, `description` (top-level-only)
 *   - no field inheritance: sub-views do not inherit `data` from the parent
 */

// ── Layout ───────────────────────────────────────────────────────────────────

/**
 * Layout names, exactly as they appear in serialized ViewSpec JSON
 * (kebab-case, matching the Rust `#[serde(rename_all = "kebab-case")]`).
 */
export const LAYOUT_TYPES = /** @type {const} */ ([
  'graph',
  'hierarchical',
  'layered',
  'list',
  'timeline',
  'side-by-side',
  'diff',
  'flow',
]);

/** @typedef {(typeof LAYOUT_TYPES)[number]} LayoutType */

// ── Data layer ───────────────────────────────────────────────────────────────

/**
 * @typedef {Object} DataFilter
 * @property {number} [min_churn]      Only nodes with churn_count_30d >= min_churn.
 * @property {string} [spec_path]      Only nodes linked to this spec. Requires repo_id.
 * @property {string} [visibility]     Only nodes with this visibility ('public' | 'private').
 */

/**
 * @typedef {Object} TraceSource
 * @property {string} [mr_id]          Use the most recent trace for this MR.
 * @property {string} [gate_run_id]    Use this specific gate run.
 */

/**
 * @typedef {Object} DataLayer
 * @property {string} [concept]        Substring match on node name / qualified_name.
 * @property {string[]} [node_types]   Filter to these node types. Empty = all.
 * @property {string[]} [edge_types]   Include only these edge types. Empty = all.
 * @property {number} [depth]          Traversal depth from matching nodes.
 * @property {DataFilter} [filter]
 * @property {string} [repo_id]        Scope to a single repo. Null = workspace-wide.
 * @property {TraceSource} [trace_source] Required for 'flow' layout.
 */

// ── Encoding layer ───────────────────────────────────────────────────────────

/**
 * Field-to-scale mapping for color-ish encodings, e.g.
 *   { field: 'node_type', scale: 'categorical' }
 *   { field: 'spec_confidence', scale: { high: '#22c55e', ... } }
 *
 * @typedef {Object} FieldScale
 * @property {string} field
 * @property {string|Object.<string, string>} scale
 */

/**
 * @typedef {Object} EncodingLayer
 * @property {FieldScale} [color]           Node fill color.
 * @property {FieldScale} [size]            Node size (linear scale with range).
 * @property {FieldScale} [border]          Node border color/style.
 * @property {FieldScale} [opacity]         Node transparency.
 * @property {string} [label]               Field displayed on/below the node.
 * @property {string} [group_by]            Visual grouping key.
 * @property {FieldScale} [edge_color]      Edge stroke color by type.
 * @property {FieldScale} [edge_style]      Edge stroke style by type.
 * @property {FieldScale} [particle_color]  Particle fill color in 'flow' layout.
 * @property {FieldScale} [particle_speed]  Particle animation speed in 'flow' layout.
 * @property {FieldScale} [node_badge]      Aggregate metric badge in 'flow' layout.
 */

// ── Highlight layer ──────────────────────────────────────────────────────────

/**
 * @typedef {Object} HighlightLayer
 * @property {string} [spec_path]     Highlight all nodes governed by this spec.
 * @property {string[]} [node_ids]    Highlight specific nodes by ID.
 * @property {string[]} [edge_types]  Highlight edges of these types.
 */

// ── Annotations ──────────────────────────────────────────────────────────────

/**
 * @typedef {Object} Annotation
 * @property {string} node_name
 * @property {string} text
 */

// ── Sub-view (side-by-side) ──────────────────────────────────────────────────

/**
 * Reduced view spec for use within a `side-by-side` layout.
 * Only `data`, `layout`, and `encoding` are permitted — no nesting,
 * no top-level-only fields, no inheritance of parent `data`.
 *
 * @typedef {Object} SubViewSpec
 * @property {DataLayer} data
 * @property {LayoutType} layout
 * @property {EncodingLayer} [encoding]
 */

// ── Top-level ViewSpec ───────────────────────────────────────────────────────

/**
 * @typedef {Object} ViewSpec
 * @property {string} name
 * @property {string} [description]
 * @property {DataLayer} data
 * @property {LayoutType} layout
 * @property {EncodingLayer} [encoding]
 * @property {Annotation[]} [annotations]
 * @property {HighlightLayer} [highlight]
 * @property {string} [explanation]
 * @property {SubViewSpec} [left]   Left sub-view for 'side-by-side'.
 * @property {SubViewSpec} [right]  Right sub-view for 'side-by-side'.
 */

// ── Validation ───────────────────────────────────────────────────────────────

/** Fields a sub-view may declare (ui-layout.md §4 composability rules). */
const SUB_VIEW_ALLOWED_FIELDS = /** @type {const} */ ({
  data: true,
  layout: true,
  encoding: true,
});

/**
 * Validate a ViewSpec against the grammar from ui-layout.md §4.
 * Client-side mirror of the Rust `validate_view_spec` — the server rejects
 * the same cases with 400; this keeps the renderer from crashing on
 * hand-edited or malformed specs by showing an error instead.
 *
 * @param {unknown} spec
 * @returns {{ valid: boolean, errors: string[] }}
 */
export function validateViewSpec(spec) {
  const errors = [];
  if (!isObject(spec)) {
    return { valid: false, errors: ['view spec must be an object'] };
  }

  if (typeof spec.name !== 'string' || spec.name.length === 0) {
    errors.push('name is required and must be a non-empty string');
  }

  if (!isObject(spec.data)) {
    errors.push('data layer is required');
  } else {
    validateDataLayer(spec.data, errors);
  }

  const layout = spec.layout;
  if (!LAYOUT_TYPES.includes(layout)) {
    errors.push(
      `unknown layout ${JSON.stringify(layout)} — known layouts: ${LAYOUT_TYPES.join(', ')}`
    );
  } else {
    // 'flow' requires trace_source.
    if (layout === 'flow' && !spec.data?.trace_source) {
      errors.push("layout 'flow' requires data.trace_source");
    }
    if (layout === 'side-by-side') {
      validateSideBySide(spec, errors);
    } else if (spec.left !== undefined || spec.right !== undefined) {
      // `left`/`right` are meaningful only for 'side-by-side'. On any other
      // layout they are smuggled content no renderer consumes — mirror of
      // the Rust validate_view_spec rule.
      errors.push("'left'/'right' sub-views are only allowed with layout 'side-by-side'");
    }
  }

  if (spec.encoding !== undefined && !isObject(spec.encoding)) {
    errors.push('encoding must be an object');
  }

  return { valid: errors.length === 0, errors };
}

/**
 * @param {Record<string, unknown>} data
 * @param {string[]} errors
 */
function validateDataLayer(data, errors) {
  // filter.spec_path requires repo_id (spec paths are repo-scoped).
  if (isObject(data.filter) && data.filter.spec_path != null && data.repo_id == null) {
    errors.push('data.filter.spec_path requires data.repo_id');
  }
  if (data.trace_source !== undefined && !isObject(data.trace_source)) {
    errors.push('data.trace_source must be an object ({mr_id?, gate_run_id?})');
  }
}

/**
 * @param {Record<string, unknown>} spec
 * @param {string[]} errors
 */
function validateSideBySide(spec, errors) {
  const { left, right } = spec;
  if (!isObject(left) || !isObject(right)) {
    errors.push("layout 'side-by-side' requires both 'left' and 'right' sub-views");
    return;
  }
  for (const [side, sub] of /** @type {[string, Record<string, unknown>][]} */ ([
    ['left', left],
    ['right', right],
  ])) {
    // Sub-views support data/layout/encoding only.
    for (const key of Object.keys(sub)) {
      if (!SUB_VIEW_ALLOWED_FIELDS[key]) {
        errors.push(
          `side-by-side sub-view '${side}' declares top-level-only field '${key}' ` +
            '(sub-views support data, layout, encoding only)'
        );
      }
    }
    if (!isObject(sub.data)) {
      errors.push(`side-by-side sub-view '${side}' requires its own data layer`);
      continue;
    }
    // No field inheritance: each sub-view must declare its own repo_id
    // when using filter.spec_path — the parent's repo_id does not apply.
    validateDataLayer(sub.data, errors);
    if (!LAYOUT_TYPES.includes(sub.layout)) {
      errors.push(`side-by-side sub-view '${side}' has unknown layout ${JSON.stringify(sub.layout)}`);
    } else if (sub.layout === 'side-by-side') {
      // Nesting depth limit: max composition depth is 1.
      errors.push('side-by-side sub-views cannot contain side-by-side layouts');
    } else if (sub.layout === 'flow' && !sub.data.trace_source) {
      errors.push(`side-by-side sub-view '${side}' uses layout 'flow' without data.trace_source`);
    }
  }
}

/**
 * @param {unknown} v
 * @returns {v is Record<string, unknown>}
 */
function isObject(v) {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

// ── Type guards ──────────────────────────────────────────────────────────────

/**
 * @param {unknown} spec
 * @returns {spec is ViewSpec}
 */
export function isViewSpec(spec) {
  return validateViewSpec(spec).valid;
}
