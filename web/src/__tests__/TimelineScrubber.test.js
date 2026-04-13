import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import TimelineScrubber from '../components/TimelineScrubber.svelte';

// ── Test data ─────────────────────────────────────────────────────────────

const NOW_TS = 1712000000; // 2024-04-01T00:00:00Z approx

const TIMELINE = [
  { id: 'd1', repo_id: 'r1', commit_sha: 'aaaa111', timestamp: NOW_TS - 86400 * 30, spec_ref: null, agent_id: null, delta_json: '{"nodes_extracted":5,"edges_extracted":3}' },
  { id: 'd2', repo_id: 'r1', commit_sha: 'bbbb222', timestamp: NOW_TS - 86400 * 20, spec_ref: 'specs/system/auth.md', agent_id: 'a1', delta_json: '{"nodes_extracted":8,"edges_extracted":5,"nodes_added":[{"name":"AuthService","node_type":"type","qualified_name":"auth.AuthService"},{"name":"AuthHandler","node_type":"function","qualified_name":"auth.AuthHandler"},{"name":"TokenValidator","node_type":"type","qualified_name":"auth.TokenValidator"},{"name":"SessionStore","node_type":"type","qualified_name":"auth.SessionStore"}]}' },
  { id: 'd3', repo_id: 'r1', commit_sha: 'cccc333', timestamp: NOW_TS - 86400 * 10, spec_ref: null, agent_id: null, delta_json: '{"nodes_extracted":10,"edges_extracted":8}' },
  { id: 'd4', repo_id: 'r1', commit_sha: 'dddd444', timestamp: NOW_TS - 86400 * 5, spec_ref: null, agent_id: null, delta_json: '{"nodes_extracted":12,"edges_extracted":10,"nodes_added":[{"name":"UserRepo","node_type":"type","qualified_name":"domain.UserRepo"},{"name":"create_user","node_type":"function","qualified_name":"api.create_user"},{"name":"get_user","node_type":"function","qualified_name":"api.get_user"},{"name":"delete_user","node_type":"function","qualified_name":"api.delete_user"},{"name":"list_users","node_type":"function","qualified_name":"api.list_users"},{"name":"UserValidator","node_type":"type","qualified_name":"domain.UserValidator"}]}' },
  { id: 'd5', repo_id: 'r1', commit_sha: 'eeee555', timestamp: NOW_TS, spec_ref: null, agent_id: null, delta_json: '{"nodes_extracted":12,"edges_extracted":10}' },
];

const NODES_WITH_MOMENTS = [
  { id: 'n1', node_type: 'type', name: 'AuthService', qualified_name: 'auth.AuthService', spec_path: 'specs/system/auth.md', spec_approved_at: NOW_TS - 86400 * 20, milestone_completed_at: null },
  { id: 'n2', node_type: 'function', name: 'create_user', qualified_name: 'api.create_user', spec_path: null, spec_approved_at: null, milestone_completed_at: NOW_TS - 86400 * 10 },
];

// ── Tests ─────────────────────────────────────────────────────────────────

