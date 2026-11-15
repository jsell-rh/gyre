<script>
  /**
   * PublicUserProfile.svelte — tenant-scoped public user profile
   * (user-management.md §User Profile Page, route /@{username}).
   *
   * Surface: display name, username, avatar, timezone, workspace
   * memberships with roles, team memberships, recent public-action
   * activity feed, and aggregate stats (MRs reviewed, specs approved,
   * agents spawned). This is NOT the private /profile settings surface
   * (HSI §12 "What the Profile Is NOT") — no tasks/MRs/agents hubs, no
   * notification or token management.
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { relativeTime, formatDate } from '../lib/timeFormat.js';
  import { getContext } from 'svelte';

  let { username = '', workspaces = [] } = $props();

  const goToEntityDetail = getContext('goToEntityDetail');

  let profile = $state(null);
  let stats = $state(null);
  let activity = $state([]);
  let loading = $state(true);
  let notFound = $state(false);
  let activityLoading = $state(true);

  // Reload whenever the routed username changes.
  $effect(() => {
    const name = username;
    if (name) {
      untrackedLoad(name);
    }
  });

  async function untrackedLoad(name) {
    loading = true;
    notFound = false;
    profile = null;
    stats = null;
    activity = [];
    activityLoading = true;
    try {
      profile = await api.publicUserProfile(name);
    } catch (e) {
      // Cross-tenant and unknown handles are indistinguishable (404).
      notFound = true;
      profile = null;
    }
    loading = false;
    if (!profile) return;
    // Stats and the activity feed load after the header so a slow feed
    // never blocks the identity block.
    api.publicUserStats(name)
      .then(s => { stats = s; })
      .catch(() => { stats = null; });
    try {
      const feed = await api.publicUserActivity(name, { limit: 30 });
      activity = feed?.activity ?? [];
    } catch {
      activity = [];
    }
    activityLoading = false;
  }

  function workspaceName(id) {
    return workspaces.find(w => w.id === id)?.name ?? null;
  }

  function membershipLabel(m) {
    const name = workspaceName(m.workspace_id);
    return name ?? m.workspace_id;
  }

  const ACTIVITY_ICONS = {
    mr_review: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true"><path d="M12 3v18M3 12h18"/></svg>',
    spec_approved: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true"><path d="M20 6L9 17l-5-5"/></svg>',
    spec_rejected: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true"><path d="M18 6L6 18M6 6l12 12"/></svg>',
    spec_revoked: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true"><path d="M18 6L6 18M6 6l12 12"/></svg>',
    agent_spawned: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 8v8M8 12h8"/></svg>',
  };

  function activityIcon(ev) {
    return ACTIVITY_ICONS[ev.event_type] ?? ACTIVITY_ICONS.agent_spawned;
  }

  function activityText(ev) {
    switch (ev.event_type) {
      case 'mr_review':
        return ev.decision === 'changes_requested'
          ? $t('public_profile.activity.requested_changes', { values: { title: ev.mr_title ?? ev.mr_id } })
          : $t('public_profile.activity.approved_mr', { values: { title: ev.mr_title ?? ev.mr_id } });
      case 'spec_approved':
        return $t('public_profile.activity.approved_spec', { values: { path: ev.spec_path } });
      case 'spec_rejected':
        return $t('public_profile.activity.rejected_spec', { values: { path: ev.spec_path } });
      case 'spec_revoked':
        return $t('public_profile.activity.revoked_spec', { values: { path: ev.spec_path } });
      case 'agent_spawned':
        return $t('public_profile.activity.spawned_agent', { values: { name: ev.agent_name ?? ev.agent_id } });
      default:
        return ev.event_type;
    }
  }

  /** Activity entries that reference an entity navigate to its detail. */
  function openActivityEntity(ev) {
    if (!goToEntityDetail) return;
    if (ev.event_type === 'mr_review' && ev.mr_id) {
      goToEntityDetail('mr', ev.mr_id, { repo_id: ev.repo_id, workspace_id: ev.workspace_id, name: ev.mr_title });
    } else if (ev.event_type === 'agent_spawned' && ev.agent_id) {
      goToEntityDetail('agent', ev.agent_id, { workspace_id: ev.workspace_id, name: ev.agent_name });
    }
  }

  function activityClickable(ev) {
    return (ev.event_type === 'mr_review' && ev.mr_id) ||
      (ev.event_type === 'agent_spawned' && ev.agent_id);
  }

  let avatarInitial = $derived(
    (profile?.display_name || profile?.username || '?').trim().charAt(0).toUpperCase() || '?'
  );
