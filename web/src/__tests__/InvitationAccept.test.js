/**
 * InvitationAccept.test.js — pending invitation accept page
 * (`/invite/{workspaceId}`, user-management.md §UI Pages).
 *
 * Covers the merged-tree contract (task-110 WorkspaceInvitation model):
 *   - Renders the workspace preview from GET /workspaces/{id}
 *   - Resolves my pending invitation through GET /users/me/invitations
 *     (a pending invitation is a WorkspaceInvitation row with
 *     status/expires_at, not an unaccepted membership row)
 *   - Accept calls POST /workspaces/{id}/invite/accept (204)
 *   - Decline calls POST /workspaces/{id}/invite/decline
 *   - Only a Pending invitation is acceptable — an Expired row renders
 *     the none state
 *   - No pending invitation renders the none state
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspace: vi.fn(),
    myInvitations: vi.fn(),
    acceptWorkspaceInvitation: vi.fn(),
    declineWorkspaceInvitation: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

import InvitationAccept from '../components/InvitationAccept.svelte';
import { api } from '../lib/api.js';

const WS = { id: 'ws-9', name: 'Payments', description: 'Payment processing', slug: 'payments' };

function pendingInvitation(overrides = {}) {
  return {
    id: 'inv-1',
    workspace_id: 'ws-9',
    user_id: 'u-1',
    invited_by: 'u-owner',
    role: 'Developer',
    status: 'Pending',
    expires_at: Math.floor(Date.now() / 1000) + 6 * 86400,
    created_at: 1700000000,
    accepted_at: null,
    ...overrides,
  };
}

describe('InvitationAccept', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.workspace.mockResolvedValue({ ...WS });
    api.myInvitations.mockResolvedValue({ invitations: [pendingInvitation()] });
    api.acceptWorkspaceInvitation.mockResolvedValue(null); // 204 No Content
    api.declineWorkspaceInvitation.mockResolvedValue(null);
  });

  it('renders the workspace preview with the invited role and expiry', async () => {
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('Workspace invitation')).toBeTruthy();
    expect(await findByText('Payments')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-workspace-preview"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-decline-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-expiry"]')).toBeTruthy();
    expect(api.workspace).toHaveBeenCalledWith('ws-9');
    expect(api.myInvitations).toHaveBeenCalledWith();
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

  it('decline calls the invitee decline endpoint', async () => {
    const { container, findByText } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    await findByText('Payments');
    await fireEvent.click(container.querySelector('[data-testid="invite-decline-btn"]'));
    await waitFor(() => {
      expect(api.declineWorkspaceInvitation).toHaveBeenCalledWith('ws-9');
    });
    expect(await findByText(/Invitation declined/i)).toBeTruthy();
  });

  it('renders the none state for an expired invitation (not acceptable)', async () => {
    api.myInvitations.mockResolvedValue({
      invitations: [pendingInvitation({ status: 'Expired', expires_at: 1690000000 })],
    });
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('No pending invitation')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
    expect(api.acceptWorkspaceInvitation).not.toHaveBeenCalled();
  });

  it('renders the no-pending-invitation state when none exists', async () => {
    api.myInvitations.mockResolvedValue({ invitations: [] });
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('No pending invitation')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
  });

  it('renders the not-found state for an unknown workspace', async () => {
    api.workspace.mockRejectedValue(Object.assign(new Error('nf'), { status: 404 }));
    const { findByText } = render(InvitationAccept, { props: { inviteId: 'ws-x' } });
    expect(await findByText('Workspace not found')).toBeTruthy();
  });

  it('renders the load-error state when the workspace fetch fails (non-404)', async () => {
    api.workspace.mockRejectedValue(Object.assign(new Error('boom'), { status: 500 }));
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('Could not load invitation')).toBeTruthy();
    // No invite card with an empty workspace name, and no actions offered
    // for an invitation whose preview could not be loaded.
    expect(container.querySelector('[data-testid="invite-card"]')).toBeNull();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
    expect(api.myInvitations).not.toHaveBeenCalled();
    expect(api.acceptWorkspaceInvitation).not.toHaveBeenCalled();
  });
});
