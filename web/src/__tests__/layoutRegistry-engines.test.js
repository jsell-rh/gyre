/**
 * Layout registry coverage (ui-layout.md §10 Layout Engines + §4
 * Extensibility): every specced layout name has a registered renderer, and
 * the SVG engines are wired to the right engine selector.
 */
import { describe, it, expect } from 'vitest';
import { listLayouts, getLayout, registerLayout } from '../lib/layoutRegistry.js';
import { LAYOUT_TYPES } from '../lib/types/view-spec.ts';

describe('layoutRegistry engine coverage', () => {
  const registered = listLayouts();
  const names = registered.map((l) => l.name);

  it('registers every layout in the grammar (all eight specced names)', () => {
    for (const name of LAYOUT_TYPES) {
      expect(names).toContain(name);
    }
    expect(names).toEqual(
      expect.arrayContaining([
        'graph',
        'hierarchical',
        'layered',
        'list',
        'timeline',
        'side-by-side',
        'diff',
        'flow',
      ])
    );
  });

  it('wires hierarchical and layered to the SVG renderer with ELK engines', () => {
    const hier = getLayout('hierarchical');
    expect(hier.component).toBeTruthy();
    expect(hier.engine).toBe('hierarchical');

    const layered = getLayout('layered');
    expect(layered.component).toBeTruthy();
    expect(layered.component).toBe(hier.component); // same SVG canvas component
    expect(layered.engine).toBe('layered');
  });

  it('wires graph to a renderer, side-by-side and diff to custom components', () => {
    expect(getLayout('graph').component).toBeTruthy();
    expect(getLayout('side-by-side').component).toBeTruthy();
    expect(getLayout('diff').component).toBeTruthy();
    expect(getLayout('flow').component).toBeTruthy();
  });

  it('rejects re-registering an existing layout (grammar closed for modification)', () => {
    const before = listLayouts().length;
    registerLayout({
      name: 'graph',
      component: /** @type {any} */ ({}),
      labelKey: 'x',
      icon: '',
    });
    expect(listLayouts().length).toBe(before);
    // Still the original renderer
    expect(getLayout('graph').component).toBeTruthy();
    expect(getLayout('graph').labelKey).toBe('moldable_view.tab_graph');
  });
});
