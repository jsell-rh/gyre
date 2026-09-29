/**
 * Sidebar.test.js — Tests for HSI §1.3 Stable Sidebar
 *
 * Covers:
 *   - All 6 items render (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin)
 *   - Active item is visually highlighted
 *   - Clicking a sidebar item calls onNavigate with the correct item id
 *   - Collapse toggle calls onToggleCollapse
 *   - Badge count renders on Inbox item
 *   - Sidebar items do NOT change based on scope
 *   - Collapsed mode shows icons only
 */

import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import Sidebar from '../lib/Sidebar.svelte';

describe('Sidebar — HSI §1.3 Stable Sidebar', () => {
  it('renders all 6 sidebar items', () => {
    const { container } = render(Sidebar);
    expect(container.querySelector('[data-testid="sidebar-item-inbox"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="sidebar-item-briefing"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="sidebar-item-explorer"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="sidebar-item-specs"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="sidebar-item-meta-specs"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="sidebar-item-admin"]')).toBeTruthy();
  });

  it('renders exactly 6 items', () => {
    const { container } = render(Sidebar);
    const items = container.querySelectorAll('.sidebar-item');
    expect(items.length).toBe(6);
  });

  it('highlights the active sidebar item', () => {
    const { container } = render(Sidebar, { props: { activeItem: 'specs' } });
    const specsItem = container.querySelector('[data-testid="sidebar-item-specs"]');
    expect(specsItem.classList.contains('active')).toBe(true);

    // Other items should NOT be active
    const inboxItem = container.querySelector('[data-testid="sidebar-item-inbox"]');
    expect(inboxItem.classList.contains('active')).toBe(false);
  });

  it('sets aria-current="page" on the active item only', () => {
    const { container } = render(Sidebar, { props: { activeItem: 'explorer' } });
    const explorerItem = container.querySelector('[data-testid="sidebar-item-explorer"]');
    expect(explorerItem.getAttribute('aria-current')).toBe('page');

    const inboxItem = container.querySelector('[data-testid="sidebar-item-inbox"]');
    expect(inboxItem.getAttribute('aria-current')).toBeNull();
  });

  it('calls onNavigate with item id when clicked', async () => {
    const onNavigate = vi.fn();
    const { container } = render(Sidebar, { props: { onNavigate } });

    await fireEvent.click(container.querySelector('[data-testid="sidebar-item-briefing"]'));
    expect(onNavigate).toHaveBeenCalledWith('briefing');

    await fireEvent.click(container.querySelector('[data-testid="sidebar-item-admin"]'));
    expect(onNavigate).toHaveBeenCalledWith('admin');
  });

  it('calls onToggleCollapse when collapse button is clicked', async () => {
    const onToggleCollapse = vi.fn();
    const { container } = render(Sidebar, { props: { onToggleCollapse } });

    const collapseBtn = container.querySelector('[data-testid="sidebar-collapse-btn"]');
    expect(collapseBtn).toBeTruthy();
    await fireEvent.click(collapseBtn);
    expect(onToggleCollapse).toHaveBeenCalledTimes(1);
  });

  it('shows decisions count badge on Inbox when nonzero', () => {
    const { container } = render(Sidebar, { props: { decisionsCount: 7 } });
    const badge = container.querySelector('.sidebar-badge');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toBe('7');
  });

  it('shows 99+ when decisions count exceeds 99', () => {
    const { container } = render(Sidebar, { props: { decisionsCount: 150 } });
    const badge = container.querySelector('.sidebar-badge');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toBe('99+');
  });

  it('does not show badge when decisions count is 0', () => {
    const { container } = render(Sidebar, { props: { decisionsCount: 0 } });
    const badges = container.querySelectorAll('.sidebar-badge');
    expect(badges.length).toBe(0);
  });

  it('sidebar items are identical regardless of activeItem prop (scope independence)', () => {
    // Render with different active items and verify the 6 items are always the same
    const items1 = render(Sidebar, { props: { activeItem: 'inbox' } });
    const items2 = render(Sidebar, { props: { activeItem: 'admin' } });

    const labels1 = Array.from(items1.container.querySelectorAll('.sidebar-label')).map(el => el.textContent);
    const labels2 = Array.from(items2.container.querySelectorAll('.sidebar-label')).map(el => el.textContent);

    expect(labels1).toEqual(['Inbox', 'Briefing', 'Explorer', 'Specs', 'Meta-specs', 'Admin']);
    expect(labels2).toEqual(['Inbox', 'Briefing', 'Explorer', 'Specs', 'Meta-specs', 'Admin']);
  });

  it('collapsed sidebar hides labels', () => {
    const { container } = render(Sidebar, { props: { collapsed: true } });
    const sidebar = container.querySelector('[data-testid="sidebar"]');
    expect(sidebar.classList.contains('collapsed')).toBe(true);
    // Labels should not be rendered in collapsed mode
    const labels = container.querySelectorAll('.sidebar-label');
    expect(labels.length).toBe(0);
  });

  it('collapsed sidebar still renders all 6 items', () => {
    const { container } = render(Sidebar, { props: { collapsed: true } });
    const items = container.querySelectorAll('.sidebar-item');
    expect(items.length).toBe(6);
  });

  it('sidebar has accessible navigation landmark', () => {
    const { container } = render(Sidebar);
    const sidebar = container.querySelector('[data-testid="sidebar"]');
    expect(sidebar.getAttribute('aria-label')).toBe('Main navigation');
  });

  it('each sidebar item has an icon', () => {
    const { container } = render(Sidebar);
    const icons = container.querySelectorAll('.sidebar-icon');
    expect(icons.length).toBe(6);
    // Each icon should contain an SVG
    icons.forEach(icon => {
      expect(icon.querySelector('svg')).toBeTruthy();
    });
  });

  // F2: server version indicator in the footer (ui-layout §1)
  it('renders the server version indicator in the footer when provided', () => {
    const { container } = render(Sidebar, { props: { serverVersion: '0.1.0' } });
    const ver = container.querySelector('[data-testid="sidebar-version"]');
    expect(ver).toBeTruthy();
    expect(ver.textContent.trim()).toBe('v0.1.0');
  });

  it('does not render the version indicator when no version is provided', () => {
    const { container } = render(Sidebar);
    expect(container.querySelector('[data-testid="sidebar-version"]')).toBeNull();
  });

  it('hides the version indicator when collapsed', () => {
    const { container } = render(Sidebar, { props: { serverVersion: '0.1.0', collapsed: true } });
    expect(container.querySelector('[data-testid="sidebar-version"]')).toBeNull();
  });
});
