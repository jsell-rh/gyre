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
 * already partially normalized (snake_case-only inputs pass through).
 *
 * `zoom_level` resolution: the live camelCase `zoom` (kept fresh by the
 * canvas component on every canvasState write) wins over a `zoom_level`
 * snapshot, which is only as fresh as whatever last wrote it (e.g. a lens
 * toggle) and can contradict the zoom levels reported in
 * recent_interactions. Inputs carrying only `zoom_level` (already
 * normalized) still pass through unchanged.
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
    zoom_level: typeof cs.zoom === 'number'
      ? cs.zoom
      : (typeof cs.zoom_level === 'number' ? cs.zoom_level : 1),
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
 * @param {boolean} [opts.allowCoalesce=true] - set false to forbid updating a
 *   trailing zoom entry (used by ZoomInteractionTracker when the trailing
 *   entry belongs to an earlier, already-ended gesture).
 * @returns {{interactions: Array, lastRecordedLevel: number|null}} new trail
 *   and the level to pass as lastRecordedLevel on the next call
 */
export function recordZoomInteraction(interactions, zoom, opts = {}) {
  const threshold = opts.threshold ?? ZOOM_INTERACTION_THRESHOLD;
  const lastRecorded = opts.lastRecordedLevel ?? null;
  const allowCoalesce = opts.allowCoalesce !== false;
  const rounded = Math.round(zoom * 100) / 100;
  const trail = Array.isArray(interactions) ? interactions : [];

  const last = trail[trail.length - 1];
  const isZoomEntry = last
    && typeof last === 'object'
    && (last.action === 'zoom_in' || last.action === 'zoom_out');

  if (allowCoalesce && isZoomEntry) {
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

/**
 * Armed/disarmed state machine deciding when camera zoom changes count as
 * user zoom interactions in the §10 recent_interactions trail.
 *
 * Only user-initiated zoom (wheel, keyboard, pinch) is recorded. Programmatic
 * camera control — the initial auto-fit, drill transitions, breadcrumb
 * navigation, query-driven "fit"/level zoom, search-result fits — must call
 * `disarm()` when it takes over the camera. While disarmed, `record()` just
 * refreshes the baseline, so the next user gesture (which re-arms via
 * `arm()`) is measured against the settled level: programmatic zoom deltas
 * are never attributed to the user.
 *
 * Coalescing is gesture-scoped: a trailing zoom entry is only updated while
 * it belongs to the current gesture. After a disarm, the previous gesture's
 * entry is frozen (its to_level keeps the level the user actually zoomed
 * to), and the next user gesture that crosses the threshold starts a NEW
 * entry instead of rewriting the old one with programmatic levels.
 */
export class ZoomInteractionTracker {
  /**
   * @param {object} [opts]
   * @param {number} [opts.threshold] - relative change threshold (default 0.2)
   */
  constructor(opts = {}) {
    this.threshold = opts.threshold ?? ZOOM_INTERACTION_THRESHOLD;
    this.armed = false;
    this.baseline = null;
    this.gestureEntry = null;
  }

  /** A user zoom gesture (wheel/keyboard/pinch) has begun or is continuing. */
  arm() {
    this.armed = true;
  }

  /**
   * Programmatic camera control takes over: zoom changes are NOT user
   * interactions until the next arm(). Freezes the current gesture's
   * trailing entry and re-baselines.
   * @param {number} [zoom] current zoom level to re-baseline against
   */
  disarm(zoom) {
    this.armed = false;
    this.gestureEntry = null;
    if (typeof zoom === 'number') {
      this.baseline = Math.round(zoom * 100) / 100;
    }
  }

  /**
   * Feed the current zoom level (e.g. once per animation frame).
   * @param {Array} interactions current trail (newest last)
   * @param {number} zoom current zoom level
   * @returns {Array} the trail to store (same reference when unchanged)
   */
  record(interactions, zoom) {
    if (!this.armed) {
      // Keep the baseline fresh so the first armed gesture compares correctly.
      this.baseline = Math.round(zoom * 100) / 100;
      this.gestureEntry = null;
      return interactions;
    }
    const trail = Array.isArray(interactions) ? interactions : [];
    // Only coalesce into the entry the current gesture created; a trailing
    // zoom entry from an earlier (pre-disarm) gesture must stay frozen.
    const canCoalesce = this.gestureEntry != null && trail[trail.length - 1] === this.gestureEntry;
    const { interactions: next, lastRecordedLevel } = recordZoomInteraction(trail, zoom, {
      threshold: this.threshold,
      lastRecordedLevel: this.baseline,
      allowCoalesce: canCoalesce,
    });
    this.baseline = lastRecordedLevel;
    if (next !== trail) {
      // The trail changed: either this gesture's entry was appended, or the
      // coalesce path rewrote it into a fresh object. Follow the object so
      // subsequent frames of the same gesture keep coalescing into it.
      const last = next[next.length - 1];
      if (last && typeof last === 'object'
        && (last.action === 'zoom_in' || last.action === 'zoom_out')) {
        this.gestureEntry = last;
      }
    }
    return next;
  }
}
