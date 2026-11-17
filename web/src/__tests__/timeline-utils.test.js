import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  parseDeltaJson,
  extractRemovedNodesFromDeltas,
  computeTimelineDeltaStats,
  computeTimelineGhostOverlays,
  resolveScrubTimestamp,
  createTimelineHistoryFetcher,
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
// n5 (OldTrait) is the shape the SERVER ?at= reconstruction returns: a node
// soft-deleted after the scrub time is resurrected with its real id (the
// client-side filter cannot contain it — it is absent from the current graph).
const HISTORICAL_NODES = [
  { id: 'n1', name: 'Foo', node_type: 'type', qualified_name: 'crate::Foo', first_seen_at: 500, last_modified_sha: 'sha1' },
  { id: 'n2', name: 'Bar', node_type: 'type', qualified_name: 'crate::Bar', first_seen_at: 500, last_modified_sha: 'sha2_old' },
  { id: 'n5', name: 'OldTrait', node_type: 'trait', qualified_name: 'crate::OldTrait', first_seen_at: 500, last_modified_sha: 'sha5' },
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

  it('excludes transient nodes added and removed after scrubber (never existed at scrubber time)', () => {
    // Transient was added in d2t (after scrubber) and removed in d3t (after scrubber).
    // It never existed at the scrubber time → NOT a backward ghost (spec §6).
    // Only OldTrait (added in d0, before scrubber) is a valid backward ghost.
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
    // OldTrait: added at d0 (before scrubber), removed at d2t (after scrubber) → backward ghost
    // Transient: added at d2t (after scrubber), removed at d3t (after scrubber) → excluded
    expect(removed.length).toBe(1);
    expect(removed[0].qualified_name).toBe('crate::OldTrait');
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
    expect(removeGhosts[0].id).toBe('n5');
    expect(removeGhosts[0].confidence).toBe('confirmed');
  });

  it('keys the backward ghost by the historical node\'s real id (F1 regression: canvas layout resolution)', () => {
    // ExplorerCanvas resolves remove ghosts through layoutNodeMap.get(ghost.id),
    // keyed by REAL graph node id, and it draws the scrubbed-time graph — so the
    // ghost is only rendered when its id is the historical node's id. The old
    // synthetic `removed:<qualified_name>` key never matched a layout node and
    // the red-dotted strikethrough was dead for every timeline ghost.
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const removeGhost = overlays.find(o => o.action === 'remove');
    expect(removeGhost).toBeTruthy();
    expect(removeGhost.id).toBe('n5');
    expect(removeGhost.id).not.toMatch(/^removed:/);
    // The id must resolve to a node in the historical graph the canvas draws.
    expect(HISTORICAL_NODES.some(n => n.id === removeGhost.id)).toBe(true);
  });

  it('falls back to the synthetic id when the removed node is absent from the historical graph', () => {
    // Client-side fallback filter (server ?at= fetch not landed yet): the
    // removed node cannot be in the filtered graph because the filter is
    // derived from the current graph. The ghost keeps the synthetic id — it
    // still counts toward the delta summary / "− Removed" chip.
    const fallbackGraph = {
      nodes: HISTORICAL_NODES.filter(n => n.qualified_name !== 'crate::OldTrait'),
      edges: HISTORICAL_EDGES,
    };
    const overlays = computeTimelineGhostOverlays({
      graph, timelineFilteredGraph: fallbackGraph, timeline: TIMELINE, scrubIndex: 1,
    });
    const removeGhost = overlays.find(o => o.action === 'remove');
    expect(removeGhost).toBeTruthy();
    expect(removeGhost.id).toBe('removed:crate::OldTrait');
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

// ── resolveScrubTimestamp ─────────────────────────────────────────────────

describe('resolveScrubTimestamp', () => {
  it('returns the delta timestamp for a valid index', () => {
    expect(resolveScrubTimestamp(TIMELINE, 1)).toBe(1000);
    expect(resolveScrubTimestamp(TIMELINE, 3)).toBe(2500);
  });

  it('returns null for present position or out-of-range index', () => {
    expect(resolveScrubTimestamp(TIMELINE, -1)).toBeNull();
    expect(resolveScrubTimestamp(TIMELINE, TIMELINE.length)).toBeNull();
    expect(resolveScrubTimestamp([], 0)).toBeNull();
    expect(resolveScrubTimestamp(null, 2)).toBeNull();
  });

  it('parses string timestamps (RFC 3339)', () => {
    const ts = [{ timestamp: '2024-04-01T00:00:00Z' }];
    const expected = Math.floor(Date.parse('2024-04-01T00:00:00Z') / 1000);
    expect(resolveScrubTimestamp(ts, 0)).toBe(expected);
  });

  it('returns null for a delta with no usable timestamp', () => {
    expect(resolveScrubTimestamp([{ timestamp: null }], 0)).toBeNull();
    expect(resolveScrubTimestamp([{}], 0)).toBeNull();
  });
});

// ── createTimelineHistoryFetcher (debounced ?at= fetch) ────────────────────

describe('createTimelineHistoryFetcher', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('debounces rapid scrub positions into one fetch for the latest position', async () => {
    const fetchAt = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const p1 = fetcher.request(500);
    const p2 = fetcher.request(1000);
    const p3 = fetcher.request(1500);

    // No fetch before the 200ms window closes
    expect(fetchAt).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(199);
    expect(fetchAt).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(fetchAt).toHaveBeenCalledTimes(1);
    expect(fetchAt).toHaveBeenCalledWith(1500);

    const results = await Promise.all([p1, p2, p3]);
    // Superseded positions resolve null (keep the client-side fallback);
    // only the latest position gets the fetched graph.
    expect(results[0]).toBeNull();
    expect(results[1]).toBeNull();
    expect(results[2]).toEqual({ nodes: [], edges: [] });
  });

  it('resolves the fetched graph for a single position', async () => {
    const graph = { nodes: [{ id: 'n1' }], edges: [] };
    const fetchAt = vi.fn().mockResolvedValue(graph);
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const p = fetcher.request(1000);
    await vi.advanceTimersByTimeAsync(200);
    await expect(p).resolves.toBe(graph);
  });

  it('rejects when the fetch fails so the caller keeps its fallback', async () => {
    const fetchAt = vi.fn().mockRejectedValue(new Error('network'));
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const p = fetcher.request(1000);
    // Attach immediately so the rejection is never unhandled.
    const settled = p.then(
      () => { throw new Error('expected rejection'); },
      (e) => e,
    );
    await vi.advanceTimersByTimeAsync(200);
    const err = await settled;
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe('network');
  });

  it('cancel releases waiters without fetching', async () => {
    const fetchAt = vi.fn();
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const p = fetcher.request(1000);
    fetcher.cancel();
    await vi.advanceTimersByTimeAsync(300);
    expect(fetchAt).not.toHaveBeenCalled();
    await expect(p).resolves.toBeNull();
  });

  it('does not settle a newer position with an older in-flight fetch (race regression)', async () => {
    // Review finding: a scrub while a fetch for an older position is still
    // in flight used to resolve the NEW position's waiter with the OLD
    // fetch's graph (waiters were matched against latestAt, not against
    // the fetch's target). The canvas then rendered the wrong point in
    // time (wrong ghosts, wrong delta counts) for as long as the user
    // stayed at that position.
    const graph500 = { nodes: [{ id: 'g500' }], edges: [] };
    const graph1000 = { nodes: [{ id: 'g1000' }], edges: [] };
    let land500;
    const fetchAt = vi.fn((at) => {
      if (at === 500) return new Promise((r) => { land500 = r; });
      return Promise.resolve(graph1000);
    });
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const p500 = fetcher.request(500);
    await vi.advanceTimersByTimeAsync(200); // debounce closes; fetch(500) in flight
    expect(fetchAt).toHaveBeenCalledTimes(1);

    // Scrub again while the fetch for 500 is still in flight.
    const p1000 = fetcher.request(1000);

    // The OLD fetch lands while the newer position is still debouncing.
    land500(graph500);
    await vi.advanceTimersByTimeAsync(0); // flush the old fetch's settlement

    // The newer debounce window closes; its own fetch runs.
    await vi.advanceTimersByTimeAsync(200);
    expect(fetchAt).toHaveBeenCalledTimes(2);
    expect(fetchAt).toHaveBeenNthCalledWith(2, 1000);

    await expect(p500).resolves.toBe(graph500);
    // With the race, p1000 resolved with the 500-graph the moment land500()
    // fired; it must instead carry its own position's graph.
    await expect(p1000).resolves.toBe(graph1000);
  });

  it('issues a new fetch after the window when the position changes later', async () => {
    const fetchAt = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
    const fetcher = createTimelineHistoryFetcher(fetchAt, 200);

    const first = fetcher.request(1000);
    await vi.advanceTimersByTimeAsync(200);
    await expect(first).resolves.toEqual({ nodes: [], edges: [] });

    const second = fetcher.request(2000);
    await vi.advanceTimersByTimeAsync(200);
    await expect(second).resolves.toEqual({ nodes: [], edges: [] });

    expect(fetchAt).toHaveBeenCalledTimes(2);
    expect(fetchAt).toHaveBeenNthCalledWith(1, 1000);
    expect(fetchAt).toHaveBeenNthCalledWith(2, 2000);
  });
});
