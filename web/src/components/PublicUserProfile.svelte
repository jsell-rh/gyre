<script>
  /**
   * PublicUserProfile.svelte — tenant-public profile page (`/@username`)
   * (user-management.md §UI Pages > User Profile).
   *
   * Public within the tenant: display name, username, avatar, timezone,
   * workspace memberships and roles, team memberships. Backed by
   * GET /api/v1/users/{username} (task-124) — the endpoint 404s both
   * nonexistent and cross-tenant handles, so the not-found state here is
   * honest for both.
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { getContext } from 'svelte';

  const goToWorkspaceHome = getContext('goToWorkspaceHome');

  let { username = '' } = $props();

  let user = $state(null);
  let workspaces = $state([]);
  let teamsByWorkspace = $state({});
  let loading = $state(true);
  let notFound = $state(false);
  let loadError = $state(null);

  // A workspace membership row: { id, user_id, workspace_id, role, accepted,
  // accepted_at, created_at }. Only accepted memberships are public profile
  // surface — a pending invitation is not a membership.
  let memberships = $derived(
    (user?.memberships ?? []).filter(m => m.accepted)
  );

  // Distinct workspaces the user is an accepted member of.
  let memberWorkspaceIds = $derived([
    ...new Set(memberships.map(m => m.workspace_id)),
  ]);

  // Team memberships: teams whose member_ids include the user. Teams are
  // listed per workspace (GET /workspaces/{id}/teams) and filtered
  // client-side — there is no per-user team listing endpoint.
  let teamMemberships = $derived.by(() => {
    const out = [];
    for (const wsId of memberWorkspaceIds) {
      for (const team of teamsByWorkspace[wsId] ?? []) {
        if ((team.member_ids ?? []).includes(user?.id)) {
          out.push({ ...team, workspace_id: wsId });
        }
      }
    }
    return out;
  });

  function workspaceName(wsId) {
    const ws = workspaces.find(w => w.id === wsId);
    return ws?.name ?? ws?.slug ?? wsId;
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    try { return new Date(ts * 1000).toLocaleDateString(); } catch { return '—'; }
  }

  async function load() {
    loading = true;
    notFound = false;
    loadError = null;
    try {
      user = await api.userByUsername(username);
    } catch (e) {
      // 404 covers both nonexistent and cross-tenant handles (tenant
      // containment) — the same message for both.
      if (e?.status === 404) notFound = true;
      else loadError = e?.message ?? String(e);
      user = null;
    }
    // Workspace list for name resolution (tenant-scoped listing).
    try { workspaces = await api.workspaces() ?? []; } catch { workspaces = []; }
    // Teams for each member workspace (best-effort: a workspace the caller
    // cannot list teams for simply contributes no team memberships).
    const next = {};
    await Promise.all(memberWorkspaceIds.map(async wsId => {
      try {
        const raw = await api.workspaceTeams(wsId);
        next[wsId] = Array.isArray(raw?.teams) ? raw.teams : Array.isArray(raw) ? raw : [];
      } catch { next[wsId] = []; }
    }));
    teamsByWorkspace = next;
    loading = false;
  }

  $effect(() => {
    if (username) load();
  });
</script>

<div class="public-profile" data-testid="public-profile">
  {#if loading}
    <div class="profile-header">
      <Skeleton width="72px" height="72px" radius="50%" />
      <div class="skeleton-col">
        <Skeleton height="24px" width="220px" />
        <Skeleton height="16px" width="140px" />
      </div>
    </div>
  {:else if notFound}
    <EmptyState
      title={$t('public_profile.not_found_title')}
      description={$t('public_profile.not_found_desc')}
    />
  {:else if loadError}
    <EmptyState
      title={$t('public_profile.load_error_title')}
      description={loadError}
    />
  {:else if user}
    <header class="profile-header" data-testid="public-profile-header">
      <div class="avatar" aria-hidden="true">
        {#if user.avatar_url}
          <img src={user.avatar_url} alt="" class="avatar-img" />
        {:else}
          <span class="avatar-initial">{(user.display_name ?? user.username ?? '?').slice(0, 1).toUpperCase()}</span>
        {/if}
      </div>
      <div class="identity">
        <h1 class="display-name">{user.display_name ?? user.username}</h1>
        <p class="username">{'@'}{user.username}</p>
        <div class="meta-row">
          <span class="meta-item">
            <span class="meta-label">{$t('public_profile.timezone')}</span>
            <span>{user.timezone ?? '—'}</span>
          </span>
          <span class="meta-item">
            <span class="meta-label">{$t('public_profile.member_since')}</span>
            <span>{fmtDate(user.created_at)}</span>
          </span>
        </div>
      </div>
    </header>

    <section class="profile-section" data-testid="public-profile-memberships">
      <h2>{$t('public_profile.workspace_memberships')}</h2>
      {#if memberships.length === 0}
        <p class="empty-text">{$t('public_profile.no_memberships')}</p>
      {:else}
        <ul class="membership-list" role="list">
          {#each memberships as m (m.id)}
            <li class="membership-row">
              <button
                class="ws-link"
                onclick={() => {
                  const ws = workspaces.find(w => w.id === m.workspace_id);
                  if (ws && goToWorkspaceHome) goToWorkspaceHome(ws);
                }}
              >{workspaceName(m.workspace_id)}</button>
              <Badge variant={m.role === 'Owner' ? 'success' : m.role === 'Admin' ? 'info' : 'default'} value={m.role} />
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="profile-section" data-testid="public-profile-teams">
      <h2>{$t('public_profile.team_memberships')}</h2>
      {#if teamMemberships.length === 0}
        <p class="empty-text">{$t('public_profile.no_teams')}</p>
      {:else}
        <ul class="membership-list" role="list">
          {#each teamMemberships as team (team.id)}
            <li class="membership-row">
              <span class="team-name">{team.name}</span>
              {#if team.lead === user.id}
                <Badge variant="info" value={$t('public_profile.lead_badge')} />
              {/if}
              <span class="team-slug mono">{team.slug}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  .public-profile {
    max-width: 720px;
    margin: 0 auto;
    padding: 24px 16px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  .profile-header {
    display: flex;
    gap: 16px;
    align-items: center;
  }
  .skeleton-col { display: flex; flex-direction: column; gap: 8px; }
  .avatar {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background: var(--color-bg-secondary, #eee);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    overflow: hidden;
  }
  .avatar-img { width: 100%; height: 100%; object-fit: cover; }
  .avatar-initial { font-size: 28px; font-weight: 600; }
  .identity { display: flex; flex-direction: column; gap: 4px; }
  .display-name { margin: 0; font-size: 22px; }
  .username { margin: 0; color: var(--color-text-secondary, #666); }
  .meta-row { display: flex; gap: 24px; margin-top: 8px; flex-wrap: wrap; }
  .meta-item { display: flex; gap: 6px; font-size: 13px; }
  .meta-label { color: var(--color-text-secondary, #666); }
  .profile-section h2 { font-size: 15px; margin: 0 0 10px; }
  .membership-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .membership-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border: 1px solid var(--color-border, #ddd);
    border-radius: 8px;
  }
  .ws-link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--color-primary, #4a6cf7);
    cursor: pointer;
  }
  .ws-link:hover { text-decoration: underline; }
  .team-name { font-weight: 500; }
  .team-slug { margin-left: auto; font-size: 12px; color: var(--color-text-secondary, #666); }
  .empty-text { color: var(--color-text-secondary, #666); font-size: 13px; margin: 0; }
  .mono { font-family: var(--font-mono, monospace); }
</style>
