<script>
  import { t } from 'svelte-i18n';
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import { notificationState, refreshUnreadCount } from '../lib/notifications.svelte.js';
  import { relativeTime } from '../lib/timeFormat.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastError } from '../lib/toast.svelte.js';

  // NotificationDrawer (user-management.md §In-App Notification UI):
  // slide-out panel from the bell — recent notifications grouped by priority
  // (Urgent first), deep links to the source entity, per-item and bulk
  // mark read/unread, filters by type / priority / workspace / repo, and a
  // link to the notification preferences page.

  let { open = $bindable(false), workspaces = [], currentWorkspaceId = null } = $props();

  const openPreferences = getContext('goToNotificationSettings');

  let notifications = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let filterType = $state('');
  let filterPriority = $state('');
  let filterWorkspace = $state('');
  let filterRepo = $state('');
  let repos = $state([]);
  let markingAll = $state(false);
  let itemBusy = $state({});
  // Stale-response guard: a load started under one filter set must not
  // overwrite state after the filters changed.
  let loadGen = 0;

  const PRIORITIES = ['Urgent', 'High', 'Medium', 'Low'];

  // Deep links: the server emits SPA paths (/workspaces/:slug/r/:repo/...).
  // Navigating pushes the path and replays the app's popstate routing.
  function navigateTo(path) {
    if (!path) return;
    window.history.pushState({}, '', path);
    window.dispatchEvent(new PopStateEvent('popstate'));
  }

  async function loadRepos(wsId) {
    if (!wsId) { repos = []; return; }
    try {
      const list = await api.workspaceRepos(wsId);
      repos = Array.isArray(list) ? list : [];
    } catch { repos = []; }
  }

  async function load() {
    const gen = ++loadGen;
    loading = true;
    error = null;
    const params = { limit: 50 };
    if (filterType) params.type = filterType;
    if (filterPriority) params.priority = filterPriority;
    if (filterWorkspace) params.workspace_id = filterWorkspace;
    if (filterRepo) params.repo_id = filterRepo;
    try {
      const res = await api.notifications(params);
      if (gen !== loadGen) return; // stale response
      notifications = Array.isArray(res?.notifications) ? res.notifications : [];
    } catch (e) {
      if (gen !== loadGen) return;
      error = e?.message ?? String(e);
      notifications = [];
    } finally {
      if (gen === loadGen) loading = false;
    }
  }

  // Reload when the drawer opens, when any filter changes, and whenever a
  // new notification arrives on the live stream (§SSE integration).
  $effect(() => {
    // Tracking the filter values makes this effect re-run on any change.
    void filterType; void filterPriority; void filterWorkspace; void filterRepo;
    if (open) {
      void loadRepos(filterWorkspace || null);
      void load();
    }
  });
  $effect(() => {
    if (open && notificationState.lastEventAt) {
      void load();
    }
  });

  // Type filter options: the server taxonomy (as_str names, what the
  // `?type=` filter matches) merged with any type observed in the user's
  // own notifications, so a renamed/new type still appears once seen.
  const TYPE_OPTIONS = [
    'AbandonedBranch', 'AgentBudgetExhausted', 'AgentBudgetWarning', 'AgentCompleted',
    'AgentEscalation', 'AgentFailed', 'AgentNeedsClarification', 'AtomicGroupFailure',
    'BreakingChangeDetected', 'CascadeTestFailed', 'CascadeTestTriggered', 'ComputeTargetUnavailable',
    'ConflictingInterpretations', 'ConstraintViolation', 'CrossWorkspaceSpecChange', 'DependencyChainTooDeep',
    'DependencyStale', 'GateFailure', 'GatePassed', 'ImpersonationAlert', 'ImpersonationRequest',
    'InvitationReceived', 'MembershipChanged', 'MergeQueueEscalation', 'MergeQueuePaused',
    'MetaSpecDrift', 'MrMerged', 'MrReverted', 'PersonaApprovalRequested', 'PersonaChangePendingApproval',
    'SecretExpiring', 'SecurityFinding', 'SpecApprovalRequested', 'SpecAssertionFailure',
    'SpecConflict', 'SpecDriftDetected', 'SpecRejected', 'SuggestedSpecLink', 'SystemAlert',
    'TrustSuggestion', 'WorkspaceOwnerless',
  ];
  let availableTypes = $derived.by(() =>
    [...new Set([...TYPE_OPTIONS, ...notifications.map(n => n.notification_type).filter(Boolean)])].sort()
  );

  let grouped = $derived.by(() => {
    const byPriority = new Map(PRIORITIES.map(p => [p, []]));
    for (const n of notifications) {
      const band = PRIORITIES.includes(n.priority_level) ? n.priority_level : 'Low';
      byPriority.get(band).push(n);
    }
    return PRIORITIES
      .map(p => ({ priority: p, items: byPriority.get(p) }))
      .filter(g => g.items.length > 0);
  });

  let unreadCount = $derived(notifications.filter(n => !n.read).length);

  async function toggleRead(n) {
    if (itemBusy[n.id]) return;
    itemBusy = { ...itemBusy, [n.id]: true };
    try {
      if (n.read) {
        await api.markNotificationUnread(n.id);
        notifications = notifications.map(x => x.id === n.id ? { ...x, read: false, read_at: null } : x);
      } else {
        await api.markNotificationRead(n.id);
        notifications = notifications.map(x => x.id === n.id ? { ...x, read: true } : x);
      }
      await refreshUnreadCount(filterWorkspace || null);
    } catch {
      toastError($t('notifications.drawer.mark_failed'));
    }
    itemBusy = { ...itemBusy, [n.id]: false };
  }

  async function openNotification(n) {
    // Mark read, then deep-link to the entity (§Deep links).
    if (!n.read) void toggleRead(n);
    open = false;
    navigateTo(n.action_url || '/settings/notifications');
  }

  async function markAllRead() {
    if (markingAll) return;
    markingAll = true;
    try {
      await api.markAllNotificationsRead(filterWorkspace || undefined);
      notifications = notifications.map(n => ({ ...n, read: true }));
      await refreshUnreadCount(filterWorkspace || null);
    } catch {
      toastError($t('notifications.drawer.mark_all_failed'));
    }
    markingAll = false;
  }

  function typeLabel(typ) {
    const key = `decisions.type_labels.${typ}`;
    const val = $t(key);
    return val === key ? typ : val;
  }

  function bodySnippet(n) {
    if (!n.body) return '';
    try {
      const parsed = JSON.parse(n.body);
      if (parsed && typeof parsed === 'object') {
        return parsed.message ?? parsed.summary ?? parsed.spec_path ?? '';
      }
      return String(parsed);
    } catch {
      return n.body;
    }
  }

  function priorityIcon(p) {
    switch (p) {
      case 'Urgent': return '🔴';
      case 'High': return '🟠';
      case 'Medium': return '🔵';
      default: return '⚪';
    }
  }

  function close() { open = false; }

  $effect(() => {
    if (!open) return;
    const onKey = (e) => { if (e.key === 'Escape') close(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="notif-backdrop" role="presentation" onclick={close}></div>
  <div
    class="notif-drawer"
    role="dialog"
    aria-label={$t('notifications.drawer.title')}
    aria-live="polite"
    data-testid="notification-drawer"
  >
    <header class="notif-header">
      <h2>{$t('notifications.drawer.title')}</h2>
      {#if notificationState.streamStatus === 'open'}
        <span class="live-dot" title={$t('notifications.drawer.live')} aria-label={$t('notifications.drawer.live')}></span>
      {/if}
      <span class="spacer"></span>
      {#if unreadCount > 0}
        <button class="mark-all-btn" onclick={markAllRead} disabled={markingAll} data-testid="mark-all-read">
          {$t('notifications.drawer.mark_all_read')}
        </button>
      {/if}
      <button class="close-btn" onclick={close} aria-label={$t('notifications.drawer.close')} data-testid="notification-drawer-close">✕</button>
    </header>

    <!-- Filter bar (§Filter by type, priority, workspace, repo) -->
    <div class="notif-filters" data-testid="notification-filters">
      <label class="filter-label">
        <span>{$t('notifications.drawer.filter_type')}</span>
        <select bind:value={filterType} data-testid="filter-type">
          <option value="">{$t('notifications.drawer.all')}</option>
          {#each availableTypes as typ (typ)}
            <option value={typ}>{typeLabel(typ)}</option>
          {/each}
        </select>
      </label>
      <label class="filter-label">
        <span>{$t('notifications.drawer.filter_priority')}</span>
        <select bind:value={filterPriority} data-testid="filter-priority">
          <option value="">{$t('notifications.drawer.all')}</option>
          {#each PRIORITIES as p (p)}
            <option value={p}>{p}</option>
          {/each}
        </select>
      </label>
      <label class="filter-label">
        <span>{$t('notifications.drawer.filter_workspace')}</span>
        <select bind:value={filterWorkspace} data-testid="filter-workspace"
          onchange={() => { filterRepo = ''; void loadRepos(filterWorkspace || null); void load(); }}>
          <option value="">{$t('notifications.drawer.all')}</option>
          {#each workspaces as ws (ws.id)}
            <option value={ws.id}>{ws.name}</option>
          {/each}
        </select>
      </label>
      <label class="filter-label">
        <span>{$t('notifications.drawer.filter_repo')}</span>
        <select bind:value={filterRepo} data-testid="filter-repo" disabled={!filterWorkspace}>
          <option value="">{$t('notifications.drawer.all')}</option>
          {#each repos as r (r.id)}
            <option value={r.id}>{r.name}</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="notif-body" data-testid="notification-list">
      {#if loading}
        <Skeleton lines={4} />
      {:else if error}
        <EmptyState title={$t('notifications.drawer.load_failed')} description={error} />
      {:else if notifications.length === 0}
        <EmptyState title={$t('notifications.drawer.empty')} />
      {:else}
        {#each grouped as group (group.priority)}
          <div class="priority-group" data-priority={group.priority}>
            <div class="priority-heading">
              {priorityIcon(group.priority)} {group.priority}
              <span class="priority-count">{group.items.length}</span>
            </div>
            {#each group.items as n (n.id)}
              <div class="notif-item" class:unread={!n.read} data-testid="notification-item" data-id={n.id}>
                <button class="notif-main" onclick={() => openNotification(n)} data-testid={`notification-open-${n.id}`}>
                  <span class="notif-title">{n.title}</span>
                  <span class="notif-meta">
                    <span class="notif-type">{typeLabel(n.notification_type)}</span>
                    {#if bodySnippet(n)}
                      <span class="notif-snippet">{bodySnippet(n)}</span>
                    {/if}
                    <span class="notif-time">{relativeTime(n.created_at)}</span>
                  </span>
                </button>
                <button
                  class="read-toggle"
                  onclick={() => toggleRead(n)}
                  disabled={itemBusy[n.id]}
                  aria-label={n.read ? $t('notifications.drawer.mark_unread') : $t('notifications.drawer.mark_read')}
                  title={n.read ? $t('notifications.drawer.mark_unread') : $t('notifications.drawer.mark_read')}
                  data-testid={`notification-toggle-${n.id}`}
                >
                  {n.read ? '↺' : '✓'}
                </button>
              </div>
            {/each}
          </div>
        {/each}
      {/if}
    </div>

    <footer class="notif-footer">
      <button class="prefs-link" onclick={() => { close(); openPreferences?.(); }} data-testid="notification-preferences-link">
        ⚙ {$t('notifications.drawer.preferences')}
      </button>
    </footer>
  </div>
{/if}

<style>
  .notif-backdrop {
    position: fixed; inset: 0; background: rgba(0,0,0,0.3); z-index: 90;
  }
  .notif-drawer {
    position: fixed; top: 0; right: 0; bottom: 0; width: min(420px, 100vw);
    background: var(--color-bg, #fff); border-left: 1px solid var(--color-border, #e2e4e9);
    box-shadow: -4px 0 24px rgba(0,0,0,0.12); z-index: 91;
    display: flex; flex-direction: column;
    animation: slide-in 0.18s ease-out;
  }
  @keyframes slide-in { from { transform: translateX(100%); } to { transform: translateX(0); } }
  .notif-header {
    display: flex; align-items: center; gap: 8px; padding: 12px 16px;
    border-bottom: 1px solid var(--color-border, #e2e4e9);
  }
  .notif-header h2 { margin: 0; font-size: 16px; }
  .spacer { flex: 1; }
  .live-dot {
    width: 8px; height: 8px; border-radius: 50%; background: #22c55e;
    display: inline-block;
  }
  .mark-all-btn {
    font-size: 12px; padding: 4px 10px; border-radius: 6px; cursor: pointer;
    border: 1px solid var(--color-border, #e2e4e9); background: transparent;
  }
  .mark-all-btn:hover { background: var(--color-bg-hover, #f4f5f7); }
  .close-btn {
    border: none; background: transparent; cursor: pointer; font-size: 14px;
    padding: 4px 8px; border-radius: 6px;
  }
  .close-btn:hover { background: var(--color-bg-hover, #f4f5f7); }
  .notif-filters {
    display: grid; grid-template-columns: 1fr 1fr; gap: 8px; padding: 10px 16px;
    border-bottom: 1px solid var(--color-border, #e2e4e9);
  }
  .filter-label { display: flex; flex-direction: column; gap: 2px; font-size: 11px; color: var(--color-text-muted, #6b7280); }
  .filter-label select {
    font-size: 13px; padding: 4px 6px; border-radius: 6px;
    border: 1px solid var(--color-border, #e2e4e9); background: var(--color-bg, #fff);
  }
  .notif-body { flex: 1; overflow-y: auto; padding: 8px 0; }
  .priority-group { margin-bottom: 8px; }
  .priority-heading {
    display: flex; align-items: center; gap: 6px; padding: 6px 16px 2px;
    font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em;
    color: var(--color-text-muted, #6b7280);
  }
  .priority-count {
    background: var(--color-bg-hover, #f4f5f7); border-radius: 10px;
    padding: 0 6px; font-size: 10px;
  }
  .notif-item {
    display: flex; align-items: stretch; border-left: 3px solid transparent;
  }
  .notif-item.unread { background: rgba(79,70,229,0.05); border-left-color: #4f46e5; }
  .notif-main {
    flex: 1; display: flex; flex-direction: column; gap: 2px; text-align: left;
    padding: 8px 8px 8px 16px; background: none; border: none; cursor: pointer;
  }
  .notif-main:hover { background: var(--color-bg-hover, #f4f5f7); }
  .notif-title { font-size: 13px; font-weight: 500; color: var(--color-text, #111827); }
  .notif-item.unread .notif-title { font-weight: 600; }
  .notif-meta { display: flex; flex-direction: column; gap: 1px; }
  .notif-type { font-size: 11px; color: var(--color-text-muted, #6b7280); }
  .notif-snippet {
    font-size: 12px; color: var(--color-text-muted, #6b7280);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 300px;
  }
  .notif-time { font-size: 11px; color: var(--color-text-muted, #9ca3af); }
  .read-toggle {
    width: 36px; border: none; background: transparent; cursor: pointer;
    color: var(--color-text-muted, #6b7280); font-size: 14px;
  }
  .read-toggle:hover { background: var(--color-bg-hover, #f4f5f7); }
  .notif-footer {
    padding: 10px 16px; border-top: 1px solid var(--color-border, #e2e4e9);
  }
  .prefs-link {
    border: none; background: transparent; cursor: pointer; font-size: 13px;
    color: #4f46e5; padding: 4px 0;
  }
  .prefs-link:hover { text-decoration: underline; }
</style>
