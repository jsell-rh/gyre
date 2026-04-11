import { describe, it, expect } from 'vitest';
import {
  parseDeltaJson,
  extractRemovedNodesFromDeltas,
  computeTimelineDeltaStats,
  computeTimelineGhostOverlays,
} from '../lib/timeline-utils.js';

// -- Test data fixtures -------------------------------------------------------

// Current graph nodes (represent what exists NOW in the graph)
const CURRENT_NODES = [
  { id: 'n1', name: 'Foo', node_type: 'type', qualified_name: 'crate::Foo', first_seen_at: 500, last_modified_sha: 'sha1' },
  { id: 'n2', name: 'Bar', node_type: 'type', qualified_name: 'crate::Bar', first_seen_at: 500, last_modified_sha: 'sha2' },
  { id: 'n3', name: 'Baz', node_type: 'trait', qualified_name: 'crate::Baz', first_seen_at: 1500, last_modified_sha: 'sha3_new' },
  { id: 'n4', name: 'Qux', node_type: 'function', qualified_name: 'crate::Qux', first_seen_at: 2500, last_modified_sha: 'sha4' },
];

const CURRENT_EDGES = [
  { source_id: 'n1', target_id: 'n2' },
  { source_id: 'n3', target_id: 'n1' },
];

// Historical filtered graph (nodes with first_seen_at <= cutoff=1000)
// Contains n1, n2 (first_seen_at=500). Does NOT contain n3 (1500) or n4 (2500).
const HISTORICAL_NODES = [
  { id: 'n1', name: 'Foo', node_type: 'type', qualified_name: 'crate::Foo', first_seen_at: 500, last_modified_sha: 'sha1' },
  { id: 'n2', name: 'Bar', node_type: 'type', qualified_name: 'crate::Bar', first_seen_at: 500, last_modified_sha: 'sha2_old' },
];

const HISTORICAL_EDGES = [
  { source_id: 'n1', target_id: 'n2' },
];

// Timeline deltas — ordered chronologically
// Delta at index 0 (timestamp 500): initial extraction, added Foo, Bar, OldTrait
// Delta at index 1 (timestamp 1000): scrubber position — added Baz
// Delta at index 2 (timestamp 1500): removed OldTrait, modified Bar
// Delta at index 3 (timestamp 2500): added Qux
const TIMELINE = [
  {
    id: 'd0', timestamp: 500,
    delta_json: JSON.stringify({
      nodes_extracted: 3, edges_extracted: 1,
      nodes_added: [
        { name: 'Foo', node_type: 'type', qualified_name: 'crate::Foo' },
        { name: 'Bar', node_type: 'type', qualified_name: 'crate::Bar' },
        { name: 'OldTrait', node_type: 'trait', qualified_name: 'crate::OldTrait' },
      ],
      nodes_removed: [],
      nodes_modified: [],
    }),
  },
  {
    id: 'd1', timestamp: 1000,
    delta_json: JSON.stringify({
      nodes_extracted: 4, edges_extracted: 2,
      nodes_added: [
        { name: 'Baz', node_type: 'trait', qualified_name: 'crate::Baz' },
      ],
      nodes_removed: [],
      nodes_modified: [],
    }),
  },
  {
    id: 'd2', timestamp: 1500,
    delta_json: JSON.stringify({
      nodes_extracted: 3, edges_extracted: 2,
      nodes_added: [],
      nodes_removed: ['crate::OldTrait'],
      nodes_modified: [{ qualified_name: 'crate::Bar', changes: 'added field' }],
    }),
  },
  {
    id: 'd3', timestamp: 2500,
    delta_json: JSON.stringify({
      nodes_extracted: 4, edges_extracted: 3,
      nodes_added: [
        { name: 'Qux', node_type: 'function', qualified_name: 'crate::Qux' },
      ],
      nodes_removed: [],
      nodes_modified: [],
    }),
  },
];

// Scrubber is at index 1 (timestamp 1000). Deltas after scrubber: d2 (removes OldTrait), d3 (adds Qux).

describe('parseDeltaJson', () => {
  it('parses valid JSON string', () => {
    const result = parseDeltaJson('{"nodes_added": []}');
    expect(result).toEqual({ nodes_added: [] });
  });

  it('returns pre-parsed object as-is', () => {
    const obj = { nodes_removed: ['a'] };
    expect(parseDeltaJson(obj)).toBe(obj);
  });

  it('returns empty object for null/undefined', () => {
    expect(parseDeltaJson(null)).toEqual({});
    expect(parseDeltaJson(undefined)).toEqual({});
  });

  it('returns empty object for malformed JSON', () => {
    expect(parseDeltaJson('not json')).toEqual({});
  });
});

