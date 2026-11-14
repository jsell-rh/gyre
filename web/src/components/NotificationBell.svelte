<script>
  import { t } from 'svelte-i18n';
  import { onMount } from 'svelte';
  import {
    notificationState,
    refreshUnreadCount,
    startNotificationStream,
  } from '../lib/notifications.svelte.js';
  import NotificationDrawer from './NotificationDrawer.svelte';

  // NotificationBell (user-management.md §In-App Notification UI): bell in
  // the top nav with an unread-count badge. The count comes from
  // GET /api/v1/notifications/unread-count and is kept live by the SSE
  // notification stream (§Email/§API Notifications: real-time push).
  // Clicking opens the notification drawer.

  let { workspaces = [], currentWorkspaceId = null } = $props();

  let drawerOpen = $state(false);

  onMount(() => {
    void refreshUnreadCount(null);
    const stop = startNotificationStream();
    return () => stop();
  });

  // Refresh the badge when the drawer closes (reads/unread changes inside).
  $effect(() => {
    if (!drawerOpen) return;
    return () => { void refreshUnreadCount(null); };
  });

  const count = $derived(notificationState.unread);
</script>

<div class="notif-bell-wrap">
  <button
    class="notif-bell-btn"
    onclick={() => (drawerOpen = !drawerOpen)}
    aria-label={count > 0
      ? $t('notifications.bell.unread', { values: { count } })
      : $t('notifications.bell.none')}
    aria-haspopup="dialog"
    aria-expanded={drawerOpen}
    data-testid="notification-bell"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="16" height="16" aria-hidden="true">
      <path d="M18 8A6 6 0 006 8c0 7-3 9-3 9h18s-3-2-3-9"/>
      <path d="M13.73 21a2 2 0 01-3.46 0"/>
    </svg>
    {#if count > 0}
      <span class="notif-badge" aria-hidden="true" data-testid="notification-badge">
        {count > 99 ? '99+' : count}
      </span>
    {/if}
  </button>
  <span class="sr-only" aria-live="polite" aria-atomic="true">
    {count > 0 ? $t('notifications.bell.unread', { values: { count } }) : ''}
  </span>

  <NotificationDrawer bind:open={drawerOpen} {workspaces} {currentWorkspaceId} />
</div>

<style>
  .notif-bell-wrap { position: relative; display: inline-flex; }
  .notif-bell-btn {
    position: relative; display: inline-flex; align-items: center; justify-content: center;
    width: 30px; height: 30px; border-radius: 8px; border: none; cursor: pointer;
    background: transparent; color: var(--color-text, #374151);
  }
  .notif-bell-btn:hover { background: var(--color-bg-hover, #f4f5f7); }
  .notif-badge {
    position: absolute; top: 1px; right: 0;
    min-width: 15px; height: 15px; padding: 0 3px;
    border-radius: 8px; background: #dc2626; color: #fff;
    font-size: 10px; font-weight: 700; line-height: 15px; text-align: center;
  }
  .sr-only {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0;
  }
</style>
