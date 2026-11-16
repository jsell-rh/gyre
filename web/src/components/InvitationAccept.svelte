<script>
  /**
   * InvitationAccept.svelte — pending invitation accept page
   * (`/invite/{workspaceId}`, user-management.md §UI Pages).
   *
   * Shows a workspace preview and Accept / Decline actions for the
   * caller's own pending workspace invitation. Backed by task-110's
   * WorkspaceInvitation model:
   *   - GET  /api/v1/workspaces/{id}            (workspace preview)
   *   - GET  /api/v1/users/me/invitations       (my pending invitations)
   *   - POST /api/v1/workspaces/{id}/invite/accept
   *   - POST /api/v1/workspaces/{id}/invite/decline
   *
   * The path segment is the workspace id: the InvitationReceived
   * notification carries the workspace id, and the authenticated
   * accept/decline routes locate the invitation by (workspace, caller)
   * with no token. The `/invite/{token}` magic-link form belongs to the
   * tenant-invitation flow (task-110's POST /api/v1/invite/{token}/accept
   * — no UI surface yet).
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { getContext } from 'svelte';

  const goToWorkspaceHome = getContext('goToWorkspaceHome');
  const goToCrossWorkspace = getContext('goToCrossWorkspace');

  let { inviteId = '', onDone = undefined } = $props();

  let workspace = $state(null);
  let invitation = $state(null); // my pending WorkspaceInvitation row, if any
  let loading = $state(true);
  let notFound = $state(false);
  let loadError = $state(null);
  let accepting = $state(false);
  let declining = $state(false);
  let outcome = $state(null); // 'accepted' | 'declined' | null

  function fmtDate(ts) {
    if (!ts) return '—';
    try { return new Date(ts * 1000).toLocaleDateString(); } catch { return '—'; }
  }

  // Expiry countdown label for a pending invitation (spec §UI Pages:
  // Workspace Invitations carry an expiry). 'Expired' invitations are
  // not acceptable — the page shows the none state for them.
  function expiryLabel(inv) {
    if (!inv?.expires_at) return '';
    const secs = inv.expires_at - Math.floor(Date.now() / 1000);
    if (secs <= 0) return 'expired';
    const days = Math.floor(secs / 86400);
    if (days >= 1) return `expires in ${days}d`;
    const hours = Math.floor(secs / 3600);
    if (hours >= 1) return `expires in ${hours}h`;
    return `expires in ${secs}m`;
  }

  async function load() {
    loading = true;
    notFound = false;
    loadError = null;
    invitation = null;
    try {
      workspace = await api.workspace(inviteId);
    } catch (e) {
      if (e?.status === 404) { notFound = true; loading = false; return; }
      workspace = null;
    }
    // Resolve my pending invitation for this workspace through the
    // invitee-facing listing (task-110: a pending invitation is a
    // WorkspaceInvitation row with status/expires_at, not an unaccepted
    // membership row). Only a Pending invitation is acceptable; Expired,
    // Revoked, Accepted and Declined rows all render the none state.
    try {
      const raw = await api.myInvitations();
      invitation = (raw?.invitations ?? []).find(
        inv => inv.workspace_id === inviteId && inv.status === 'Pending'
      ) ?? null;
    } catch { /* no resolvable identity — leave invitation null */ }
    loading = false;
  }

  $effect(() => {
    if (inviteId) load();
  });

  async function accept() {
    if (accepting || !invitation) return;
    accepting = true;
    try {
      // 204 No Content on success — the activated membership is visible
      // through the workspace surface the page navigates to.
      await api.acceptWorkspaceInvitation(inviteId);
      outcome = 'accepted';
      showToast($t('invite_accept.accepted_toast', { values: { name: workspace?.name ?? '' } }), { type: 'success' });
      onDone?.('accepted', workspace);
    } catch (e) {
      showToast($t('invite_accept.accept_failed') + ': ' + (e?.message ?? e), { type: 'error' });
    } finally {
      accepting = false;
    }
  }

  async function decline() {
    if (declining || !invitation) return;
    declining = true;
    try {
      await api.declineWorkspaceInvitation(inviteId);
      outcome = 'declined';
      showToast($t('invite_accept.declined_toast'), { type: 'info' });
      onDone?.('declined', workspace);
    } catch (e) {
      showToast($t('invite_accept.decline_failed') + ': ' + (e?.message ?? e), { type: 'error' });
    } finally {
      declining = false;
    }
  }
