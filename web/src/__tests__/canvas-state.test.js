// Tests for web/src/lib/canvas-state.js - the §10 interaction-context wire
// format (view-query-grammar.md §10). These test the REAL functions used by
// ExplorerChat.svelte (sendMessage) and ExplorerCanvas.svelte (zoom tracking).
import { describe, it, expect } from 'vitest';
import {
  toWireCanvasState,
  recordZoomInteraction,
  ZOOM_INTERACTION_THRESHOLD,
} from '../lib/canvas-state.js';

describe('toWireCanvasState (§10 wire format)', () => {
  it('maps camelCase selectedNode to snake_case selected_node with §10 fields', () => {
    // This is the shape ExplorerView/ExplorerCanvas actually maintain.
    const cs = {
      selectedNode: {
        id: 'node-1',
        name: 'TaskPort',
        node_type: 'interface',
        qualified_name: 'gyre_ports::TaskPort',
      },
      zoom: 2.3,
      visibleGroups: ['gyre-server'],
      breadcrumb: [{ id: 'b1', name: 'pkg' }],
      active_filter: 'calls',
      active_lens: 'structural',
      active_query: { type: 'blast_radius', node: 'TaskPort' },
      recent_interactions: [{ action: 'click', node: 'TaskPort::find_by_id' }],
    };
    const wire = toWireCanvasState(cs);
    expect(wire.selected_node).toEqual({
      id: 'node-1',
      name: 'TaskPort',
      node_type: 'interface',
      qualified_name: 'gyre_ports::TaskPort',
    });
    expect(wire.zoom_level).toBe(2.3);
    expect(wire.visible_tree_groups).toEqual(['gyre-server']);
    expect(wire.active_filter).toBe('calls');
    expect(wire.active_lens).toBe('structural');
    expect(wire.active_query).toEqual({ type: 'blast_radius', node: 'TaskPort' });
    expect(wire.recent_interactions).toEqual([
      { action: 'click', node: 'TaskPort::find_by_id' },
    ]);
    // Rendering-only fields must not leak into the wire format.
    expect(wire.selectedNode).toBeUndefined();
    expect(wire.zoom).toBeUndefined();
    expect(wire.breadcrumb).toBeUndefined();
    expect(wire.visibleGroups).toBeUndefined();
  });

  it('without normalization the camelCase selection would be dropped server-side', () => {
    // Regression guard: the server CanvasState struct only knows snake_case
    // keys. Sending the raw internal state loses the selection - which breaks
    // $clicked/$selected resolution and the §10 selected_node context.
    const raw = JSON.stringify({ selectedNode: { id: 'n1', name: 'Foo' }, zoom: 1.5 });
    const parsed = JSON.parse(raw);
    expect(parsed.selected_node).toBeUndefined();
    expect(parsed.selectedNode).toBeDefined();
    // The wire helper fixes exactly this:
    const wire = toWireCanvasState(JSON.parse(raw));
    expect(wire.selected_node).toEqual({ id: 'n1', name: 'Foo', node_type: '', qualified_name: null });
  });

  it('prefers explicit zoom_level over legacy zoom and defaults missing values', () => {
    const wire = toWireCanvasState({ zoom_level: 4.2, zoom: 1 });
    expect(wire.zoom_level).toBe(4.2);
    expect(toWireCanvasState({}).zoom_level).toBe(1);
    expect(toWireCanvasState(null).zoom_level).toBe(1);
    expect(toWireCanvasState(null).visible_tree_groups).toEqual([]);
    expect(toWireCanvasState(null).recent_interactions).toEqual([]);
    expect(toWireCanvasState(null).selected_node).toBeUndefined();
  });

  it('omits selected_node / optionals when absent so serde defaults apply', () => {
    const wire = toWireCanvasState({ selectedNode: null, recent_interactions: [] });
    expect(wire.selected_node).toBeUndefined();
    expect(wire.active_filter).toBeUndefined();
    expect(wire.active_lens).toBeUndefined();
    expect(wire.active_query).toBeUndefined();
  });

  it('accepts already-normalized snake_case input unchanged', () => {
    const cs = {
      selected_node: { id: 'n2', name: 'Bar', node_type: 'function', qualified_name: 'pkg::Bar' },
      zoom_level: 0.8,
      visible_tree_groups: [],
      active_filter: 'all',
      active_lens: 'evaluative',
      recent_interactions: [],
    };
    const wire = toWireCanvasState(cs);
    expect(wire.selected_node).toEqual(cs.selected_node);
    expect(wire.zoom_level).toBe(0.8);
    expect(wire.active_filter).toBe('all');
    expect(wire.active_lens).toBe('evaluative');
  });
});

