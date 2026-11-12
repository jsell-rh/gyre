/**
 * Timeline computation utilities for the Architectural Timeline feature (system-explorer.md §6).
 *
 * Extracted from ExplorerView.svelte for testability. These functions compute:
 * - Delta stats (added/removed/modified counts with per-type breakdown)
 * - Ghost overlays (forward/backward/modified ghost entries for canvas rendering)
 */

/**
 * Safely parse delta_json from an ArchitecturalDelta record.
 * @param {string|object|null} deltaJson - The delta_json field (may be a JSON string or pre-parsed object)
 * @returns {object} Parsed delta object with fields like nodes_added, nodes_removed, nodes_modified
 */
export function parseDeltaJson(deltaJson) {
  if (!deltaJson) return {};
  try {
    return typeof deltaJson === 'string' ? JSON.parse(deltaJson) : deltaJson;
  } catch { return {}; }
}

/**
 * Extract removed nodes from ArchitecturalDelta records after the scrubber position.
 *
 * Returns an array of { qualified_name, name, node_type } for nodes removed between
 * the scrubber time and now. Uses nodes_removed from delta_json and cross-references
 * nodes_added from all deltas for name/type info.
 *
 * @param {Array} timeline - Array of ArchitecturalDelta records
 * @param {number} scrubIndex - Current scrubber index into the timeline array
 * @param {Set<string>} currentQualifiedNames - Set of qualified_names in the current graph
 * @returns {Array<{qualified_name: string, name: string, node_type: string}>}
 */
export function extractRemovedNodesFromDeltas(timeline, scrubIndex, currentQualifiedNames) {
  if (!timeline?.length || scrubIndex < 0) return [];

  // Build a lookup of qualified_name → {name, node_type} from all deltas' nodes_added
  const nodeInfoLookup = new Map();
  for (const delta of timeline) {
    const parsed = parseDeltaJson(delta.delta_json);
    if (parsed.nodes_added) {
      for (const entry of parsed.nodes_added) {
        const qn = typeof entry === 'string' ? entry : entry.qualified_name;
        if (qn) {
          nodeInfoLookup.set(qn, {
            name: (typeof entry === 'object' ? entry.name : null) ?? qn.split('::').pop() ?? qn,
            node_type: (typeof entry === 'object' ? entry.node_type : null) ?? 'unknown',
          });
        }
      }
    }
  }

  // Build set of nodes added at or before the scrubber position (existed at scrubber time)
  const addedAtOrBefore = new Set();
  for (let i = 0; i <= scrubIndex && i < timeline.length; i++) {
    const parsed = parseDeltaJson(timeline[i].delta_json);
    if (parsed.nodes_added) {
      for (const entry of parsed.nodes_added) {
        const qn = typeof entry === 'string' ? entry : entry.qualified_name;
        if (qn) addedAtOrBefore.add(qn);
      }
    }
  }

  // Collect qualified_names from nodes_removed in deltas AFTER the scrubber position
  const removedAfter = new Set();
  const addedAfter = new Set();
  for (let i = scrubIndex + 1; i < timeline.length; i++) {
    const parsed = parseDeltaJson(timeline[i].delta_json);
    if (parsed.nodes_removed) {
      for (const qn of parsed.nodes_removed) {
        if (typeof qn === 'string') removedAfter.add(qn);
      }
    }
    if (parsed.nodes_added) {
      for (const entry of parsed.nodes_added) {
        const qn = typeof entry === 'string' ? entry : entry.qualified_name;
        if (qn) addedAfter.add(qn);
      }
    }
  }

  // Backward ghosts = removed after scrubber AND existed at scrubber time AND NOT in current graph
  const result = [];
  for (const qn of removedAfter) {
    if (currentQualifiedNames.has(qn)) continue; // still exists in current graph
    // Transient node: added after scrubber but never added at or before → never existed at scrubber time
    if (addedAfter.has(qn) && !addedAtOrBefore.has(qn)) continue;
    const info = nodeInfoLookup.get(qn) ?? { name: qn.split('::').pop() ?? qn, node_type: 'unknown' };
    result.push({ qualified_name: qn, name: info.name, node_type: info.node_type });
  }
  return result;
}

