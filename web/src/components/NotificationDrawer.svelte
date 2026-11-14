<script>
  /**
   * NotificationDrawer.svelte — slide-out notification panel from the
   * top-nav bell (user-management.md §UI Pages > Notification Drawer and
   * §In-App Notification UI).
   *
   * - Recent notifications grouped by priority band (Urgent / High /
   *   Medium / Low — the four-level bands of §Notification Entity).
   * - Deep links: clicking a notification navigates via `onnavigate`
   *     with the notification's action_url (server-resolved SPA path) or,
   *     for InvitationReceived, the invitation accept page.
   * - Mark read / unread individually (task-125 endpoints) and mark all
   *   read.
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { open = $bindable(false), onnavigate = undefined } = $props();

  const PRIORITY_ORDER = ['Urgent', 'High', 'Medium', 'Low'];
  const PRIORITY_VARIANTS = {
    Urgent: 'danger',
    High: 'warning',
    Medium: 'info',
    Low: 'muted',
  };

  let notifications = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let busyId = $state(null);
  let markingAll = $state(false);

  let grouped = $derived.by(() => {
    const groups = { Urgent: [], High: [], Medium: [], Low: [] };
    for (const n of notifications) {
      const band = PRIORITY_ORDER.includes(n.priority_level) ? n.priority_level : 'Low';
      groups[band].push(n);
    }
    return PRIORITY_ORDER
      .filter(band => groups[band].length > 0)
      .map(band => ({ band, items: groups[band] }));
  });

  let unreadCount = $derived(notifications.filter(n => !n.read).length);

  function fmtTime(ts) {
    if (!ts) return '';
    try {
      const d = new Date(ts * 1000);
      const diff = (Date.now() - d.getTime()) / 1000;
      if (diff < 60) return $t('notification_drawer.just_now');
      if (diff < 3600) return $t('notification_drawer.minutes_ago', { values: { n: Math.floor(diff / 60) } });
      if (diff < 86400) return $t('notification_drawer.hours_ago', { values: { n: Math.floor(diff / 3600) } });
      return d.toLocaleDateString();
    } catch { return ''; }
  }

  /** Resolve the deep-link target for a notification. */
  function targetFor(n) {
    if (n.action_url) return n.action_url;
    // Workspace invitations deep-link to the accept page — the invitation
    // identifier the invitee holds is the workspace (task-124 models a
    // pending invitation as a membership row keyed by workspace, not a token).
    if (n.notification_type === 'InvitationReceived' && n.workspace_id) {
      return `/invite/${n.workspace_id}`;
    }
    return null;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const raw = await api.notifications({ limit: 50 });
      notifications = Array.isArray(raw?.notifications) ? raw.notifications : [];
    } catch (e) {
      error = e?.message ?? String(e);
      notifications = [];
    } finally {
      loading = false;
    }
  }

  // Reload whenever the drawer opens so the list and read state are fresh.
  $effect(() => {
    if (open) load();
  });

  async function toggleRead(n) {
    if (busyId) return;
    busyId = n.id;
    try {
      if (n.read) {
        await api.markNotificationUnread(n.id);
        notifications = notifications.map(item => item.id === n.id ? { ...item, read: false } : item);
      } else {
        await api.markNotificationRead(n.id);
        notifications = notifications.map(item => item.id === n.id ? { ...item, read: true } : item);
      }
    } catch (e) {
      showToast($t('notification_drawer.mark_read_failed') + ': ' + (e?.message ?? e), { type: 'error' });
    } finally {
      busyId = null;
    }
  }

  async function markAllRead() {
    if (markingAll) return;
    markingAll = true;
    try {
      await api.markAllNotificationsRead();
      notifications = notifications.map(n => ({ ...n, read: true }));
    } catch (e) {
      showToast($t('notification_drawer.mark_read_failed') + ': ' + (e?.message ?? e), { type: 'error' });
    } finally {
      markingAll = false;
    }
  }

  function handleClick(n) {
    const target = targetFor(n);
    if (target) {
      open = false;
      onnavigate?.(target, n);
    }
    // Notifications without a deep link are informational only — no
    // fabricated navigation target.
  }

  function onKeydown(e) {
    if (e.key === 'Escape' && open) {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="drawer-backdrop"
    role="presentation"
    onclick={() => (open = false)}
  ></div>
  <div
    class="notification-drawer"
    role="dialog"
    aria-label={$t('notification_drawer.title')}
    data-testid="notification-drawer"
  >
    <header class="drawer-head">
      <h2>{$t('notification_drawer.title')}</h2>
      {#if unreadCount > 0}
        <span class="unread-pill" data-testid="drawer-unread-count">
          {$t('notification_drawer.unread_count', { values: { n: unreadCount } })}
        </span>
      {/if}
      <div class="drawer-actions">
        {#if unreadCount > 0}
          <button
            class="link-btn"
            onclick={markAllRead}
            disabled={markingAll}
            data-testid="drawer-mark-all-read"
          >{markingAll ? '…' : $t('notification_drawer.mark_all_read')}</button>
        {/if}
        <button
          class="close-btn"
          onclick={() => (open = false)}
          aria-label={$t('common.close')}
        >✕</button>
      </div>
    </header>

    <div class="drawer-body">
      {#if loading}
        <p class="drawer-note" role="status">{$t('common.loading')}</p>
      {:else if error}
        <p class="drawer-error" role="alert">{error}</p>
      {:else if grouped.length === 0}
        <p class="drawer-note">{$t('notification_drawer.empty')}</p>
      {:else}
        {#each grouped as group (group.band)}
          <section class="priority-group" data-testid={`drawer-group-${group.band.toLowerCase()}`}>
            <h3 class="group-label">
              <Badge variant={PRIORITY_VARIANTS[group.band]} value={group.band} />
              <span class="group-count">{group.items.length}</span>
            </h3>
            <ul class="notif-list" role="list">
              {#each group.items as n (n.id)}
                {@const target = targetFor(n)}
                <li class="notif-item" class:unread={!n.read} data-testid="drawer-notif-item">
                  <button
                    class="notif-main"
                    class:link={!!target}
                    onclick={() => handleClick(n)}
                    disabled={!target}
                    title={target ?? $t('notification_drawer.no_link')}
                  >
                    <span class="notif-title">{n.title}</span>
                    {#if n.body}
                      <span class="notif-body">{n.body}</span>
                    {/if}
                    <span class="notif-time">{fmtTime(n.created_at)}</span>
                  </button>
                  <button
                    class="read-toggle"
                    onclick={() => toggleRead(n)}
                    disabled={busyId === n.id}
                    aria-label={n.read ? $t('notification_drawer.mark_unread') : $t('notification_drawer.mark_read')}
                    data-testid={`drawer-toggle-read-${n.id}`}
                  >{busyId === n.id ? '…' : (n.read ? '↺' : '✓')}</button>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      {/if}
    </div>
  </div>
{/if}

<style>
  .drawer-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.25);
    z-index: 90;
  }
  .notification-drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(400px, 90vw);
    background: var(--color-surface, #fff);
    border-left: 1px solid var(--color-border, #ddd);
    box-shadow: -4px 0 16px rgba(0, 0, 0, 0.12);
    z-index: 100;
    display: flex;
    flex-direction: column;
  }
  .drawer-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--color-border, #ddd);
  }
  .drawer-head h2 { font-size: 15px; margin: 0; }
  .unread-pill {
    background: var(--color-primary, #4a6cf7);
    color: #fff;
    border-radius: 10px;
    padding: 1px 8px;
    font-size: 11px;
    font-weight: 600;
  }
  .drawer-actions { margin-left: auto; display: flex; align-items: center; gap: 8px; }
  .link-btn {
    background: none;
    border: none;
    color: var(--color-primary, #4a6cf7);
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }
  .link-btn:disabled { opacity: 0.6; cursor: default; }
  .close-btn {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 14px;
    padding: 2px 6px;
  }
  .drawer-body { overflow-y: auto; flex: 1; padding: 12px 16px; }
  .drawer-note, .drawer-error { font-size: 13px; color: var(--color-text-secondary, #666); }
  .drawer-error { color: var(--color-danger, #c33); }
  .priority-group { margin-bottom: 16px; }
  .group-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    margin: 0 0 6px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .group-count { color: var(--color-text-secondary, #666); }
  .notif-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .notif-item {
    display: flex;
    align-items: stretch;
    gap: 4px;
    border: 1px solid var(--color-border, #ddd);
    border-radius: 8px;
    padding: 6px 8px;
  }
  .notif-item.unread {
    border-left: 3px solid var(--color-primary, #4a6cf7);
    background: var(--color-surface-elevated, #f7f8fa);
  }
  .notif-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: none;
    border: none;
    text-align: left;
    padding: 0;
    font: inherit;
    color: inherit;
    min-width: 0;
  }
  .notif-main.link { cursor: pointer; }
  .notif-main.link .notif-title { color: var(--color-primary, #4a6cf7); }
  .notif-main:disabled { cursor: default; }
  .notif-title { font-size: 13px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .notif-body {
    font-size: 12px;
    color: var(--color-text-secondary, #666);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .notif-time { font-size: 11px; color: var(--color-text-secondary, #999); }
  .read-toggle {
    background: none;
    border: 1px solid var(--color-border, #ddd);
    border-radius: 6px;
    width: 26px;
    cursor: pointer;
    font-size: 12px;
    flex-shrink: 0;
  }
  .read-toggle:disabled { opacity: 0.5; cursor: default; }
</style>
