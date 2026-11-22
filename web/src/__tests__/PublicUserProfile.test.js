/**
 * PublicUserProfile.test.js — Tests for the tenant-scoped public user
 * profile page (user-management.md §User Profile Page, /@{username}).
 *
 * Covers:
 *   - Header: display name, @username, timezone
 *   - Workspace memberships with role badges
 *   - Team memberships
 *   - Stats bar: MRs reviewed / specs approved / agents spawned
 *   - Activity feed (public actions only) and entity navigation
 *   - 404 handling (cross-tenant / unknown handles are indistinguishable)
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    publicUserProfile: vi.fn(),
    publicUserStats: vi.fn(),
    publicUserActivity: vi.fn(),
  },
}));

import { api } from '../lib/api.js';
import PublicUserProfile from '../components/PublicUserProfile.svelte';

const PROFILE = {
  id: 'u-1',
  username: 'jsell',
  display_name: 'James Sell',
  avatar_url: null,
  timezone: 'America/New_York',
  created_at: 1690000000,
  memberships: [
    { id: 'm-1', workspace_id: 'ws-1', role: 'Owner', accepted: true },
  ],
  teams: [
    { id: 't-1', workspace_id: 'ws-1', name: 'Platform', slug: 'platform' },
  ],
};

const STATS = { mrs_reviewed: 3, specs_approved: 7, agents_spawned: 12 };

const ACTIVITY = [
  { event_type: 'agent_spawned', timestamp: 1700000300, agent_id: 'a-1', agent_name: 'implementer', workspace_id: 'ws-1' },
  { event_type: 'spec_approved', timestamp: 1700000200, spec_path: 'specs/system/vision.md' },
  { event_type: 'mr_review', timestamp: 1700000100, mr_id: 'mr-1', mr_title: 'Add feature', repo_id: 'repo-1', workspace_id: 'ws-1', decision: 'approved' },
];

const WORKSPACES = [{ id: 'ws-1', name: 'Payments', slug: 'payments' }];

const goToEntityDetail = vi.fn();
const CTX = new Map([['goToEntityDetail', goToEntityDetail]]);
const r = (props = {}) => render(PublicUserProfile, { props, context: CTX });

beforeEach(() => {
  vi.clearAllMocks();
  api.publicUserProfile.mockResolvedValue(JSON.parse(JSON.stringify(PROFILE)));
  api.publicUserStats.mockResolvedValue({ ...STATS });
  api.publicUserActivity.mockResolvedValue({ activity: JSON.parse(JSON.stringify(ACTIVITY)), total: 3 });
});

describe('PublicUserProfile', () => {
  it('renders without throwing', () => {
    expect(() => r({ username: 'jsell' })).not.toThrow();
  });

  it('shows display name, @username, and timezone after loading', async () => {
    const { findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    expect(await findByText('James Sell')).toBeTruthy();
    expect(await findByText('@jsell')).toBeTruthy();
    expect(await findByText('America/New_York')).toBeTruthy();
  });

  it('shows workspace memberships with role badges and teams', async () => {
    const { container, findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    await findByText('James Sell');
    // Workspace name resolved from the workspaces prop, with the role badge.
    expect(await findByText('Payments')).toBeTruthy();
    const roleBadge = container.querySelector('[data-testid="public-profile-memberships"] .badge, [data-testid="public-profile-memberships"] [class*="badge"]');
    expect(roleBadge?.textContent).toContain('Owner');
    // Team membership.
    expect(await findByText('Platform')).toBeTruthy();
    expect(container.querySelector('[data-testid="public-profile-memberships"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="public-profile-teams"]')).toBeTruthy();
  });

  it('shows the stats bar with real counts', async () => {
    const { container, findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    await findByText('James Sell');
    const stats = container.querySelector('[data-testid="public-profile-stats"]');
    expect(stats).toBeTruthy();
    await waitFor(() => {
      expect(stats.textContent).toContain('3');
      expect(stats.textContent).toContain('7');
      expect(stats.textContent).toContain('12');
    });
    expect(stats.textContent).toContain('MRs reviewed');
    expect(stats.textContent).toContain('Specs approved');
    expect(stats.textContent).toContain('Agents spawned');
  });

  it('renders public-action activity and navigates to the reviewed MR', async () => {
    const { container, findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    await findByText('James Sell');
    const feed = container.querySelector('[data-testid="public-profile-activity"]');
    expect(feed).toBeTruthy();
    // Spec approval text includes the spec path.
    expect(feed.textContent).toContain('specs/system/vision.md');
    // MR review entry is a link into the MR detail view (the agent-spawn
    // entry is also clickable — select the MR one by its text).
    const mrLink = [...feed.querySelectorAll('button.activity-link')]
      .find(el => el.textContent.includes('Add feature'));
    expect(mrLink).toBeTruthy();
    expect(mrLink.textContent).toContain('Add feature');
    await fireEvent.click(mrLink);
    expect(goToEntityDetail).toHaveBeenCalledWith('mr', 'mr-1', expect.objectContaining({ repo_id: 'repo-1' }));
  });

  it('shows an empty activity message when the feed is empty', async () => {
    api.publicUserActivity.mockResolvedValue({ activity: [], total: 0 });
    const { container, findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    await findByText('James Sell');
    await waitFor(() => {
      expect(container.querySelector('[data-testid="public-profile-activity"]').textContent)
        .toContain('No recent public activity');
    });
  });

  it('shows a not-found state for unknown or cross-tenant handles (404)', async () => {
    api.publicUserProfile.mockRejectedValue(new Error('404'));
    const { container, findByText } = r({ username: 'nobody', workspaces: WORKSPACES });
    expect(await findByText('User not found')).toBeTruthy();
    expect(container.querySelector('[data-testid="public-profile-header"]')).toBeNull();
    // No stats/activity probes after the profile itself failed.
    expect(api.publicUserStats).not.toHaveBeenCalled();
    expect(api.publicUserActivity).not.toHaveBeenCalled();
  });

  it('does not render private /profile surfaces (HSI §12 NOT-list)', async () => {
    const { container, findByText } = r({ username: 'jsell', workspaces: WORKSPACES });
    await findByText('James Sell');
    const text = container.textContent;
    expect(text).not.toContain('My Tasks');
    expect(text).not.toContain('My MRs');
    expect(text).not.toContain('My Agents');
    expect(text).not.toContain('API Token');
  });
});