/**
 * Compute delta stats between the scrubber position and the current graph.
 *
 * @param {object} params
 * @param {object} params.graph - Current full graph { nodes: [], edges: [] }
 * @param {object} params.timelineFilteredGraph - Historical filtered graph { nodes: [], edges: [] }
 * @param {Array} params.timeline - Array of ArchitecturalDelta records
 * @param {number} params.scrubIndex - Current scrubber index
 * @returns {object|null} { added, removed, modified, addedByType, removedByType, modifiedByType }
 */
export function computeTimelineDeltaStats({ graph, timelineFilteredGraph, timeline, scrubIndex }) {
  if (!timelineFilteredGraph || !graph?.nodes?.length) return null;
  const historicalIds = new Set(timelineFilteredGraph.nodes.map(n => n.id));
  const currentQualifiedNames = new Set(graph.nodes.map(n => n.qualified_name).filter(Boolean));

  // Forward ghosts: exist now but not at scrubber time (added since)
  const added = graph.nodes.filter(n => !historicalIds.has(n.id));
  // Backward ghosts: existed at scrubber time but removed since — use delta records
  const removedNodes = extractRemovedNodesFromDeltas(timeline, scrubIndex, currentQualifiedNames);
  // Modified: exist at both times but may have changed
  const modified = graph.nodes.filter(n => {
    if (!historicalIds.has(n.id)) return false;
    const hist = timelineFilteredGraph.nodes.find(h => h.id === n.id);
    if (!hist) return false;
    return n.last_modified_sha !== hist.last_modified_sha;
  });

  // Per-type breakdown for all three categories (spec §6: "+12 types, -3 types, +2 traits")
  const addedByType = {};
  for (const n of added) { const t = n.node_type ?? 'unknown'; addedByType[t] = (addedByType[t] ?? 0) + 1; }
  const removedByType = {};
  for (const n of removedNodes) { const t = n.node_type ?? 'unknown'; removedByType[t] = (removedByType[t] ?? 0) + 1; }
  const modifiedByType = {};
  for (const n of modified) { const t = n.node_type ?? 'unknown'; modifiedByType[t] = (modifiedByType[t] ?? 0) + 1; }

  return {
    added: added.length,
    removed: removedNodes.length,
    modified: modified.length,
    addedByType,
    removedByType,
    modifiedByType,
  };
}

/**
 * Compute ghost overlays for historical time travel rendering.
 *
 * @param {object} params
 * @param {object} params.graph - Current full graph { nodes: [], edges: [] }
 * @param {object} params.timelineFilteredGraph - Historical filtered graph { nodes: [], edges: [] }
 * @param {Array} params.timeline - Array of ArchitecturalDelta records
 * @param {number} params.scrubIndex - Current scrubber index
 * @returns {Array} Ghost overlay entries with { id, name, type, action, confidence, reason }
 */
export function computeTimelineGhostOverlays({ graph, timelineFilteredGraph, timeline, scrubIndex }) {
  if (!timelineFilteredGraph || !graph?.nodes?.length) return [];
  const historicalIds = new Set(timelineFilteredGraph.nodes.map(n => n.id));
  const currentQualifiedNames = new Set(graph.nodes.map(n => n.qualified_name).filter(Boolean));
  const overlays = [];

  // Forward ghosts: added since scrubber time (green dotted)
  for (const n of graph.nodes) {
    if (!historicalIds.has(n.id)) {
      overlays.push({
        id: n.id,
        name: n.name ?? n.qualified_name ?? n.id,
        type: n.node_type ?? 'type',
        action: 'add',
        confidence: 'confirmed',
        reason: 'Added after this point in time',
      });
    }
  }

  // Backward ghosts: removed since scrubber time (red strikethrough)
  // Use ArchitecturalDelta records to find nodes removed after the scrubber position,
  // since removed nodes are absent from graph.nodes and cannot be found by set subtraction.
  const removedNodes = extractRemovedNodesFromDeltas(timeline, scrubIndex, currentQualifiedNames);
  for (const rn of removedNodes) {
    overlays.push({
      id: `removed:${rn.qualified_name}`,
      name: rn.name,
      type: rn.node_type ?? 'type',
      action: 'remove',
      confidence: 'confirmed',
      reason: 'Removed after this point in time',
    });
  }

  // Modified highlights (yellow)
  for (const n of graph.nodes) {
    if (!historicalIds.has(n.id)) continue;
    const hist = timelineFilteredGraph.nodes.find(h => h.id === n.id);
    if (!hist) continue;
    if (n.last_modified_sha !== hist.last_modified_sha) {
      overlays.push({
        id: n.id,
        name: n.name ?? n.qualified_name ?? n.id,
        type: n.node_type ?? 'type',
        action: 'change',
        confidence: 'confirmed',
        reason: 'Modified after this point in time',
      });
    }
  }

  return overlays;
}

