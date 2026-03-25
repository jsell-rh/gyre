/**
 * ViewEvent system — standardized entity interaction events.
 *
 * All interactive elements (graph nodes, rows, cards, entity references)
 * dispatch ViewEvents. The shell handles them uniformly:
 *   click        → open detail panel
 *   dblclick     → drill down to next scope
 *   hover        → show tooltip
 *   context-menu → open context menu
 *
 * Spec ref: ui-layout.md §10 (Interaction Events)
 */

/**
 * @typedef {'click'|'dblclick'|'hover'|'context-menu'} ViewEventType
 * @typedef {'node'|'edge'|'spec'|'agent'|'mr'|'task'|'workspace'|'repo'} EntityType
 *
 * @typedef {Object} ViewEvent
 * @property {ViewEventType} type
 * @property {EntityType} entity_type
 * @property {string} entity_id
 * @property {{ x: number, y: number }} position
 * @property {any} [data]  — optional raw entity data for immediate use
 */

const listeners = new Set();

/**
 * Dispatch a ViewEvent to all registered handlers.
 * Also fires a DOM CustomEvent for components using event bubbling.
 *
 * @param {EventTarget|null} target — DOM element to fire event on (optional)
 * @param {ViewEvent} event
 */
export function dispatchViewEvent(target, event) {
  for (const handler of listeners) {
    try { handler(event); } catch (e) { console.error('ViewEvent handler error', e); }
  }
  if (target) {
    target.dispatchEvent(
      new CustomEvent('viewevent', { bubbles: true, composed: true, detail: event })
    );
  }
}

/**
 * Register a global ViewEvent handler.
 * Returns an unsubscribe function.
 *
 * @param {(event: ViewEvent) => void} handler
 * @returns {() => void}
 */
export function onViewEvent(handler) {
  listeners.add(handler);
  return () => listeners.delete(handler);
}

/**
 * Build a ViewEvent from a DOM mouse/pointer event.
 *
 * @param {MouseEvent} domEvent
 * @param {EntityType} entity_type
 * @param {string} entity_id
 * @param {any} [data]
 * @returns {ViewEvent}
 */
export function viewEventFromDom(domEvent, entity_type, entity_id, data) {
  let type;
  switch (domEvent.type) {
    case 'click':       type = 'click'; break;
    case 'dblclick':    type = 'dblclick'; break;
    case 'mouseenter':
    case 'mouseover':   type = 'hover'; break;
    case 'contextmenu': type = 'context-menu'; break;
    default:            type = 'click';
  }
  return {
    type,
    entity_type,
    entity_id,
    position: { x: domEvent.clientX, y: domEvent.clientY },
    data,
  };
}
