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

  // Backward ghosts = removed after scrubber, NOT re-added after scrubber, NOT in current graph
  const result = [];
  for (const qn of removedAfter) {
    if (addedAfter.has(qn) && currentQualifiedNames.has(qn)) continue; // re-added and exists now
    if (currentQualifiedNames.has(qn)) continue; // still exists in current graph
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
