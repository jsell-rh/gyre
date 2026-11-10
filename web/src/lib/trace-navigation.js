// Detail-panel trace step navigation (explorer-canvas.md §6 step 6:
// "The detail panel shows the full trace as a list, each step clickable").
//
// Clicking a step swaps the detail panel to that step's node while the host
// pans/zooms the canvas to it. The step list must survive the navigation:
// NodeDetailPanel renders it from node._traceSteps, and the raw graph node
// carries no trace enrichment — so the steps travel onto the target. Without
// that, the first step click unmounts the list and the trace can no longer
// be walked from the panel.

/**
 * Resolve the detail-panel node for a trace step click.
 *
 * @param {object|null} detailNode node currently shown in the detail panel
 *   (carries the `_traceSteps` enrichment of the active trace, if any)
 * @param {{ nodeId: string, step: number }} step the clicked trace step
 * @param {any[]} nodes graph nodes (navigation target lookup)
 * @returns {object|null} the node to display next — the target graph node
 *   with the trace steps carried over — or null when the target is unknown
 */
export function traceStepTargetNode(detailNode, step, nodes) {
  const target = (nodes ?? []).find(n => n.id === step.nodeId);
  if (!target) return null;
  const steps = detailNode?._traceSteps;
  if (!Array.isArray(steps) || steps.length === 0) return target;
  return {
    ...target,
    _traceSteps: steps,
    _traceStep: step.step,
    _traceTotal: steps.length,
  };
}
