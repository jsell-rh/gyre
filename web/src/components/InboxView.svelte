<script>
  import Card from '../lib/Card.svelte';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { api } from '../lib/api.js';
  import { toastError } from '../lib/toast.svelte.js';
  import { getContext } from 'svelte';

  const navigate = getContext('navigate');

  let notifications = $state([]);
  let myTasks = $state([]);
  let myMrs = $state([]);
  let loading = $state(true);

  async function fetchAll() {
    loading = true;
    const [notifR, tasksR, mrsR] = await Promise.allSettled([
      api.myNotifications(),
      api.myTasks(),
      api.myMrs(),
    ]);
    if (notifR.status === 'fulfilled') {
      const r = notifR.value;
      notifications = Array.isArray(r?.notifications) ? r.notifications : Array.isArray(r) ? r : [];
    }
    if (tasksR.status === 'fulfilled') {
      const r = tasksR.value;
      myTasks = Array.isArray(r?.tasks) ? r.tasks : Array.isArray(r) ? r : [];
    }
    if (mrsR.status === 'fulfilled') {
      const r = mrsR.value;
      myMrs = Array.isArray(r?.merge_requests) ? r.merge_requests : Array.isArray(r) ? r : [];
    }
    loading = false;
  }

  $effect(() => { fetchAll(); });

  async function markRead(id) {
    try {
      await api.markNotificationRead(id);
      notifications = notifications.map(n => n.id === id ? { ...n, read: true } : n);
    } catch (e) {
      toastError(e.message);
    }
  }

  const PRIORITY_VARIANT = {
    Critical: 'danger',
    High:     'warning',
    Medium:   'info',
    Low:      'default',
    critical: 'danger',
    high:     'warning',
    medium:   'info',
    low:      'default',
  };

  const NOTIF_TYPE_LABEL = {
    MrNeedsReview:  'Review needed',
    GateFailure:    'Gate failure',
    MrMerged:       'MR merged',
  };

  function relativeTime(ts) {
    if (!ts) return '';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60)   return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  let unreadCount = $derived(notifications.filter(n => !n.read).length);
</script>

<div class="inbox">
  <div class="inbox-header">
    <h1 class="inbox-title">Inbox</h1>
    {#if unreadCount > 0}
      <Badge value={String(unreadCount)} variant="danger" />
    {/if}
  </div>

  {#if loading}
    <div class="sections">
      {#each [0,1,2] as _}
        <div class="section-skeleton">
          <Skeleton width="120px" height="14px" />
          {#each [0,1] as __}
            <Skeleton width="100%" height="56px" />
          {/each}
        </div>
      {/each}
    </div>
  {:else}
    <div class="sections">

      <!-- Notifications -->
      <section class="inbox-section" aria-labelledby="section-notifications">
        <h2 class="section-title" id="section-notifications">
          Notifications
          {#if notifications.length > 0}
            <span class="section-count">{notifications.length}</span>
          {/if}
        </h2>
        {#if notifications.length === 0}
          <EmptyState title="No notifications" description="You're all caught up." />
        {:else}
          <ul class="item-list" role="list">
            {#each notifications as notif}
              <li class="inbox-item" class:unread={!notif.read}>
                <div class="item-meta">
                  <Badge
                    value={NOTIF_TYPE_LABEL[notif.notification_type] ?? notif.notification_type ?? 'Notification'}
                    variant={notif.priority === 'High' || notif.priority === 'Critical' ? 'warning' : 'default'}
                  />
                  <span class="item-time">{relativeTime(notif.created_at)}</span>
                </div>
                <p class="item-body">{notif.title ?? notif.message ?? ''}</p>
                {#if !notif.read}
                  <button class="mark-read-btn" onclick={() => markRead(notif.id)} aria-label="Mark as read">
                    Mark read
                  </button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <!-- My Tasks -->
      <section class="inbox-section" aria-labelledby="section-tasks">
        <h2 class="section-title" id="section-tasks">
          My Tasks
          {#if myTasks.length > 0}
            <span class="section-count">{myTasks.length}</span>
          {/if}
        </h2>
        {#if myTasks.length === 0}
          <EmptyState title="No tasks assigned" description="No tasks are currently assigned to you." />
        {:else}
          <ul class="item-list" role="list">
            {#each myTasks as task}
              <li class="inbox-item clickable">
                <button
                  class="item-link"
                  onclick={() => navigate?.('task-detail', { task })}
                  aria-label="Open task: {task.title}"
                >
                  <div class="item-meta">
                    <Badge
                      value={task.priority ?? 'Medium'}
                      variant={PRIORITY_VARIANT[task.priority] ?? 'default'}
                    />
                    <Badge value={task.status ?? ''} variant="default" />
                    <span class="item-time">{relativeTime(task.updated_at ?? task.created_at)}</span>
                  </div>
                  <p class="item-body">{task.title}</p>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <!-- MRs needing review -->
      <section class="inbox-section" aria-labelledby="section-mrs">
        <h2 class="section-title" id="section-mrs">
          Merge Requests
          {#if myMrs.length > 0}
            <span class="section-count">{myMrs.length}</span>
          {/if}
        </h2>
        {#if myMrs.length === 0}
          <EmptyState title="No merge requests" description="No MRs authored by you." />
        {:else}
          <ul class="item-list" role="list">
            {#each myMrs as mr}
              <li class="inbox-item clickable">
                <button
                  class="item-link"
                  onclick={() => navigate?.('mr-detail', { mr })}
                  aria-label="Open merge request: {mr.title}"
                >
                  <div class="item-meta">
                    <Badge
                      value={mr.status ?? 'Open'}
                      variant={mr.status === 'Merged' || mr.status === 'merged' ? 'success' : mr.status === 'Closed' || mr.status === 'closed' ? 'danger' : 'info'}
                    />
                    <span class="item-time">{relativeTime(mr.updated_at ?? mr.created_at)}</span>
                  </div>
                  <p class="item-body">{mr.title}</p>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

    </div>
  {/if}
</div>

<style>
  .inbox {
    padding: var(--space-6);
    max-width: 720px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .inbox-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .inbox-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .sections {
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
  }

  .section-skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .inbox-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .section-count {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: 999px;
    padding: 0 6px;
    line-height: 1.6;
  }

  .item-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .inbox-item {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--transition-fast);
  }

  .inbox-item.unread {
    border-left: 3px solid var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 4%, var(--color-surface));
  }

  .inbox-item.clickable {
    padding: 0;
  }

  .item-link {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    text-align: left;
    cursor: pointer;
    width: 100%;
    color: inherit;
    font-family: inherit;
  }

  .inbox-item.clickable:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 4%, var(--color-surface));
  }

  .item-meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .item-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .item-body {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: 0;
    line-height: 1.4;
  }

  .mark-read-btn {
    align-self: flex-start;
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .mark-read-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }
</style>
