<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { getContext } from 'svelte';

  const navigate = getContext('navigate');

  // ── State ──────────────────────────────────────────────────────────────────
  let me = $state(null);
  let notifications = $state([]);
  let judgments = $state([]);
  let workspaces = $state([]);
  let loading = $state(true);
  let editing = $state(false);
  let saving = $state(false);
  let editForm = $state({ display_name: '', timezone: '', locale: '' });
  let activeTab = $state('info');
  let unread = $state(0);

  // Notification preferences — per-type toggles.
  // Per HSI §12, stored in user_notification_preferences table (backend).
  // Using localStorage as fallback until backend endpoint is wired.
  const NOTIF_TYPES = [
    { id: 'SpecApproval',      label: 'Spec Approvals' },
    { id: 'GateOverride',      label: 'Gate Overrides' },
    { id: 'TrustChange',       label: 'Trust Level Changes' },
    { id: 'MetaSpecEdit',      label: 'Meta-Spec Edits' },
    { id: 'MergeRequestReview',label: 'Merge Request Reviews' },
    { id: 'MergeRequestMerged',label: 'Merge Request Merged' },
    { id: 'AgentFailure',      label: 'Agent Failures' },
    { id: 'TrustSuggestion',   label: 'Trust Suggestions' },
    { id: 'SpecDrift',         label: 'Spec Drift Alerts' },
    { id: 'AgentNeedsClarification', label: 'Agent Clarification Requests' },
  ];

  function loadPrefs() {
    try {
      const raw = localStorage.getItem('gyre_notif_prefs');
      if (raw) return JSON.parse(raw);
    } catch { /* ignore */ }
    return Object.fromEntries(NOTIF_TYPES.map(t => [t.id, true]));
  }

  let notifPrefs = $state(loadPrefs());
  let prefsSaving = $state(false);

  async function savePrefs() {
    prefsSaving = true;
    try {
      localStorage.setItem('gyre_notif_prefs', JSON.stringify(notifPrefs));
      showToast('Notification preferences saved', { type: 'success' });
    } catch { /* ignore */ }
    prefsSaving = false;
  }

  const tabs = [
    { id: 'info',        label: 'Profile' },
    { id: 'memberships', label: 'Workspaces' },
    { id: 'ledger',      label: 'Judgment Ledger' },
    { id: 'notif-prefs', label: 'Notification Preferences' },
    { id: 'notifications', label: 'Notifications' },
  ];

  $effect(() => { loadAll(); });

  async function loadAll() {
    loading = true;
    try {
      const [meR, ntR, jdR, wsR] = await Promise.allSettled([
        api.me(),
        api.myNotifications(),
        api.myJudgments(),
        api.workspaces(),
      ]);
      if (meR.status === 'fulfilled') {
        me = meR.value;
        editForm = {
          display_name: me.display_name ?? '',
          timezone: me.timezone ?? '',
          locale: me.locale ?? '',
        };
      }
      if (ntR.status === 'fulfilled') {
        const raw = ntR.value;
        notifications = Array.isArray(raw?.notifications) ? raw.notifications : Array.isArray(raw) ? raw : [];
        unread = notifications.filter(n => !n.read).length;
      }
      if (jdR.status === 'fulfilled') {
        const raw = jdR.value;
        judgments = Array.isArray(raw?.judgments) ? raw.judgments : Array.isArray(raw?.items) ? raw.items : Array.isArray(raw) ? raw : [];
      }
      if (wsR.status === 'fulfilled') {
        const raw = wsR.value;
        workspaces = Array.isArray(raw?.workspaces) ? raw.workspaces : Array.isArray(raw) ? raw : [];
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
    } catch {
      showToast('Failed to mark notification as read', { type: 'error' });
    }
  }

  function judgmentEventColor(ev) {
    if (!ev) return 'default';
    const e = ev.toLowerCase();
    if (e.includes('approv')) return 'success';
    if (e.includes('revok') || e.includes('reject') || e.includes('invalidat')) return 'danger';
    if (e.includes('override') || e.includes('trust')) return 'warning';
    if (e.includes('meta') || e.includes('edit') || e.includes('publish')) return 'info';
    return 'neutral';
  }

  function judgmentLabel(j) {
    return j.event_type ?? j.event ?? j.action ?? j.type ?? 'event';
  }

  function judgmentTarget(j) {
    return j.spec_path ?? j.path ?? j.resource_id ?? j.mr_id ?? j.resource ?? '—';
  }

  function judgmentWorkspace(j) {
    return j.workspace_name ?? j.workspace_slug ?? j.workspace_id ?? null;
  }

  function notifColor(type) {
    const t = (type ?? '').toLowerCase();
    if (t.includes('failure') || t.includes('conflict') || t.includes('drift')) return 'danger';
    if (t.includes('merged') || t.includes('complete') || t.includes('approval')) return 'success';
    if (t.includes('review') || t.includes('clarification')) return 'info';
    return 'default';
  }

  function rel(ts) {
    if (!ts) return '—';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const secs = Math.floor((Date.now() - d.getTime()) / 1000);
    if (secs < 60) return `${secs}s ago`;
    if (secs < 3600) return `${Math.floor(secs/60)}m ago`;
    if (secs < 86400) return `${Math.floor(secs/3600)}h ago`;
    return `${Math.floor(secs/86400)}d ago`;
  }

  function switchWorkspace(ws) {
    navigate?.('inbox', { type: 'workspace', workspaceId: ws.id });
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
        <Badge variant="info" value={me.global_role} />
      {/if}
    </div>
    {#if !editing}
      <button class="btn-edit" onclick={() => (editing = true)}>Edit</button>
    {/if}
    {#if unread > 0}
      <div class="notif-bell" role="status" aria-live="polite" aria-label="{unread} unread notifications">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18" aria-hidden="true">
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
        <input class="field-input" bind:value={editForm.display_name} placeholder="Your display name" autocomplete="name" />
      </label>
      <label class="field-label">Timezone
        <input class="field-input" bind:value={editForm.timezone} placeholder="e.g. America/New_York" autocomplete="off" />
      </label>
      <label class="field-label">Locale
        <input class="field-input" bind:value={editForm.locale} placeholder="e.g. en-US" autocomplete="off" />
      </label>
      <div class="edit-actions">
        <button class="btn-secondary" onclick={() => { editForm = { display_name: me?.display_name ?? '', timezone: me?.timezone ?? '', locale: me?.locale ?? '' }; editing = false; }}>Cancel</button>
        <button class="btn-primary" onclick={saveEdit} disabled={saving} aria-busy={saving}>
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  {/if}

  <Tabs {tabs} bind:active={activeTab} />

  <div class="tab-body" role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" aria-busy={loading}>
    {#if loading}
      <Skeleton lines={5} />
    {:else if activeTab === 'info'}
      {#if me}
        <div class="info-grid">
          {#each [['Username', me.username],['Email', me.email],['Display Name', me.display_name],['Timezone', me.timezone],['Locale', me.locale],['Role', me.global_role],['Auth Provider', me.oidc_issuer]] as [label, val]}
            {#if val}
              <div class="info-row">
                <span class="info-label">{label}</span>
                <span class="info-val">{val}</span>
              </div>
            {/if}
          {/each}
        </div>
      {:else}
        <EmptyState description="Profile data unavailable." />
      {/if}

    {:else if activeTab === 'memberships'}
      <!-- Workspace memberships with quick-switch -->
      {#if workspaces.length === 0}
        <EmptyState description="No workspace memberships." />
      {:else}
        <div class="memberships-list">
          {#each workspaces as ws}
            <div class="membership-item">
              <div class="membership-info">
                <span class="membership-name" title={ws.name ?? ws.slug ?? ws.id}>{ws.name ?? ws.slug ?? ws.id}</span>
                {#if ws.role}
                  <Badge value={ws.role} color="neutral" />
                {/if}
              </div>
              {#if ws.trust_level}
                <span class="membership-trust muted">Trust: {ws.trust_level}</span>
              {/if}
              <button
                class="btn-switch"
                onclick={() => switchWorkspace(ws)}
                aria-label="Switch to {ws.name ?? ws.id} workspace"
              >
                Switch
              </button>
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'ledger'}
      <!-- Judgment Ledger: chronological log of human judgment decisions -->
      <!-- Sourced from GET /api/v1/users/me/judgments -->
      {#if judgments.length === 0}
        <EmptyState description="No judgment events recorded. Spec approvals, gate overrides, trust changes, and meta-spec edits will appear here." />
      {:else}
        <div class="ledger-list">
          {#each judgments as j}
            <div class="ledger-item">
              <div class="ledger-row">
                <Badge value={judgmentLabel(j)} color={judgmentEventColor(judgmentLabel(j))} />
                <span class="ledger-target mono" title={judgmentTarget(j)}>{judgmentTarget(j)}</span>
                <span class="ledger-time muted">{rel(j.timestamp ?? j.created_at ?? j.approved_at)}</span>
              </div>
              <div class="ledger-meta">
                {#if judgmentWorkspace(j)}
                  <span class="ledger-ws muted">{judgmentWorkspace(j)}</span>
                {/if}
                {#if j.sha || j.spec_sha}
                  <span class="ledger-sha mono muted">{(j.sha ?? j.spec_sha).slice(0, 7)}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'notif-prefs'}
      <!-- Notification Preferences: per-type toggles (HSI §12) -->
      <div class="prefs-section">
        <p class="prefs-desc">Choose which notification types you receive in your Inbox.</p>
        <div class="prefs-list">
          {#each NOTIF_TYPES as nt}
            <label class="pref-row">
              <input
                type="checkbox"
                class="pref-checkbox"
                bind:checked={notifPrefs[nt.id]}
                aria-label="Enable {nt.label} notifications"
              />
              <span class="pref-label">{nt.label}</span>
            </label>
          {/each}
        </div>
        <div class="prefs-actions">
          <button class="btn-primary" onclick={savePrefs} disabled={prefsSaving} aria-busy={prefsSaving}>
            {prefsSaving ? 'Saving…' : 'Save Preferences'}
          </button>
        </div>
      </div>

    {:else if activeTab === 'notifications'}
      {#if notifications.length === 0}
        <EmptyState description="No notifications." />
      {:else}
        <div class="notif-list">
          {#each notifications as notif}
            <div class="notif-item" class:unread={!notif.read}>
              <div class="notif-top">
                <Badge variant={notifColor(notif.notification_type)} value={notif.notification_type ?? 'info'} />
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
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
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
    color: var(--color-surface, #fff);
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
  .field-input:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible {
    outline: none;
    border-color: var(--color-focus, #4db0ff);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-focus, #4db0ff) 30%, transparent);
  }

  .edit-actions { display: flex; gap: var(--space-2); align-items: center; }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-surface, #fff);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .btn-primary:hover:not(:disabled) { opacity: 0.88; }
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
  .btn-secondary:hover { border-color: var(--color-text-muted); color: var(--color-text); }

  .tab-body { flex: 1; overflow-y: auto; padding: var(--space-6); }

  /* Profile info */
  .info-grid { display: flex; flex-direction: column; gap: var(--space-3); max-width: 480px; }
  .info-row { display: flex; gap: var(--space-4); font-size: var(--text-sm); }
  .info-label { color: var(--color-text-muted); width: 120px; flex-shrink: 0; }
  .info-val { color: var(--color-text); }

  /* Workspace memberships */
  .memberships-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .membership-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }
  .membership-info { display: flex; align-items: center; gap: var(--space-2); flex: 1; min-width: 0; }
  .membership-name { font-size: var(--text-sm); font-weight: 500; color: var(--color-text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .membership-trust { font-size: var(--text-xs); }
  .btn-switch {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    cursor: pointer;
    flex-shrink: 0;
    white-space: nowrap;
  }
  .btn-switch:hover { border-color: var(--color-primary); color: var(--color-primary); }

  /* Judgment Ledger */
  .ledger-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .ledger-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .ledger-row { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .ledger-target { font-size: var(--text-xs); color: var(--color-text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .ledger-time { font-size: var(--text-xs); margin-left: auto; flex-shrink: 0; }
  .ledger-meta { display: flex; align-items: center; gap: var(--space-3); }
  .ledger-ws { font-size: var(--text-xs); }
  .ledger-sha { font-size: var(--text-xs); }

  /* Notification Preferences */
  .prefs-section { display: flex; flex-direction: column; gap: var(--space-4); max-width: 480px; }
  .prefs-desc { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); }
  .prefs-list { display: flex; flex-direction: column; gap: var(--space-3); }
  .pref-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text);
  }
  .pref-checkbox { accent-color: var(--color-focus, #4db0ff); width: 16px; height: 16px; cursor: pointer; }
  .pref-label { flex: 1; }
  .prefs-actions { display: flex; }

  /* Notifications */
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
  .notif-item.unread { border-left: 3px solid var(--color-focus, #4db0ff); }
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

  .mono { font-family: var(--font-mono); }
  .muted { color: var(--color-text-muted); }

  .btn-edit:focus-visible,
  .btn-secondary:focus-visible,
  .btn-primary:focus-visible,
  .btn-switch:focus-visible,
  .mark-read-btn:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }
</style>