/**
 * Resolve the scrubber position to a unix-epoch-seconds timestamp.
 *
 * @param {Array} timeline - Array of ArchitecturalDelta records
 * @param {number} scrubIndex - Current scrubber index (-1 = present)
 * @returns {number|null} Epoch seconds at the scrubber position, or null
 *   when the timeline is empty, the index is out of range, or the delta
 *   has no usable timestamp.
 */
export function resolveScrubTimestamp(timeline, scrubIndex) {
  if (!timeline?.length || scrubIndex < 0 || scrubIndex >= timeline.length) return null;
  const delta = timeline[scrubIndex];
  if (!delta) return null;
  const ts = typeof delta.timestamp === 'number'
    ? delta.timestamp
    : Number(delta.timestamp);
  if (Number.isFinite(ts) && ts > 0) return ts;
  const parsed = typeof delta.timestamp === 'string' ? Date.parse(delta.timestamp) : NaN;
  return Number.isFinite(parsed) ? Math.floor(parsed / 1000) : null;
}

/**
 * Debounced point-in-time graph fetcher (system-explorer.md §6).
 *
 * Coalesces rapid scrub positions into one server `GET /repos/:id/graph?at=`
 * request per debounce window (200ms default), and drops stale responses:
 * a fetch only settles the waiters for its OWN target position, so a
 * request made while an older fetch is still in flight never resolves with
 * the older position's graph; it is served by its own fetch once its
 * debounce window closes. Positions superseded within a debounce window
 * (and cancel()) resolve null so the caller keeps its client-side fallback
 * graph. A failed fetch rejects so the caller can do the same.
 *
 * @param {function} fetchAt - `(at: number) => Promise<object>` graph fetcher
 * @param {number} [delayMs=200] - debounce window in milliseconds
 * @returns {{ request: (at: number) => Promise<object>, cancel: () => void }}
 */
export function createTimelineHistoryFetcher(fetchAt, delayMs = 200) {
  let timer = null;
  let latestAt = null;
  let pending = [];

  // Settle the waiters for ONE fetch target. Waiters for other positions
  // stay pending: their own fetch starts when their debounce window closes
  // (a newer request restarts the timer), so they must never receive this
  // fetch's graph. Matching by the fetch's target (not by `latestAt`) is
  // what prevents an in-flight fetch for an older position from resolving
  // a newer position's waiter with the older graph.
  function settleTarget(target, ok, value) {
    const remaining = [];
    for (const w of pending) {
      if (w.at !== target) {
        remaining.push(w);
        continue;
      }
      if (ok) w.resolve(value);
      else w.reject(value);
    }
    pending = remaining;
  }

  // Resolve every waiter whose position will NOT get its own fetch with
  // null (keep the client-side fallback instead of hanging): positions
  // superseded within the debounce window, or every waiter on cancel().
  function releaseSuperseded(exceptAt) {
    const remaining = [];
    for (const w of pending) {
      if (exceptAt != null && w.at === exceptAt) {
        remaining.push(w);
        continue;
      }
      w.resolve(null);
    }
    pending = remaining;
  }

  return {
    request(at) {
      latestAt = at;
      return new Promise((resolve, reject) => {
        pending.push({ at, resolve, reject });
        if (timer != null) clearTimeout(timer);
        timer = setTimeout(() => {
          timer = null;
          const target = latestAt;
          if (target == null) {
            releaseSuperseded(null);
            return;
          }
          // Only the latest position in the window gets a fetch; earlier
          // positions in the same window are superseded and resolve null.
          releaseSuperseded(target);
          fetchAt(target)
            .then((graph) => settleTarget(target, true, graph))
            .catch((err) => settleTarget(target, false, err));
        }, delayMs);
      });
    },
    cancel() {
      if (timer != null) {
        clearTimeout(timer);
        timer = null;
      }
      latestAt = null;
      releaseSuperseded(null); // release all waiters without a fetch
    },
  };
}
