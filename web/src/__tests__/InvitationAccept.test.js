/**
 * InvitationAccept.test.js — pending invitation accept page
 * (`/invite/{workspaceId}`, user-management.md §UI Pages).
 *
 * Covers:
 *   - Renders the workspace preview from GET /workspaces/{id}
 *   - Resolves my pending membership through GET /users/{username}
 *   - Accept calls POST /workspaces/{id}/invite/accept
 *   - Decline calls DELETE /workspaces/{id}/invitations/{invitation_id}
 *   - No pending invitation renders the none state
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspace: vi.fn(),
    me: vi.fn(),
    userByUsername: vi.fn(),
    acceptWorkspaceInvitation: vi.fn(),
    revokeWorkspaceInvitation: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

import InvitationAccept from '../components/InvitationAccept.svelte';
import { api } from '../lib/api.js';

const WS = { id: 'ws-9', name: 'Payments', description: 'Payment processing', slug: 'payments' };
const ME = { id: 'u-1', username: 'jsell' };
const PROFILE = {
  id: 'u-1', username: 'jsell',
  memberships: [
    { id: 'inv-1', user_id: 'u-1', workspace_id: 'ws-9', role: 'Developer', accepted: false, created_at: 1700000000 },
  ],
};

describe('InvitationAccept', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.workspace.mockResolvedValue({ ...WS });
    api.me.mockResolvedValue({ ...ME });
    api.userByUsername.mockResolvedValue(JSON.parse(JSON.stringify(PROFILE)));
    api.acceptWorkspaceInvitation.mockResolvedValue({ accepted: true });
    api.revokeWorkspaceInvitation.mockResolvedValue(null);
  });

  it('renders the workspace preview with the invited role', async () => {
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('Workspace invitation')).toBeTruthy();
    expect(await findByText('Payments')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-workspace-preview"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-decline-btn"]')).toBeTruthy();
    expect(api.workspace).toHaveBeenCalledWith('ws-9');
    expect(api.userByUsername).toHaveBeenCalledWith('jsell');
  });

  it('accept calls the accept endpoint and shows the accepted state', async () => {
    const { container, findByText } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    await findByText('Payments');
    await fireEvent.click(container.querySelector('[data-testid="invite-accept-btn"]'));
    await waitFor(() => {
      expect(api.acceptWorkspaceInvitation).toHaveBeenCalledWith('ws-9');
    });
    expect(await findByText(/Invitation accepted/i)).toBeTruthy();
  });

  it('decline calls the invitation revoke (self-decline) endpoint', async () => {
    const { container, findByText } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    await findByText('Payments');
    await fireEvent.click(container.querySelector('[data-testid="invite-decline-btn"]'));
    await waitFor(() => {
      expect(api.revokeWorkspaceInvitation).toHaveBeenCalledWith('ws-9', 'inv-1');
    });
    expect(await findByText(/Invitation declined/i)).toBeTruthy();
  });

  it('renders the no-pending-invitation state when none exists', async () => {
    api.userByUsername.mockResolvedValue({ id: 'u-1', username: 'jsell', memberships: [] });
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('No pending invitation')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
  });

  it('renders the not-found state for an unknown workspace', async () => {
    api.workspace.mockRejectedValue(Object.assign(new Error('nf'), { status: 404 }));
    const { findByText } = render(InvitationAccept, { props: { inviteId: 'ws-x' } });
    expect(await findByText('Workspace not found')).toBeTruthy();
  });
});
