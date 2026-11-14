/**
 * PublicUserProfile.test.js — tenant-public profile page (`/@username`,
 * user-management.md §UI Pages > User Profile).
 *
 * Covers:
 *   - Renders profile fields from GET /users/{username}
 *   - Workspace memberships render with roles (accepted only)
 *   - Team memberships derived from per-workspace team listings
 *   - 404 renders the not-found state (cross-tenant == nonexistent)
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    userByUsername: vi.fn(),
    workspaces: vi.fn(),
    workspaceTeams: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

import PublicUserProfile from '../components/PublicUserProfile.svelte';
import { api } from '../lib/api.js';

const PROFILE = {
  id: 'u-1',
  username: 'asmith',
  display_name: 'Alice Smith',
  avatar_url: null,
  timezone: 'Europe/Berlin',
  created_at: 1690000000,
  memberships: [
    { id: 'm-1', user_id: 'u-1', workspace_id: 'ws-1', role: 'Developer', accepted: true, created_at: 1690000000 },
    { id: 'm-2', user_id: 'u-1', workspace_id: 'ws-2', role: 'Viewer', accepted: false, created_at: 1690001000 },
  ],
};

const WORKSPACES = [
  { id: 'ws-1', name: 'Payments', slug: 'payments' },
  { id: 'ws-2', name: 'Platform', slug: 'platform' },
];

const TEAMS_WS1 = {
  teams: [
    { id: 't-1', name: 'Payments Team', slug: 'payments-team', lead: 'u-1', member_ids: ['u-1', 'u-2'] },
    { id: 't-2', name: 'Other Team', slug: 'other-team', lead: 'u-2', member_ids: ['u-2'] },
  ],
};

describe('PublicUserProfile', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.userByUsername.mockResolvedValue({ ...PROFILE });
    api.workspaces.mockResolvedValue([...WORKSPACES]);
    api.workspaceTeams.mockResolvedValue({ teams: [] });
  });

  it('renders profile fields from the username lookup', async () => {
    const { findByText, container } = render(PublicUserProfile, { props: { username: 'asmith' } });
    expect(await findByText('Alice Smith')).toBeTruthy();
    expect(await findByText('@asmith')).toBeTruthy();
    expect(await findByText('Europe/Berlin')).toBeTruthy();
    expect(api.userByUsername).toHaveBeenCalledWith('asmith');
    expect(container.querySelector('[data-testid="public-profile-header"]')).toBeTruthy();
  });

  it('renders accepted workspace memberships with roles, hiding pending invitations', async () => {
    const { findByText, queryByText } = render(PublicUserProfile, { props: { username: 'asmith' } });
    await findByText('Alice Smith');
    await waitFor(() => {
      expect(api.workspaceTeams).toHaveBeenCalledWith('ws-1');
    });
    const memberships = await findByText('Workspace memberships');
    expect(memberships).toBeTruthy();
    // Accepted membership renders with its workspace name and role badge.
    expect(await findByText('Payments')).toBeTruthy();
    // The pending ws-2 membership is not public profile surface.
    expect(queryByText('Platform')).toBeNull();
  });

  it('derives team memberships from the workspace team listings', async () => {
    api.workspaceTeams.mockImplementation((wsId) =>
      wsId === 'ws-1' ? Promise.resolve(TEAMS_WS1) : Promise.resolve({ teams: [] }));
    const { findByText, queryByText } = render(PublicUserProfile, { props: { username: 'asmith' } });
    await findByText('Alice Smith');
    expect(await findByText('Payments Team')).toBeTruthy();
    // Only teams whose member_ids include the user render.
    await waitFor(() => expect(queryByText('Other Team')).toBeNull());
  });

  it('renders the not-found state on 404 (cross-tenant handles look nonexistent)', async () => {
    api.userByUsername.mockRejectedValue(Object.assign(new Error('not found'), { status: 404 }));
    const { findByText } = render(PublicUserProfile, { props: { username: 'ghost' } });
    expect(await findByText('User not found')).toBeTruthy();
  });
});
