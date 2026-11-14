<script>
  /**
   * InvitationAccept.svelte — pending invitation accept page
   * (`/invite/{id}`, user-management.md §UI Pages).
   *
   * Shows a workspace preview and Accept / Decline actions for the
   * caller's own pending workspace invitation. Backed by task-124
   * endpoints:
   *   - GET  /api/v1/workspaces/{id}            (workspace preview)
   *   - GET  /api/v1/users/{username}           (my pending memberships)
   *   - POST /api/v1/workspaces/{id}/invite/accept
   *   - DELETE /api/v1/workspaces/{id}/invitations/{invitation_id}
   *     (invitee self-decline — §Workspace Invitation Flow step 4)
   *
   * The path segment is the workspace id: task-124 models a pending
   * workspace invitation as a membership row keyed by workspace (there is
   * no token), and the InvitationReceived notification carries the
   * workspace id. The `/invite/{token}` magic-link form belongs to the
   * tenant-invitation flow (task-110).
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
  let membership = $state(null); // my pending membership row, if any
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

  async function load() {
    loading = true;
    notFound = false;
    loadError = null;
    membership = null;
    try {
      workspace = await api.workspace(inviteId);
    } catch (e) {
      if (e?.status === 404) { notFound = true; loading = false; return; }
      workspace = null;
    }
    // Resolve my pending membership for this workspace through my
    // tenant-public profile (memberships include pending invitations).
    try {
      const me = await api.me();
      if (me?.username) {
        const profile = await api.userByUsername(me.username);
        membership = (profile?.memberships ?? []).find(
          m => m.workspace_id === inviteId && !m.accepted
        ) ?? null;
        // An already-accepted membership is not a pending invitation.
      }
    } catch { /* no resolvable identity — leave membership null */ }
    loading = false;
  }

  $effect(() => {
    if (inviteId) load();
  });

  async function accept() {
    if (accepting || !membership) return;
    accepting = true;
    try {
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
    if (declining || !membership) return;
    declining = true;
    try {
      await api.revokeWorkspaceInvitation(inviteId, membership.id);
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
  {:else if !membership}
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
          <Badge variant="info" value={membership.role} />
        </div>
        {#if workspace?.description}
          <p class="ws-preview-desc">{workspace.description}</p>
        {/if}
        <div class="ws-preview-meta">
          <span>{$t('invite_accept.invited_role')}: <strong>{membership.role}</strong></span>
          <span>{$t('invite_accept.invited_on')}: {fmtDate(membership.created_at)}</span>
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
