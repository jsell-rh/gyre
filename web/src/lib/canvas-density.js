/**
 * Rendering density policy for the Explorer canvas
 * (ui-layout.md §10 Canvas Rendering).
 *
 * SVG-based graph views are DOM-heavy: every node is an addressable SVG
 * element. To keep them responsive, the spec caps what the canvas renders:
 *
 *   - ≤ 500 nodes                     → render everything ('full')
 *   - > 500 nodes                     → render public nodes only
 *                                        ('public-only', banner + [Show All])
 *   - > 1000 nodes *even after* the
 *     public filter                    → the Explorer switches to the list
 *                                        layout with virtual scrolling and a
 *                                        warning ('list')
 *
 * The policy is pure so it is unit-testable and shareable between the
 * Canvas-2D ExplorerCanvas (which applies the public-only filter in its
 * layout pipeline), the SVG canvas (SvgGraphCanvas, which applies the full
 * cascade), and the Explorer shell (ExplorerView, which owns the list
 * fallback swap).
 */

/** Auto-filter threshold: above this count only public nodes render. */
export const PUBLIC_FILTER_THRESHOLD = 500;

/** List-fallback threshold: above this count *after* filtering, switch to list. */
export const LIST_FALLBACK_THRESHOLD = 1000;

/**
 * A node counts as public when its visibility is 'public' or missing.
 * ('pub_crate'/'internal' items are not part of the public API surface —
 * they are counted among the hidden nodes.)
 *
 * @param {{ visibility?: string } | null | undefined} node
 * @returns {boolean}
 */
export function isPublicNode(node) {
  if (!node) return false;
  return node.visibility == null || node.visibility === 'public';
}

/**
 * Apply the rendering density policy.
 *
 * @param {Array<{ visibility?: string }>} nodes Nodes the view wants to render.
 * @param {{ showAll?: boolean }} [options] `showAll` is the user's [Show All]
 *        override from the public-only banner — it disables both filters
 *        (the user explicitly asked for everything).
 * @returns {{
 *   mode: 'full' | 'public-only' | 'list',
 *   nodes: Array<object>,       nodes to render (in 'list' mode: all of them —
 *                               the virtual-scrolling list has no SVG cost)
 *   total: number,              original node count
 *   publicCount: number,        public nodes in the input
 *   hiddenCount: number,        nodes hidden by the public-only filter
 * }}
 */
export function applyDensityPolicy(nodes, options = {}) {
  const input = Array.isArray(nodes) ? nodes : [];
  const showAll = options.showAll === true;
  const total = input.length;
  const publicCount = input.filter(isPublicNode).length;
  const hiddenCount = total - publicCount;

  if (showAll || total <= PUBLIC_FILTER_THRESHOLD) {
    return { mode: 'full', nodes: input, total, publicCount, hiddenCount };
  }

  // "For views exceeding ~1000 nodes even after filtering, the Explorer
  // switches to list layout with virtual scrolling and a warning."
  if (total > LIST_FALLBACK_THRESHOLD && publicCount > LIST_FALLBACK_THRESHOLD) {
    // The list virtualizes, so it renders every node — the threshold exists
    // to protect the SVG canvas, not the list.
    return { mode: 'list', nodes: input, total, publicCount, hiddenCount };
  }

  if (hiddenCount > 0) {
    return {
      mode: 'public-only',
      nodes: input.filter(isPublicNode),
      total,
      publicCount,
      hiddenCount,
    };
  }

  // Over threshold but nothing to hide — render everything.
  return { mode: 'full', nodes: input, total, publicCount, hiddenCount: 0 };
}
