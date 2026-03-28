import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import Modal from '../lib/Modal.svelte';

// Svelte 5 render snippets are not easy to pass from tests,
// so we test the component's JS logic via a thin wrapper.
// For Svelte 5 snippet props we pass children via the component API.

describe('Modal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('does not render when open=false', () => {
    const { container } = render(Modal, { props: { open: false, title: 'Test' } });
    expect(container.querySelector('.modal-backdrop')).toBeNull();
  });

  it('renders when open=true', () => {
    const { container } = render(Modal, { props: { open: true, title: 'Test Modal' } });
    expect(container.querySelector('.modal-backdrop')).toBeTruthy();
  });

  it('renders the title text', () => {
    const { getByText } = render(Modal, { props: { open: true, title: 'Confirm Delete' } });
    expect(getByText('Confirm Delete')).toBeTruthy();
  });

  it('has role="dialog" and aria-modal="true"', () => {
    const { container } = render(Modal, { props: { open: true, title: 'A11y' } });
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).toBeTruthy();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });

  it('aria-labelledby points to the title element', () => {
    const { container } = render(Modal, { props: { open: true, title: 'Linked' } });
    const dialog = container.querySelector('[role="dialog"]');
    const labelledBy = dialog.getAttribute('aria-labelledby');
    expect(labelledBy).toBeTruthy();
    const titleEl = container.querySelector(`#${labelledBy}`);
    expect(titleEl).toBeTruthy();
    expect(titleEl.textContent).toBe('Linked');
  });

  it('close button has correct aria-label', () => {
    const { container } = render(Modal, { props: { open: true, title: 'My Dialog' } });
    const closeBtn = container.querySelector('.modal-close');
    expect(closeBtn).toBeTruthy();
    expect(closeBtn.getAttribute('aria-label')).toBe('Close My Dialog');
  });

  it('applies size class (sm, md, lg, xl)', () => {
    const { container: c1 } = render(Modal, { props: { open: true, title: 'S', size: 'sm' } });
    expect(c1.querySelector('.modal-sm')).toBeTruthy();

    const { container: c2 } = render(Modal, { props: { open: true, title: 'L', size: 'lg' } });
    expect(c2.querySelector('.modal-lg')).toBeTruthy();

    const { container: c3 } = render(Modal, { props: { open: true, title: 'XL', size: 'xl' } });
    expect(c3.querySelector('.modal-xl')).toBeTruthy();
  });

  it('defaults to md size', () => {
    const { container } = render(Modal, { props: { open: true, title: 'Default' } });
    expect(container.querySelector('.modal-md')).toBeTruthy();
  });

  it('calls onclose when close button is clicked', async () => {
    const onclose = vi.fn();
    const { container } = render(Modal, { props: { open: true, title: 'X', onclose } });
    const closeBtn = container.querySelector('.modal-close');
    await fireEvent.click(closeBtn);
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('calls onclose when overlay is clicked', async () => {
    const onclose = vi.fn();
    const { container } = render(Modal, { props: { open: true, title: 'Overlay', onclose } });
    const overlay = container.querySelector('.modal-overlay');
    await fireEvent.click(overlay);
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('calls onclose on Escape key', async () => {
    const onclose = vi.fn();
    const { container } = render(Modal, { props: { open: true, title: 'Esc', onclose } });
    const dialog = container.querySelector('[role="dialog"]');
    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it('calls onsubmit on Enter key (not from textarea)', async () => {
    const onsubmit = vi.fn();
    const { container } = render(Modal, { props: { open: true, title: 'Enter', onsubmit } });
    const dialog = container.querySelector('[role="dialog"]');
    // Simulate Enter from a generic element (not textarea)
    await fireEvent.keyDown(dialog, { key: 'Enter' });
    expect(onsubmit).toHaveBeenCalledTimes(1);
  });

  it('does NOT call onsubmit when Enter is pressed inside a TEXTAREA', async () => {
    const onsubmit = vi.fn();
    const { container } = render(Modal, { props: { open: true, title: 'TA', onsubmit } });
    const dialog = container.querySelector('[role="dialog"]');
    // Create a textarea inside the modal body and dispatch a native KeyboardEvent
    const body = dialog.querySelector('.modal-body');
    const fakeTextarea = document.createElement('textarea');
    body.appendChild(fakeTextarea);
    fakeTextarea.focus();

    // Dispatch a native event that bubbles — target will be the textarea
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true });
    fakeTextarea.dispatchEvent(event);

    // onsubmit should NOT have fired because target is a TEXTAREA
    expect(onsubmit).not.toHaveBeenCalled();
  });

  it('renders modal-footer only when footer snippet is provided', () => {
    // Without footer
    const { container } = render(Modal, { props: { open: true, title: 'No Footer' } });
    expect(container.querySelector('.modal-footer')).toBeNull();
  });

  it('has modal-body for content area', () => {
    const { container } = render(Modal, { props: { open: true, title: 'Body' } });
    expect(container.querySelector('.modal-body')).toBeTruthy();
  });
});
