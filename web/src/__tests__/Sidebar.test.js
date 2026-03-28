import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import Sidebar from '../components/Sidebar.svelte';

describe('Sidebar', () => {
  it('renders without throwing', () => {
    expect(() => render(Sidebar)).not.toThrow();
  });

  it('renders all six navigation items', () => {
    const { getByText } = render(Sidebar);
    expect(getByText('Inbox')).toBeTruthy();
    expect(getByText('Briefing')).toBeTruthy();
    expect(getByText('Explorer')).toBeTruthy();
    expect(getByText('Specs')).toBeTruthy();
    expect(getByText('Meta-specs')).toBeTruthy();
    expect(getByText('Admin')).toBeTruthy();
  });

  it('renders the Gyre logo text', () => {
    const { getByText } = render(Sidebar);
    expect(getByText('Gyre')).toBeTruthy();
  });

  it('shows version text', () => {
    const { getByText } = render(Sidebar, { props: { version: 'v1.2.3' } });
    expect(getByText('v1.2.3')).toBeTruthy();
  });

  it('highlights the active nav item with aria-current', () => {
    const { getByRole } = render(Sidebar, { props: { currentNav: 'explorer' } });
    const explorerBtn = getByRole('button', { name: 'Explorer' });
    expect(explorerBtn.getAttribute('aria-current')).toBe('page');
  });

  it('does not set aria-current on inactive nav items', () => {
    const { getByRole } = render(Sidebar, { props: { currentNav: 'inbox' } });
    const specsBtn = getByRole('button', { name: 'Specs' });
    expect(specsBtn.getAttribute('aria-current')).toBeNull();
  });

  it('shows inbox badge count when inboxBadge > 0', () => {
    const { getByText } = render(Sidebar, { props: { inboxBadge: 5 } });
    expect(getByText('5')).toBeTruthy();
  });

  it('caps inbox badge at 99+', () => {
    const { getByText } = render(Sidebar, { props: { inboxBadge: 150 } });
    expect(getByText('99+')).toBeTruthy();
  });

  it('does not show badge when inboxBadge is 0', () => {
    const { queryByText } = render(Sidebar, { props: { inboxBadge: 0 } });
    // No numeric badge should appear
    expect(queryByText('0')).toBeNull();
  });

  it('inbox aria-label includes unresolved count when badge > 0', () => {
    const { getByRole } = render(Sidebar, { props: { inboxBadge: 3 } });
    const inboxBtn = getByRole('button', { name: /Inbox, 3 unresolved/ });
    expect(inboxBtn).toBeTruthy();
  });

  it('has a collapse button with correct aria-label', () => {
    const { getByRole } = render(Sidebar);
    const collapseBtn = getByRole('button', { name: 'Collapse sidebar' });
    expect(collapseBtn).toBeTruthy();
    expect(collapseBtn.getAttribute('aria-expanded')).toBe('true');
  });

  it('collapse button toggles to expand label after click', async () => {
    const { getByRole } = render(Sidebar);
    const collapseBtn = getByRole('button', { name: 'Collapse sidebar' });
    await fireEvent.click(collapseBtn);
    const expandBtn = getByRole('button', { name: 'Expand sidebar' });
    expect(expandBtn).toBeTruthy();
    expect(expandBtn.getAttribute('aria-expanded')).toBe('false');
  });

  it('has main navigation landmark', () => {
    const { container } = render(Sidebar);
    const nav = container.querySelector('nav[aria-label="Main navigation"]');
    expect(nav).toBeTruthy();
  });
});
