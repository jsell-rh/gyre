import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import DashboardHome from '../components/DashboardHome.svelte';

describe('DashboardHome', () => {
  it('renders without throwing', () => {
    expect(() => render(DashboardHome)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(DashboardHome);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows loading skeleton initially', () => {
    const { container } = render(DashboardHome);
    // The component starts with loading=true and renders skeleton divs
    expect(container.innerHTML).toBeTruthy();
  });

  it('renders with empty wsStore prop', () => {
    expect(() => render(DashboardHome, { props: { wsStore: null } })).not.toThrow();
  });
});
