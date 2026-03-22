<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let me = $state(null);
  let agents = $state([]);
  let tasks = $state([]);
  let mrs = $state([]);
  let notifications = $state([]);
  let loading = $state(true);
  let editing = $state(false);
  let saving = $state(false);
  let editForm = $state({ display_name: '', timezone: '', locale: '' });
  let activeTab = $state('info');
  let unread = $state(0);

  const tabs = [
    { id: 'info', label: 'Profile' },
    { id: 'agents', label: 'My Agents' },
    { id: 'tasks', label: 'My Tasks' },
    { id: 'mrs', label: 'My MRs' },
    { id: 'notifications', label: 'Notifications' },
  ];

  $effect(() => { loadAll(); });

  async function loadAll() {
    loading = true;
    try {
      const [meR, agR, tkR, mrR, ntR] = await Promise.allSettled([
        api.me(),
        api.myAgents(),
        api.myTasks(),
        api.myMrs(),
        api.myNotifications(),
      ]);
      if (meR.status === 'fulfilled') {
        me = meR.value;
        editForm = {
          display_name: me.display_name ?? '',
          timezone: me.timezone ?? '',
          locale: me.locale ?? '',
        };
      }
      if (agR.status === 'fulfilled') agents = agR.value ?? [];
      if (tkR.status === 'fulfilled') tasks = tkR.value ?? [];
      if (mrR.status === 'fulfilled') mrs = mrR.value ?? [];
      if (ntR.status === 'fulfilled') {
        notifications = ntR.value ?? [];
        unread = notifications.filter(n => !n.read).length;
      }
    } catch (e) {
      showToast('Failed to load profile: ' + e.message, { type: 'error' });
    } finally {
      loading = false;
    }
  }

  async function saveEdit() {
    saving = true;
    try {
      me = await api.updateMe(editForm);
      editing = false;
      showToast('Profile updated', { type: 'success' });
    } catch (e) {
      showToast('Failed to update profile: ' + e.message, { type: 'error' });
    } finally {
      saving = false;
    }
  }

  async function markRead(id) {
    try {
      await api.markNotificationRead(id);
      notifications = notifications.map(n => n.id === id ? { ...n, read: true } : n);
      unread = notifications.filter(n => !n.read).length;
    } catch { /* ignore */ }
  }

  function statusColor(s) {
    const v = (s ?? '').toLowerCase();
    if (v === 'active') return 'success';
    if (v === 'dead' || v === 'failed') return 'danger';
    if (v === 'idle') return 'info';
    return 'default';
  }

  function priorityColor(p) {
    const v = (p ?? '').toLowerCase();
    if (v === 'high' || v === 'critical') return 'danger';
    if (v === 'medium') return 'warning';
    return 'default';
  }

  function notifColor(type) {
    const t = (type ?? '').toLowerCase();
    if (t.includes('failure') || t.includes('conflict')) return 'danger';
    if (t.includes('merged') || t.includes('complete')) return 'success';
    if (t.includes('review')) return 'info';
    return 'default';
  }

  function rel(ts) {
    if (!ts) return '—';
    const d = new Date(ts);
    const secs = Math.floor((Date.now() - d.getTime()) / 1000);
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs/60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs/3600)}h ago`;
    return `${Math.floor(secs/86400)}d ago`;
  }
</script>

<div class="user-profile">
  <div class="profile-header">
    <div class="avatar" aria-hidden="true">
      {me?.display_name?.[0]?.toUpperCase() ?? me?.username?.[0]?.toUpperCase() ?? '?'}
    </div>
    <div class="profile-meta">
      <h2>{me?.display_name ?? me?.username ?? 'My Profile'}</h2>
      {#if me?.username}
        <p class="username">@{me.username}</p>
      {/if}
      {#if me?.global_role}
        <Badge variant="info" label={me.global_role} />
      {/if}
    </div>
    {#if !editing}
      <button class="btn-edit" onclick={() => (editing = true)}>Edit</button>
    {/if}
    {#if unread > 0}
      <div class="notif-bell" role="status" aria-label="{unread} unread notifications">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18">
          <path d="M18 8A6 6 0 006 8c0 7-3 9-3 9h18s-3-2-3-9"/>
          <path d="M13.73 21a2 2 0 01-3.46 0"/>
        </svg>
        <span class="notif-count">{unread}</span>
      </div>
    {/if}
  </div>

  {#if editing}
    <div class="edit-form">
      <label class="field-label">Display Name
        <input class="field-input" bind:value={editForm.display_name} placeholder="Your display name" />
      </label>
      <label class="field-label">Timezone
        <input class="field-input" bind:value={editForm.timezone} placeholder="e.g. America/New_York" />
      </label>
      <label class="field-label">Locale
        <input class="field-input" bind:value={editForm.locale} placeholder="e.g. en-US" />
      </label>
      <div class="edit-actions">
        <button class="btn-secondary" onclick={() => (editing = false)}>Cancel</button>
        <button class="btn-primary" onclick={saveEdit} disabled={saving}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  {/if}

  <Tabs {tabs} bind:activeTab />

  <div class="tab-body">
    {#if loading}
      <Skeleton lines={5} />
    {:else if activeTab === 'info'}
      {#if me}
        <div class="info-grid">
          {#each [['Username', me.username],['Email', me.email],['Display Name', me.display_name],['Timezone', me.timezone],['Locale', me.locale],['Role', me.global_role]] as [label, val]}
            {#if val}
              <div class="info-row">
                <span class="info-label">{label}</span>
                <span class="info-val">{val}</span>
              </div>
            {/if}
          {/each}
        </div>
      {:else}
        <EmptyState message="Profile data unavailable." />
      {/if}

    {:else if activeTab === 'agents'}
      {#if agents.length === 0}
        <EmptyState message="No agents spawned by you." />
      {:else}
        <table class="data-table">
          <thead><tr><th>Name</th><th>Status</th><th>Created</th></tr></thead>
          <tbody>
            {#each agents as a}
              <tr>
                <td>{a.name}</td>
                <td><Badge variant={statusColor(a.status)} label={a.status} /></td>
                <td class="muted">{rel(a.created_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if activeTab === 'tasks'}
      {#if tasks.length === 0}
        <EmptyState message="No tasks assigned to you." />
      {:else}
        <table class="data-table">
          <thead><tr><th>Title</th><th>Status</th><th>Priority</th></tr></thead>
          <tbody>
            {#each tasks as t}
              <tr>
                <td>{t.title}</td>
                <td><Badge variant={statusColor(t.status)} label={t.status} /></td>
                <td><Badge variant={priorityColor(t.priority)} label={t.priority ?? 'medium'} /></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if activeTab === 'mrs'}
      {#if mrs.length === 0}
        <EmptyState message="No merge requests authored by you." />
      {:else}
        <table class="data-table">
          <thead><tr><th>Title</th><th>Status</th><th>Created</th></tr></thead>
          <tbody>
            {#each mrs as mr}
              <tr>
                <td>{mr.title}</td>
                <td><Badge variant={statusColor(mr.status)} label={mr.status} /></td>
                <td class="muted">{rel(mr.created_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if activeTab === 'notifications'}
      {#if notifications.length === 0}
        <EmptyState message="No notifications." />
      {:else}
        <div class="notif-list">
          {#each notifications as notif}
            <div class="notif-item" class:unread={!notif.read}>
              <div class="notif-top">
                <Badge variant={notifColor(notif.notification_type)} label={notif.notification_type ?? 'info'} />
                <span class="notif-time muted">{rel(notif.created_at)}</span>
                {#if !notif.read}
                  <button class="mark-read-btn" onclick={() => markRead(notif.id)} aria-label="Mark as read">✓</button>
                {/if}
              </div>
              <p class="notif-msg">{notif.message ?? notif.title ?? ''}</p>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .user-profile { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .profile-header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .avatar {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: rgba(238, 0, 0, 0.15);
    color: var(--color-primary);
    font-weight: 700;
    font-size: var(--text-xl);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .profile-meta { flex: 1; }
  .profile-meta h2 { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .username { margin: 0 0 var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); font-family: var(--font-mono); }

  .btn-edit {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .btn-edit:hover { border-color: var(--color-text-muted); }

  .notif-bell { position: relative; display: flex; align-items: center; color: var(--color-text-muted); }
  .notif-count {
    position: absolute;
    top: -4px;
    right: -6px;
    background: var(--color-danger);
    color: #fff;
    border-radius: 999px;
    font-size: 9px;
    padding: 0 4px;
    min-width: 14px;
    text-align: center;
  }

  .edit-form {
    padding: var(--space-4) var(--space-6);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    flex-shrink: 0;
    align-items: flex-end;
  }

  .field-label { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); font-weight: 500; color: var(--color-text); flex: 1; min-width: 180px; }
  .field-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }
  .field-input:focus { outline: none; border-color: var(--color-primary); }

  .edit-actions { display: flex; gap: var(--space-2); align-items: center; }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .tab-body { flex: 1; overflow-y: auto; padding: var(--space-6); }

  .info-grid { display: flex; flex-direction: column; gap: var(--space-3); max-width: 480px; }
  .info-row { display: flex; gap: var(--space-4); font-size: var(--text-sm); }
  .info-label { color: var(--color-text-muted); width: 120px; flex-shrink: 0; }
  .info-val { color: var(--color-text); }

  .data-table { width: 100%; border-collapse: collapse; font-size: var(--text-sm); }
  .data-table th {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font-weight: 500;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .data-table td { padding: var(--space-3) var(--space-4); border-bottom: 1px solid var(--color-border); color: var(--color-text); }

  .muted { color: var(--color-text-muted); font-size: var(--text-xs); }

  .notif-list { display: flex; flex-direction: column; gap: var(--space-3); }
  .notif-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .notif-item.unread { border-left: 3px solid var(--color-primary); }
  .notif-top { display: flex; align-items: center; gap: var(--space-2); }
  .notif-time { margin-left: auto; font-size: var(--text-xs); }
  .notif-msg { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); }
  .mark-read-btn {
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 1px 6px;
  }
  .mark-read-btn:hover { color: var(--color-success); border-color: var(--color-success); }
</style>