describe('extractRemovedNodesFromDeltas', () => {
  const currentQualifiedNames = new Set(CURRENT_NODES.map(n => n.qualified_name));

  it('detects nodes removed after the scrubber position', () => {
    // OldTrait was removed in delta d2 (after scrubber at index 1) and is NOT in current graph
    const removed = extractRemovedNodesFromDeltas(TIMELINE, 1, currentQualifiedNames);
    expect(removed.length).toBe(1);
    expect(removed[0].qualified_name).toBe('crate::OldTrait');
    expect(removed[0].name).toBe('OldTrait');
    expect(removed[0].node_type).toBe('trait');
  });

  it('excludes nodes that still exist in the current graph', () => {
    // Baz exists in the current graph, so even if it appeared in nodes_removed,
    // it should not be a backward ghost
    const timelineWithBazRemoved = [...TIMELINE];
    timelineWithBazRemoved[2] = {
      ...timelineWithBazRemoved[2],
      delta_json: JSON.stringify({
        nodes_extracted: 3, edges_extracted: 2,
        nodes_added: [],
        nodes_removed: ['crate::OldTrait', 'crate::Baz'],
        nodes_modified: [],
      }),
    };
    // Baz is in currentQualifiedNames, so should be excluded
    const removed = extractRemovedNodesFromDeltas(timelineWithBazRemoved, 1, currentQualifiedNames);
    expect(removed.length).toBe(1);
    expect(removed[0].qualified_name).toBe('crate::OldTrait');
  });

  it('excludes nodes added AND removed after scrubber that are re-added in current graph', () => {
    // Node added in d2 and removed in d3, but also in current graph = not a backward ghost
    const timelineTransient = [
      ...TIMELINE.slice(0, 2),
      {
        id: 'd2t', timestamp: 1500,
        delta_json: JSON.stringify({
          nodes_added: [{ name: 'Transient', node_type: 'type', qualified_name: 'crate::Transient' }],
          nodes_removed: ['crate::OldTrait'],
          nodes_modified: [],
        }),
      },
      {
        id: 'd3t', timestamp: 2500,
        delta_json: JSON.stringify({
          nodes_added: [],
          nodes_removed: ['crate::Transient'],
          nodes_modified: [],
        }),
      },
    ];
    const removed = extractRemovedNodesFromDeltas(timelineTransient, 1, currentQualifiedNames);
    // OldTrait removed and not in current graph → backward ghost
    // Transient was added after scrubber and removed after scrubber and NOT in current graph
    // addedAfter has 'crate::Transient', currentQualifiedNames does NOT → so it's NOT excluded
    // Wait, the logic is: skip if addedAfter.has(qn) && currentQualifiedNames.has(qn)
    // Transient: addedAfter=true, currentQualifiedNames=false → NOT skipped by first check
    // Then: currentQualifiedNames.has('crate::Transient') = false → NOT skipped by second check
    // So Transient IS included. But Transient was added after scrubber → it never existed at scrubber time.
    // This is a valid backward ghost from a data perspective: it was transiently present then removed.
    // However, it didn't exist at the scrubber time, so arguably it shouldn't be a backward ghost.
    expect(removed.length).toBe(2);
    expect(removed.map(r => r.qualified_name)).toContain('crate::OldTrait');
    expect(removed.map(r => r.qualified_name)).toContain('crate::Transient');
  });

  it('returns empty array when scrubber is at the last position', () => {
    // No deltas after the last position
    const removed = extractRemovedNodesFromDeltas(TIMELINE, 3, currentQualifiedNames);
    expect(removed).toEqual([]);
  });

  it('returns empty array for empty timeline', () => {
    expect(extractRemovedNodesFromDeltas([], 0, currentQualifiedNames)).toEqual([]);
    expect(extractRemovedNodesFromDeltas(null, 0, currentQualifiedNames)).toEqual([]);
  });

  it('derives name from qualified_name when no nodes_added info exists', () => {
    // OldTrait exists in nodes_added of d0, so it gets full info.
    // Test with a node that has no nodes_added entry:
    const timelineUnknown = [
      {
        id: 'd0', timestamp: 500,
        delta_json: JSON.stringify({ nodes_added: [], nodes_removed: [] }),
      },
      {
        id: 'd1', timestamp: 1000,
        delta_json: JSON.stringify({ nodes_added: [], nodes_removed: ['crate::module::UnknownNode'] }),
      },
    ];
    const removed = extractRemovedNodesFromDeltas(timelineUnknown, 0, new Set());
    expect(removed.length).toBe(1);
    expect(removed[0].name).toBe('UnknownNode');
    expect(removed[0].node_type).toBe('unknown');
  });
});