</script>

<div class="invite-page" data-testid="invite-accept-page">
  {#if loading}
    <p class="invite-note" role="status">{$t('common.loading')}</p>
  {:else if notFound}
    <EmptyState
      title={$t('invite_accept.not_found_title')}
      description={$t('invite_accept.not_found_desc')}
    />
  {:else if loadError}
    <EmptyState title={$t('invite_accept.load_error_title')} description={loadError} />
  {:else if outcome === 'accepted'}
    <EmptyState
      title={$t('invite_accept.accepted_title', { values: { name: workspace?.name ?? '' } })}
      description={$t('invite_accept.accepted_desc')}
    >
      <button class="btn-primary" onclick={() => goToWorkspaceHome?.(workspace)} data-testid="invite-go-workspace">
        {$t('invite_accept.go_to_workspace')}
      </button>
    </EmptyState>
  {:else if outcome === 'declined'}
    <EmptyState
      title={$t('invite_accept.declined_title')}
      description={$t('invite_accept.declined_desc')}
    >
      <button class="btn-primary" onclick={() => goToCrossWorkspace?.()} data-testid="invite-go-all">
        {$t('invite_accept.go_to_all')}
      </button>
    </EmptyState>
  {:else if !invitation}
    <EmptyState
      title={$t('invite_accept.none_title')}
      description={$t('invite_accept.none_desc')}
    />
  {:else}
    <div class="invite-card" data-testid="invite-card">
      <h1 class="invite-title">{$t('invite_accept.title')}</h1>
      <p class="invite-subtitle">{$t('invite_accept.subtitle')}</p>

      <div class="ws-preview" data-testid="invite-workspace-preview">
        <div class="ws-preview-head">
          <span class="ws-preview-name">{workspace?.name ?? workspace?.id}</span>
          <Badge variant="info" value={invitation.role} />
        </div>
        {#if workspace?.description}
          <p class="ws-preview-desc">{workspace.description}</p>
        {/if}
        <div class="ws-preview-meta">
          <span>{$t('invite_accept.invited_role')}: <strong>{invitation.role}</strong></span>
          <span>{$t('invite_accept.invited_on')}: {fmtDate(invitation.created_at)}</span>
          {#if expiryLabel(invitation)}
            <span data-testid="invite-expiry">{expiryLabel(invitation)}</span>
          {/if}
        </div>
      </div>

      <div class="invite-actions">
        <button
          class="btn-primary"
          onclick={accept}
          disabled={accepting || declining}
          data-testid="invite-accept-btn"
        >{accepting ? '…' : $t('invite_accept.accept')}</button>
        <button
          class="btn-secondary"
          onclick={decline}
          disabled={accepting || declining}
          data-testid="invite-decline-btn"
        >{declining ? '…' : $t('invite_accept.decline')}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .invite-page {
    max-width: 520px;
    margin: 48px auto;
    padding: 0 16px;
  }
  .invite-note { color: var(--color-text-secondary, #666); }
  .invite-card {
    border: 1px solid var(--color-border, #ddd);
    border-radius: 12px;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    background: var(--color-surface, #fff);
  }
  .invite-title { margin: 0; font-size: 20px; }
  .invite-subtitle { margin: 0; color: var(--color-text-secondary, #666); font-size: 14px; }
  .ws-preview {
    border: 1px solid var(--color-border, #ddd);
    border-radius: 8px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--color-surface-elevated, #f7f8fa);
  }
  .ws-preview-head { display: flex; align-items: center; gap: 10px; }
  .ws-preview-name { font-size: 16px; font-weight: 600; }
  .ws-preview-desc { margin: 0; font-size: 13px; color: var(--color-text-secondary, #666); }
  .ws-preview-meta { display: flex; flex-direction: column; gap: 4px; font-size: 13px; }
  .invite-actions { display: flex; gap: 10px; }
  .btn-primary, .btn-secondary {
    border-radius: 8px;
    padding: 8px 16px;
    font-size: 14px;
    cursor: pointer;
    border: 1px solid transparent;
  }
  .btn-primary {
    background: var(--color-primary, #4a6cf7);
    color: #fff;
  }
  .btn-secondary {
    background: none;
    border-color: var(--color-border, #ddd);
    color: inherit;
  }
  .btn-primary:disabled, .btn-secondary:disabled { opacity: 0.6; cursor: default; }
</style>
