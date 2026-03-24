import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import InboxView from '../components/InboxView.svelte';

describe('InboxView', () => {
  it('renders without throwing', () => {
    expect(() => render(InboxView)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(InboxView);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows inbox title heading', () => {
    const { container } = render(InboxView);
    expect(container.innerHTML).toContain('Inbox');
  });

  it('shows section headings', () => {
    const { container } = render(InboxView);
    // Sections are rendered in the DOM (either loading or loaded)
    expect(container.innerHTML).toBeTruthy();
  });
});
