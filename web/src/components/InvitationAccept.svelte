<script>
  /**
   * InvitationAccept.svelte — pending invitation accept page
   * (`/invite/{workspaceId}` and `/invite/{token}`, user-management.md
   * §UI Pages > Pending Invitation Accept Page).
   *
   * Two disjoint URL shapes (App.svelte parseUrl disambiguates —
   * workspace ids are UUIDs, magic-link tokens are 64 hex chars):
   *
   *  - `/invite/{workspaceId}` — the authenticated invitee flow:
   *    GET /workspaces/{id} (workspace preview) +
   *    GET /users/me/invitations (resolve my Pending WorkspaceInvitation),
   *    then POST /workspaces/{id}/invite/accept | /decline.
   *
   *  - `/invite/{token}` — the task-110 magic-link flow: the 256-bit
   *    CSPRNG token in the URL is the auth factor (no session needed;
   *    api.js sends no Bearer header on these calls). Preview via
   *    GET /invite/{token}/preview (workspace preview, invited email and
   *    role, expiry), then accept / decline via the token endpoints:
   *    POST /invite/{token}/accept | /decline (tenant invitations) and
   *    POST /workspaces/invitations/{token}/accept | /decline (workspace
   *    invitations). A local-mode tenant accept mints the account's
   *    FIRST API key — returned exactly once and displayed here with a
   *    copy action (the platform has no password login; the key is the
   *    credential).
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { getContext } from 'svelte';

  const goToWorkspaceHome = getContext('goToWorkspaceHome');
  const goToCrossWorkspace = getContext('goToCrossWorkspace');

  let { inviteId = '', token = '', onDone = undefined } = $props();

  let workspace = $state(null);
  let invitation = $state(null); // my pending WorkspaceInvitation row, if any
  let preview = $state(null); // magic-link preview: { kind, status, email, role, workspaces, expires_at, ... }
  let acceptResult = $state(null); // tenant token-accept response (may carry the minted first API key)
  let loading = $state(true);
  let notFound = $state(false);
  let loadError = $state(null);
  let accepting = $state(false);
  let declining = $state(false);
  let outcome = $state(null); // 'accepted' | 'declined' | null
  let keyCopied = $state(false);

  const isTokenMode = $derived(!!token);
  // The magic-link preview's workspace rows are the "workspace preview"
  // for the token flow; the workspace fetch is the preview for the
  // authenticated flow.
  const previewWorkspaces = $derived(preview?.workspaces ?? []);

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
    const minutes = Math.floor(secs / 60);
    return `expires in ${minutes}m`;
  }

  async function copyKey() {
    const key = acceptResult?.api_key?.key;
    if (!key) return;
    try {
      await navigator.clipboard.writeText(key);
      keyCopied = true;
    } catch { /* clipboard unavailable — the key is still displayed for manual copy */ }
  }

  async function load() {
    loading = true;
    notFound = false;
    loadError = null;
    invitation = null;
    preview = null;
    acceptResult = null;
    outcome = null;
    keyCopied = false;
    try {
      if (isTokenMode) {
        // Magic-link flow: the token resolves the invitation; only a
        // Pending preview is acceptable (Accepted/Declined/Expired/
        // Revoked links render the none state).
        preview = await api.invitationPreview(token);
      } else {
        workspace = await api.workspace(inviteId);
        // Resolve my pending invitation for this workspace through the
        // invitee-facing listing (task-110: a pending invitation is a
        // WorkspaceInvitation row with status/expires_at, not an
        // unaccepted membership row). Only a Pending invitation is
        // acceptable; Expired, Revoked, Accepted and Declined rows all
        // render the none state.
        try {
          const raw = await api.myInvitations();
          invitation = (raw?.invitations ?? []).find(
            inv => inv.workspace_id === inviteId && inv.status === 'Pending'
          ) ?? null;
        } catch { /* no resolvable identity — leave invitation null */ }
      }
    } catch (e) {
      if (e?.status === 404) { notFound = true; loading = false; return; }
      // Non-404 failure (e.g. HTTP 500): the preview cannot be rendered —
      // surface the load-error state instead of an invite card with an
      // empty workspace name.
      loadError = e?.message ?? String(e);
    }
    loading = false;
  }

  $effect(() => {
    if (inviteId || token) load();
  });

  async function accept() {
    if (accepting) return;
    if (isTokenMode ? !preview : !invitation) return;
    accepting = true;
    try {
      if (isTokenMode) {
        if (preview.kind === 'tenant') {
          // Local mode: the response carries the account's first API key
          // (minted exactly once) and the activated workspaces.
          const result = await api.acceptTenantInvitationByToken(token, {});
          acceptResult = result ?? null;
          outcome = 'accepted';
          showToast($t('invite_accept.accepted_toast', { values: { name: previewWorkspaces[0]?.name ?? '' } }), { type: 'success' });
          onDone?.('accepted', previewWorkspaces[0] ?? null);
        } else {
          // 204 No Content — the activated membership is visible through
          // the workspace surface the page navigates to.
          await api.acceptWorkspaceInvitationByToken(token);
          outcome = 'accepted';
          showToast($t('invite_accept.accepted_toast', { values: { name: previewWorkspaces[0]?.name ?? '' } }), { type: 'success' });
          onDone?.('accepted', previewWorkspaces[0] ?? null);
        }
      } else {
        // 204 No Content on success — the activated membership is visible
        // through the workspace surface the page navigates to.
        await api.acceptWorkspaceInvitation(inviteId);
        outcome = 'accepted';
        showToast($t('invite_accept.accepted_toast', { values: { name: workspace?.name ?? '' } }), { type: 'success' });
        onDone?.('accepted', workspace);
      }
    } catch (e) {
      showToast($t('invite_accept.accept_failed') + ': ' + (e?.message ?? e), { type: 'error' });
    } finally {
      accepting = false;
    }
  }

  async function decline() {
    if (declining) return;
    if (isTokenMode ? !preview : !invitation) return;
    declining = true;
    try {
      if (isTokenMode) {
        if (preview.kind === 'tenant') {
          await api.declineTenantInvitationByToken(token);
        } else {
          await api.declineWorkspaceInvitationByToken(token);
        }
      } else {
        await api.declineWorkspaceInvitation(inviteId);
      }
      outcome = 'declined';
      showToast($t('invite_accept.declined_toast'), { type: 'info' });
      onDone?.('declined', isTokenMode ? (previewWorkspaces[0] ?? null) : workspace);
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
      title={isTokenMode ? $t('invite_accept.token_not_found_title') : $t('invite_accept.not_found_title')}
      description={isTokenMode ? $t('invite_accept.token_not_found_desc') : $t('invite_accept.not_found_desc')}
    />
  {:else if loadError}
    <EmptyState title={$t('invite_accept.load_error_title')} description={loadError} />
  {:else if outcome === 'accepted' && acceptResult}
    <!-- Tenant magic-link accept: local mode minted the account's first
         API key (shown exactly once) or an existing SSO account was
         linked (mode "linked", no key). -->
    <div class="invite-card" data-testid="invite-accepted-card">
      <h1 class="invite-title">{$t('invite_accept.accepted_title')}</h1>
      <p class="invite-subtitle">
        {acceptResult.mode === 'linked'
          ? $t('invite_accept.account_linked_desc')
          : $t('invite_accept.account_created_desc')}
      </p>
      <div class="ws-preview" data-testid="invite-account-summary">
        <div class="ws-preview-head">
          <span class="ws-preview-name">@{acceptResult.username}</span>
        </div>
        {#if (acceptResult.workspaces ?? []).length > 0}
          <div class="ws-preview-meta">
            <span>{$t('invite_accept.workspaces_activated')}:</span>
            <ul class="ws-list">
              {#each acceptResult.workspaces as wsId (wsId)}
                <li class="mono">{wsId}</li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
      {#if acceptResult.api_key?.key}
        <div class="api-key-banner" data-testid="invite-minted-key">
          <p>{$t('invite_accept.api_key_note')}</p>
          <code class="mono">{acceptResult.api_key.key}</code>
          <div class="invite-actions">
            <button class="btn-primary" onclick={copyKey} data-testid="invite-copy-key-btn">
              {keyCopied ? $t('invite_accept.copied') : $t('invite_accept.copy_key')}
            </button>
          </div>
        </div>
      {/if}
      <div class="invite-actions">
        <button class="btn-primary" onclick={() => goToCrossWorkspace?.()} data-testid="invite-go-all">
          {$t('invite_accept.go_to_all')}
        </button>
      </div>
    </div>
  {:else if outcome === 'accepted'}
    <EmptyState
      title={$t('invite_accept.accepted_title', { values: { name: (isTokenMode ? previewWorkspaces[0]?.name : workspace?.name) ?? '' } })}
      description={$t('invite_accept.accepted_desc')}
    >
      <button
        class="btn-primary"
        onclick={() => goToWorkspaceHome?.(isTokenMode ? previewWorkspaces[0] : workspace)}
        data-testid="invite-go-workspace"
      >
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
  {:else if isTokenMode && preview?.status !== 'Pending'}
    <!-- Terminal-status magic link (already used / expired / revoked):
         nothing to accept. -->
    <EmptyState
      title={$t('invite_accept.token_none_title')}
      description={$t('invite_accept.token_none_desc', { values: { status: preview?.status ?? '' } })}
    />
  {:else if !isTokenMode && !invitation}
    <EmptyState
      title={$t('invite_accept.none_title')}
      description={$t('invite_accept.none_desc')}
    />
  {:else if isTokenMode}
    <!-- Magic-link preview card: workspace preview + invited email/role
         + expiry, with token-form accept/decline. -->
    <div class="invite-card" data-testid="invite-card">
      <h1 class="invite-title">{$t('invite_accept.token_title')}</h1>
      <p class="invite-subtitle">{$t('invite_accept.token_subtitle')}</p>

      {#if preview?.email}
        <p class="invite-note" data-testid="invite-invited-email">
          {$t('invite_accept.invited_email')}: <strong>{preview.email}</strong>
        </p>
      {/if}

      <div class="ws-preview" data-testid="invite-workspace-preview">
        {#each previewWorkspaces as ws (ws.id)}
          <div class="ws-preview-head">
            <span class="ws-preview-name">{ws.name}</span>
            <Badge variant="info" value={ws.role} />
          </div>
          {#if ws.description}
            <p class="ws-preview-desc">{ws.description}</p>
          {/if}
        {:else}
          <div class="ws-preview-head">
            <span class="ws-preview-name">{$t('invite_accept.token_no_workspaces')}</span>
          </div>
        {/each}
        <div class="ws-preview-meta">
          {#if preview?.role}
            <span>{$t('invite_accept.invited_role')}: <strong>{preview.role}</strong></span>
          {/if}
          <span>{$t('invite_accept.invited_on')}: {fmtDate(preview?.created_at)}</span>
          {#if expiryLabel(preview)}
            <span data-testid="invite-expiry">{expiryLabel(preview)}</span>
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
  .ws-list { margin: 4px 0 0; padding-left: 18px; font-size: 13px; }
  .api-key-banner {
    border: 1px solid var(--color-border, #ddd);
    border-radius: 8px;
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--color-surface-elevated, #f7f8fa);
  }
  .api-key-banner p { margin: 0; font-size: 13px; }
  .mono { font-family: ui-monospace, monospace; font-size: 13px; word-break: break-all; }
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
