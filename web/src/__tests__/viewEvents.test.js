import { describe, it, expect, vi, beforeEach } from 'vitest';
import { dispatchViewEvent, onViewEvent, viewEventFromDom } from '../lib/viewEvents.js';

describe('viewEvents', () => {
  beforeEach(() => {
    // Clear all listeners by subscribing/unsubscribing nothing — the module
    // keeps an internal Set, so we just ensure tests don't bleed.
  });

  it('onViewEvent registers a handler and dispatchViewEvent calls it', () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);

    const event = {
      type: 'click',
      entity_type: 'agent',
      entity_id: 'agent-123',
      position: { x: 10, y: 20 },
    };

    dispatchViewEvent(null, event);
    expect(handler).toHaveBeenCalledWith(event);

    unsub();
  });

  it('unsubscribe removes the handler', () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    unsub();

    dispatchViewEvent(null, {
      type: 'click',
      entity_type: 'mr',
      entity_id: 'mr-1',
      position: { x: 0, y: 0 },
    });

    expect(handler).not.toHaveBeenCalled();
  });

  it('multiple handlers all receive events', () => {
    const h1 = vi.fn();
    const h2 = vi.fn();
    const u1 = onViewEvent(h1);
    const u2 = onViewEvent(h2);

    const event = { type: 'hover', entity_type: 'node', entity_id: 'n-1', position: { x: 5, y: 5 } };
    dispatchViewEvent(null, event);

    expect(h1).toHaveBeenCalledWith(event);
    expect(h2).toHaveBeenCalledWith(event);

    u1();
    u2();
  });

  it('dispatchViewEvent fires a DOM CustomEvent when target is provided', () => {
    const target = document.createElement('div');
    const domListener = vi.fn();
    target.addEventListener('viewevent', domListener);

    const event = { type: 'dblclick', entity_type: 'spec', entity_id: 'spec-1', position: { x: 0, y: 0 } };
    dispatchViewEvent(target, event);

    expect(domListener).toHaveBeenCalledOnce();
    expect(domListener.mock.calls[0][0].detail).toEqual(event);
  });

  describe('viewEventFromDom', () => {
    function makeMouseEvent(type, x = 100, y = 200) {
      return new MouseEvent(type, { clientX: x, clientY: y, bubbles: true });
    }

    it('maps click to click type', () => {
      const ev = viewEventFromDom(makeMouseEvent('click'), 'agent', 'a-1');
      expect(ev.type).toBe('click');
      expect(ev.entity_type).toBe('agent');
      expect(ev.entity_id).toBe('a-1');
      expect(ev.position).toEqual({ x: 100, y: 200 });
    });

    it('maps dblclick to dblclick type', () => {
      const ev = viewEventFromDom(makeMouseEvent('dblclick'), 'node', 'n-1');
      expect(ev.type).toBe('dblclick');
    });

    it('maps mouseenter to hover type', () => {
      const ev = viewEventFromDom(makeMouseEvent('mouseenter'), 'mr', 'mr-1');
      expect(ev.type).toBe('hover');
    });

    it('maps contextmenu to context-menu type', () => {
      const ev = viewEventFromDom(makeMouseEvent('contextmenu'), 'task', 't-1');
      expect(ev.type).toBe('context-menu');
    });

    it('includes optional data', () => {
      const data = { name: 'auth-service' };
      const ev = viewEventFromDom(makeMouseEvent('click'), 'node', 'n-2', data);
      expect(ev.data).toEqual(data);
    });
  });
});
