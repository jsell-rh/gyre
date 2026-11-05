// Canvas state wire-format helpers (view-query-grammar.md §10).
//
// The internal explorer canvas state uses camelCase (`selectedNode`, `zoom`)
// plus rendering-only fields (`breadcrumb`, `visibleGroups`). The server's
// CanvasState struct expects the §10 snake_case shape. These helpers convert
// and normalize so the agent receives exactly the specced interaction context.

/**
 * Convert the internal canvas state object into the §10 wire format sent with
 * each WebSocket user message.
 *
 * Emits: selected_node, zoom_level, visible_tree_groups, active_filter,
 * active_lens, active_query, recent_interactions. Tolerates inputs that are
 * already partially normalized (snake_case keys win over camelCase).
 *
 * @param {object|null|undefined} cs - internal canvas state
 * @returns {object} wire-format canvas state (always safe to JSON.stringify)
 */
export function toWireCanvasState(cs) {
  if (!cs || typeof cs !== 'object') {
    return {
      zoom_level: 1,
      visible_tree_groups: [],
      recent_interactions: [],
    };
  }
  const sel = cs.selectedNode ?? cs.selected_node ?? null;
  const wire = {
    zoom_level: typeof cs.zoom_level === 'number'
      ? cs.zoom_level
      : (typeof cs.zoom === 'number' ? cs.zoom : 1),
    visible_tree_groups: Array.isArray(cs.visible_tree_groups)
      ? cs.visible_tree_groups
      : (Array.isArray(cs.visibleGroups) ? cs.visibleGroups : []),
    recent_interactions: Array.isArray(cs.recent_interactions) ? cs.recent_interactions : [],
  };
  if (sel && typeof sel === 'object' && sel.id != null) {
    wire.selected_node = {
      id: sel.id,
      name: sel.name ?? '',
      node_type: sel.node_type ?? '',
      qualified_name: sel.qualified_name ?? null,
    };
  }
  if (cs.active_filter) wire.active_filter = cs.active_filter;
  if (cs.active_lens) wire.active_lens = cs.active_lens;
  if (cs.active_query) wire.active_query = cs.active_query;
  return wire;
}

// Minimum relative zoom change (20%) before a zoom interaction is recorded.
// Below this, zoom drift from smoothing/fit adjustments is ignored.
export const ZOOM_INTERACTION_THRESHOLD = 0.2;

/**
 * Record a zoom interaction in the recent-interactions trail (§10:
 * `{ "action": "zoom_in", "to_level": 2.3 }`).
 *
 * Continuous zoom gestures (wheel bursts, inertial decay) must not flood the
 * trail: a trailing zoom entry is updated in place, and only changes larger
 * than `threshold` (relative to the last recorded level) start a new entry.
 *
 * @param {Array} interactions - current trail (newest last)
 * @param {number} zoom - current zoom level
 * @param {object} [opts]
 * @param {number} [opts.threshold] - relative change threshold (default 0.2)
 * @param {number|null} [opts.lastRecordedLevel] - last recorded zoom level;
 *   pass null on first call. Returned via the result for subsequent calls.
 * @returns {{interactions: Array, lastRecordedLevel: number|null}} new trail
 *   and the level to pass as lastRecordedLevel on the next call
 */
export function recordZoomInteraction(interactions, zoom, opts = {}) {
  const threshold = opts.threshold ?? ZOOM_INTERACTION_THRESHOLD;
  const lastRecorded = opts.lastRecordedLevel ?? null;
  const rounded = Math.round(zoom * 100) / 100;
  const trail = Array.isArray(interactions) ? interactions : [];

  const last = trail[trail.length - 1];
  const isZoomEntry = last
    && typeof last === 'object'
    && (last.action === 'zoom_in' || last.action === 'zoom_out');

  if (isZoomEntry) {
    // Coalesce: a continuous gesture updates the trailing entry (direction
    // is measured against the level where the gesture started).
    const base = typeof last.base_level === 'number' ? last.base_level : rounded;
    const entry = {
      action: rounded >= base ? 'zoom_in' : 'zoom_out',
      to_level: rounded,
      base_level: base,
    };
    return {
      interactions: [...trail.slice(0, -1), entry],
      lastRecordedLevel: rounded,
    };
  }

  if (lastRecorded === null) {
    // First observation - just establish the baseline, record nothing.
    return { interactions: trail, lastRecordedLevel: rounded };
  }

  const relative = Math.abs(rounded - lastRecorded) / Math.max(Math.abs(lastRecorded), 0.01);
  if (relative < threshold) {
    return { interactions: trail, lastRecordedLevel: lastRecorded };
  }

  const entry = {
    action: rounded > lastRecorded ? 'zoom_in' : 'zoom_out',
    to_level: rounded,
    base_level: lastRecorded,
  };
  return {
    interactions: [...trail.slice(-9), entry],
    lastRecordedLevel: rounded,
  };
}