describe('recordZoomInteraction (§10 zoom actions)', () => {
  it('records zoom_in with to_level on significant increase', () => {
    const r = recordZoomInteraction([], 2.3, { lastRecordedLevel: 1.0 });
    expect(r.interactions).toEqual([
      { action: 'zoom_in', to_level: 2.3, base_level: 1.0 },
    ]);
    expect(r.lastRecordedLevel).toBe(2.3);
  });

  it('records zoom_out when the level drops significantly', () => {
    const r = recordZoomInteraction([], 0.5, { lastRecordedLevel: 2.0 });
    expect(r.interactions[0].action).toBe('zoom_out');
    expect(r.interactions[0].to_level).toBe(0.5);
  });

  it('ignores drift below the relative threshold (no new entry, same array)', () => {
    const trail = [];
    const r1 = recordZoomInteraction(trail, 1.0, { lastRecordedLevel: null }); // baseline
    expect(r1.interactions).toBe(trail);
    const r2 = recordZoomInteraction(r1.interactions, 1.1, { lastRecordedLevel: r1.lastRecordedLevel });
    expect(r2.interactions).toBe(trail); // 10% < 20% threshold
    expect(r2.lastRecordedLevel).toBe(1.0);
  });

  it('coalesces continuous zoom into one trailing entry instead of flooding', () => {
    // Simulated wheel burst: 1.0 → 1.4 → 1.9 → 2.6, each above threshold.
    let state = { interactions: [], lastRecordedLevel: 1.0 };
    state = recordZoomInteraction(state.interactions, 1.4, state);
    state = recordZoomInteraction(state.interactions, 1.9, state);
    state = recordZoomInteraction(state.interactions, 2.6, state);
    expect(state.interactions).toHaveLength(1);
    expect(state.interactions[0].action).toBe('zoom_in');
    expect(state.interactions[0].to_level).toBe(2.6);
    expect(state.interactions[0].base_level).toBe(1.0);
  });

  it('direction flips when a gesture reverses past its start level', () => {
    let state = { interactions: [], lastRecordedLevel: 2.0 };
    state = recordZoomInteraction(state.interactions, 2.6, state); // zoom_in
    expect(state.interactions[0].action).toBe('zoom_in');
    // Same gesture continues back below the base level → now zoom_out
    state = recordZoomInteraction(state.interactions, 1.8, state);
    expect(state.interactions).toHaveLength(1);
    expect(state.interactions[0].action).toBe('zoom_out');
    expect(state.interactions[0].to_level).toBe(1.8);
  });

  it('a zoom entry after a click entry starts a new record (trail order kept)', () => {
    const trail = [{ action: 'click', node: 'Foo' }];
    const r = recordZoomInteraction(trail, 3.0, { lastRecordedLevel: 1.0 });
    expect(r.interactions).toEqual([
      { action: 'click', node: 'Foo' },
      { action: 'zoom_in', to_level: 3.0, base_level: 1.0 },
    ]);
  });

  it('caps the trail at 10 entries', () => {
    let state = {
      interactions: Array.from({ length: 10 }, (_, i) => ({ action: 'click', node: `n${i}` })),
      lastRecordedLevel: 1.0,
    };
    state = recordZoomInteraction(state.interactions, 3.0, state);
    expect(state.interactions).toHaveLength(10);
    expect(state.interactions[0]).toEqual({ action: 'click', node: 'n1' }); // n0 evicted
    expect(state.interactions[9].action).toBe('zoom_in');
  });

  it('first observation only establishes the baseline', () => {
    const r = recordZoomInteraction([], 5.0, { lastRecordedLevel: null });
    expect(r.interactions).toEqual([]);
    expect(r.lastRecordedLevel).toBe(5.0);
  });

  it('default threshold matches the exported constant', () => {
    expect(ZOOM_INTERACTION_THRESHOLD).toBe(0.2);
  });
});
