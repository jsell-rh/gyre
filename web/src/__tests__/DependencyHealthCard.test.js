import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import DependencyHealthCard from '../components/DependencyHealthCard.svelte';

describe('DependencyHealthCard', () => {
  // ── Rendering with stats ──────────────────────────────────────────────────

  it('displays total repos with dependencies', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 5, staleCount: 0, breakingCount: 0 },
    });

    const stats = container.querySelector('[data-testid="dep-health-stats"]');
    expect(stats).toBeTruthy();
    expect(stats.textContent).toContain('5');
    expect(stats.textContent).toContain('repos with dependencies');
  });

  it('displays stale count in yellow', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 5, staleCount: 3, breakingCount: 0 },
    });

    const stale = container.querySelector('[data-testid="dep-health-stale"]');
    expect(stale).toBeTruthy();
    expect(stale.textContent).toContain('3');
    expect(stale.textContent).toContain('repos with stale dependencies');
  });

  it('displays breaking count in red', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 5, staleCount: 0, breakingCount: 2 },
    });

    const breaking = container.querySelector('[data-testid="dep-health-breaking"]');
    expect(breaking).toBeTruthy();
    expect(breaking.textContent).toContain('2');
    expect(breaking.textContent).toContain('breaking changes unacknowledged');
  });

  it('shows healthy message when no stale or breaking', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 5, staleCount: 0, breakingCount: 0 },
    });

    const healthy = container.querySelector('[data-testid="dep-health-healthy"]');
    expect(healthy).toBeTruthy();
    expect(healthy.textContent).toContain('All dependencies healthy');
  });

  // ── Empty state ───────────────────────────────────────────────────────────

  it('shows empty state when no dependencies', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 0 },
    });

    const empty = container.querySelector('[data-testid="dep-health-empty"]');
    expect(empty).toBeTruthy();
    expect(empty.textContent).toContain('No dependencies detected');
  });

  // ── Loading state ─────────────────────────────────────────────────────────

  it('shows skeleton when loading', () => {
    const { container } = render(DependencyHealthCard, {
      props: { loading: true },
    });

    const skeleton = container.querySelector('[data-testid="dep-health-loading"]');
    expect(skeleton).toBeTruthy();
    expect(container.querySelector('[data-testid="dep-health-stats"]')).toBeNull();
  });

  // ── View graph link ───────────────────────────────────────────────────────

  it('calls onViewGraph when link is clicked', async () => {
    const onViewGraph = vi.fn();
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 3, onViewGraph },
    });

    const link = container.querySelector('[data-testid="dep-health-view-graph"]');
    expect(link).toBeTruthy();
    await fireEvent.click(link);
    expect(onViewGraph).toHaveBeenCalledTimes(1);
  });

  // ── Title ─────────────────────────────────────────────────────────────────

  it('displays Dependency Health title', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 1 },
    });

    const title = container.querySelector('.dep-health-title');
    expect(title.textContent).toBe('Dependency Health');
  });

  // ── Singular/plural labels ────────────────────────────────────────────────

  it('uses singular "repo" when count is 1', () => {
    const { container } = render(DependencyHealthCard, {
      props: { totalWithDeps: 1, staleCount: 1, breakingCount: 1 },
    });

    const stats = container.querySelector('[data-testid="dep-health-stats"]');
    expect(stats.textContent).toContain('1');
    expect(stats.textContent).toContain('repo with dependencies');

    const stale = container.querySelector('[data-testid="dep-health-stale"]');
    expect(stale.textContent).toContain('repo with stale');

    const breaking = container.querySelector('[data-testid="dep-health-breaking"]');
    expect(breaking.textContent).toContain('breaking change unacknowledged');
  });
});