describe('TimelineScrubber', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders nothing when timeline is empty', () => {
    const { container } = render(TimelineScrubber, { props: { timeline: [] } });
    expect(container.querySelector('.timeline-scrubber-bar')).toBeNull();
  });

  it('renders the scrubber bar when timeline has data', () => {
    const { container } = render(TimelineScrubber, { props: { timeline: TIMELINE } });
    const bar = container.querySelector('.timeline-scrubber-bar');
    expect(bar).toBeTruthy();
    expect(bar.getAttribute('role')).toBe('toolbar');
    expect(bar.getAttribute('aria-label')).toBe('Architectural timeline scrubber');
  });

  it('shows toggle button with Timeline label when inactive', () => {
    const { container } = render(TimelineScrubber, { props: { timeline: TIMELINE, active: false } });
    const toggle = container.querySelector('.tl-toggle-btn');
    expect(toggle).toBeTruthy();
    expect(toggle.textContent).toContain('Timeline');
    // Should not be active
    expect(toggle.classList.contains('active')).toBe(false);
  });

  it('shows active toggle button when active', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: TIMELINE.length - 1 }
    });
    const toggle = container.querySelector('.tl-toggle-btn');
    expect(toggle).toBeTruthy();
    expect(toggle.classList.contains('active')).toBe(true);
  });

  it('calls onToggle when toggle button is clicked', async () => {
    const onToggle = vi.fn();
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: false, onToggle }
    });
    const toggle = container.querySelector('.tl-toggle-btn');
    await fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('shows slider and date when active', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 2 }
    });
    const slider = container.querySelector('.tl-slider');
    expect(slider).toBeTruthy();
    expect(slider.getAttribute('min')).toBe('0');
    expect(slider.getAttribute('max')).toBe(String(TIMELINE.length - 1));
    // Date label should be present
    const dateLabel = container.querySelector('.tl-date');
    expect(dateLabel).toBeTruthy();
    expect(dateLabel.textContent.length).toBeGreaterThan(0);
  });

  it('calls onScrub when slider value changes', async () => {
    const onScrub = vi.fn();
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, onScrub }
    });
    const slider = container.querySelector('.tl-slider');
    await fireEvent.input(slider, { target: { value: '1' } });
    expect(onScrub).toHaveBeenCalledWith(1);
  });

  it('shows range summary when inactive', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: false }
    });
    const summary = container.querySelector('.tl-range-summary');
    expect(summary).toBeTruthy();
    expect(summary.textContent).toContain('5 changes');
    // Should show date range
    const dates = container.querySelector('.tl-range-dates');
    expect(dates).toBeTruthy();
  });

  it('shows delta stats with per-type breakdown when provided and active', () => {
    const stats = {
      added: 5, removed: 2, modified: 3,
      addedByType: { type: 3, function: 2 },
      removedByType: { trait: 2 },
      modifiedByType: { type: 3 },
    };
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 2, deltaStats: stats }
    });
    const statsEl = container.querySelector('.tl-delta-stats');
    expect(statsEl).toBeTruthy();
    // Per-type added: "+3 types" and "+2 functions"
    const addEls = container.querySelectorAll('.tl-stat-add');
    expect(addEls.length).toBe(2);
    expect(addEls[0].textContent).toBe('+3 types');
    expect(addEls[1].textContent).toBe('+2 functions');
    // Per-type removed: "-2 traits"
    const removeEls = container.querySelectorAll('.tl-stat-remove');
    expect(removeEls.length).toBe(1);
    expect(removeEls[0].textContent).toBe('-2 traits');
    // Per-type modified: "3 types modified"
    const modEls = container.querySelectorAll('.tl-stat-modify');
    expect(modEls.length).toBe(1);
    expect(modEls[0].textContent).toContain('3 types modified');
  });

  it('does not show delta stats section when not provided', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 2, deltaStats: null }
    });
    expect(container.querySelector('.tl-delta-stats')).toBeNull();
  });

  it('renders key moment markers for significant timeline entries', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: [] }
    });
    // Delta d2 has spec_ref (spec_approval) and d4 has >5 nodes_added (structural_change)
    const markers = container.querySelectorAll('.tl-marker');
    expect(markers.length).toBeGreaterThan(0);
  });

  it('renders key moment markers from node spec_approved_at', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: NODES_WITH_MOMENTS }
    });
    const markers = container.querySelectorAll('.tl-marker');
    // Should have markers for spec_approval (d2 spec_ref + node n1 spec_approved_at)
    // and milestone (node n2 milestone_completed_at)
    // and structural_change (d4 >5 added)
    expect(markers.length).toBeGreaterThan(0);
  });

  it('shows key moment detail popover when marker is clicked', async () => {
    const onMarkerClick = vi.fn();
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: [], onMarkerClick }
    });
    const markers = container.querySelectorAll('.tl-marker');
    expect(markers.length).toBeGreaterThan(0);
    // Click the first marker
    await fireEvent.click(markers[0]);
    expect(onMarkerClick).toHaveBeenCalledTimes(1);
    // Popover should be visible
    const popover = container.querySelector('.tl-popover');
    expect(popover).toBeTruthy();
    // Should show event type
    const typeEl = container.querySelector('.tl-popover-type');
    expect(typeEl).toBeTruthy();
    expect(typeEl.textContent.length).toBeGreaterThan(0);
  });

  it('popover shows timestamp, spec, and commit details', async () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: [] }
    });
    const markers = container.querySelectorAll('.tl-marker');
    // Find the marker for d2 (spec_approval) - first marker
    await fireEvent.click(markers[0]);

    const popover = container.querySelector('.tl-popover');
    expect(popover).toBeTruthy();

    // Check for Time field
    const fields = popover.querySelectorAll('.tl-popover-field');
    expect(fields.length).toBeGreaterThan(0);
    const labels = Array.from(popover.querySelectorAll('.tl-popover-label')).map(el => el.textContent);
    expect(labels).toContain('Time');
  });

  it('popover closes when backdrop is clicked', async () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: [] }
    });
    const markers = container.querySelectorAll('.tl-marker');
    await fireEvent.click(markers[0]);
    expect(container.querySelector('.tl-popover')).toBeTruthy();
    const backdrop = container.querySelector('.tl-popover-backdrop');
    await fireEvent.click(backdrop);
    expect(container.querySelector('.tl-popover')).toBeNull();
  });

  it('popover "View at this point" button calls onScrub and closes popover', async () => {
    const onScrub = vi.fn();
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 4, nodes: [], onScrub }
    });
    const markers = container.querySelectorAll('.tl-marker');
    await fireEvent.click(markers[0]);
    const viewBtn = container.querySelector('.tl-popover-view-btn');
    expect(viewBtn).toBeTruthy();
    await fireEvent.click(viewBtn);
    expect(onScrub).toHaveBeenCalledTimes(1);
    // Popover should be closed
    expect(container.querySelector('.tl-popover')).toBeNull();
  });

  it('shows "Now" label when scrubber is at the latest position', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: TIMELINE.length - 1 }
    });
    const presentLabel = container.querySelector('.tl-present-label');
    expect(presentLabel).toBeTruthy();
    expect(presentLabel.textContent.trim()).toBe('Now');
  });

  it('shows empty "Now" label when scrubber is at historical position', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: true, scrubIndex: 1 }
    });
    const presentLabel = container.querySelector('.tl-present-label');
    expect(presentLabel).toBeTruthy();
    expect(presentLabel.textContent.trim()).toBe('');
  });

  it('shows key moments count when inactive', () => {
    const { container } = render(TimelineScrubber, {
      props: { timeline: TIMELINE, active: false, nodes: [] }
    });
    // TIMELINE has d2 (spec_ref → spec_approval) and d4 (6 nodes_added → structural_change)
    const keyCount = container.querySelector('.tl-key-count');
    expect(keyCount).toBeTruthy();
    expect(keyCount.textContent).toContain('key moment');
  });

  it('handles repo with no deltas gracefully (hidden)', () => {
    const { container } = render(TimelineScrubber, { props: { timeline: [] } });
    expect(container.querySelector('.timeline-scrubber-bar')).toBeNull();
    // No errors thrown
  });

  it('handles repo with single delta gracefully', () => {
    const singleTimeline = [TIMELINE[0]];
    const { container } = render(TimelineScrubber, {
      props: { timeline: singleTimeline, active: true, scrubIndex: 0 }
    });
    const bar = container.querySelector('.timeline-scrubber-bar');
    expect(bar).toBeTruthy();
    const slider = container.querySelector('.tl-slider');
    expect(slider).toBeTruthy();
    expect(slider.getAttribute('max')).toBe('0');
  });
});