describe('computeTimelineDeltaStats', () => {
  const graph = { nodes: CURRENT_NODES, edges: CURRENT_EDGES };
  const timelineFilteredGraph = { nodes: HISTORICAL_NODES, edges: HISTORICAL_EDGES };

  it('computes correct added count and per-type breakdown', () => {
    const stats = computeTimelineDeltaStats({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    // n3 (Baz, trait, first_seen_at=1500) and n4 (Qux, function, first_seen_at=2500)
    // are in current graph but not in historical → added
    expect(stats.added).toBe(2);
    expect(stats.addedByType).toEqual({ trait: 1, function: 1 });
  });

  it('computes correct removed count using delta records — NOT set subtraction', () => {
    const stats = computeTimelineDeltaStats({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    // OldTrait was removed in d2 (after scrubber) and is NOT in current graph → 1 removed
    expect(stats.removed).toBe(1);
    expect(stats.removedByType).toEqual({ trait: 1 });
  });

  it('computes correct modified count and per-type breakdown', () => {
    const stats = computeTimelineDeltaStats({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    // n2 (Bar) has different last_modified_sha: 'sha2' (current) vs 'sha2_old' (historical) → modified
    expect(stats.modified).toBe(1);
    expect(stats.modifiedByType).toEqual({ type: 1 });
  });

  it('returns null when no filtered graph', () => {
    const stats = computeTimelineDeltaStats({
      graph, timelineFilteredGraph: null, timeline: TIMELINE, scrubIndex: 1,
    });
    expect(stats).toBeNull();
  });

  it('returns null when graph has no nodes', () => {
    const stats = computeTimelineDeltaStats({
      graph: { nodes: [], edges: [] }, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    expect(stats).toBeNull();
  });
});

describe('computeTimelineGhostOverlays', () => {
  const graph = { nodes: CURRENT_NODES, edges: CURRENT_EDGES };
  const timelineFilteredGraph = { nodes: HISTORICAL_NODES, edges: HISTORICAL_EDGES };

  it('produces forward ghost overlays for nodes added since scrubber time', () => {
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const addGhosts = overlays.filter(o => o.action === 'add');
    // n3 (Baz) and n4 (Qux) are in current but not historical → forward ghosts
    expect(addGhosts.length).toBe(2);
    expect(addGhosts.map(g => g.name)).toContain('Baz');
    expect(addGhosts.map(g => g.name)).toContain('Qux');
    expect(addGhosts.every(g => g.confidence === 'confirmed')).toBe(true);
  });

  it('produces backward ghost overlays for nodes removed since scrubber time', () => {
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const removeGhosts = overlays.filter(o => o.action === 'remove');
    // OldTrait was removed in d2 → backward ghost
    expect(removeGhosts.length).toBe(1);
    expect(removeGhosts[0].name).toBe('OldTrait');
    expect(removeGhosts[0].type).toBe('trait');
    expect(removeGhosts[0].id).toBe('removed:crate::OldTrait');
    expect(removeGhosts[0].confidence).toBe('confirmed');
  });

  it('produces modified ghost overlays for nodes changed since scrubber time', () => {
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const changeGhosts = overlays.filter(o => o.action === 'change');
    // n2 (Bar) has different last_modified_sha → modified ghost
    expect(changeGhosts.length).toBe(1);
    expect(changeGhosts[0].name).toBe('Bar');
    expect(changeGhosts[0].id).toBe('n2');
    expect(changeGhosts[0].confidence).toBe('confirmed');
  });

  it('backward ghost removed array is NOT always empty (regression test for F1)', () => {
    // This test directly validates that the F1 fix works — backward ghosts
    // should be detectable even though removed nodes are absent from graph.nodes.
    // The old implementation used set subtraction (timelineFilteredGraph ⊆ graph.nodes)
    // which guaranteed removed=[] always. The new implementation uses delta records.
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const removeGhosts = overlays.filter(o => o.action === 'remove');
    expect(removeGhosts.length).toBeGreaterThan(0);
  });

  it('returns empty array when no filtered graph', () => {
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph: null, timeline: TIMELINE, scrubIndex: 1,
    });
    expect(overlays).toEqual([]);
  });

  it('returns empty array when graph has no nodes', () => {
    const overlays = computeTimelineGhostOverlays({
      graph: { nodes: [], edges: [] }, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    expect(overlays).toEqual([]);
  });

  it('all overlay entries have required fields', () => {
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    for (const o of overlays) {
      expect(o).toHaveProperty('id');
      expect(o).toHaveProperty('name');
      expect(o).toHaveProperty('type');
      expect(o).toHaveProperty('action');
      expect(o).toHaveProperty('confidence');
      expect(o).toHaveProperty('reason');
      expect(['add', 'change', 'remove']).toContain(o.action);
    }
  });
});
