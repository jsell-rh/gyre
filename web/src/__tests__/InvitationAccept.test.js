/**
 * InvitationAccept.test.js — pending invitation accept page
 * (`/invite/{workspaceId}` and `/invite/{token}`, user-management.md
 * §UI Pages).
 *
 * Authenticated-workspaceId flow (merged-tree contract, task-110
 * WorkspaceInvitation model):
 *   - Renders the workspace preview from GET /workspaces/{id}
 *   - Resolves my pending invitation through GET /users/me/invitations
 *     (a pending invitation is a WorkspaceInvitation row with
 *     status/expires_at, not an unaccepted membership row)
 *   - Accept calls POST /workspaces/{id}/invite/accept (204)
 *   - Decline calls POST /workspaces/{id}/invite/decline
 *   - Only a Pending invitation is acceptable — an Expired row renders
 *     the none state
 *   - No pending invitation renders the none state
 *
 * Magic-link token flow (`/invite/{token}`, task-110):
 *   - Preview comes from GET /invite/{token}/preview (kind/status/
 *     email/role/workspaces)
 *   - Tenant accept calls POST /invite/{token}/accept and displays the
 *     minted first API key exactly once
 *   - Workspace-kind accept calls POST /workspaces/invitations/{token}/accept
 *   - Decline calls the matching token decline endpoint
 *   - A terminal-status or unknown token renders no accept action
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspace: vi.fn(),
    myInvitations: vi.fn(),
    acceptWorkspaceInvitation: vi.fn(),
    declineWorkspaceInvitation: vi.fn(),
    invitationPreview: vi.fn(),
    acceptTenantInvitationByToken: vi.fn(),
    declineTenantInvitationByToken: vi.fn(),
    acceptWorkspaceInvitationByToken: vi.fn(),
    declineWorkspaceInvitationByToken: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

import InvitationAccept from '../components/InvitationAccept.svelte';
import { api } from '../lib/api.js';

// Frozen wall clock for the whole-minute expiry assertions: without it, any
// >=1s gap between seeding expires_at from a live Date.now() and the render
// flips floor((expires_at - now)/60) from 30 to 29 (flaky 29m/30m race).
let dateNowSpy = null;

afterEach(() => {
  dateNowSpy?.mockRestore();
  dateNowSpy = null;
});

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
    api.acceptWorkspaceInvitationByToken.mockResolvedValue(null); // 204
    api.declineWorkspaceInvitationByToken.mockResolvedValue(null); // 204
    api.acceptTenantInvitationByToken.mockResolvedValue(null);
    api.declineTenantInvitationByToken.mockResolvedValue(null); // 204
  });

  it('renders the workspace preview with the invited role and expiry', async () => {
    // Sub-hour pending invitation: the expiry countdown must render whole
    // minutes, not raw seconds with a minutes unit. Date.now() is frozen so
    // the seeded expiry and the render-time computation agree exactly.
    const nowMs = 1735689600000;
    dateNowSpy = vi.spyOn(Date, 'now').mockReturnValue(nowMs);
    api.myInvitations.mockResolvedValue({
      invitations: [pendingInvitation({ expires_at: Math.floor(nowMs / 1000) + 30 * 60 })],
    });
    const { findByText, container } = render(InvitationAccept, { props: { inviteId: 'ws-9' } });
    expect(await findByText('Workspace invitation')).toBeTruthy();
    expect(await findByText('Payments')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-workspace-preview"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-decline-btn"]')).toBeTruthy();
    // "expires in 30m", never "expires in 1800m".
    expect(container.querySelector('[data-testid="invite-expiry"]').textContent).toBe('expires in 30m');
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

const TOKEN = 'a1b2c3d4'.repeat(8); // 64-hex magic-link token

function tenantPreview(overrides = {}) {
  return {
    kind: 'tenant',
    status: 'Pending',
    created_at: 1700000000,
    expires_at: Math.floor(Date.now() / 1000) + 6 * 86400,
    email: 'invitee@example.com',
    role: 'TenantAdmin',
    workspaces: [
      { id: 'ws-1', name: 'Platform', description: 'The platform workspace', role: 'Admin' },
    ],
    ...overrides,
  };
}

describe('InvitationAccept (magic-link token flow, /invite/{token})', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.invitationPreview.mockResolvedValue(tenantPreview());
    api.acceptTenantInvitationByToken.mockResolvedValue({
      user_id: 'u-new',
      username: 'invitee',
      mode: 'created',
      tenant_id: 'default',
      workspaces: ['ws-1'],
      api_key: { key: 'gyre_minted_key_123', name: 'invitation' },
    });
    api.declineTenantInvitationByToken.mockResolvedValue(null);
    api.acceptWorkspaceInvitationByToken.mockResolvedValue(null);
    api.declineWorkspaceInvitationByToken.mockResolvedValue(null);
  });

  it('previews via the token endpoint with the invited email, role, and workspace preview', async () => {
    // Sub-hour pending link: the expiry countdown must render whole
    // minutes, not raw seconds with a minutes unit. Date.now() is frozen so
    // the seeded expiry and the render-time computation agree exactly.
    const nowMs = 1735689600000;
    dateNowSpy = vi.spyOn(Date, 'now').mockReturnValue(nowMs);
    api.invitationPreview.mockResolvedValue(tenantPreview({ expires_at: Math.floor(nowMs / 1000) + 30 * 60 }));
    const { findByText, container } = render(InvitationAccept, { props: { token: TOKEN } });
    expect(await findByText('Platform')).toBeTruthy();
    expect(await findByText('invitee@example.com')).toBeTruthy();
    expect(api.invitationPreview).toHaveBeenCalledWith(TOKEN);
    // The authenticated flow's endpoints are never touched in token mode.
    expect(api.workspace).not.toHaveBeenCalled();
    expect(api.myInvitations).not.toHaveBeenCalled();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-decline-btn"]')).toBeTruthy();
    // "expires in 30m", never "expires in 1800m".
    expect(container.querySelector('[data-testid="invite-expiry"]').textContent).toBe('expires in 30m');
  });

  it('tenant accept calls the token accept endpoint and displays the minted first API key', async () => {
    const { container, findByText } = render(InvitationAccept, { props: { token: TOKEN } });
    await findByText('Platform');
    await fireEvent.click(container.querySelector('[data-testid="invite-accept-btn"]'));
    await waitFor(() => {
      expect(api.acceptTenantInvitationByToken).toHaveBeenCalledWith(TOKEN, {});
    });
    // The minted credential is displayed exactly once, with a copy action.
    const keyBanner = container.querySelector('[data-testid="invite-minted-key"]');
    expect(keyBanner).toBeTruthy();
    expect(keyBanner.textContent).toContain('gyre_minted_key_123');
    expect(container.querySelector('[data-testid="invite-copy-key-btn"]')).toBeTruthy();
    // The workspace-kind endpoints are not used for a tenant invitation.
    expect(api.acceptWorkspaceInvitationByToken).not.toHaveBeenCalled();
  });

  it('tenant accept without a minted key (SSO link) shows the linked-account state, no key banner', async () => {
    api.acceptTenantInvitationByToken.mockResolvedValue({
      user_id: 'u-sso', username: 'sso-user', mode: 'linked',
      tenant_id: 'default', workspaces: [], api_key: null,
    });
    const { container, findByText } = render(InvitationAccept, { props: { token: TOKEN } });
    await findByText('Platform');
    await fireEvent.click(container.querySelector('[data-testid="invite-accept-btn"]'));
    await waitFor(() => {
      expect(api.acceptTenantInvitationByToken).toHaveBeenCalled();
    });
    expect(await findByText('@sso-user')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-minted-key"]')).toBeNull();
  });

  it('decline calls the tenant token decline endpoint', async () => {
    const { container, findByText } = render(InvitationAccept, { props: { token: TOKEN } });
    await findByText('Platform');
    await fireEvent.click(container.querySelector('[data-testid="invite-decline-btn"]'));
    await waitFor(() => {
      expect(api.declineTenantInvitationByToken).toHaveBeenCalledWith(TOKEN);
    });
    expect(await findByText(/Invitation declined/i)).toBeTruthy();
  });

  it('workspace-kind token accept uses the workspace token endpoint', async () => {
    api.invitationPreview.mockResolvedValue(tenantPreview({
      kind: 'workspace', email: null, role: null,
      workspaces: [{ id: 'ws-9', name: 'Payments', description: null, role: 'Developer' }],
    }));
    const { container, findByText } = render(InvitationAccept, { props: { token: TOKEN } });
    await findByText('Payments');
    await fireEvent.click(container.querySelector('[data-testid="invite-accept-btn"]'));
    await waitFor(() => {
      expect(api.acceptWorkspaceInvitationByToken).toHaveBeenCalledWith(TOKEN);
    });
    expect(api.acceptTenantInvitationByToken).not.toHaveBeenCalled();
    expect(await findByText(/Invitation accepted/i)).toBeTruthy();
  });

  it('renders the none state for a terminal-status link (already used / expired / revoked)', async () => {
    api.invitationPreview.mockResolvedValue(tenantPreview({ status: 'Expired' }));
    const { findByText, container } = render(InvitationAccept, { props: { token: TOKEN } });
    expect(await findByText('Invitation no longer pending')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
    expect(api.acceptTenantInvitationByToken).not.toHaveBeenCalled();
  });

  it('renders the not-found state for an unknown token', async () => {
    api.invitationPreview.mockRejectedValue(Object.assign(new Error('nf'), { status: 404 }));
    const { findByText, container } = render(InvitationAccept, { props: { token: TOKEN } });
    expect(await findByText('Invitation not found')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
  });

  it('renders the load-error state when the preview fetch fails (non-404)', async () => {
    api.invitationPreview.mockRejectedValue(Object.assign(new Error('boom'), { status: 500 }));
    const { findByText, container } = render(InvitationAccept, { props: { token: TOKEN } });
    expect(await findByText('Could not load invitation')).toBeTruthy();
    expect(container.querySelector('[data-testid="invite-card"]')).toBeNull();
    expect(container.querySelector('[data-testid="invite-accept-btn"]')).toBeNull();
  });
});
