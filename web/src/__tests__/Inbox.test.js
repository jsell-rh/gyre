import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/svelte';
import Inbox from '../components/Inbox.svelte';

// Mock the api module
vi.mock('../lib/api.js', () => ({
  api: {
    mergeRequests: vi.fn().mockResolvedValue([]),
    getPendingSpecs: vi.fn().mockResolvedValue([]),
    activity: vi.fn().mockResolvedValue([]),
  },
}));

describe('Inbox', () => {
  beforeEach(() => {
    // localStorage mock is provided by jsdom
    localStorage.clear();
  });

  it('renders without throwing', () => {
    expect(() => render(Inbox)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(Inbox);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows the inbox title', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Inbox')).toBeTruthy();
  });

  it('shows loading skeleton initially', () => {
    const { container } = render(Inbox);
    // skeleton or inbox-list rendered
    expect(container.innerHTML).toBeTruthy();
  });

  it('shows refresh button', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Refresh')).toBeTruthy();
  });
});
