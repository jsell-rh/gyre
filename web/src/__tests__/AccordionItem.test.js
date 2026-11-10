/**
 * AccordionItem.test.js — ui-layout.md §3 Inline Expansion
 *
 * The reusable accordion item used by Inbox cards and Briefing sections.
 * Contract under test:
 *   - body renders only when open (expand/collapse)
 *   - header is a real button (keyboard accessible) with aria-expanded/controls
 *   - ontoggle fires with the item id; the PARENT enforces single-expansion
 *     (controlled) — verified here by a host simulating the accordion group
 */

import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import AccordionActionsHost from '../lib/__accordion_actions_host.svelte';
import AccordionGroup from '../lib/__accordion_group.svelte';
import AccordionItem from '../lib/AccordionItem.svelte';

describe('AccordionItem (ui-layout.md §3 Inline Expansion)', () => {
  it('renders body content only when open', () => {
    const { container } = render(AccordionItem, {
      props: { id: 'a', open: false, ontoggle: () => {}, header: undefined, children: undefined },
    });
    // No body element when closed
    expect(container.querySelector('.accordion-body')).toBeNull();
    expect(container.querySelector('[aria-expanded="false"]')).toBeTruthy();
  });

  it('header click fires ontoggle with the item id', async () => {
    const ontoggle = vi.fn();
    const { container } = render(AccordionItem, {
      props: { id: 'item-7', open: false, ontoggle },
    });
    await fireEvent.click(container.querySelector('.accordion-header'));
    expect(ontoggle).toHaveBeenCalledWith('item-7');
  });

  it('header is a button wired with aria-expanded and aria-controls', () => {
    const { container } = render(AccordionItem, {
      props: { id: 'a', open: true, ontoggle: () => {} },
    });
    const header = container.querySelector('.accordion-header');
    expect(header.tagName).toBe('BUTTON');
    expect(header.getAttribute('aria-expanded')).toBe('true');
    const bodyEl = container.querySelector('.accordion-body');
    expect(bodyEl).toBeTruthy();
    expect(header.getAttribute('aria-controls')).toBe(bodyEl.id);
  });

  it('header is a type=button button (native keyboard activation, no form submission)', () => {
    const { container } = render(AccordionItem, {
      props: { id: 'kb', open: false, ontoggle: () => {} },
    });
    const header = container.querySelector('.accordion-header');
    // Keyboard accessibility comes from the element itself: browsers
    // natively activate <button> from Enter/Space. jsdom does not synthesize
    // the keydown->click step (that is browser behavior, not DOM), so the
    // honest assertions are the element tag and type: a real BUTTON with
    // type=button is keyboard-activatable by construction and never
    // triggers implicit form submission inside a <form>. The activation
    // path itself is covered by the click test above.
    expect(header.tagName).toBe('BUTTON');
    expect(header.getAttribute('type')).toBe('button');
  });

  it('header button id resolves the body region aria-labelledby and headingId override', () => {
    const { container } = render(AccordionItem, {
      props: { id: 'lbl', open: true, ontoggle: () => {}, headingId: 'custom-h' },
    });
    const header = container.querySelector('.accordion-header');
    // headingId names the header button; the open body region points at it.
    expect(header.id).toBe('custom-h');
    const bodyEl = container.querySelector('.accordion-body');
    expect(bodyEl.getAttribute('aria-labelledby')).toBe('custom-h');
    expect(document.getElementById('custom-h')).toBe(header);
  });

  it('actions snippet renders as a sibling of the header button, never inside it', async () => {
    // Compiled host (snippets are compiled fragment functions in Svelte 5,
    // not runtime values) — same pattern as __accordion_group.svelte.
    const { container } = render(AccordionActionsHost);
    const header = container.querySelector('.accordion-header');
    const probe = container.querySelector('.probe-action');
    // Sibling, not descendant: HTML forbids interactive descendants of
    // <button>; browsers may drop or mis-render nested buttons.
    expect(probe).toBeTruthy();
    expect(header.tagName).toBe('BUTTON');
    expect(header.contains(probe)).toBe(false);
    expect(probe.closest('.accordion-actions')).toBeTruthy();
    // Clicking the action never toggles the accordion.
    await fireEvent.click(probe);
    expect(header.getAttribute('aria-expanded')).toBe('false');
  });

  it('no actions row renders when no actions snippet is passed', () => {
    const { container } = render(AccordionItem, {
      props: { id: 'noact', open: false, ontoggle: () => {} },
    });
    expect(container.querySelector('.accordion-actions')).toBeNull();
    expect(container.querySelector('.accordion-row').className).not.toContain('has-actions');
  });
});

describe('Accordion group single-expansion (ui-layout.md §3)', () => {
  it('expanding one item collapses the other (parent-controlled openId)', async () => {
    const { container } = render(AccordionGroup);
    const headers = container.querySelectorAll('.accordion-header');
    expect(headers.length).toBe(2);

    // Nothing open initially
    expect(container.querySelectorAll('.accordion-body').length).toBe(0);

    // Open first
    await fireEvent.click(headers[0]);
    expect(headers[0].getAttribute('aria-expanded')).toBe('true');
    expect(container.querySelectorAll('.accordion-body').length).toBe(1);

    // Open second — first must collapse (single-expansion constraint)
    const headersAfter = container.querySelectorAll('.accordion-header');
    await fireEvent.click(headersAfter[1]);
    const headersFinal = container.querySelectorAll('.accordion-header');
    expect(headersFinal[0].getAttribute('aria-expanded')).toBe('false');
    expect(headersFinal[1].getAttribute('aria-expanded')).toBe('true');
    expect(container.querySelectorAll('.accordion-body').length).toBe(1);

    // Clicking the open item collapses it
    await fireEvent.click(headersFinal[1]);
    expect(container.querySelectorAll('.accordion-body').length).toBe(0);
  });
});
