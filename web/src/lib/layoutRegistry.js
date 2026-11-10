/**
 * Layout registry — maps ViewSpec layout names to renderer components
 * (ui-layout.md §4 Extensibility).
 *
 * New layout types can be added without modifying the Explorer core:
 *
 *   1. Define a new layout name (e.g. 'sankey', 'matrix', 'chord').
 *   2. Implement a renderer component that accepts the data + encoding spec.
 *   3. registerLayout({ name, component, labelKey, icon, requiresRepo }).
 *
 * The grammar is open for new layouts but closed for modification —
 * existing layouts never change semantics. `LAYOUT_TYPES` in
 * types/view-spec.ts (the grammar's closed name set) is extended when a
 * layout is registered, keeping client-side validation in sync.
 *
 * Renderer component contract (every registered renderer must accept):
 *   nodes     — graph nodes from the API, filtered by the active query
 *   edges     — graph edges
 *   repoId    — current repo scope ('' at workspace scope)
 *   engine    — layout-engine selector consumed by SVG renderers
 *              ('graph' | 'hierarchical' | 'layered'; ui-layout.md §10)
 *   onSelectNode — callback(node) when a node is selected
 */

import { registerLayoutName } from './types/view-spec.ts';
import ExplorerCanvas from './ExplorerCanvas.svelte';
import SvgGraphCanvas from './SvgGraphCanvas.svelte';
import SideBySideView from './SideBySideView.svelte';
import DiffView from './DiffView.svelte';

/**
 * @typedef {Object} LayoutRegistration
 * @property {string} name             Layout name as it appears in ViewSpec JSON.
 * @property {import('svelte').Component} component Renderer component.
 * @property {string} labelKey         i18n key for the view-switcher tab label.
 * @property {string} icon             Inline SVG for the tab icon.
 * @property {boolean} [requiresRepo]  Tab is only meaningful with a repo scope.
 * @property {string} [deprecatedTabId] Legacy MoldableView tab id kept for state compat.
 * @property {string} [engine]         Layout-engine selector for SVG renderers
 *                                     (ui-layout.md §10 Layout Engines).
 */

/** @type {LayoutRegistration[]} */
const registry = [
  {
    name: 'graph',
    component: ExplorerCanvas,
    labelKey: 'explorer_view.layout_graph',
    icon: '<circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/><path d="M7 12h10M17 7l-10 4M17 17L7 13"/>',
  },
  {
    // ui-layout.md §10: hierarchical → ELK (elkjs, WASM), rendered as SVG
    name: 'hierarchical',
    component: SvgGraphCanvas,
    engine: 'hierarchical',
    labelKey: 'explorer_view.layout_hierarchical',
    icon: '<rect x="9" y="3" width="6" height="4" rx="1"/><rect x="4" y="17" width="6" height="4" rx="1"/><rect x="14" y="17" width="6" height="4" rx="1"/><path d="M12 7v4M7 17v-3h10v3"/>',
  },
  {
    // ui-layout.md §10: layered → ELK (elkjs, WASM) left-to-right, SVG
    name: 'layered',
    component: SvgGraphCanvas,
    engine: 'layered',
    labelKey: 'explorer_view.layout_layered',
    icon: '<rect x="3" y="4" width="6" height="4" rx="1"/><rect x="15" y="10" width="6" height="4" rx="1"/><rect x="3" y="16" width="6" height="4" rx="1"/><path d="M9 6h3a2 2 0 012 2v2M15 14h-3a2 2 0 00-2 2v0"/>',
  },
  {
    // ui-layout.md §10: side-by-side → CSS Grid (50/50 or 60/40)
    name: 'side-by-side',
    component: SideBySideView,
    labelKey: 'explorer_view.layout_side_by_side',
    icon: '<rect x="3" y="4" width="7" height="16" rx="1"/><rect x="14" y="4" width="7" height="16" rx="1"/>',
  },
  {
    // ui-layout.md §10: diff → custom component (added/modified/removed cards)
    name: 'diff',
    component: DiffView,
    labelKey: 'explorer_view.layout_diff',
    icon: '<path d="M12 3v18"/><rect x="3" y="6" width="7" height="5" rx="1"/><rect x="14" y="13" width="7" height="5" rx="1"/>',
    requiresRepo: true,
  },
  {
    name: 'list',
    component: null, // list renders as the shell's virtualized node list (VirtualNodeList)
    labelKey: 'explorer_view.layout_list',
    icon: '<line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>',
  },
  {
    name: 'timeline',
    component: null, // timeline renders via the canvas TimelineScrubber (d3-scale time axis)
    labelKey: 'explorer_view.layout_timeline',
    icon: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/>',
  },
  {
    // task-065 unified cutover: the separate FlowRenderer tab was merged
    // into ExplorerCanvas — flow/trace data renders through the canvas's
    // evaluative lens, so the 'flow' ViewSpec layout resolves to the same
    // unified canvas component.
    name: 'flow',
    component: ExplorerCanvas,
    labelKey: 'explorer_view.layout_flow',
    icon: '<circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/><path d="M7 11.5l9-5M7 12.5l9 5"/>',
    requiresRepo: true,
  },
];

/**
 * Register a new layout renderer and extend the grammar's accepted layout
 * names to match (ui-layout.md §4 Extensibility — the registry is the only
 * sanctioned way to add a layout). Adding a name that already exists is a
 * no-op — the grammar is closed for modification, so an existing layout's
 * renderer can never be swapped by a later registration.
 *
 * @param {LayoutRegistration} layout
 */
export function registerLayout(layout) {
  if (registry.some((l) => l.name === layout.name)) {
    console.warn(`layoutRegistry: layout '${layout.name}' is already registered — ignoring (grammar is closed for modification)`);
    return;
  }
  registry.push(layout);
  // Keep validateViewSpec's accepted layout set in sync: without this, a
  // spec using the newly registered layout name would be rejected by the
  // client-side grammar check even though the renderer exists.
  registerLayoutName(layout.name);
}

/**
 * Look up a registered layout by name.
 * @param {string} name
 * @returns {LayoutRegistration | undefined}
 */
export function getLayout(name) {
  return registry.find((l) => l.name === name);
}

/** All registered layouts, in registration order. @returns {LayoutRegistration[]} */
export function listLayouts() {
  return [...registry];
}
