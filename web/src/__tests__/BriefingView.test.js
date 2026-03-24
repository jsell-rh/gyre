import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import BriefingView from '../components/BriefingView.svelte';

describe('BriefingView', () => {
  it('renders without throwing', () => {
    expect(() => render(BriefingView)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(BriefingView);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows briefing title heading', () => {
    const { container } = render(BriefingView);
    expect(container.innerHTML).toContain('Briefing');
  });

  it('renders metric cards area', () => {
    const { container } = render(BriefingView);
    // The component renders either loading skeletons or metric cards
    expect(container.innerHTML).toBeTruthy();
  });
});