</script>

<div class="public-profile" data-testid="public-profile">
  {#if loading}
    <div class="profile-header">
      <Skeleton width="72px" height="72px" />
      <div class="profile-header-text">
        <Skeleton width="220px" height="1.75rem" />
        <Skeleton width="140px" height="1rem" />
      </div>
    </div>
  {:else if notFound}
    <EmptyState
      title={$t('public_profile.not_found_title')}
      description={$t('public_profile.not_found_description')}
    />
  {:else if profile}
    <!-- Header: avatar, display name, @username, timezone -->
    <header class="profile-header" data-testid="public-profile-header">
      <div class="profile-avatar" aria-hidden="true">
        {#if profile.avatar_url}
          <img src={profile.avatar_url} alt="" loading="lazy" />
        {:else}
          <span class="avatar-initial">{avatarInitial}</span>
        {/if}
      </div>
      <div class="profile-header-text">
        <h1 class="profile-display-name">{profile.display_name}</h1>
        <p class="profile-username mono">&#64;{profile.username}</p>
        <p class="profile-meta">
          <span class="meta-item" title={$t('public_profile.timezone')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/></svg>
            {profile.timezone}
          </span>
          <span class="meta-item" title={$t('public_profile.joined')}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/></svg>
            {$t('public_profile.joined')} {formatDate(profile.created_at)}
          </span>
        </p>
      </div>
    </header>

    <!-- Stats bar: MRs reviewed, specs approved, agents spawned -->
    <section class="stats-bar" aria-label={$t('public_profile.stats.label')} data-testid="public-profile-stats">
      <div class="stat-card">
        <span class="stat-value">{stats ? stats.mrs_reviewed : '—'}</span>
        <span class="stat-label">{$t('public_profile.stats.mrs_reviewed')}</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{stats ? stats.specs_approved : '—'}</span>
        <span class="stat-label">{$t('public_profile.stats.specs_approved')}</span>
      </div>
      <div class="stat-card">
        <span class="stat-value">{stats ? stats.agents_spawned : '—'}</span>
        <span class="stat-label">{$t('public_profile.stats.agents_spawned')}</span>
      </div>
    </section>

    <div class="profile-grid">
      <div class="profile-column">
        <!-- Workspace memberships and roles -->
        <section class="profile-section" aria-label={$t('public_profile.memberships')} data-testid="public-profile-memberships">
          <h2>{$t('public_profile.memberships')}</h2>
          {#if profile.memberships?.length}
            <ul class="membership-list">
              {#each profile.memberships as m (m.id)}
                <li class="membership-item">
                  <span class="membership-name">{membershipLabel(m)}</span>
                  <Badge value={m.role} />
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted">{$t('public_profile.no_memberships')}</p>
          {/if}
        </section>

        <!-- Team memberships -->
        <section class="profile-section" aria-label={$t('public_profile.teams')} data-testid="public-profile-teams">
          <h2>{$t('public_profile.teams')}</h2>
          {#if profile.teams?.length}
            <ul class="team-list">
              {#each profile.teams as team (team.id)}
                <li class="team-item">
                  <span class="team-name">{team.name}</span>
                  <span class="team-slug mono" title={team.slug}>team:{team.slug}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted">{$t('public_profile.no_teams')}</p>
          {/if}
        </section>
      </div>

      <!-- Recent activity feed (public actions only) -->
      <section class="profile-section activity-section" aria-label={$t('public_profile.activity.label')} data-testid="public-profile-activity">
        <h2>{$t('public_profile.activity.label')}</h2>
        {#if activityLoading}
          <div class="activity-skeleton">
            {#each Array(4) as _}
              <Skeleton width="100%" height="2.25rem" />
            {/each}
          </div>
        {:else if activity.length}
          <ul class="activity-list">
            {#each activity as ev (ev.event_type + ':' + ev.timestamp + ':' + (ev.mr_id ?? ev.spec_path ?? ev.agent_id ?? ''))}
              <li class="activity-item">
                <span class="activity-icon" aria-hidden="true">{@html activityIcon(ev)}</span>
                {#if activityClickable(ev)}
                  <button
                    class="activity-text activity-link"
                    onclick={() => openActivityEntity(ev)}
                    title={$t('public_profile.activity.view_entity')}
                  >{activityText(ev)}</button>
                {:else}
                  <span class="activity-text">{activityText(ev)}</span>
                {/if}
                <span class="activity-time" title={formatDate(ev.timestamp)}>{relativeTime(ev.timestamp)}</span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="muted">{$t('public_profile.no_activity')}</p>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .public-profile {
    max-width: 960px;
    margin: 0 auto;
    padding: 1.5rem 1rem 3rem;
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }

  /* ── Header ─────────────────────────────────────────────────────── */
  .profile-header {
    display: flex;
    align-items: center;
    gap: 1.25rem;
    flex-wrap: wrap;
  }
  .profile-avatar {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background: var(--color-primary-muted, #dbeafe);
    color: var(--color-primary, #2563eb);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    flex-shrink: 0;
  }
  .profile-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .avatar-initial {
    font-size: 1.75rem;
    font-weight: 600;
  }
  .profile-header-text {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }
  .profile-display-name {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .profile-username {
    margin: 0;
    color: var(--color-text-muted, #6b7280);
  }
  .profile-meta {
    margin: 0.25rem 0 0;
    display: flex;
    gap: 1rem;
    flex-wrap: wrap;
    color: var(--color-text-muted, #6b7280);
    font-size: 0.85rem;
  }
  .meta-item {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }

  /* ── Stats bar ──────────────────────────────────────────────────── */
  .stats-bar {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.75rem;
  }
  .stat-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 1rem 0.5rem;
    border: 1px solid var(--color-border, #e5e7eb);
    border-radius: 8px;
    background: var(--color-surface, #fff);
  }
  .stat-value {
    font-size: 1.5rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .stat-label {
    font-size: 0.8rem;
    color: var(--color-text-muted, #6b7280);
    text-align: center;
  }

  /* ── Two-column layout, collapsing on small screens ─────────────── */
  .profile-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.5rem;
    align-items: start;
  }
  .profile-column {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    min-width: 0;
  }

  .profile-section {
    border: 1px solid var(--color-border, #e5e7eb);
    border-radius: 8px;
    background: var(--color-surface, #fff);
    padding: 1rem 1.25rem;
    min-width: 0;
  }
  .profile-section h2 {
    margin: 0 0 0.75rem;
    font-size: 1rem;
    font-weight: 600;
  }

  .membership-list,
  .team-list,
  .activity-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }

  .membership-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--color-border, #f3f4f6);
  }
  .membership-item:last-child {
    border-bottom: none;
  }
  .membership-name {
    overflow-wrap: anywhere;
  }

  .team-item {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--color-border, #f3f4f6);
  }
  .team-item:last-child {
    border-bottom: none;
  }
  .team-slug {
    font-size: 0.8rem;
    color: var(--color-text-muted, #6b7280);
    overflow-wrap: anywhere;
  }

  /* ── Activity feed ──────────────────────────────────────────────── */
  .activity-section {
    min-height: 200px;
  }
  .activity-item {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--color-border, #f3f4f6);
  }
  .activity-item:last-child {
    border-bottom: none;
  }
  .activity-icon {
    display: inline-flex;
    margin-top: 0.15rem;
    color: var(--color-text-muted, #6b7280);
    flex-shrink: 0;
  }
  .activity-text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: 0.9rem;
  }
  .activity-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: var(--color-primary, #2563eb);
    cursor: pointer;
    font: inherit;
  }
  .activity-link:hover {
    text-decoration: underline;
  }
  .activity-time {
    flex-shrink: 0;
    font-size: 0.8rem;
    color: var(--color-text-muted, #6b7280);
    font-variant-numeric: tabular-nums;
  }
  .activity-skeleton {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .muted {
    color: var(--color-text-muted, #6b7280);
    margin: 0;
  }
  .mono {
    font-family: var(--font-mono, monospace);
  }

  /* ── Responsive: single column below 720px ──────────────────────── */
  @media (max-width: 720px) {
    .profile-grid {
      grid-template-columns: 1fr;
    }
    .stats-bar {
      grid-template-columns: repeat(3, 1fr);
      gap: 0.5rem;
    }
    .stat-card {
      padding: 0.75rem 0.25rem;
    }
    .stat-value {
      font-size: 1.25rem;
    }
    .profile-header {
      gap: 1rem;
    }
    .profile-avatar {
      width: 56px;
      height: 56px;
    }
  }
  @media (max-width: 420px) {
    .stats-bar {
      grid-template-columns: 1fr;
    }
  }
</style>
